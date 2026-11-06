use anyhow::Result;
use ktrx_core::{Manifest, Permission, ToolPackage};
use std::{
    collections::HashMap,
    io::{IsTerminal, Read, Write},
    net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

wasmtime::component::bindgen!({ path: "../../wit", world: "ktrx-tool" });

pub struct Runtime {
    engine: Engine,
    limits: StoreLimits,
    components: Mutex<HashMap<[u8; 32], Arc<Component>>>,
    network: Arc<NetworkState>,
}

struct NetworkState {
    dns_cache: Mutex<HashMap<(String, u16), (Instant, Vec<IpAddr>)>>,
    clients: Mutex<HashMap<(String, u16, Vec<IpAddr>), reqwest::blocking::Client>>,
    gate: Mutex<()>,
}

impl NetworkState {
    fn resolve(&self, host: &str, port: u16) -> std::result::Result<Vec<IpAddr>, String> {
        let key = (host.to_ascii_lowercase(), port);
        if let Some((stored, addresses)) = self
            .dns_cache
            .lock()
            .map_err(|_| "DNS cache lock poisoned")?
            .get(&key)
        {
            if stored.elapsed() < Duration::from_secs(60) {
                return Ok(addresses.clone());
            }
        }
        let addresses: Vec<IpAddr> = (host, port)
            .to_socket_addrs()
            .map_err(|e| format!("DNS lookup failed: {e}"))?
            .map(|a| a.ip())
            .collect();
        if addresses.is_empty() {
            return Err("hostname did not resolve to any address".into());
        }
        let mut addresses = addresses;
        addresses.sort();
        addresses.dedup();
        let mut cache = self
            .dns_cache
            .lock()
            .map_err(|_| "DNS cache lock poisoned")?;
        cache.retain(|_, (created, _)| created.elapsed() < Duration::from_secs(60));
        if cache.len() >= 256 {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (created, _))| *created)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(key, (Instant::now(), addresses.clone()));
        Ok(addresses)
    }

    fn client(
        &self,
        host: &str,
        port: u16,
        addresses: &[IpAddr],
    ) -> std::result::Result<reqwest::blocking::Client, String> {
        let mut sorted = addresses.to_vec();
        sorted.sort();
        sorted.dedup();
        let key = (host.to_ascii_lowercase(), port, sorted);
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| "HTTP client pool lock poisoned")?;
        if let Some(client) = clients.get(&key) {
            return Ok(client.clone());
        }
        if clients.len() >= 32 {
            return Err("network client pool has reached its 32-host limit".into());
        }
        let sockets: Vec<_> = addresses
            .iter()
            .map(|ip| std::net::SocketAddr::new(*ip, port))
            .collect();
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(4)
            .resolve_to_addrs(host, &sockets)
            .build()
            .map_err(|e| e.to_string())?;
        clients.insert(key, client.clone());
        Ok(client)
    }
}

struct HostState {
    limits: StoreLimits,
    wasi: WasiCtx,
    table: ResourceTable,
    manifest: Manifest,
    network_approved: bool,
    file_read_approved: bool,
    open_files: HashMap<String, std::fs::File>,
    progress_active: bool,
    last_progress_at: Option<Instant>,
    network: Arc<NetworkState>,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl ktrx::tool::host::Host for HostState {
    fn request(&mut self, operation: String, input: String) -> std::result::Result<String, String> {
        let request: serde_json::Value =
            serde_json::from_str(&input).map_err(|e| format!("invalid request JSON: {e}"))?;
        if operation == "progress.update" {
            let current = request
                .get("current")
                .and_then(serde_json::Value::as_u64)
                .ok_or("progress update requires current")?;
            let recovered = request
                .get("recovered")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let phase = match request.get("phase").and_then(serde_json::Value::as_str) {
                Some("wordlist") => "dictionnaire",
                Some("bruteforce") => "recherche exhaustive",
                Some("fingerprint") => "empreinte web",
                _ => "exécution",
            };
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
            return Ok(serde_json::json!({"accepted":true}).to_string());
        }
        if operation == "file.read" {
            if !self.file_read_approved
                || !self
                    .manifest
                    .permissions
                    .iter()
                    .any(|p| matches!(p, Permission::FileRead))
            {
                return Err("file-read permission was not granted for this run".into());
            }
            let path = request
                .get("path")
                .and_then(|x| x.as_str())
                .ok_or("request requires path")?;
            let offset = request
                .get("offset")
                .and_then(|x| x.as_u64())
                .ok_or("request requires offset")?;
            let length = request
                .get("length")
                .and_then(|x| x.as_u64())
                .ok_or("request requires length")?;
            if length == 0 || length > 64 * 1024 {
                return Err("file read chunks must be 1-65536 bytes".into());
            }
            if !self.open_files.contains_key(path) {
                if self.open_files.len() >= 8 {
                    return Err("a tool may open at most eight file paths".into());
                }
                let canonical =
                    std::fs::canonicalize(path).map_err(|e| format!("cannot open file: {e}"))?;
                let file = std::fs::File::open(&canonical)
                    .map_err(|e| format!("cannot open file: {e}"))?;
                let metadata = file.metadata().map_err(|e| e.to_string())?;
                if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
                    return Err("file must be regular and no larger than 64 MiB".into());
                }
                self.open_files.insert(path.to_owned(), file);
            }
            let file = self
                .open_files
                .get_mut(path)
                .ok_or("file handle unavailable")?;
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            let mut bytes = Vec::with_capacity(length as usize);
            file.take(length)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            use base64::Engine as _;
            return Ok(serde_json::json!({"path":path,"offset":offset,"bytes_base64":base64::engine::general_purpose::STANDARD.encode(bytes)}).to_string());
        }
        if !self.network_approved
            || !self
                .manifest
                .permissions
                .iter()
                .any(|p| matches!(p, Permission::Network))
        {
            return Err("network permission was not granted for this run".into());
        }
        let host = request
            .get("host")
            .and_then(|x| x.as_str())
            .map(str::to_owned)
            .or_else(|| {
                request
                    .get("url")
                    .and_then(|v| v.as_str())
                    .and_then(url_host)
            })
            .ok_or("request must include host or url")?;
        match operation.as_str() {
            "dns.lookup" => {
                let _guard = self
                    .network
                    .gate
                    .lock()
                    .map_err(|_| "network gate lock poisoned")?;
                let port = match request.get("port").and_then(|v| v.as_u64()) {
                    Some(value) => {
                        u16::try_from(value).map_err(|_| "port must be between 1 and 65535")?
                    }
                    None => 443,
                };
                if port == 0 {
                    return Err("port must be between 1 and 65535".into());
                }
                let addresses = self.network.resolve(&host, port)?;
                Ok(serde_json::json!({"host":host,"addresses":addresses}).to_string())
            }
            "tcp.scan" => {
                let _guard = self
                    .network
                    .gate
                    .lock()
                    .map_err(|_| "network gate lock poisoned")?;
                let ports = request
                    .get("ports")
                    .and_then(|v| v.as_array())
                    .ok_or("request requires a ports array")?
                    .iter()
                    .map(|v| {
                        let port = v.as_u64().ok_or("ports must be integers")?;
                        let port = u16::try_from(port).map_err(|_| "ports must be 1-65535")?;
                        if port == 0 {
                            return Err("ports must be 1-65535");
                        }
                        Ok(port)
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                if ports.is_empty() || ports.len() > 65_535 {
                    return Err("scan must contain 1-65535 ports".into());
                }
                let mut unique = ports.clone();
                unique.sort_unstable();
                unique.dedup();
                if unique.len() != ports.len() {
                    return Err("ports must not contain duplicates".into());
                }
                let timeout_ms = request
                    .get("timeout_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(250)
                    .clamp(50, 2_000);
                let concurrency = request
                    .get("concurrency")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(128)
                    .clamp(1, 128) as usize;
                let detect_services = request
                    .get("service_detection")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let resolved = self.network.resolve(&host, 443)?;
                let target = *resolved
                    .first()
                    .ok_or("hostname did not resolve to a public address")?;
                let (results, complete) = scan_tcp_ports(
                    &host,
                    target,
                    &ports,
                    Duration::from_millis(timeout_ms),
                    concurrency.min(ports.len()),
                    detect_services,
                );
                let open = results.iter().filter(|r| r.state == "open").count();
                let closed = results.iter().filter(|r| r.state == "closed").count();
                let filtered = results.iter().filter(|r| r.state == "filtered").count();
                Ok(serde_json::json!({
                    "host": host,
                    "target_address": target,
                    "ports_scanned": results.len(),
                    "ports_not_scanned": ports.len() - results.len(),
                    "complete": complete,
                    "open": open,
                    "closed": closed,
                    "filtered": filtered,
                    "method": "TCP connect",
                    "service_fingerprint_limit": if detect_services { 1024 } else { 0 },
                    "results": results,
                })
                .to_string())
            }
            "http.get" | "tls.inspect" => {
                let _guard = self
                    .network
                    .gate
                    .lock()
                    .map_err(|_| "network gate lock poisoned")?;
                let url = request
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or("request requires url")?;
                let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    return Err("only http and https destinations are supported".into());
                }
                if operation == "tls.inspect" && parsed.scheme() != "https" {
                    return Err("TLS inspection requires an https URL".into());
                }
                let port = parsed
                    .port_or_known_default()
                    .ok_or("URL must include a valid port")?;
                let resolved = self.network.resolve(&host, port)?;
                let client = self.network.client(&host, port, &resolved)?;
                let response = client
                    .get(parsed.clone())
                    .send()
                    .map_err(|e| format!("request failed: {e}"))?;
                let status = response.status().as_u16();
                let headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("<binary>").to_owned()))
                    .collect();
                if operation == "tls.inspect" {
                    return Ok(serde_json::json!({"host":host,"verified":true,"status":status,"headers":headers,"note":"TLS certificate chain and hostname were verified by rustls; certificate fields are not exposed in this API version."}).to_string());
                }
                let mut body = Vec::new();
                response
                    .take(1_048_577)
                    .read_to_end(&mut body)
                    .map_err(|e| e.to_string())?;
                if body.len() > 1_048_576 {
                    return Err("response body exceeds 1 MiB limit".into());
                }
                let mut result = serde_json::json!({
                    "url":url,
                    "status":status,
                    "headers":headers,
                    "body_bytes":body.len()
                });
                if request
                    .get("include_body")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
                {
                    use base64::Engine as _;
                    let sample_limit = request
                        .get("max_body_bytes")
                        .and_then(|value| value.as_u64())
                        .unwrap_or(262_144)
                        .clamp(1, 262_144) as usize;
                    let sample_len = body.len().min(sample_limit);
                    result["body_base64"] = serde_json::Value::String(
                        base64::engine::general_purpose::STANDARD.encode(&body[..sample_len]),
                    );
                    result["body_sampled_bytes"] = serde_json::Value::Number(sample_len.into());
                    result["body_truncated"] = serde_json::Value::Bool(sample_len < body.len());
                }
                Ok(result.to_string())
            }
            other => Err(format!("unsupported host operation '{other}'")),
        }
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
            network: Arc::new(NetworkState {
                dns_cache: Mutex::new(HashMap::new()),
                clients: Mutex::new(HashMap::new()),
                gate: Mutex::new(()),
            }),
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
        network_approved: bool,
        file_read_approved: bool,
    ) -> Result<String> {
        let component = self.component(package)?;
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
                network_approved,
                file_read_approved,
                open_files: HashMap::new(),
                progress_active: show_run_status,
                last_progress_at: None,
                network: Arc::clone(&self.network),
            },
        );
        store.limiter(|state| &mut state.limits);
        let execution_timeout = package.manifest.execution_timeout_seconds.unwrap_or(0);
        let unlimited_execution = execution_timeout == 0;
        if unlimited_execution {
            // Epoch interruption is enabled for this engine; an unlimited tool still needs a future deadline.
            store.set_epoch_deadline(u64::MAX);
        } else {
            store.set_epoch_deadline(1);
        }
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
        let timer = if unlimited_execution {
            None
        } else {
            let (stop_tx, stop_rx) = std::sync::mpsc::channel();
            let engine = self.engine.clone();
            let timer = std::thread::spawn(move || {
                if stop_rx
                    .recv_timeout(Duration::from_secs(execution_timeout))
                    .is_err()
                {
                    engine.increment_epoch();
                }
            });
            Some((stop_tx, timer))
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
        if let Some((stop_tx, timer)) = timer {
            let _ = stop_tx.send(());
            let _ = timer.join();
        }
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

#[derive(serde::Serialize)]
struct TcpPortResult {
    port: u16,
    address: IpAddr,
    state: &'static str,
    rtt_ms: Option<u64>,
    service_guess: &'static str,
    service: Option<String>,
    product: Option<String>,
    banner: Option<String>,
}

fn scan_tcp_ports(
    host: &str,
    address: IpAddr,
    ports: &[u16],
    timeout: Duration,
    concurrency: usize,
    detect_services: bool,
) -> (Vec<TcpPortResult>, bool) {
    let (job_tx, job_rx) = mpsc::channel::<u16>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let (result_tx, result_rx) = mpsc::channel();
    let workers = concurrency.clamp(1, 128).min(ports.len());
    let fingerprint_budget = Arc::new(AtomicUsize::new(1_024));
    let mut results = Vec::with_capacity(ports.len());
    let mut open_count = 0usize;
    let mut last_progress = Instant::now();
    let mut progress_active = false;
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let jobs = Arc::clone(&job_rx);
            let output = result_tx.clone();
            let remaining_fingerprints = Arc::clone(&fingerprint_budget);
            scope.spawn(move || {
                loop {
                    let port = match jobs.lock().ok().and_then(|rx| rx.recv().ok()) {
                        Some(port) => port,
                        None => break,
                    };
                    let started = Instant::now();
                    let socket = SocketAddr::new(address, port);
                    let result = match TcpStream::connect_timeout(&socket, timeout) {
                        Ok(stream) => {
                            let rtt = started.elapsed().as_millis() as u64;
                            let should_fingerprint = detect_services
                                && remaining_fingerprints
                                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                                        left.checked_sub(1)
                                    })
                                    .is_ok();
                            let (service, product, banner) = if should_fingerprint {
                                fingerprint_tcp_service(stream, host, port)
                            } else {
                                (None, None, None)
                            };
                            TcpPortResult {
                                port,
                                address,
                                state: "open",
                                rtt_ms: Some(rtt),
                                service_guess: service_guess(port),
                                service,
                                product,
                                banner,
                            }
                        }
                        Err(error) => {
                            let state = match error.kind() {
                                std::io::ErrorKind::ConnectionRefused => "closed",
                                std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::HostUnreachable
                                | std::io::ErrorKind::NetworkUnreachable
                                | std::io::ErrorKind::PermissionDenied => "filtered",
                                _ => "filtered",
                            };
                            TcpPortResult {
                                port,
                                address,
                                state,
                                rtt_ms: Some(started.elapsed().as_millis() as u64),
                                service_guess: service_guess(port),
                                service: None,
                                product: None,
                                banner: None,
                            }
                        }
                    };
                    let _ = output.send(result);
                }
            });
        }
        drop(result_tx);
        for port in ports {
            let _ = job_tx.send(*port);
        }
        drop(job_tx);
        while let Ok(result) = result_rx.recv() {
            if result.state == "open" {
                open_count += 1;
            }
            results.push(result);
            let scanned = results.len();
            let should_report = scanned == ports.len()
                || (scanned % 100 == 0 && last_progress.elapsed() >= Duration::from_millis(100));
            if should_report && std::io::stderr().is_terminal() {
                eprint!(
                    "\rPort Explorer — scan TCP : {scanned}/{} ports, {open_count} ouverts",
                    ports.len()
                );
                let _ = std::io::stderr().flush();
                progress_active = true;
                last_progress = Instant::now();
            }
        }
    });
    if progress_active {
        eprintln!();
    }
    results.sort_by_key(|result| result.port);
    let complete = results.len() == ports.len();
    (results, complete)
}

fn fingerprint_tcp_service(
    mut stream: TcpStream,
    host: &str,
    port: u16,
) -> (Option<String>, Option<String>, Option<String>) {
    use std::io::ErrorKind;
    let service = service_guess(port);
    let timeout = Duration::from_millis(250);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let http_ports = [80, 3000, 5000, 7001, 8000, 8008, 8080, 8081, 8888, 9000];
    let probe = if http_ports.contains(&port) || service == "unknown" {
        Some(format!(
            "HEAD / HTTP/1.0\r\nHost: {host}\r\nUser-Agent: KTRX-Port-Explorer/1.0\r\nConnection: close\r\n\r\n"
        ))
    } else if port == 6379 {
        Some("PING\r\n".to_owned())
    } else {
        None
    };
    if let Some(probe) = probe {
        if stream.write_all(probe.as_bytes()).is_err() {
            return (None, None, None);
        }
    } else if !matches!(port, 21 | 22 | 25 | 110 | 143 | 3306 | 587) {
        return (None, None, None);
    }

    let mut bytes = [0; 4096];
    let count = match stream.read(&mut bytes) {
        Ok(0) => return (None, None, None),
        Ok(count) => count,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
            ) =>
        {
            return (None, None, None);
        }
        Err(_) => return (None, None, None),
    };
    let raw = String::from_utf8_lossy(&bytes[..count]);
    let clean = raw
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\r' | '\n' | '\t'))
        .take(512)
        .collect::<String>()
        .trim()
        .to_owned();
    if clean.is_empty() {
        return (None, None, None);
    }
    if http_ports.contains(&port) || service == "unknown" {
        let first = clean.lines().next().unwrap_or_default().to_owned();
        if !first.starts_with("HTTP/") {
            return (None, Some(first), Some(clean));
        }
        let server = clean.lines().find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("server")
                    .then(|| value.trim().to_owned())
            })
        });
        return (Some("http".into()), server.or(Some(first)), Some(clean));
    }
    let first = clean.lines().next().unwrap_or_default().to_owned();
    let detected = match port {
        21 => first.starts_with("220 ").then_some("ftp"),
        22 => first.starts_with("SSH-").then_some("ssh"),
        25 | 587 => first.starts_with("220 ").then_some("smtp"),
        110 => first.starts_with("+OK").then_some("pop3"),
        143 => first
            .to_ascii_uppercase()
            .starts_with("* OK")
            .then_some("imap"),
        3306 => (bytes.get(4) == Some(&0x0a)).then_some("mysql"),
        6379 => first.starts_with("+PONG").then_some("redis"),
        _ => None,
    };
    (
        detected.map(str::to_owned),
        detected.map(|_| first),
        Some(clean),
    )
}

fn service_guess(port: u16) -> &'static str {
    match port {
        20 => "ftp-data",
        21 => "ftp",
        22 => "ssh",
        23 => "telnet",
        25 | 587 => "smtp",
        53 => "domain",
        80 | 3000 | 5000 | 7001 | 8000 | 8008 | 8080 | 8081 | 8888 | 9000 => "http",
        110 => "pop3",
        143 => "imap",
        443 => "https",
        445 => "microsoft-ds",
        3306 => "mysql",
        3389 => "ms-wbt-server",
        5432 => "postgresql",
        5900 => "vnc",
        6379 => "redis",
        8443 => "https-alt",
        _ => "unknown",
    }
}

fn url_host(url: &str) -> Option<String> {
    reqwest::Url::parse(url).ok()?.host_str().map(str::to_owned)
}
#[cfg(test)]
mod tcp_scan_tests {
    use super::*;

    #[test]
    fn finds_open_and_closed_tcp_ports_and_reads_http_banner() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let open_port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 256];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.0 200 OK\r\nServer: KTRX-Test/1.0\r\n\r\n")
                .unwrap();
        });
        let reservation = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let closed_port = reservation.local_addr().unwrap().port();
        drop(reservation);

        let (results, complete) = scan_tcp_ports(
            "localhost",
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            &[open_port, closed_port],
            Duration::from_millis(500),
            2,
            true,
        );
        server.join().unwrap();
        assert!(complete);
        let open = results.iter().find(|r| r.port == open_port).unwrap();
        let closed = results.iter().find(|r| r.port == closed_port).unwrap();
        assert_eq!(open.state, "open");
        assert_eq!(open.service.as_deref(), Some("http"));
        assert!(open.banner.as_deref().unwrap().contains("200 OK"));
        assert_eq!(closed.state, "closed");
    }
}
