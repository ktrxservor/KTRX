//! Generic, permission-gated process primitives exposed to sandboxed tools.
//!
//! A tool with the `process` permission can run a program to completion
//! (`process.exec`) or drive it interactively (`process.spawn`, then
//! `process.read`, `process.write`, `process.wait`, `process.kill`, and
//! `process.close`). Which programs to run, how to build their arguments, and
//! how to interpret their output are the tool's business; the host only
//! starts the program, moves bytes, and reports how it ended.
//!
//! Safety properties the host guarantees:
//!
//! * Programs are started directly, never through a shell, so arguments are
//!   passed verbatim and cannot inject further commands.
//! * When the manifest (or the user, with `--allow-program`) restricts the
//!   programs, only a bare name on that list is accepted and it is looked up
//!   on `PATH`; paths are refused.
//! * Every program runs in its own process tree (a Windows job object, a Unix
//!   process group). Killing, closing, or finishing a process — and the end
//!   of the run, whatever its outcome — terminates the whole tree, so nothing
//!   started by a tool outlives the run.
//! * Pipes are bounded and every blocking call observes the run's
//!   cancellation flag.

use base64::Engine as _;
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) const OPERATIONS: [&str; 7] = [
    "process.exec",
    "process.spawn",
    "process.read",
    "process.write",
    "process.wait",
    "process.kill",
    "process.close",
];

/// Processes a run may keep alive at the same time (spawned and executing).
const MAX_PROCESSES: usize = 16;
const MAX_ARGS: usize = 1_024;
/// Program and arguments together; Windows command lines stop at 32 767.
const MAX_COMMAND_BYTES: usize = 32 * 1024;
const MAX_ENV_VARS: usize = 256;
const MAX_ENV_BYTES: usize = 64 * 1024;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_IO_CHUNK: usize = 64 * 1024;
/// Chunks buffered per pipe before the child blocks on a full pipe.
const PIPE_QUEUE: usize = 16;
const DEFAULT_EXEC_OUTPUT: u64 = 1024 * 1024;
const MAX_EXEC_OUTPUT: u64 = 16 * 1024 * 1024;
const MAX_EXEC_STDIN: usize = 16 * 1024 * 1024;
/// Longest single blocking `process.read`, `process.write`, or `process.wait`.
const MAX_WAIT_MS: u64 = 10_000;
/// Longest `process.exec` timeout: one day. `0` means no limit.
const MAX_EXEC_TIMEOUT_MS: u64 = 24 * 60 * 60 * 1_000;
/// After a program exits, how long `process.exec` waits for its pipes to
/// close before it terminates whatever the program left running.
const EXIT_GRACE: Duration = Duration::from_millis(500);
const SLICE: Duration = Duration::from_millis(10);

const CANCELLED: &str = "[cancelled] the run was cancelled";

/// A program a tool started, for the runner's audit trail.
#[derive(Clone, Debug)]
pub(crate) struct Started {
    pub(crate) pid: u32,
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    fn parse(request: &Value) -> Result<Self, String> {
        match request
            .get("stream")
            .and_then(Value::as_str)
            .unwrap_or("stdout")
        {
            "stdout" => Ok(Self::Stdout),
            "stderr" => Ok(Self::Stderr),
            _ => Err("stream must be stdout or stderr".into()),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// One output pipe, drained by a reader thread into a bounded queue.
struct Pipe {
    rx: Receiver<Vec<u8>>,
    /// Part of a received chunk that a short read left behind.
    leftover: Vec<u8>,
    eof: bool,
    total: u64,
}

impl Pipe {
    fn start(mut source: impl Read + Send + 'static) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(PIPE_QUEUE);
        thread::spawn(move || {
            let mut buffer = vec![0u8; MAX_IO_CHUNK];
            loop {
                match source.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        // A dropped receiver means nobody reads any more.
                        if tx.send(buffer[..count].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        });
        Self {
            rx,
            leftover: Vec::new(),
            eof: false,
            total: 0,
        }
    }

    /// Take up to `limit` buffered bytes, waiting at most `wait` for a chunk.
    fn take(&mut self, limit: usize, wait: Duration) -> Vec<u8> {
        if self.leftover.is_empty() && !self.eof {
            let received = if wait.is_zero() {
                self.rx.try_recv().map_err(|e| match e {
                    TryRecvError::Empty => RecvTimeoutError::Timeout,
                    TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                })
            } else {
                self.rx.recv_timeout(wait)
            };
            match received {
                Ok(chunk) => {
                    self.total += chunk.len() as u64;
                    self.leftover = chunk;
                }
                Err(RecvTimeoutError::Disconnected) => self.eof = true,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        let count = self.leftover.len().min(limit);
        let rest = self.leftover.split_off(count);
        std::mem::replace(&mut self.leftover, rest)
    }

    fn finished(&self) -> bool {
        self.eof && self.leftover.is_empty()
    }
}

/// Collects a pipe's output for `process.exec`, keeping at most `limit` bytes.
struct Capture {
    bytes: Vec<u8>,
    limit: usize,
}

impl Capture {
    fn keep(&mut self, chunk: &[u8]) {
        let room = self.limit.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
}

struct Process {
    child: Child,
    pid: u32,
    program: String,
    tree: sys::Tree,
    stdin: Option<SyncSender<Vec<u8>>>,
    stdout: Pipe,
    stderr: Pipe,
    status: Option<ExitStatus>,
    started: Instant,
    ended: Option<Instant>,
    killed: bool,
}

impl Process {
    /// Record the exit status if the program has ended.
    fn poll_exit(&mut self) -> Result<Option<ExitStatus>, String> {
        if self.status.is_none()
            && let Some(status) = self
                .child
                .try_wait()
                .map_err(|e| format!("[other] cannot query the process: {e}"))?
        {
            self.status = Some(status);
            self.ended = Some(Instant::now());
        }
        Ok(self.status)
    }

    /// Terminate the program and everything it started.
    fn terminate(&mut self) {
        self.tree.terminate();
        let _ = self.child.kill();
        self.killed = true;
    }

    fn status_json(&self) -> Value {
        let mut status = json!({
            "pid": self.pid,
            "running": self.status.is_none(),
            "killed": self.killed,
            "duration_ms": self
                .ended
                .unwrap_or_else(Instant::now)
                .duration_since(self.started)
                .as_millis() as u64,
        });
        if let Some(exit) = self.status {
            status["exit_code"] = json!(exit.code());
            status["signal"] = json!(exit_signal(&exit));
            status["success"] = json!(exit.success());
        }
        status
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.status.is_none() {
            self.terminate();
        }
        // Reap the child so it does not linger as a zombie on Unix.
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => thread::sleep(SLICE),
            }
        }
    }
}

pub(crate) struct ProcessState {
    processes: HashMap<u64, Process>,
    next_id: u64,
    /// Programs this run may start; `None` allows any program.
    allowed: Option<Vec<String>>,
    cancelled: Arc<AtomicBool>,
    started: Vec<Started>,
    /// Number of `process.exec` calls in progress (always 0 or 1, since host
    /// calls are synchronous), counted against [`MAX_PROCESSES`].
    executing: usize,
}

impl ProcessState {
    /// State for one run. `allowed` lists the programs the tool may start
    /// (`None`: any program); `cancelled` is the run's cancellation flag.
    pub(crate) fn new(allowed: Option<Vec<String>>, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            processes: HashMap::new(),
            next_id: 0,
            allowed,
            cancelled,
            started: Vec::new(),
            executing: 0,
        }
    }

    /// Programs started since the last call, for the runner's audit trail.
    pub(crate) fn take_started(&mut self) -> Vec<Started> {
        std::mem::take(&mut self.started)
    }

    pub(crate) fn request(&mut self, operation: &str, request: &Value) -> Result<String, String> {
        match operation {
            "process.exec" => self.exec(request),
            "process.spawn" => self.spawn_handle(request),
            "process.read" => self.read(request),
            "process.write" => self.write(request),
            "process.wait" => self.wait(request),
            "process.kill" => self.kill(request),
            "process.close" => self.close(request),
            other => Err(format!("unsupported process capability '{other}'")),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn check_capacity(&self) -> Result<(), String> {
        if self.processes.len() + self.executing >= MAX_PROCESSES {
            Err(format!(
                "a tool may run at most {MAX_PROCESSES} processes at a time; close finished ones with process.close"
            ))
        } else {
            Ok(())
        }
    }

    fn start(&mut self, spec: &Spec, input: Input) -> Result<Process, String> {
        self.check_capacity()?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(if matches!(input, Input::None) {
                Stdio::null()
            } else {
                Stdio::piped()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if spec.clear_env {
            command.env_clear();
        }
        for (name, value) in &spec.env {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        sys::prepare(&mut command);
        let mut child = command.spawn().map_err(|e| {
            let kind = match e.kind() {
                std::io::ErrorKind::NotFound => "not_found",
                std::io::ErrorKind::PermissionDenied => "denied",
                _ => "other",
            };
            format!("[{kind}] cannot start {}: {e}", spec.program.display())
        })?;
        let tree = match sys::attach(&mut child) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("[other] cannot start the process: {error}"));
            }
        };
        let pid = child.id();
        let stdout = Pipe::start(child.stdout.take().ok_or("stdout pipe unavailable")?);
        let stderr = Pipe::start(child.stderr.take().ok_or("stderr pipe unavailable")?);
        let stdin = match (input, child.stdin.take()) {
            (Input::Interactive, Some(pipe)) => Some(start_writer(pipe)),
            (Input::Feed(bytes), Some(pipe)) => {
                feed(pipe, bytes);
                None
            }
            _ => None,
        };
        let program = spec.program.display().to_string();
        self.started.push(Started {
            pid,
            program: program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.display().to_string(),
        });
        Ok(Process {
            child,
            pid,
            program,
            tree,
            stdin,
            stdout,
            stderr,
            status: None,
            started: Instant::now(),
            ended: None,
            killed: false,
        })
    }

    fn exec(&mut self, request: &Value) -> Result<String, String> {
        let spec = Spec::from_request(request, self.allowed.as_deref())?;
        let input = match request.get("stdin_base64") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(text)
                    .map_err(|e| format!("invalid Base64 stdin: {e}"))?;
                if bytes.len() > MAX_EXEC_STDIN {
                    return Err("stdin_base64 is limited to 16 MiB".into());
                }
                Some(bytes)
            }
            Some(_) => return Err("stdin_base64 must be a string".into()),
        };
        let limit = request
            .get("max_output")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_EXEC_OUTPUT)
            .clamp(1, MAX_EXEC_OUTPUT) as usize;
        let timeout_ms = request
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if timeout_ms > MAX_EXEC_TIMEOUT_MS {
            return Err("timeout_ms may not exceed one day (86400000)".into());
        }
        let text = output_as_text(request)?;
        let mut process = self.start(&spec, input.map_or(Input::None, Input::Feed))?;
        self.executing += 1;
        let outcome = self.collect(&mut process, limit, timeout_ms);
        self.executing -= 1;
        let (stdout, stderr, timed_out) = outcome?;
        let mut result = process.status_json();
        if let Value::Object(fields) = &mut result {
            fields.remove("running");
            fields.insert("program".into(), json!(process.program));
            fields.insert("timed_out".into(), json!(timed_out));
            put_output(
                fields,
                "stdout",
                &stdout.bytes,
                process.stdout.total,
                limit,
                text,
            );
            put_output(
                fields,
                "stderr",
                &stderr.bytes,
                process.stderr.total,
                limit,
                text,
            );
        }
        Ok(result.to_string())
    }

    /// Run `process` to completion, capturing its output. Returns the
    /// captured stdout and stderr and whether the timeout fired.
    fn collect(
        &self,
        process: &mut Process,
        limit: usize,
        timeout_ms: u64,
    ) -> Result<(Capture, Capture, bool), String> {
        let deadline = (timeout_ms > 0).then(|| Instant::now() + Duration::from_millis(timeout_ms));
        let mut stdout = Capture {
            bytes: Vec::new(),
            limit,
        };
        let mut stderr = Capture {
            bytes: Vec::new(),
            limit,
        };
        let mut timed_out = false;
        let mut lingering_killed_at: Option<Instant> = None;
        loop {
            if self.is_cancelled() {
                process.terminate();
                return Err(CANCELLED.into());
            }
            // Block briefly on whichever pipe is still open, then sweep the
            // other one, so the loop never spins.
            let out_wait = if process.stdout.finished() {
                Duration::ZERO
            } else {
                SLICE
            };
            stdout.keep(&process.stdout.take(MAX_IO_CHUNK, out_wait));
            let mut err_wait = if process.stdout.finished() && !process.stderr.finished() {
                SLICE
            } else {
                Duration::ZERO
            };
            loop {
                let chunk = process.stderr.take(MAX_IO_CHUNK, err_wait);
                err_wait = Duration::ZERO;
                if chunk.is_empty() {
                    break;
                }
                stderr.keep(&chunk);
            }
            let exited = process.poll_exit()?.is_some();
            let drained = process.stdout.finished() && process.stderr.finished();
            if exited && drained {
                break;
            }
            if drained {
                // Output is closed but the program still runs.
                thread::sleep(SLICE);
            }
            let now = Instant::now();
            if !exited && deadline.is_some_and(|d| now >= d) && !timed_out {
                timed_out = true;
                process.terminate();
                lingering_killed_at = Some(now);
            }
            if exited && !drained {
                match lingering_killed_at {
                    // The program exited but something it started still holds
                    // its output pipes open: end the rest of the tree.
                    None if process
                        .ended
                        .is_some_and(|t| now.duration_since(t) >= EXIT_GRACE) =>
                    {
                        process.tree.terminate();
                        lingering_killed_at = Some(now);
                    }
                    // A process that escaped the tree keeps the pipe open;
                    // stop waiting for it rather than hang the run.
                    Some(at) if now.duration_since(at) >= EXIT_GRACE * 4 => break,
                    _ => {}
                }
            }
        }
        Ok((stdout, stderr, timed_out))
    }

    fn spawn_handle(&mut self, request: &Value) -> Result<String, String> {
        let spec = Spec::from_request(request, self.allowed.as_deref())?;
        let pipe_stdin = request
            .get("stdin")
            .map(|v| v.as_bool().ok_or("stdin must be a boolean"))
            .transpose()?
            .unwrap_or(false);
        let input = if pipe_stdin {
            Input::Interactive
        } else {
            Input::None
        };
        let process = self.start(&spec, input)?;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        let reply = json!({
            "process": id,
            "pid": process.pid,
            "program": process.program,
            "stdin": pipe_stdin,
        });
        self.processes.insert(id, process);
        Ok(reply.to_string())
    }

    fn handle(&mut self, request: &Value) -> Result<(u64, &mut Process), String> {
        let id = request
            .get("process")
            .and_then(Value::as_u64)
            .ok_or("request requires process")?;
        let process = self.processes.get_mut(&id).ok_or("unknown process")?;
        Ok((id, process))
    }

    fn read(&mut self, request: &Value) -> Result<String, String> {
        let stream = Stream::parse(request)?;
        let length = request
            .get("length")
            .and_then(Value::as_u64)
            .unwrap_or(MAX_IO_CHUNK as u64);
        if length == 0 || length > MAX_IO_CHUNK as u64 {
            return Err(format!("process reads must be 1-{MAX_IO_CHUNK} bytes"));
        }
        let timeout = wait_duration(request, 1_000)?;
        let cancelled = Arc::clone(&self.cancelled);
        let (id, process) = self.handle(request)?;
        let pipe = match stream {
            Stream::Stdout => &mut process.stdout,
            Stream::Stderr => &mut process.stderr,
        };
        let deadline = Instant::now() + timeout;
        let bytes = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let bytes = pipe.take(length as usize, remaining.min(Duration::from_millis(100)));
            if !bytes.is_empty() || pipe.finished() || remaining.is_zero() {
                break bytes;
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(CANCELLED.into());
            }
        };
        let eof = pipe.finished();
        Ok(json!({
            "process": id,
            "stream": stream.name(),
            "bytes_base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
            "eof": eof,
            "timeout": bytes.is_empty() && !eof,
        })
        .to_string())
    }

    fn write(&mut self, request: &Value) -> Result<String, String> {
        let bytes = match request.get("bytes_base64") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(text)) => base64::engine::general_purpose::STANDARD
                .decode(text)
                .map_err(|e| format!("invalid Base64 process input: {e}"))?,
            Some(_) => return Err("bytes_base64 must be a string".into()),
        };
        if bytes.len() > MAX_IO_CHUNK {
            return Err(format!(
                "process writes are limited to {MAX_IO_CHUNK} bytes"
            ));
        }
        let close = request
            .get("close")
            .map(|v| v.as_bool().ok_or("close must be a boolean"))
            .transpose()?
            .unwrap_or(false);
        let timeout = wait_duration(request, 5_000)?;
        let cancelled = Arc::clone(&self.cancelled);
        let (id, process) = self.handle(request)?;
        let stdin = process
            .stdin
            .as_ref()
            .ok_or("[closed] the process has no open stdin; spawn it with stdin: true")?;
        let written = bytes.len();
        if !bytes.is_empty() {
            let deadline = Instant::now() + timeout;
            let mut pending = bytes;
            loop {
                match stdin.try_send(pending) {
                    Ok(()) => break,
                    Err(TrySendError::Disconnected(_)) => {
                        process.stdin = None;
                        return Err("[closed] the process closed its stdin".into());
                    }
                    Err(TrySendError::Full(returned)) => {
                        if cancelled.load(Ordering::Acquire) {
                            return Err(CANCELLED.into());
                        }
                        if Instant::now() >= deadline {
                            return Err("[timeout] the process is not reading its stdin".into());
                        }
                        pending = returned;
                        thread::sleep(SLICE);
                    }
                }
            }
        }
        if close {
            // Dropping the sender lets the writer flush and close the pipe.
            process.stdin = None;
        }
        Ok(
            json!({"process": id, "written": written, "stdin_open": process.stdin.is_some()})
                .to_string(),
        )
    }

    fn wait(&mut self, request: &Value) -> Result<String, String> {
        let timeout = wait_duration(request, 1_000)?;
        let cancelled = Arc::clone(&self.cancelled);
        let (id, process) = self.handle(request)?;
        let deadline = Instant::now() + timeout;
        while process.poll_exit()?.is_none() && Instant::now() < deadline {
            if cancelled.load(Ordering::Acquire) {
                return Err(CANCELLED.into());
            }
            thread::sleep(SLICE);
        }
        let mut status = process.status_json();
        status["process"] = json!(id);
        Ok(status.to_string())
    }

    fn kill(&mut self, request: &Value) -> Result<String, String> {
        let (id, process) = self.handle(request)?;
        let was_running = process.poll_exit()?.is_none();
        // Also clears anything the program left running after it exited.
        process.terminate();
        let deadline = Instant::now() + Duration::from_secs(2);
        while process.poll_exit()?.is_none() && Instant::now() < deadline {
            thread::sleep(SLICE);
        }
        let mut status = process.status_json();
        status["process"] = json!(id);
        status["was_running"] = json!(was_running);
        Ok(status.to_string())
    }

    fn close(&mut self, request: &Value) -> Result<String, String> {
        let (id, process) = self.handle(request)?;
        process.poll_exit()?;
        let mut status = process.status_json();
        status["process"] = json!(id);
        status["closed"] = json!(true);
        // Dropping the process terminates its tree if it is still running.
        self.processes.remove(&id);
        Ok(status.to_string())
    }
}

/// A validated request to start a program.
struct Spec {
    program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, Option<String>)>,
    clear_env: bool,
}

impl Spec {
    fn from_request(request: &Value, allowed: Option<&[String]>) -> Result<Self, String> {
        let program = request
            .get("program")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .ok_or("request requires program")?;
        if program.len() > MAX_PATH_BYTES || program.contains('\0') {
            return Err("program must be 1-4096 characters without NUL bytes".into());
        }
        let args: Vec<String> = match request.get("args") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => {
                if items.len() > MAX_ARGS {
                    return Err(format!("args may hold at most {MAX_ARGS} entries"));
                }
                items
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .filter(|a| !a.contains('\0'))
                            .map(str::to_owned)
                            .ok_or_else(|| "args must be strings without NUL bytes".to_owned())
                    })
                    .collect::<Result<_, _>>()?
            }
            Some(_) => return Err("args must be an array of strings".into()),
        };
        let command_bytes = program.len() + args.iter().map(|a| a.len() + 1).sum::<usize>();
        if command_bytes > MAX_COMMAND_BYTES {
            return Err("program and args together may not exceed 32 KiB".into());
        }
        let cwd = match request.get("cwd") {
            None | Some(Value::Null) => std::env::current_dir()
                .map_err(|e| format!("cannot read the current directory: {e}"))?,
            Some(Value::String(dir)) if !dir.is_empty() && !dir.contains('\0') => {
                let canonical = std::fs::canonicalize(dir)
                    .map_err(|e| format!("cwd '{dir}' is not usable: {e}"))?;
                if !canonical.is_dir() {
                    return Err(format!("cwd '{dir}' is not a directory"));
                }
                canonical
            }
            Some(_) => return Err("cwd must be a non-empty path".into()),
        };
        let clear_env = request
            .get("clear_env")
            .map(|v| v.as_bool().ok_or("clear_env must be a boolean"))
            .transpose()?
            .unwrap_or(false);
        let env = parse_env(request.get("env"))?;
        let program = resolve_program(program, allowed, &cwd, search_path(&env, clear_env))?;
        Ok(Self {
            program,
            args,
            cwd: display_cwd(cwd),
            env,
            clear_env,
        })
    }
}

/// `canonicalize` adds the `\\?\` prefix on Windows; some programs (and
/// `cmd.exe` in particular) refuse such a working directory.
fn display_cwd(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\")
            && !rest.starts_with("UNC\\")
        {
            return PathBuf::from(rest);
        }
    }
    path
}

fn parse_env(value: Option<&Value>) -> Result<Vec<(String, Option<String>)>, String> {
    let map = match value {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(map)) => map,
        Some(_) => return Err("env must be an object of names to strings or null".into()),
    };
    if map.len() > MAX_ENV_VARS {
        return Err(format!("env may set at most {MAX_ENV_VARS} variables"));
    }
    let mut total = 0;
    let mut env = Vec::with_capacity(map.len());
    for (name, value) in map {
        if name.is_empty() || name.contains(['=', '\0']) {
            return Err(format!("invalid environment variable name {name:?}"));
        }
        let value = match value {
            Value::Null => None,
            Value::String(text) if !text.contains('\0') => Some(text.clone()),
            _ => return Err(format!("env value for {name} must be a string or null")),
        };
        total += name.len() + value.as_ref().map_or(0, String::len);
        env.push((name.clone(), value));
    }
    if total > MAX_ENV_BYTES {
        return Err("env may not exceed 64 KiB".into());
    }
    Ok(env)
}

/// The `PATH` the child will see, which is also where its program is found.
fn search_path(env: &[(String, Option<String>)], clear_env: bool) -> Option<OsString> {
    let overridden = env.iter().rev().find(|(name, _)| {
        if cfg!(windows) {
            name.eq_ignore_ascii_case("PATH")
        } else {
            name == "PATH"
        }
    });
    match overridden {
        Some((_, value)) => value.clone().map(OsString::from),
        None if clear_env => None,
        None => std::env::var_os("PATH"),
    }
}

/// Turn the requested program into the absolute path that will be started.
fn resolve_program(
    requested: &str,
    allowed: Option<&[String]>,
    cwd: &Path,
    path: Option<OsString>,
) -> Result<PathBuf, String> {
    let bare = !requested.contains(['/', '\\']) && Path::new(requested).components().count() == 1;
    if let Some(list) = allowed {
        if !bare {
            return Err(format!(
                "[denied] '{requested}' is a path; this tool may only start the programs it lists by name"
            ));
        }
        if !list.iter().any(|entry| program_matches(requested, entry)) {
            return Err(format!(
                "[denied] '{requested}' is not among the programs this run may start ({})",
                if list.is_empty() {
                    "none".to_owned()
                } else {
                    list.join(", ")
                }
            ));
        }
    }
    if !bare {
        let candidate = cwd.join(requested);
        return if candidate.is_file() {
            Ok(display_cwd(
                std::fs::canonicalize(&candidate).unwrap_or(candidate),
            ))
        } else {
            Err(format!("[not_found] program '{requested}' does not exist"))
        };
    }
    let directories = path
        .ok_or_else(|| format!("[not_found] cannot find '{requested}': the process has no PATH"))?;
    for directory in std::env::split_paths(&directories) {
        if directory.as_os_str().is_empty() || !directory.is_absolute() {
            // Relative PATH entries would depend on the working directory.
            continue;
        }
        for candidate in candidates(&directory, requested) {
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }
    }
    Err(format!(
        "[not_found] program '{requested}' was not found on PATH"
    ))
}

/// Whether `requested` names the allow-list `entry`. Windows names are
/// case-insensitive and may carry an executable extension (`git.exe`).
fn program_matches(requested: &str, entry: &str) -> bool {
    if cfg!(windows) {
        let requested = requested.to_ascii_lowercase();
        let entry = entry.to_ascii_lowercase();
        requested == entry
            || executable_extensions()
                .iter()
                .any(|ext| requested == format!("{entry}{ext}"))
    } else {
        requested == entry
    }
}

/// Lower-case executable extensions, with their dot, from `PATHEXT`.
fn executable_extensions() -> Vec<String> {
    let raw = std::env::var("PATHEXT").unwrap_or_default();
    let mut extensions: Vec<String> = raw
        .split(';')
        .map(|ext| ext.trim().to_ascii_lowercase())
        .filter(|ext| ext.starts_with('.') && ext.len() > 1)
        .collect();
    if extensions.is_empty() {
        extensions = [".com", ".exe", ".bat", ".cmd"].map(String::from).to_vec();
    }
    extensions
}

fn candidates(directory: &Path, name: &str) -> Vec<PathBuf> {
    if cfg!(windows) {
        let extensions = executable_extensions();
        let lower = name.to_ascii_lowercase();
        let mut list = Vec::new();
        // A name that already ends in an executable extension is used as is.
        if extensions.iter().any(|ext| lower.ends_with(ext.as_str())) {
            list.push(directory.join(name));
        }
        list.extend(
            extensions
                .iter()
                .map(|ext| directory.join(format!("{name}{ext}"))),
        );
        list
    } else {
        vec![directory.join(name)]
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn wait_duration(request: &Value, default_ms: u64) -> Result<Duration, String> {
    let ms = request
        .get("timeout_ms")
        .map(|v| {
            v.as_u64()
                .ok_or("timeout_ms must be a non-negative integer")
        })
        .transpose()?
        .unwrap_or(default_ms);
    if ms > MAX_WAIT_MS {
        return Err(format!("timeout_ms may not exceed {MAX_WAIT_MS}"));
    }
    Ok(Duration::from_millis(ms))
}

fn output_as_text(request: &Value) -> Result<bool, String> {
    match request.get("output").and_then(Value::as_str) {
        None | Some("base64") => Ok(false),
        Some("text") => Ok(true),
        Some(_) => Err("output must be base64 or text".into()),
    }
}

fn put_output(
    fields: &mut Map<String, Value>,
    name: &str,
    bytes: &[u8],
    total: u64,
    limit: usize,
    text: bool,
) {
    if text {
        fields.insert(
            name.into(),
            json!(String::from_utf8_lossy(bytes).into_owned()),
        );
    } else {
        fields.insert(
            format!("{name}_base64"),
            json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
        );
    }
    fields.insert(format!("{name}_bytes"), json!(total));
    fields.insert(format!("{name}_truncated"), json!(total > limit as u64));
}

/// What a new process reads on its standard input.
enum Input {
    /// Nothing: stdin is the null device.
    None,
    /// The tool writes with `process.write`.
    Interactive,
    /// These bytes, then end of input (`process.exec`).
    Feed(Vec<u8>),
}

/// Write `bytes` to a child's stdin on a separate thread, then close it. The
/// thread ends early if the child stops reading and exits, so a program that
/// ignores its input can never block the run.
fn feed(mut stdin: ChildStdin, bytes: Vec<u8>) {
    thread::spawn(move || {
        let _ = stdin.write_all(&bytes).and_then(|()| stdin.flush());
    });
}

/// Feed a child's stdin from a bounded queue; dropping the sender closes it.
fn start_writer(mut stdin: ChildStdin) -> SyncSender<Vec<u8>> {
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(PIPE_QUEUE);
    thread::spawn(move || {
        for chunk in rx {
            if stdin
                .write_all(&chunk)
                .and_then(|()| stdin.flush())
                .is_err()
            {
                break;
            }
        }
    });
    tx
}

#[cfg(unix)]
fn exit_signal(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: &ExitStatus) -> Option<i32> {
    None
}

#[cfg(unix)]
mod sys {
    use std::process::{Child, Command};

    /// The child's process group; the child leads it.
    pub(super) struct Tree {
        group: libc::pid_t,
    }

    pub(super) fn prepare(command: &mut Command) {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    pub(super) fn attach(child: &mut Child) -> std::io::Result<Tree> {
        let group = libc::pid_t::try_from(child.id())
            .map_err(|_| std::io::Error::other("process id out of range"))?;
        Ok(Tree { group })
    }

    impl Tree {
        /// Kill every process left in the group.
        pub(super) fn terminate(&self) {
            // SAFETY: plain system call; a negative id addresses the group
            // this run created, and failure (no such group) is harmless.
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            self.terminate();
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::{
        os::windows::{io::AsRawHandle, process::CommandExt},
        process::{Child, Command},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{
                CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, OpenThread, ResumeThread,
                THREAD_SUSPEND_RESUME,
            },
        },
    };

    /// A job object holding the child and everything it starts. Closing the
    /// last handle kills whatever is still inside.
    pub(super) struct Tree {
        job: Option<HANDLE>,
    }

    // SAFETY: a job handle is a kernel object reference usable from any thread.
    unsafe impl Send for Tree {}

    /// Start the child suspended, so it cannot start anything before it is
    /// inside its job object.
    pub(super) fn prepare(command: &mut Command) {
        command.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
    }

    pub(super) fn attach(child: &mut Child) -> std::io::Result<Tree> {
        let job = create_job(child);
        resume(child.id())?;
        Ok(Tree { job })
    }

    /// A kill-on-close job containing `child`, or `None` when the system
    /// refuses one (the child is then terminated on its own).
    fn create_job(child: &Child) -> Option<HANDLE> {
        // SAFETY: documented Win32 calls with valid arguments; every handle
        // is closed on the failure paths.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) != 0;
            if !configured || AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                CloseHandle(job);
                return None;
            }
            Some(job)
        }
    }

    /// Resume the main thread of a process created suspended.
    fn resume(pid: u32) -> std::io::Result<()> {
        // SAFETY: documented Win32 calls; the snapshot and thread handles are
        // closed before returning.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut resumed = 0;
            let mut more = Thread32First(snapshot, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        if ResumeThread(thread) != u32::MAX {
                            resumed += 1;
                        }
                        CloseHandle(thread);
                    }
                }
                more = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            if resumed == 0 {
                Err(std::io::Error::other(
                    "the new process could not be resumed",
                ))
            } else {
                Ok(())
            }
        }
    }

    impl Tree {
        /// Kill every process in the job.
        pub(super) fn terminate(&self) {
            if let Some(job) = self.job {
                // SAFETY: `job` is a live handle owned by this value.
                unsafe {
                    TerminateJobObject(job, 1);
                }
            }
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            if let Some(job) = self.job.take() {
                // SAFETY: closing the handle this value owns; the job's
                // kill-on-close limit ends anything still inside.
                unsafe {
                    CloseHandle(job);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_environment_overrides() {
        assert!(parse_env(Some(&json!({"A": "1", "B": null}))).is_ok());
        for bad in [
            json!({"": "x"}),
            json!({"A=B": "x"}),
            json!({"A": 1}),
            json!({"A": "x\u{0}"}),
            json!(["A"]),
        ] {
            assert!(parse_env(Some(&bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_child_path_follows_overrides() {
        let env = vec![("PATH".to_owned(), Some("/opt/bin".to_owned()))];
        assert_eq!(search_path(&env, false), Some(OsString::from("/opt/bin")));
        assert_eq!(search_path(&[], true), None);
        let removed = vec![("PATH".to_owned(), None)];
        assert_eq!(search_path(&removed, false), None);
    }

    #[test]
    fn an_allow_list_accepts_only_listed_bare_names() {
        let cwd = std::env::current_dir().unwrap();
        let allowed = vec!["definitely-not-installed-ktrx".to_owned()];
        let path = std::env::var_os("PATH");
        let error = resolve_program("cargo", Some(&allowed), &cwd, path.clone()).unwrap_err();
        assert!(error.starts_with("[denied]"), "{error}");
        let error = resolve_program(
            "./definitely-not-installed-ktrx",
            Some(&allowed),
            &cwd,
            path.clone(),
        )
        .unwrap_err();
        assert!(error.contains("is a path"), "{error}");
        let error = resolve_program("definitely-not-installed-ktrx", Some(&allowed), &cwd, path)
            .unwrap_err();
        assert!(error.starts_with("[not_found]"), "{error}");
        let error = resolve_program("cargo", Some(&[]), &cwd, None).unwrap_err();
        assert!(error.contains("(none)"), "{error}");
    }

    #[test]
    fn matches_program_names_like_the_operating_system() {
        assert!(program_matches("git", "git"));
        assert!(!program_matches("gitk", "git"));
        if cfg!(windows) {
            assert!(program_matches("GIT", "git"));
            assert!(program_matches("git.exe", "git"));
            assert!(program_matches("Git.EXE", "git"));
        } else {
            assert!(!program_matches("GIT", "git"));
            assert!(!program_matches("git.exe", "git"));
        }
    }

    #[test]
    fn rejects_malformed_requests_before_starting_anything() {
        let mut state = ProcessState::new(None, Arc::new(AtomicBool::new(false)));
        for (op, body) in [
            ("process.exec", json!({})),
            ("process.exec", json!({"program": ""})),
            ("process.exec", json!({"program": "x", "args": "a b"})),
            ("process.exec", json!({"program": "x", "args": [1]})),
            ("process.exec", json!({"program": "x", "cwd": 3})),
            (
                "process.exec",
                json!({"program": "x", "timeout_ms": 90_000_000}),
            ),
            ("process.read", json!({"process": 1})),
            ("process.read", json!({"process": 1, "stream": "stdin"})),
            ("process.wait", json!({"process": 1, "timeout_ms": 60_000})),
            ("process.close", json!({})),
        ] {
            assert!(state.request(op, &body).is_err(), "{op} {body}");
        }
        assert!(state.take_started().is_empty());
    }
}
