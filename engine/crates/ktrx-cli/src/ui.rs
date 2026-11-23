//! `ktrx ui`: a graphical interface served on this computer only.
//!
//! A tiny HTTP server bound to 127.0.0.1 serves one page (embedded in the binary) and a small JSON
//! API. The API adds no capability of its own: installing, updating and removing tools run the very
//! same `ktrx install | update | uninstall` commands, and running a tool runs `ktrx run`, so the
//! engine's rules apply unchanged. In particular a tool only starts once the page has shown the
//! permissions its manifest declares and the user approved them for that run, writes stay inside
//! the chosen directory, and programs stay within the manifest's list.
//!
//! The server defends against other web pages and other local users: it listens on the loopback
//! interface, checks the `Host` and `Origin` headers (DNS rebinding, cross-site requests) and
//! requires a random per-launch token, handed to the browser in the URL fragment (never sent over
//! the network), on every API call.

use anyhow::{Context, Result, bail};
use ktrx_core::{Manifest, Permission, ToolPackage};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    hash::{BuildHasher, Hasher},
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::registry;

const INDEX_HTML: &str = include_str!("ui/index.html");
const APP_CSS: &str = include_str!("ui/app.css");
const APP_JS: &str = include_str!("ui/app.js");
const FAVICON: &str = include_str!("ui/favicon.svg");

const DEFAULT_PORT: u16 = 4747;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_JOB_OUTPUT: usize = 16 * 1024 * 1024;
const MAX_JOBS: usize = 60;
const MAX_PROGRESS_EVENTS: usize = 400;
const CATALOG_TTL: Duration = Duration::from_secs(60);

pub struct Options {
    pub port: Option<u16>,
    pub open: bool,
    pub write_dir: Option<PathBuf>,
    pub registry: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// state

struct Job {
    id: u64,
    tool: String,
    tool_name: String,
    started: Instant,
    started_at_ms: u128,
    state: Mutex<JobState>,
    child: Mutex<Option<Child>>,
}

#[derive(Default)]
struct JobState {
    /// "running", "done", "failed" or "cancelled".
    status: &'static str,
    progress: Vec<Value>,
    processes: Vec<Value>,
    output: Option<String>,
    error: Option<String>,
    elapsed_ms: Option<u128>,
    cancel_requested: bool,
}

struct Server {
    token: String,
    port: u16,
    install_dir: PathBuf,
    default_write_dir: PathBuf,
    registry: Option<String>,
    exe: PathBuf,
    jobs: Mutex<Vec<Arc<Job>>>,
    next_job: AtomicU64,
    write_dirs: Mutex<BTreeSet<PathBuf>>,
    catalog: Mutex<Option<(Instant, Value)>>,
}

pub fn serve(options: Options) -> Result<()> {
    let install_dir = registry::install_dir(None)?;
    let default_write_dir = match options.write_dir {
        Some(dir) => dir,
        None => install_dir
            .parent()
            .map(|p| p.join("workspace"))
            .context("cannot place the workspace directory")?,
    };
    // Fail early on a bad registry address instead of at the first click.
    registry::Registry::new(options.registry.clone())?;
    let listener = match options.port {
        Some(port) => TcpListener::bind(("127.0.0.1", port))
            .with_context(|| format!("cannot listen on 127.0.0.1:{port}"))?,
        None => TcpListener::bind(("127.0.0.1", DEFAULT_PORT))
            .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
            .context("cannot open a local port")?,
    };
    let port = listener.local_addr()?.port();
    let server = Arc::new(Server {
        token: random_token(),
        port,
        install_dir,
        default_write_dir,
        registry: options.registry,
        exe: std::env::current_exe().context("locating the ktrx program")?,
        jobs: Mutex::new(Vec::new()),
        next_job: AtomicU64::new(1),
        write_dirs: Mutex::new(BTreeSet::new()),
        catalog: Mutex::new(None),
    });
    let url = format!("http://127.0.0.1:{port}/#t={}", server.token);
    println!(
        "KTRX {} — interface sur http://127.0.0.1:{port}",
        env!("CARGO_PKG_VERSION")
    );
    println!("{url}");
    println!("Ctrl+C pour arrêter.");
    if options.open && !open_target(&url) {
        println!("(Ouvrez l'adresse ci-dessus dans votre navigateur.)");
    }
    run_listener(listener, server);
    Ok(())
}

fn run_listener(listener: TcpListener, server: Arc<Server>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let server = server.clone();
        std::thread::spawn(move || {
            let _ = handle_connection(&server, stream);
        });
    }
}

fn random_token() -> String {
    // `RandomState` is seeded from the operating system's random source for every instance.
    let mut out = String::new();
    for _ in 0..4 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out
}

fn open_target(target: &str) -> bool {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = Command::new("open");
        c.arg(target);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", target]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut c = Command::new("xdg-open");
        c.arg(target);
        c
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

// ---------------------------------------------------------------------------------------------
// HTTP

struct Request {
    method: String,
    path: String,
    query: HashMap<String, String>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: value.to_string().into_bytes(),
        }
    }
    fn error(status: u16, message: impl Into<String>) -> Self {
        Self::json(status, &json!({"error": message.into()}))
    }
    fn text(content_type: &'static str, body: &str) -> Self {
        Self {
            status: 200,
            content_type,
            body: body.as_bytes().to_vec(),
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        _ => "Internal Server Error",
    }
}

fn handle_connection(server: &Server, mut stream: TcpStream) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let response = match read_request(&mut stream) {
        Ok(request) => route(server, &request),
        Err(error) => Response::error(400, format!("{error:#}")),
    };
    write_response(&mut stream, server, &response)?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn write_response(stream: &mut TcpStream, server: &Server, response: &Response) -> Result<()> {
    // Icons come from the registry (https); everything else from this server only.
    let registry_origin = registry_origin(server.registry.as_deref());
    let headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\n\
         Content-Security-Policy: default-src 'self'; img-src 'self' data: {}; style-src 'self'; script-src 'self'; \
         connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\nConnection: close\r\n\r\n",
        response.status,
        reason(response.status),
        response.content_type,
        response.body.len(),
        registry_origin,
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}

fn registry_origin(registry: Option<&str>) -> String {
    let base = registry
        .map(str::to_owned)
        .or_else(|| std::env::var("KTRX_REGISTRY").ok())
        .unwrap_or_else(|| registry::DEFAULT_REGISTRY.to_owned());
    let base = base.trim_end_matches('/');
    let rest = base.split_once("://").map(|(_, r)| r).unwrap_or(base);
    let scheme = base.split_once("://").map(|(s, _)| s).unwrap_or("https");
    let host = rest.split('/').next().unwrap_or("");
    // Only plain host[:port] characters may end up in a header.
    if host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
        && !host.is_empty()
    {
        format!("{scheme}://{host}")
    } else {
        String::new()
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let n = reader
            .by_ref()
            .take(MAX_HEADER_BYTES as u64)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            bail!("connection closed");
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEADER_BYTES {
            bail!("request headers too large");
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.lines();
    let first = lines.next().context("empty request")?;
    let mut parts = first.split_whitespace();
    let method = parts.next().context("missing method")?.to_ascii_uppercase();
    let target = parts.next().context("missing target")?;
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_owned(), parse_query(q)),
        None => (target.to_owned(), HashMap::new()),
    };
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if length > MAX_BODY_BYTES {
        bail!("request body too large");
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// routing and guards

fn host_allowed(server: &Server, request: &Request) -> bool {
    let allowed = [
        format!("127.0.0.1:{}", server.port),
        format!("localhost:{}", server.port),
    ];
    request
        .headers
        .get("host")
        .is_some_and(|h| allowed.iter().any(|a| a.eq_ignore_ascii_case(h)))
}

fn origin_allowed(server: &Server, request: &Request) -> bool {
    match request.headers.get("origin") {
        None => true,
        Some(origin) => [
            format!("http://127.0.0.1:{}", server.port),
            format!("http://localhost:{}", server.port),
        ]
        .iter()
        .any(|a| a.eq_ignore_ascii_case(origin)),
    }
}

fn token_ok(server: &Server, request: &Request) -> bool {
    let given = request
        .headers
        .get("x-ktrx-token")
        .map(String::as_str)
        .unwrap_or("");
    // Compare every byte so the check does not leak where the first difference is.
    given.len() == server.token.len()
        && given
            .bytes()
            .zip(server.token.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

fn route(server: &Server, request: &Request) -> Response {
    if !host_allowed(server, request) {
        return Response::error(403, "hôte non autorisé");
    }
    if !origin_allowed(server, request) {
        return Response::error(403, "origine non autorisée");
    }
    if !request.path.starts_with("/api/") {
        if request.method != "GET" && request.method != "HEAD" {
            return Response::error(405, "méthode non autorisée");
        }
        return match request.path.as_str() {
            "/" | "/index.html" => Response::text("text/html; charset=utf-8", INDEX_HTML),
            "/app.css" => Response::text("text/css; charset=utf-8", APP_CSS),
            "/app.js" => Response::text("text/javascript; charset=utf-8", APP_JS),
            "/favicon.svg" => Response::text("image/svg+xml", FAVICON),
            _ => Response::error(404, "introuvable"),
        };
    }
    if !token_ok(server, request) {
        return Response::error(401, "jeton invalide : rouvrez l'interface avec `ktrx ui`");
    }
    let is_post = request.method == "POST";
    if is_post {
        let ctype = request
            .headers
            .get("content-type")
            .map(String::as_str)
            .unwrap_or("");
        if !ctype.to_ascii_lowercase().starts_with("application/json") {
            return Response::error(415, "application/json attendu");
        }
    } else if request.method != "GET" {
        return Response::error(405, "méthode non autorisée");
    }
    let body: Value = if is_post && !request.body.is_empty() {
        match serde_json::from_slice(&request.body) {
            Ok(v) => v,
            Err(_) => return Response::error(400, "JSON invalide"),
        }
    } else {
        Value::Null
    };
    let result = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/state") => Ok(state(server)),
        ("GET", "/api/installed") => Ok(installed(server)),
        ("GET", "/api/catalog") => catalog(server, request.query.contains_key("refresh")),
        ("POST", "/api/install") => install(server, &body),
        ("POST", "/api/update") => update(server, &body),
        ("POST", "/api/uninstall") => uninstall(server, &body),
        ("POST", "/api/run") => start_run(server, &body),
        ("POST", "/api/open") => open_path(server, &body),
        ("GET", "/api/fs") => list_dir(
            request.query.get("path").map(String::as_str),
            request.query.contains_key("hidden"),
        ),
        ("GET", p) if p.starts_with("/api/jobs/") => job_status(server, &p["/api/jobs/".len()..]),
        ("POST", p) if p.starts_with("/api/jobs/") && p.ends_with("/cancel") => {
            cancel_job(server, &p["/api/jobs/".len()..p.len() - "/cancel".len()])
        }
        _ => return Response::error(404, "introuvable"),
    };
    match result {
        Ok(value) => Response::json(200, &value),
        Err(error) => Response::error(400, format!("{error:#}")),
    }
}

// ---------------------------------------------------------------------------------------------
// API: state, installed tools, catalog

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn state(server: &Server) -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "install_dir": server.install_dir,
        "write_dir": server.default_write_dir,
        "registry": registry_origin_full(server.registry.as_deref()),
        "home": home_dir(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "separator": std::path::MAIN_SEPARATOR.to_string(),
    })
}

fn registry_origin_full(registry: Option<&str>) -> String {
    registry
        .map(str::to_owned)
        .or_else(|| std::env::var("KTRX_REGISTRY").ok())
        .unwrap_or_else(|| registry::DEFAULT_REGISTRY.to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn manifest_json(manifest: &Manifest) -> Value {
    json!({
        "id": manifest.id,
        "name": manifest.name,
        "version": manifest.version,
        "description": manifest.description,
        "author": manifest.author,
        "permissions": manifest.permissions.iter().map(Permission::as_str).collect::<Vec<_>>(),
        "programs": manifest.programs,
        "min_engine_version": manifest.min_engine_version,
        "timeout_seconds": manifest.execution_timeout_seconds,
        "examples": manifest.examples,
        "arguments": manifest.arguments.iter().map(|a| json!({
            "name": a.name,
            "description": a.description,
            "type": a.value_type,
            "required": a.required,
            "positional": a.positional,
            "default": a.default.as_ref().and_then(|d| serde_json::to_value(d).ok()),
        })).collect::<Vec<_>>(),
    })
}

fn installed(server: &Server) -> Value {
    let tools: Vec<Value> = registry::installed_tools(&server.install_dir)
        .into_iter()
        .map(|(path, package)| {
            let mut value = manifest_json(&package.manifest);
            value["file"] = json!(path.file_name().map(|n| n.to_string_lossy().into_owned()));
            value["bytes"] = json!(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
            value
        })
        .collect();
    json!({ "tools": tools })
}

fn catalog(server: &Server, refresh: bool) -> Result<Value> {
    if !refresh {
        if let Some((at, value)) = &*server.catalog.lock().unwrap() {
            if at.elapsed() < CATALOG_TTL {
                return Ok(value.clone());
            }
        }
    }
    let registry = registry::Registry::new(server.registry.clone())?;
    let value = registry.get_json("/api/tools")?;
    *server.catalog.lock().unwrap() = Some((Instant::now(), value.clone()));
    Ok(value)
}

// ---------------------------------------------------------------------------------------------
// API: install, update, uninstall (the same commands as the terminal)

/// A tool reference as typed in `ktrx install`: `name`, `K~NAME`, `name@1.2.3`.
fn valid_spec(spec: &str) -> bool {
    !spec.is_empty()
        && spec.len() <= 100
        && !spec.starts_with('-')
        && spec
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '~' | '-' | '_' | '@' | '.'))
}

fn run_ktrx(server: &Server, args: &[String], timeout: Duration) -> Result<String> {
    let mut command = Command::new(&server.exe);
    command.args(args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context("starting ktrx")?;
    let mut stdout = child.stdout.take().context("no stdout")?;
    let mut stderr = child.stderr.take().context("no stderr")?;
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("l'opération a dépassé {} secondes", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok(stdout.trim().to_owned())
    } else {
        let message = if stderr.trim().is_empty() {
            stdout.trim().to_owned()
        } else {
            stderr.trim().to_owned()
        };
        bail!("{}", message.trim_start_matches("ktrx: "))
    }
}

fn registry_args(server: &Server) -> Vec<String> {
    match &server.registry {
        Some(url) => vec!["--registry".into(), url.clone()],
        None => vec![],
    }
}

fn invalidate_catalog(server: &Server) {
    *server.catalog.lock().unwrap() = None;
}

fn install(server: &Server, body: &Value) -> Result<Value> {
    let spec = body["spec"].as_str().unwrap_or("").trim();
    if !valid_spec(spec) {
        bail!("nom d'outil invalide");
    }
    let mut args = vec![
        "install".to_owned(),
        spec.to_owned(),
        "--dir".into(),
        server.install_dir.to_string_lossy().into_owned(),
    ];
    if body["force"].as_bool().unwrap_or(false) {
        args.push("--force".into());
    }
    args.extend(registry_args(server));
    let output = run_ktrx(server, &args, Duration::from_secs(180))?;
    invalidate_catalog(server);
    Ok(json!({"ok": true, "output": output}))
}

fn update(server: &Server, body: &Value) -> Result<Value> {
    let mut args = vec!["update".to_owned()];
    if let Some(name) = body["name"].as_str().filter(|n| !n.is_empty()) {
        if !valid_spec(name) || name.contains('@') {
            bail!("nom d'outil invalide");
        }
        args.push(name.to_owned());
    }
    args.extend([
        "--dir".into(),
        server.install_dir.to_string_lossy().into_owned(),
    ]);
    args.extend(registry_args(server));
    let output = run_ktrx(server, &args, Duration::from_secs(600))?;
    invalidate_catalog(server);
    Ok(json!({"ok": true, "output": output}))
}

fn uninstall(server: &Server, body: &Value) -> Result<Value> {
    let name = body["name"].as_str().unwrap_or("").trim();
    if !valid_spec(name) || name.contains('@') {
        bail!("nom d'outil invalide");
    }
    let args = vec![
        "uninstall".to_owned(),
        name.to_owned(),
        "--dir".into(),
        server.install_dir.to_string_lossy().into_owned(),
    ];
    let output = run_ktrx(server, &args, Duration::from_secs(60))?;
    Ok(json!({"ok": true, "output": output}))
}

// ---------------------------------------------------------------------------------------------
// API: running a tool

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The command-line form of the values entered in the form: positionals first, in manifest order,
/// then `--name value` options. Empty fields are left out so defaults apply.
fn build_argv(manifest: &Manifest, values: &Value) -> Result<Vec<String>> {
    let given = match values {
        Value::Object(map) => map.clone(),
        Value::Null => Default::default(),
        _ => bail!("les valeurs doivent être un objet"),
    };
    for key in given.keys() {
        if !manifest.arguments.iter().any(|a| &a.name == key) {
            bail!("argument inconnu : {key}");
        }
    }
    let mut argv = Vec::new();
    // A positional can only be given if every earlier one is, so an empty one that is followed by
    // a filled one is an error rather than a silent shift.
    let positional: Vec<_> = manifest.arguments.iter().filter(|a| a.positional).collect();
    let mut gap: Option<&str> = None;
    for argument in &positional {
        let text = given
            .get(&argument.name)
            .and_then(scalar)
            .filter(|t| !t.is_empty());
        match text {
            Some(text) => {
                if let Some(missing) = gap {
                    bail!("renseignez « {missing} » avant « {} »", argument.name);
                }
                argv.push(text);
            }
            None => gap = gap.or(Some(&argument.name)),
        }
    }
    for argument in manifest.arguments.iter().filter(|a| !a.positional) {
        let Some(value) = given.get(&argument.name) else {
            continue;
        };
        if argument.value_type == "boolean" {
            match value {
                Value::Bool(true) => argv.push(format!("--{}", argument.name)),
                Value::Bool(false) | Value::Null => {}
                _ => bail!("« {} » doit être vrai ou faux", argument.name),
            }
        } else if let Some(text) = scalar(value).filter(|t| !t.is_empty()) {
            // `--name=value` so a value starting with `-` is never taken for an option.
            argv.push(format!("--{}={}", argument.name, text));
        }
    }
    Ok(argv)
}

fn find_tool(server: &Server, name: &str) -> Result<(PathBuf, ToolPackage)> {
    let wanted = registry::normalize(name);
    registry::installed_tools(&server.install_dir)
        .into_iter()
        .find(|(_, p)| registry::normalize(&p.manifest.name) == wanted || p.manifest.id == wanted)
        .with_context(|| format!("« {name} » n'est pas installé"))
}

fn start_run(server: &Server, body: &Value) -> Result<Value> {
    let name = body["tool"].as_str().unwrap_or("");
    if !valid_spec(name) || name.contains('@') {
        bail!("nom d'outil invalide");
    }
    let (path, package) = find_tool(server, name)?;
    let manifest = &package.manifest;
    // The user approves the manifest's permissions for this run, all of them or none, exactly as
    // `ktrx run` does on a terminal.
    let approved: Vec<String> = body["approved"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    for permission in &manifest.permissions {
        if !approved.iter().any(|a| a == permission.as_str()) {
            bail!(
                "la permission « {} » n'a pas été accordée pour cette exécution",
                permission.as_str()
            );
        }
    }
    let argv = build_argv(manifest, &body["values"])?;
    let mut command = Command::new(&server.exe);
    command.arg("run");
    if !manifest.permissions.is_empty() {
        command.arg("-y");
    }
    let write_dir = if manifest.declares(Permission::FileWrite) {
        let dir = match body["write_dir"].as_str().filter(|d| !d.trim().is_empty()) {
            Some(dir) => PathBuf::from(dir.trim()),
            None => server.default_write_dir.clone(),
        };
        if !dir.is_absolute() {
            bail!("le dossier d'écriture doit être un chemin absolu");
        }
        Some(dir)
    } else {
        None
    };
    if let Some(dir) = &write_dir {
        command.arg("--write-dir").arg(dir);
    }
    if manifest.declares(Permission::Process) {
        let chosen: Vec<String> = body["programs"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if manifest.programs.is_empty() {
            // The manifest does not restrict programs: the user must have accepted "any program".
            if !body["any_program"].as_bool().unwrap_or(false) {
                bail!("autorisez explicitement l'exécution de programmes pour lancer cet outil");
            }
        } else {
            if chosen.is_empty() {
                bail!("choisissez au moins un programme que l'outil peut lancer");
            }
            for program in &chosen {
                if !manifest.programs.contains(program)
                    || !ktrx_core::is_valid_program_name(program)
                {
                    bail!("programme non prévu par l'outil : {program}");
                }
                command.arg("--allow-program").arg(program);
            }
        }
    }
    command.arg("--events").arg(&path).args(&argv);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context("démarrage de l'outil")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let stderr = child.stderr.take().context("no stderr")?;
    if let Some(dir) = &write_dir {
        server.write_dirs.lock().unwrap().insert(dir.clone());
    }
    let id = server.next_job.fetch_add(1, Ordering::SeqCst);
    let job = Arc::new(Job {
        id,
        tool: manifest.id.clone(),
        tool_name: manifest.name.clone(),
        started: Instant::now(),
        started_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        state: Mutex::new(JobState {
            status: "running",
            ..Default::default()
        }),
        child: Mutex::new(Some(child)),
    });
    {
        let mut jobs = server.jobs.lock().unwrap();
        jobs.push(job.clone());
        while jobs.len() > MAX_JOBS {
            match jobs
                .iter()
                .position(|j| j.state.lock().unwrap().status != "running")
            {
                Some(i) => {
                    jobs.remove(i);
                }
                None => break,
            }
        }
    }
    let watcher = job.clone();
    std::thread::spawn(move || watch_job(watcher, stdout, stderr));
    Ok(json!({"job": id}))
}

fn watch_job(
    job: Arc<Job>,
    stdout: impl Read + Send + 'static,
    mut stderr: impl Read + Send + 'static,
) {
    let err_thread = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_string(&mut text);
        text
    });
    let mut finished = false;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let mut state = job.state.lock().unwrap();
        match event["type"].as_str() {
            Some("progress") => {
                state.progress.push(event["data"].clone());
                if state.progress.len() > MAX_PROGRESS_EVENTS {
                    state.progress.remove(0);
                }
            }
            Some("process") => state.processes.push(event),
            Some("finished") => {
                finished = true;
                let success = event["success"].as_bool().unwrap_or(false);
                let output = event["output"].as_str().map(str::to_owned);
                if let Some(text) = &output {
                    if text.len() > MAX_JOB_OUTPUT {
                        state.error =
                            Some("le résultat est trop volumineux pour être affiché".into());
                        state.status = "failed";
                        continue;
                    }
                }
                state.output = output;
                state.error = event["error"].as_str().map(str::to_owned);
                state.status = if success { "done" } else { "failed" };
            }
            _ => {}
        }
    }
    let child = job.child.lock().unwrap().take();
    let code = child.map(|mut c| c.wait());
    let stderr_text = err_thread.join().unwrap_or_default();
    let mut state = job.state.lock().unwrap();
    if state.cancel_requested {
        state.status = "cancelled";
    } else if !finished {
        state.status = "failed";
        let message = stderr_text.trim().trim_start_matches("ktrx: ").to_owned();
        state.error = Some(if message.is_empty() {
            match code {
                Some(Ok(status)) => format!("l'outil s'est arrêté ({status})"),
                _ => "l'outil s'est arrêté".into(),
            }
        } else {
            message
        });
    }
    state.elapsed_ms = Some(job.started.elapsed().as_millis());
}

fn job_status(server: &Server, id: &str) -> Result<Value> {
    let id: u64 = id.parse().context("identifiant invalide")?;
    let job = server
        .jobs
        .lock()
        .unwrap()
        .iter()
        .find(|j| j.id == id)
        .cloned()
        .context("exécution introuvable")?;
    let state = job.state.lock().unwrap();
    let elapsed = state
        .elapsed_ms
        .unwrap_or_else(|| job.started.elapsed().as_millis());
    Ok(json!({
        "id": job.id,
        "tool": job.tool,
        "tool_name": job.tool_name,
        "status": state.status,
        "progress": state.progress,
        "processes": state.processes,
        "output": state.output,
        "error": state.error,
        "elapsed_ms": elapsed,
        "started_at": job.started_at_ms,
    }))
}

fn cancel_job(server: &Server, id: &str) -> Result<Value> {
    let id: u64 = id.parse().context("identifiant invalide")?;
    let job = server
        .jobs
        .lock()
        .unwrap()
        .iter()
        .find(|j| j.id == id)
        .cloned()
        .context("exécution introuvable")?;
    {
        let mut state = job.state.lock().unwrap();
        if state.status != "running" {
            return Ok(json!({"ok": true}));
        }
        state.cancel_requested = true;
    }
    let pid = job.child.lock().unwrap().as_ref().map(Child::id);
    if let Some(pid) = pid {
        interrupt(pid);
        // `ktrx run` cancels the tool at its next checkpoint after an interrupt; if it has not
        // stopped a few seconds later, end it.
        let watched = job.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            if let Some(child) = watched.child.lock().unwrap().as_mut() {
                let _ = child.kill();
            }
        });
    }
    Ok(json!({"ok": true}))
}

#[cfg(unix)]
fn interrupt(pid: u32) {
    let _ = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn interrupt(_pid: u32) {
    // No console signal to send: the follow-up kill in `cancel_job` ends the run.
}

// ---------------------------------------------------------------------------------------------
// API: file browser and "show in the file manager"

fn list_dir(path: Option<&str>, hidden: bool) -> Result<Value> {
    let home = home_dir().context("dossier personnel introuvable")?;
    let dir = match path.filter(|p| !p.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => home.clone(),
    };
    if !dir.is_absolute() {
        bail!("chemin absolu attendu");
    }
    let dir = dir
        .canonicalize()
        .with_context(|| format!("dossier inaccessible : {}", dir.display()))?;
    let mut entries: Vec<Value> = Vec::new();
    let mut truncated = false;
    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("lecture impossible : {}", dir.display()))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !hidden && name.starts_with('.') {
            continue;
        }
        let Ok(meta) = std::fs::metadata(entry.path()) else {
            continue;
        };
        if entries.len() >= 3000 {
            truncated = true;
            break;
        }
        entries.push(json!({"name": name, "dir": meta.is_dir(), "size": if meta.is_dir() { 0 } else { meta.len() }}));
    }
    entries.sort_by(|a, b| {
        b["dir"].as_bool().cmp(&a["dir"].as_bool()).then_with(|| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
        })
    });
    Ok(json!({
        "path": dir,
        "parent": dir.parent(),
        "home": home,
        "entries": entries,
        "truncated": truncated,
    }))
}

fn open_path(server: &Server, body: &Value) -> Result<Value> {
    let requested = PathBuf::from(body["path"].as_str().unwrap_or(""));
    let path = requested
        .canonicalize()
        .with_context(|| format!("introuvable : {}", requested.display()))?;
    // Only places a tool was allowed to write into (or the default workspace) may be shown.
    let dirs = server.write_dirs.lock().unwrap();
    let allowed = dirs
        .iter()
        .chain(std::iter::once(&server.default_write_dir))
        .filter_map(|d| d.canonicalize().ok())
        .any(|d| path.starts_with(d));
    if !allowed {
        bail!("ce dossier n'est pas un dossier d'écriture de cette session");
    }
    if !open_target(&path.to_string_lossy()) {
        bail!("impossible d'ouvrir le gestionnaire de fichiers");
    }
    Ok(json!({"ok": true}))
}

#[allow(dead_code)]
fn local_addr(listener: &TcpListener) -> Option<SocketAddr> {
    listener.local_addr().ok()
}

// ---------------------------------------------------------------------------------------------
// tests

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(toml: &str) -> Manifest {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn query_strings_are_decoded() {
        let q = parse_query("path=%2FUsers%2Fa%20b&hidden&x=a+b%zz");
        assert_eq!(q["path"], "/Users/a b");
        assert!(q.contains_key("hidden"));
        assert_eq!(q["x"], "a b%zz");
    }

    #[test]
    fn specs_cannot_be_options_or_paths() {
        for bad in ["", "-y", "../x", "a b", "x;y", "/etc/passwd", "a\\b"] {
            assert!(!valid_spec(bad), "{bad:?}");
        }
        for good in ["nyx", "K~NYX", "uuid-forge", "nyx@1.0.2"] {
            assert!(valid_spec(good), "{good:?}");
        }
    }

    #[test]
    fn form_values_become_arguments_in_manifest_order() {
        let m = manifest(
            "id='demo'\nname='K~DEMO'\ndescription='d'\nversion='1.0.0'\nauthor='a'\napi_version='1'\n\
             [[arguments]]\nname='source'\ndescription='s'\nvalue_type='path'\nrequired=true\npositional=true\n\
             [[arguments]]\nname='second'\ndescription='s'\npositional=true\n\
             [[arguments]]\nname='target'\ndescription='t'\n\
             [[arguments]]\nname='force'\ndescription='f'\nvalue_type='boolean'\n\
             [[arguments]]\nname='count'\ndescription='c'\nvalue_type='integer'\n",
        );
        let argv = build_argv(
            &m,
            &json!({"count": 3, "force": true, "target": "-x all", "source": "a.c", "second": ""}),
        )
        .unwrap();
        assert_eq!(argv, ["a.c", "--target=-x all", "--force", "--count=3"]);
        assert!(
            build_argv(&m, &json!({"force": false, "target": ""}))
                .unwrap()
                .is_empty()
        );
        assert!(build_argv(&m, &json!({"nope": 1})).is_err());
        assert!(
            build_argv(&m, &json!({"second": "b"})).is_err(),
            "a positional cannot skip an empty one"
        );
        assert!(build_argv(&m, &json!([1])).is_err());
    }

    fn test_server(dir: &std::path::Path) -> Arc<Server> {
        Arc::new(Server {
            token: "secret".into(),
            port: 0,
            install_dir: dir.join("tools"),
            default_write_dir: dir.join("work"),
            registry: None,
            exe: PathBuf::from("ktrx-does-not-exist"),
            jobs: Mutex::new(Vec::new()),
            next_job: AtomicU64::new(1),
            write_dirs: Mutex::new(BTreeSet::new()),
            catalog: Mutex::new(None),
        })
    }

    /// Starts the server on a free port; returns the shared state with the real port filled in.
    fn start(dir: &std::path::Path) -> (Arc<Server>, u16) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut server = test_server(dir);
        Arc::get_mut(&mut server).unwrap().port = port;
        let shared = server.clone();
        std::thread::spawn(move || run_listener(listener, server));
        (shared, port)
    }

    fn call(
        port: u16,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
            head.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
        }
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(body.as_bytes()).unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        let status = raw.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = raw
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_owned())
            .unwrap_or_default();
        (status, body)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ktrx-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn serves_the_page_and_guards_the_api() {
        let dir = temp_dir("guards");
        let (_, port) = start(&dir);
        let (status, page) = call(port, "GET", "/", &[], "");
        assert_eq!(status, 200);
        assert!(page.contains("<title>KTRX"));
        assert_eq!(call(port, "GET", "/app.js", &[], "").0, 200);
        assert_eq!(call(port, "GET", "/nothing", &[], "").0, 404);
        // no token, wrong token
        assert_eq!(call(port, "GET", "/api/state", &[], "").0, 401);
        assert_eq!(
            call(port, "GET", "/api/state", &[("X-KTRX-Token", "nope")], "").0,
            401
        );
        // wrong Host (DNS rebinding) or Origin (another web page) is refused even with the token
        let token = [("X-KTRX-Token", "secret")];
        assert_eq!(
            call(
                port,
                "GET",
                "/api/state",
                &[("Host", "evil.example"), token[0]],
                ""
            )
            .0,
            403
        );
        assert_eq!(
            call(
                port,
                "GET",
                "/api/state",
                &[("Origin", "https://evil.example"), token[0]],
                ""
            )
            .0,
            403
        );
        // writes need JSON, not a form post a page could forge
        assert_eq!(
            call(
                port,
                "POST",
                "/api/uninstall",
                &[token[0], ("Content-Type", "text/plain")],
                "{}"
            )
            .0,
            415
        );
        let (status, body) = call(port, "GET", "/api/state", &token, "");
        assert_eq!(status, 200);
        let state: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(state["version"], env!("CARGO_PKG_VERSION"));
        let (_, listing) = call(port, "GET", "/api/installed", &token, "");
        assert_eq!(
            serde_json::from_str::<Value>(&listing).unwrap()["tools"],
            json!([])
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn write_tool(dir: &std::path::Path, permissions: &str) {
        std::fs::create_dir_all(dir.join("tools")).unwrap();
        let file = std::fs::File::create(dir.join("tools").join("K~HELLO.tool")).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.toml", options).unwrap();
        zip.write_all(
            format!(
                "id='hello-tool'\nname='K~HELLO'\ndescription='Fixture'\nversion='1.0.0'\nauthor='KTRX'\napi_version='1'\npermissions={permissions}\n\
                 [[arguments]]\nname='name'\ndescription='Who'\nrequired=true\npositional=true\n"
            )
            .as_bytes(),
        )
        .unwrap();
        zip.start_file("module.wasm", options).unwrap();
        zip.write_all(include_bytes!(
            "../../../tests/fixtures/hello-tool.component.wasm"
        ))
        .unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn lists_installed_tools_and_refuses_unapproved_runs() {
        let dir = temp_dir("run");
        write_tool(&dir, "['file-write']");
        let (_, port) = start(&dir);
        let token = [
            ("X-KTRX-Token", "secret"),
            ("Content-Type", "application/json"),
        ];
        let (_, body) = call(port, "GET", "/api/installed", &token, "");
        let tools = serde_json::from_str::<Value>(&body).unwrap();
        assert_eq!(tools["tools"][0]["name"], "K~HELLO");
        assert_eq!(tools["tools"][0]["permissions"], json!(["file-write"]));
        assert_eq!(tools["tools"][0]["arguments"][0]["positional"], true);
        // the manifest declares file-write: running without approving it is refused
        let (status, body) = call(
            port,
            "POST",
            "/api/run",
            &token,
            r#"{"tool":"hello","values":{"name":"Ada"},"approved":[]}"#,
        );
        assert_eq!(status, 400);
        assert!(body.contains("file-write"), "{body}");
        // names that are not installed tools, or look like paths, are refused
        assert_eq!(
            call(
                port,
                "POST",
                "/api/run",
                &token,
                r#"{"tool":"../x","approved":[]}"#
            )
            .0,
            400
        );
        assert_eq!(
            call(
                port,
                "POST",
                "/api/run",
                &token,
                r#"{"tool":"ghost","approved":[]}"#
            )
            .0,
            400
        );
        assert_eq!(
            call(
                port,
                "POST",
                "/api/install",
                &token,
                r#"{"spec":"--force"}"#
            )
            .0,
            400
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn runs_a_tool_through_ktrx_run_when_approved() {
        // The server starts `ktrx run`; use the real binary built for this test run.
        let Some(exe) = option_env!("CARGO_BIN_EXE_ktrx") else {
            return;
        };
        let dir = temp_dir("exec");
        write_tool(&dir, "[]");
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut server = test_server(&dir);
        {
            let s = Arc::get_mut(&mut server).unwrap();
            s.port = port;
            s.exe = PathBuf::from(exe);
        }
        let shared = server.clone();
        std::thread::spawn(move || run_listener(listener, server));
        let token = [
            ("X-KTRX-Token", "secret"),
            ("Content-Type", "application/json"),
        ];
        let (status, body) = call(
            port,
            "POST",
            "/api/run",
            &token,
            r#"{"tool":"hello","values":{"name":"Ada"},"approved":[]}"#,
        );
        assert_eq!(status, 200, "{body}");
        let job = serde_json::from_str::<Value>(&body).unwrap()["job"]
            .as_u64()
            .unwrap();
        let mut result = Value::Null;
        for _ in 0..200 {
            let (_, text) = call(port, "GET", &format!("/api/jobs/{job}"), &token, "");
            result = serde_json::from_str(&text).unwrap();
            if result["status"] != "running" {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(result["status"], "done", "{result}");
        let output: Value = serde_json::from_str(result["output"].as_str().unwrap()).unwrap();
        assert_eq!(output["message"], "Hello, Ada!");
        assert_eq!(shared.jobs.lock().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_file_browser_lists_directories_first_and_hides_dotfiles() {
        let dir = temp_dir("fs");
        std::fs::create_dir_all(dir.join("b-dir")).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        std::fs::write(dir.join(".hidden"), "x").unwrap();
        let listing = list_dir(Some(dir.to_str().unwrap()), false).unwrap();
        let names: Vec<_> = listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["b-dir", "a.txt"]);
        let all = list_dir(Some(dir.to_str().unwrap()), true).unwrap();
        assert_eq!(all["entries"].as_array().unwrap().len(), 3);
        assert!(list_dir(Some("relative/path"), false).is_err());
        assert!(list_dir(Some(dir.join("missing").to_str().unwrap()), false).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tokens_are_long_and_different_each_time() {
        let (a, b) = (random_token(), random_token());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}
