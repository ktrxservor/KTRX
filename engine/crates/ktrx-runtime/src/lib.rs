use anyhow::Result;
use ktrx_core::{Manifest, Permission, ToolPackage};
mod dns;
mod fs;
mod network;
mod process;
mod rawnet;
mod tls;
use std::{
    collections::HashMap,
    io::{IsTerminal, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

wasmtime::component::bindgen!({ path: "../../wit", world: "ktrx-tool" });

/// What the user approved for one run. A tool still needs the matching
/// permission in its manifest; an approval alone grants nothing.
#[derive(Clone, Debug, Default)]
pub struct Approvals {
    /// DNS, TCP, TLS, and UDP traffic.
    pub network: bool,
    /// Reading files and listing directories.
    pub file_read: bool,
    /// Writing files, confined to this directory tree. `None` denies writes.
    pub file_write: Option<PathBuf>,
    /// ICMP, ARP, and raw TCP probes.
    pub raw_net: bool,
    /// Starting programs on the host. `None` denies it.
    pub process: Option<ProcessApproval>,
}

/// The user's approval to start programs during one run.
#[derive(Clone, Debug, Default)]
pub struct ProcessApproval {
    /// Programs the user restricted this run to (`ktrx run --allow-program`),
    /// as bare names. Empty adds no restriction beyond the manifest's own
    /// `programs` list.
    pub programs: Vec<String>,
}

impl Approvals {
    fn grants(&self, permission: Permission) -> bool {
        match permission {
            Permission::Network => self.network,
            Permission::FileRead => self.file_read,
            Permission::FileWrite => self.file_write.is_some(),
            Permission::RawNet => self.raw_net,
            Permission::Process => self.process.is_some(),
        }
    }
}

/// The programs a run may start: the manifest's list, narrowed by the
/// user's `--allow-program` choices. `None` means any program.
pub fn effective_programs(manifest: &Manifest, approval: &ProcessApproval) -> Option<Vec<String>> {
    let matches = |a: &str, b: &str| {
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    match (manifest.programs.is_empty(), approval.programs.is_empty()) {
        (true, true) => None,
        (false, true) => Some(manifest.programs.clone()),
        (true, false) => Some(approval.programs.clone()),
        (false, false) => Some(
            manifest
                .programs
                .iter()
                .filter(|p| approval.programs.iter().any(|q| matches(p, q)))
                .cloned()
                .collect(),
        ),
    }
}

/// One host operation and the permissions that unlock it.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Capability {
    pub operation: &'static str,
    /// Any one of these permissions unlocks the operation; empty means the
    /// operation needs no permission.
    pub permissions: &'static [Permission],
}

/// Every operation the host bridge implements, in a stable order.
pub fn capabilities() -> Vec<Capability> {
    let mut list = vec![
        Capability {
            operation: "host.info",
            permissions: &[],
        },
        Capability {
            operation: "progress.update",
            permissions: &[],
        },
    ];
    let group = |operations: &[&'static str], permissions: &'static [Permission]| {
        operations
            .iter()
            .map(|operation| Capability {
                operation,
                permissions,
            })
            .collect::<Vec<_>>()
    };
    list.extend(group(&fs::READ_OPERATIONS, &[Permission::FileRead]));
    list.extend(group(&fs::WRITE_OPERATIONS, &[Permission::FileWrite]));
    list.extend(group(
        &[fs::CLOSE_OPERATION],
        &[Permission::FileRead, Permission::FileWrite],
    ));
    list.extend(group(&network::ALLOWED_OPERATIONS, &[Permission::Network]));
    list.extend(group(&rawnet::OPERATIONS, &[Permission::RawNet]));
    list.extend(group(&process::OPERATIONS, &[Permission::Process]));
    list
}

pub struct Runtime {
    engine: Engine,
    limits: StoreLimits,
    components: Mutex<HashMap<[u8; 32], Arc<Component>>>,
}

/// Structured events emitted during a tool invocation. The queue is bounded; if
/// a consumer falls behind, intermediate progress snapshots may be dropped.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    Progress {
        data: serde_json::Value,
    },
    /// The tool started a program on the host (`process.exec` or
    /// `process.spawn`). Published for every program, for the audit trail.
    Process {
        pid: u32,
        program: String,
        args: Vec<String>,
        cwd: String,
    },
    Finished {
        success: bool,
        output: Option<String>,
        /// Why the run failed; absent on success.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

pub struct RunSession {
    pub events: Receiver<RunEvent>,
    result: std::thread::JoinHandle<Result<String>>,
    cancelled: Arc<AtomicBool>,
    engine: Engine,
}

impl RunSession {
    /// Interrupt this invocation at its next Wasm epoch checkpoint.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.engine.increment_epoch();
    }

    pub fn join(self) -> Result<String> {
        self.result
            .join()
            .map_err(|_| anyhow::anyhow!("tool worker thread panicked"))?
    }
}

struct HostState {
    limits: StoreLimits,
    wasi: WasiCtx,
    table: ResourceTable,
    manifest: Manifest,
    approvals: Approvals,
    files: fs::FileState,
    progress_active: bool,
    last_progress_at: Option<Instant>,
    network: network::NetworkState,
    processes: process::ProcessState,
    events: Option<SyncSender<RunEvent>>,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl HostState {
    /// A capability is available only when the manifest declares it and the
    /// user approved it for this run.
    fn require(&self, permission: Permission) -> std::result::Result<(), String> {
        if self.granted(permission) {
            Ok(())
        } else {
            Err(format!(
                "{} permission was not granted for this run",
                permission.as_str()
            ))
        }
    }

    fn granted(&self, permission: Permission) -> bool {
        self.manifest.declares(permission) && self.approvals.grants(permission)
    }

    /// Describe the engine and which operations this run may use, so a tool
    /// can adapt to the host instead of probing operations one by one.
    fn host_info(&self) -> String {
        let operations: Vec<serde_json::Value> = capabilities()
            .into_iter()
            .map(|capability| {
                let granted = capability.permissions.is_empty()
                    || capability.permissions.iter().any(|p| self.granted(*p));
                serde_json::json!({
                    "operation": capability.operation,
                    "permissions": capability
                        .permissions
                        .iter()
                        .map(Permission::as_str)
                        .collect::<Vec<_>>(),
                    "granted": granted,
                })
            })
            .collect();
        let granted: Vec<&str> = Permission::ALL
            .into_iter()
            .filter(|p| self.granted(*p))
            .map(|p| p.as_str())
            .collect();
        serde_json::json!({
            "engine": "ktrx",
            "engine_version": ktrx_core::ENGINE_VERSION,
            "api_version": ktrx_core::API_VERSION,
            "os": std::env::consts::OS,
            "family": std::env::consts::FAMILY,
            "arch": std::env::consts::ARCH,
            "cpus": std::thread::available_parallelism().map_or(1, |n| n.get()),
            "permissions": granted,
            "programs": self.allowed_programs(),
            "operations": operations,
        })
        .to_string()
    }

    /// Programs this run may start, when the process permission is granted.
    fn allowed_programs(&self) -> serde_json::Value {
        match &self.approvals.process {
            Some(approval) if self.manifest.declares(Permission::Process) => {
                match effective_programs(&self.manifest, approval) {
                    Some(list) => serde_json::json!(list),
                    None => serde_json::json!("any"),
                }
            }
            _ => serde_json::Value::Null,
        }
    }

    /// Publish and display the programs the tool just started.
    fn announce_processes(&mut self) {
        for started in self.processes.take_started() {
            if std::io::stderr().is_terminal() {
                if self.progress_active {
                    eprintln!();
                }
                let label: String = self
                    .manifest
                    .name
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(48)
                    .collect();
                eprintln!(
                    "{label} ▸ exécute : {}",
                    command_line(&started.program, &started.args)
                );
                self.progress_active = false;
            }
            if let Some(events) = &self.events {
                let _ = events.try_send(RunEvent::Process {
                    pid: started.pid,
                    program: started.program,
                    args: started.args,
                    cwd: started.cwd,
                });
            }
        }
    }
}

/// A one-line, control-character-free rendering of a command for display.
fn command_line(program: &str, args: &[String]) -> String {
    let quote = |part: &str| -> String {
        let clean: String = part
            .chars()
            .map(|c| if c.is_control() { '·' } else { c })
            .collect();
        if clean.is_empty() || clean.contains([' ', '"', '\'']) {
            format!("\"{}\"", clean.replace('"', "\\\""))
        } else {
            clean
        }
    };
    let mut line = quote(program);
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
        if line.len() > 400 {
            let mut end = 400;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
            line.push('…');
            break;
        }
    }
    line
}

impl ktrx::tool::host::Host for HostState {
    fn request(&mut self, operation: String, input: String) -> std::result::Result<String, String> {
        let request: serde_json::Value =
            serde_json::from_str(&input).map_err(|e| format!("invalid request JSON: {e}"))?;
        if operation == "host.info" {
            return Ok(self.host_info());
        }
        if operation == "progress.update" {
            let current = request
                .get("current")
                .and_then(serde_json::Value::as_u64)
                .ok_or("progress update requires current")?;
            let recovered = request
                .get("recovered")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let label = |field: &str, fallback: &str| {
                request
                    .get(field)
                    .and_then(serde_json::Value::as_str)
                    .map(|value| {
                        value
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(32)
                            .collect::<String>()
                    })
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| fallback.to_owned())
            };
            let phase = label("phase", "progression");
            let item_label = label("item_label", "éléments traités");
            let result_label = label("result_label", "résultats");
            let position = request
                .get("total")
                .and_then(serde_json::Value::as_u64)
                .filter(|total| *total > 0)
                .map(|total| format!("{current}/{total}"))
                .unwrap_or_else(|| current.to_string());
            let done = request
                .get("done")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if std::io::stderr().is_terminal() {
                let now = Instant::now();
                let throttled = self
                    .last_progress_at
                    .is_some_and(|last| now.duration_since(last) < Duration::from_millis(100));
                if !throttled || done {
                    let tool_name: String = self
                        .manifest
                        .name
                        .chars()
                        .filter(|character| !character.is_control())
                        .take(48)
                        .collect();
                    eprint!(
                        "\r{} — {} : {} {}, {} {}{}",
                        tool_name,
                        phase,
                        position,
                        item_label,
                        recovered,
                        result_label,
                        if done { " — terminé" } else { "" }
                    );
                    let _ = std::io::Write::flush(&mut std::io::stderr());
                    self.progress_active = true;
                    self.last_progress_at = Some(now);
                }
            }
            if let Some(events) = &self.events {
                let _ = events.try_send(RunEvent::Progress {
                    data: request.clone(),
                });
            }
            return Ok(serde_json::json!({"accepted":true}).to_string());
        }
        let operation = operation.as_str();
        if fs::READ_OPERATIONS.contains(&operation) {
            self.require(Permission::FileRead)?;
            return self.files.request(operation, &request);
        }
        if fs::WRITE_OPERATIONS.contains(&operation) {
            self.require(Permission::FileWrite)?;
            return self.files.request(operation, &request);
        }
        if operation == fs::CLOSE_OPERATION {
            if self.require(Permission::FileRead).is_err() {
                self.require(Permission::FileWrite)?;
            }
            return self.files.request(operation, &request);
        }
        if rawnet::OPERATIONS.contains(&operation) {
            self.require(Permission::RawNet)?;
            return rawnet::request(operation, &request);
        }
        if process::OPERATIONS.contains(&operation) {
            self.require(Permission::Process)?;
            let result = self.processes.request(operation, &request);
            self.announce_processes();
            return result;
        }
        self.require(Permission::Network)?;
        if !network::ALLOWED_OPERATIONS.contains(&operation) {
            return Err(format!("unsupported host capability '{operation}'"));
        }
        self.network.request(operation, &request)
    }
}

impl Runtime {
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true).epoch_interruption(true);
        let engine = Engine::new(&config)?;
        Ok(Self {
            engine,
            limits: StoreLimitsBuilder::new()
                .memory_size(64 * 1024 * 1024)
                .instances(8)
                .memories(4)
                .tables(8)
                .build(),
            components: Mutex::new(HashMap::new()),
        })
    }

    pub fn validate(&self, package: &ToolPackage) -> Result<()> {
        self.component(package)?;
        Ok(())
    }

    pub fn run(
        &self,
        package: &ToolPackage,
        input_json: &str,
        approvals: Approvals,
    ) -> Result<String> {
        self.run_inner(
            package,
            input_json,
            approvals,
            None,
            Arc::new(AtomicBool::new(false)),
        )
    }

    /// Start a run on a worker thread and receive bounded progress events while
    /// it executes. A consumer must call `join` to collect the final JSON result.
    pub fn start_run(
        self: &Arc<Self>,
        package: ToolPackage,
        input_json: String,
        approvals: Approvals,
    ) -> RunSession {
        let (tx, rx) = mpsc::sync_channel(256);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let runtime = Arc::clone(self);
        let worker = std::thread::spawn(move || {
            let result = runtime.run_inner(
                &package,
                &input_json,
                approvals,
                Some(tx.clone()),
                worker_cancelled,
            );
            let event = RunEvent::Finished {
                success: result.is_ok(),
                output: result.as_ref().ok().cloned(),
                error: result.as_ref().err().map(|e| format!("{e:#}")),
            };
            let _ = tx.send(event);
            result
        });
        RunSession {
            events: rx,
            result: worker,
            cancelled,
            engine: self.engine.clone(),
        }
    }

    fn run_inner(
        &self,
        package: &ToolPackage,
        input_json: &str,
        approvals: Approvals,
        events: Option<SyncSender<RunEvent>>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<String> {
        let component = self.component(package)?;
        let files =
            fs::FileState::new(approvals.file_write.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;
        let allowed_programs = approvals
            .process
            .as_ref()
            .and_then(|approval| effective_programs(&package.manifest, approval));
        let show_run_status = std::io::stderr().is_terminal();
        let run_label: String = package
            .manifest
            .name
            .chars()
            .filter(|character| !character.is_control())
            .take(48)
            .collect();
        let mut store = Store::new(
            &self.engine,
            HostState {
                limits: self.limits.clone(),
                wasi: WasiCtx::builder().build(),
                table: ResourceTable::new(),
                manifest: package.manifest.clone(),
                approvals,
                files,
                progress_active: show_run_status,
                last_progress_at: None,
                network: network::NetworkState::new(Arc::clone(&cancelled)),
                processes: process::ProcessState::new(allowed_programs, Arc::clone(&cancelled)),
                events,
            },
        );
        store.limiter(|state| &mut state.limits);
        let execution_timeout = package.manifest.execution_timeout_seconds.unwrap_or(0);
        let unlimited_execution = execution_timeout == 0;
        store.set_epoch_deadline(1);
        let started = Instant::now();
        let deadline = (execution_timeout != 0).then(|| Duration::from_secs(execution_timeout));
        let cancel_check = Arc::clone(&cancelled);
        store.epoch_deadline_callback(move |_| {
            if cancel_check.load(Ordering::Acquire)
                || deadline.is_some_and(|limit| started.elapsed() >= limit)
            {
                Ok(wasmtime::UpdateDeadline::Interrupt)
            } else {
                Ok(wasmtime::UpdateDeadline::Continue(1))
            }
        });
        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        KtrxTool::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
        let bindings = KtrxTool::instantiate(&mut store, &component, &linker).map_err(|e| {
            anyhow::anyhow!("component does not implement the KTRX tool interface: {e}")
        })?;
        if show_run_status {
            eprint!("{run_label} — exécution en cours…");
            let _ = std::io::stderr().flush();
        }
        let timer = {
            let (stop_tx, stop_rx) = std::sync::mpsc::channel();
            let engine = self.engine.clone();
            let timer_cancelled = Arc::clone(&cancelled);
            let timer = std::thread::spawn(move || {
                loop {
                    if stop_rx.recv_timeout(Duration::from_millis(10)).is_ok() {
                        break;
                    }
                    if timer_cancelled.load(Ordering::Acquire)
                        || deadline.is_some_and(|limit| started.elapsed() >= limit)
                    {
                        engine.increment_epoch();
                        break;
                    }
                }
            });
            (stop_tx, timer)
        };
        let result = bindings
            .ktrx_tool_tool()
            .call_run(&mut store, input_json)
            .map_err(|e| {
                if unlimited_execution {
                    anyhow::anyhow!("tool execution failed: {e}")
                } else {
                    anyhow::anyhow!(
                        "tool execution failed ({execution_timeout}-second execution limit): {e}"
                    )
                }
            });
        let _ = timer.0.send(());
        let _ = timer.1.join();
        if store.data().progress_active {
            eprintln!();
        }
        result
    }

    fn component(&self, package: &ToolPackage) -> Result<Arc<Component>> {
        use sha2::{Digest, Sha256};
        let key: [u8; 32] = Sha256::digest(&package.wasm).into();
        let mut cache = self
            .components
            .lock()
            .map_err(|_| anyhow::anyhow!("component cache lock poisoned"))?;
        if let Some(component) = cache.get(&key) {
            return Ok(Arc::clone(component));
        }
        let component =
            Arc::new(Component::new(&self.engine, &package.wasm).map_err(|e| {
                anyhow::anyhow!("invalid or unsupported WebAssembly component: {e}")
            })?);
        cache.insert(key, Arc::clone(&component));
        Ok(component)
    }
}
