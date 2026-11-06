use anyhow::Result;
use ktrx_core::{Manifest, Permission, ToolPackage};
mod network;
use std::{
    collections::HashMap,
    io::{IsTerminal, Read, Write},
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
    Finished {
        success: bool,
        output: Option<String>,
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
    network_approved: bool,
    file_read_approved: bool,
    open_files: HashMap<String, std::fs::File>,
    progress_active: bool,
    last_progress_at: Option<Instant>,
    network: network::NetworkState,
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
        if !network::ALLOWED_OPERATIONS.contains(&operation.as_str()) {
            return Err(format!("unsupported host capability '{operation}'"));
        }
        self.network.request(&operation, &request)
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
        network_approved: bool,
        file_read_approved: bool,
    ) -> Result<String> {
        self.run_inner(
            package,
            input_json,
            network_approved,
            file_read_approved,
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
        network_approved: bool,
        file_read_approved: bool,
    ) -> RunSession {
        let (tx, rx) = mpsc::sync_channel(256);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let runtime = Arc::clone(self);
        let worker = std::thread::spawn(move || {
            let result = runtime.run_inner(
                &package,
                &input_json,
                network_approved,
                file_read_approved,
                Some(tx.clone()),
                worker_cancelled,
            );
            let event = RunEvent::Finished {
                success: result.is_ok(),
                output: result.as_ref().ok().cloned(),
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
        network_approved: bool,
        file_read_approved: bool,
        events: Option<SyncSender<RunEvent>>,
        cancelled: Arc<AtomicBool>,
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
                network: network::NetworkState::default(),
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
