//! Generic, permission-gated socket primitives exposed to sandboxed tools.
//! Protocols and scanning strategies belong in guest tools, not in this module.
//!
//! Failures that a tool may want to branch on carry a stable, locale-independent
//! kind: either an `error_kind` field (connection results from `socket.poll`
//! and `socket.wait`) or a `[kind]` prefix on the error string (every other
//! call). See [`error_kind`] for the vocabulary.

use crate::{dns, tls};
use openssl::ssl::{SslConnector, SslStream};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) const ALLOWED_OPERATIONS: [&str; 15] = [
    "network.resolve",
    "dns.query",
    "socket.open",
    "socket.connect",
    "socket.poll",
    "socket.wait",
    "socket.read",
    "socket.write",
    "socket.close",
    "socket.shutdown",
    "socket.starttls",
    "udp.open",
    "udp.send",
    "udp.recv",
    "udp.close",
];
const MAX_UDP_DATAGRAM: usize = 65_507;
const MAX_RECORD_TYPES: usize = 8;
const MAX_OPEN_SOCKETS: usize = 128;
const MAX_IO_CHUNK: usize = 64 * 1024;
/// Longest a single `socket.poll` or `socket.wait` call may block.
const MAX_WAIT_MS: u64 = 1_000;
/// Blocking reads are split into slices of this length so a cancelled run
/// stops waiting promptly instead of after the full I/O timeout.
const CANCEL_SLICE: Duration = Duration::from_millis(100);
/// How often `socket.wait` re-checks stream readability while it waits.
const READABLE_POLL: Duration = Duration::from_millis(5);
/// TLS connectors kept per run, keyed by their option set.
const MAX_CACHED_CONNECTORS: usize = 16;

/// A locale-independent classification of an I/O failure.
///
/// The vocabulary is `refused`, `timeout`, `reset`, `unreachable`, `closed`,
/// `denied`, and `other`; the bridge also uses `dns`, `tls`, and `cancelled`.
/// Operating-system messages are translated on some systems (a French Windows
/// does not say "refused"), so tools must branch on this kind, not on text.
pub(crate) fn error_kind(error: &io::Error) -> &'static str {
    use io::ErrorKind::*;
    match error.kind() {
        ConnectionRefused => "refused",
        TimedOut | WouldBlock => "timeout",
        ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof => "reset",
        HostUnreachable | NetworkUnreachable | NetworkDown | AddrNotAvailable => "unreachable",
        NotConnected => "closed",
        PermissionDenied => "denied",
        _ => "other",
    }
}

/// `[kind] context: message`, the error-string form used by every socket call.
fn tagged(context: &str, error: &io::Error) -> String {
    format!("[{}] {context}: {error}", error_kind(error))
}

enum Socket {
    Tcp(TcpStream),
    Tls(SslStream<TcpStream>),
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buf),
            Self::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buf),
            Self::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

impl Socket {
    fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.set_read_timeout(Some(timeout)),
            Self::Tls(stream) => stream.get_ref().set_read_timeout(Some(timeout)),
        }
    }

    /// Whether a read would return without blocking: buffered TLS plaintext,
    /// bytes on the wire, end of stream, or a pending error.
    fn readable(&self) -> bool {
        match self {
            Self::Tcp(stream) => tcp_readable(stream),
            Self::Tls(stream) => stream.ssl().pending() > 0 || tcp_readable(stream.get_ref()),
        }
    }
}

fn tcp_readable(stream: &TcpStream) -> bool {
    if stream.set_nonblocking(true).is_err() {
        return true;
    }
    let mut probe = [0u8; 1];
    let readable =
        !matches!(stream.peek(&mut probe), Err(e) if e.kind() == io::ErrorKind::WouldBlock);
    let _ = stream.set_nonblocking(false);
    readable
}

fn udp_readable(socket: &UdpSocket) -> bool {
    if socket.set_nonblocking(true).is_err() {
        return true;
    }
    let mut probe = [0u8; 1];
    // A datagram larger than the probe buffer reports an error on some
    // systems; any outcome other than "would block" means data is waiting.
    let readable =
        !matches!(socket.peek_from(&mut probe), Err(e) if e.kind() == io::ErrorKind::WouldBlock);
    let _ = socket.set_nonblocking(false);
    readable
}

/// Runs `attempt` with a timeout no longer than [`CANCEL_SLICE`] until it
/// succeeds, fails with something other than a timeout, the overall `timeout`
/// elapses, or the run is cancelled (`None`).
fn sliced<T>(
    timeout: Duration,
    cancelled: &AtomicBool,
    mut attempt: impl FnMut(Duration) -> io::Result<T>,
) -> Option<io::Result<T>> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let slice = remaining.min(CANCEL_SLICE).max(Duration::from_millis(1));
        match attempt(slice) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                if cancelled.load(Ordering::Acquire) {
                    return None;
                }
                if Instant::now() >= deadline {
                    return Some(Err(error));
                }
            }
            other => return Some(other),
        }
    }
}

const CANCELLED: &str = "[cancelled] the run was cancelled";

struct ConnectFailure {
    kind: &'static str,
    message: String,
}

type ConnectOutcome = Result<(TcpStream, SocketAddr), ConnectFailure>;

pub(crate) struct NetworkState {
    sockets: HashMap<u64, Socket>,
    udp: HashMap<u64, UdpSocket>,
    next_id: u64,
    /// Connections started by `socket.connect` and not yet claimed by
    /// `socket.poll` or `socket.wait`.
    pending: HashSet<u64>,
    /// Outcomes that arrived for pending connections, waiting to be claimed.
    finished: HashMap<u64, ConnectOutcome>,
    /// Every connection worker reports on this one channel, so a single
    /// blocking receive can wait on all of them at once.
    outcomes_tx: Sender<(u64, ConnectOutcome)>,
    outcomes_rx: Receiver<(u64, ConnectOutcome)>,
    connectors: HashMap<String, SslConnector>,
    /// The host name each TCP socket was opened to (the TLS name used by
    /// `socket.starttls` when the request names none).
    hosts: HashMap<u64, String>,
    /// Options of pending connections, applied when they are claimed.
    connect_options: HashMap<u64, ConnectOptions>,
    cancelled: Arc<AtomicBool>,
}

struct ConnectOptions {
    host: String,
    nodelay: bool,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self::new(Arc::new(AtomicBool::new(false)))
    }
}

impl NetworkState {
    /// State for one run. `cancelled` is the run's cancellation flag; blocking
    /// calls check it so Ctrl+C does not wait for a full socket timeout.
    pub(crate) fn new(cancelled: Arc<AtomicBool>) -> Self {
        let (outcomes_tx, outcomes_rx) = mpsc::channel();
        Self {
            sockets: HashMap::new(),
            udp: HashMap::new(),
            next_id: 0,
            pending: HashSet::new(),
            finished: HashMap::new(),
            outcomes_tx,
            outcomes_rx,
            connectors: HashMap::new(),
            hosts: HashMap::new(),
            connect_options: HashMap::new(),
            cancelled,
        }
    }

    pub(crate) fn request(&mut self, operation: &str, input: &Value) -> Result<String, String> {
        match operation {
            "network.resolve" => self.resolve(input),
            "dns.query" => dns::query(input),
            "udp.open" => self.udp_open(input),
            "udp.send" => self.udp_send(input),
            "udp.recv" => self.udp_recv(input),
            "udp.close" => self.udp_close(input),
            "socket.open" => self.open(input),
            "socket.connect" => self.connect(input),
            "socket.poll" => self.poll(input),
            "socket.wait" => self.wait(input),
            "socket.read" => self.read(input),
            "socket.write" => self.write(input),
            "socket.close" => self.close(input),
            "socket.shutdown" => self.shutdown(input),
            "socket.starttls" => self.starttls(input),
            other => Err(format!("unsupported network capability '{other}'")),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn allocate_id(&mut self) -> u64 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }

    fn resolve(&self, request: &Value) -> Result<String, String> {
        let host = string(request, "host")?;
        let port = request.get("port").and_then(Value::as_u64).unwrap_or(0);
        let port = u16::try_from(port).map_err(|_| "port must be between 0 and 65535")?;
        if let Some(types) = request.get("records") {
            return self.resolve_with_records(&host, port, types, request);
        }
        let addresses: Vec<IpAddr> = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("[dns] name resolution failed: {e}"))?
            .map(|addr| addr.ip())
            .collect();
        if addresses.is_empty() {
            return Err("[dns] hostname did not resolve to any address".into());
        }
        let mut unique = addresses;
        unique.sort();
        unique.dedup();
        Ok(serde_json::json!({"host":host,"addresses":unique}).to_string())
    }

    /// Resolve addresses through the system resolver and additionally query the
    /// requested record types. A failed system lookup is reported, not fatal,
    /// so a dangling CNAME can still be read from the `records` list.
    fn resolve_with_records(
        &self,
        host: &str,
        port: u16,
        types: &Value,
        request: &Value,
    ) -> Result<String, String> {
        let types: Vec<&str> = types
            .as_array()
            .ok_or("records must be an array of record type names")?
            .iter()
            .map(|t| t.as_str().ok_or("records entries must be strings"))
            .collect::<Result<_, _>>()?;
        if types.is_empty() || types.len() > MAX_RECORD_TYPES {
            return Err("records must list between 1 and 8 record types".into());
        }
        let options = dns::Options::from_request(request, "dns_port")?;
        let (mut addresses, resolution_error) = match (host, port).to_socket_addrs() {
            Ok(found) => (found.map(|a| a.ip()).collect::<Vec<IpAddr>>(), None),
            Err(error) => (Vec::new(), Some(format!("name resolution failed: {error}"))),
        };
        addresses.sort();
        addresses.dedup();
        let mut records = Vec::new();
        let mut rcodes = serde_json::Map::new();
        let mut record_errors = serde_json::Map::new();
        for record_type in types {
            let key = record_type.to_ascii_uppercase();
            match dns::lookup(host, record_type, &options) {
                Ok(answer) => {
                    rcodes.insert(key, answer.json["rcode"].clone());
                    records.extend(answer.records);
                }
                Err(error) => {
                    record_errors.insert(key, Value::String(error));
                }
            }
        }
        Ok(serde_json::json!({
            "host": host,
            "addresses": addresses,
            "resolution_error": resolution_error,
            "records": records,
            "rcodes": rcodes,
            "record_errors": record_errors,
        })
        .to_string())
    }

    fn connect(&mut self, request: &Value) -> Result<String, String> {
        if self.socket_count() + self.pending.len() >= MAX_OPEN_SOCKETS {
            return Err(format!(
                "a tool may hold at most {MAX_OPEN_SOCKETS} pending or open sockets"
            ));
        }
        let host = string(request, "host")?;
        let port = request
            .get("port")
            .and_then(Value::as_u64)
            .ok_or("request requires a port")?;
        let port = u16::try_from(port).map_err(|_| "port must be between 1 and 65535")?;
        if port == 0 {
            return Err("port must be between 1 and 65535".into());
        }
        let timeout_ms = request
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(1_000)
            .clamp(50, 10_000);
        let nodelay = flag(request, "nodelay")?;
        let id = self.allocate_id();
        self.connect_options.insert(
            id,
            ConnectOptions {
                host: host.clone(),
                nodelay,
            },
        );
        let outcomes = self.outcomes_tx.clone();
        thread::spawn(move || {
            let outcome = match (host.as_str(), port).to_socket_addrs() {
                Err(error) => Err(ConnectFailure {
                    kind: "dns",
                    message: format!("name resolution failed: {error}"),
                }),
                Ok(addresses) => {
                    let mut last_error: Option<io::Error> = None;
                    let mut connected = None;
                    for address in addresses {
                        match TcpStream::connect_timeout(
                            &address,
                            Duration::from_millis(timeout_ms),
                        ) {
                            Ok(stream) => {
                                connected = Some((stream, address));
                                break;
                            }
                            Err(error) => last_error = Some(error),
                        }
                    }
                    connected.ok_or_else(|| match last_error {
                        Some(error) => ConnectFailure {
                            kind: error_kind(&error),
                            message: error.to_string(),
                        },
                        None => ConnectFailure {
                            kind: "dns",
                            message: "hostname did not resolve to any address".into(),
                        },
                    })
                }
            };
            // The run may have ended; a closed channel is not an error.
            let _ = outcomes.send((id, outcome));
        });
        self.pending.insert(id);
        Ok(serde_json::json!({"connection":id}).to_string())
    }

    /// Move outcomes that already arrived into `finished`.
    fn drain_outcomes(&mut self) {
        while let Ok((id, outcome)) = self.outcomes_rx.try_recv() {
            self.store_outcome(id, outcome);
        }
    }

    fn store_outcome(&mut self, id: u64, outcome: ConnectOutcome) {
        if self.pending.contains(&id) {
            self.finished.insert(id, outcome);
        }
    }

    /// Block up to `timeout` for the next connection outcome.
    fn receive_outcome(&mut self, timeout: Duration) -> Option<u64> {
        match self.outcomes_rx.recv_timeout(timeout) {
            Ok((id, outcome)) => {
                self.store_outcome(id, outcome);
                Some(id)
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Turn a finished connection into an open socket (or a failure report).
    fn claim(&mut self, id: u64) -> Option<Value> {
        let outcome = self.finished.remove(&id)?;
        self.pending.remove(&id);
        let options = self.connect_options.remove(&id);
        Some(match outcome {
            Ok((stream, address)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                if options.as_ref().is_some_and(|o| o.nodelay) {
                    let _ = stream.set_nodelay(true);
                }
                let socket = self.allocate_id();
                self.sockets.insert(socket, Socket::Tcp(stream));
                if let Some(options) = options {
                    self.hosts.insert(socket, options.host);
                }
                serde_json::json!({
                    "connection": id,
                    "state": "connected",
                    "socket": socket,
                    "address": address.ip().to_string(),
                    "port": address.port(),
                })
            }
            Err(failure) => serde_json::json!({
                "connection": id,
                "state": "failed",
                "error": failure.message,
                "error_kind": failure.kind,
            }),
        })
    }

    fn poll(&mut self, request: &Value) -> Result<String, String> {
        let id = request
            .get("connection")
            .and_then(Value::as_u64)
            .ok_or("request requires connection")?;
        if !self.pending.contains(&id) {
            return Err("unknown pending connection".into());
        }
        let wait = Duration::from_millis(
            request
                .get("wait_ms")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .min(MAX_WAIT_MS),
        );
        let deadline = Instant::now() + wait;
        self.drain_outcomes();
        while !self.finished.contains_key(&id) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || self.is_cancelled() {
                return Ok(serde_json::json!({"connection":id,"state":"pending"}).to_string());
            }
            self.receive_outcome(remaining.min(CANCEL_SLICE));
        }
        let claimed = self.claim(id).ok_or("unknown pending connection")?;
        Ok(claimed.to_string())
    }

    /// Wait for several connections and sockets at once.
    ///
    /// Input: `connections` (pending connection ids; omitted means every
    /// pending connection), `sockets` (open TCP, TLS, or UDP socket ids to
    /// check for readability), and `timeout_ms` (0-1000, default 100). Returns
    /// as soon as at least one connection finished or one socket is readable,
    /// or when the timeout elapses.
    fn wait(&mut self, request: &Value) -> Result<String, String> {
        let ids = |key: &str| -> Result<Option<Vec<u64>>, String> {
            match request.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Array(items)) => {
                    if items.len() > MAX_OPEN_SOCKETS * 2 {
                        return Err(format!("{key} lists too many handles"));
                    }
                    items
                        .iter()
                        .map(|item| {
                            item.as_u64()
                                .ok_or_else(|| format!("{key} must be an array of handles"))
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map(Some)
                }
                Some(_) => Err(format!("{key} must be an array of handles")),
            }
        };
        let connections = match ids("connections")? {
            Some(list) => {
                if let Some(unknown) = list.iter().find(|id| !self.pending.contains(id)) {
                    return Err(format!("unknown pending connection {unknown}"));
                }
                list
            }
            None => self.pending.iter().copied().collect(),
        };
        let sockets = ids("sockets")?.unwrap_or_default();
        if let Some(unknown) = sockets
            .iter()
            .find(|id| !self.sockets.contains_key(id) && !self.udp.contains_key(id))
        {
            return Err(format!("unknown socket {unknown}"));
        }
        let timeout = Duration::from_millis(
            request
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(100)
                .min(MAX_WAIT_MS),
        );
        let deadline = Instant::now() + timeout;
        let watched: HashSet<u64> = connections.iter().copied().collect();
        loop {
            self.drain_outcomes();
            let ready: Vec<u64> = connections
                .iter()
                .copied()
                .filter(|id| self.finished.contains_key(id))
                .collect();
            let readable: Vec<u64> = sockets
                .iter()
                .copied()
                .filter(|id| match (self.sockets.get(id), self.udp.get(id)) {
                    (Some(socket), _) => socket.readable(),
                    (None, Some(socket)) => udp_readable(socket),
                    (None, None) => false,
                })
                .collect();
            let remaining = deadline.saturating_duration_since(Instant::now());
            let nothing_to_wait_for = watched.is_empty() && sockets.is_empty();
            if !ready.is_empty()
                || !readable.is_empty()
                || remaining.is_zero()
                || nothing_to_wait_for
                || self.is_cancelled()
            {
                let finished: Vec<Value> =
                    ready.into_iter().filter_map(|id| self.claim(id)).collect();
                return Ok(serde_json::json!({
                    "connections": finished,
                    "readable": readable,
                    "pending": self.pending.len(),
                })
                .to_string());
            }
            // Without sockets to re-check, sleep until a connection reports;
            // otherwise wake regularly to look at the sockets again.
            let slice = if sockets.is_empty() {
                remaining.min(CANCEL_SLICE)
            } else {
                remaining.min(READABLE_POLL)
            };
            self.receive_outcome(slice);
        }
    }

    fn connector(&mut self, options: &tls::TlsOptions) -> Result<SslConnector, String> {
        let key = options.connector_key();
        if let Some(connector) = self.connectors.get(&key) {
            return Ok(connector.clone());
        }
        let connector = options.connector()?;
        if self.connectors.len() >= MAX_CACHED_CONNECTORS {
            self.connectors.clear();
        }
        self.connectors.insert(key, connector.clone());
        Ok(connector)
    }

    fn open(&mut self, request: &Value) -> Result<String, String> {
        if self.socket_count() + self.pending.len() >= MAX_OPEN_SOCKETS {
            return Err(format!(
                "a tool may hold at most {MAX_OPEN_SOCKETS} sockets"
            ));
        }
        let tls_options = tls::TlsOptions::from_request(request)?;
        let host = string(request, "host")?;
        let port = request
            .get("port")
            .and_then(Value::as_u64)
            .ok_or("request requires a port")?;
        let port = u16::try_from(port).map_err(|_| "port must be between 1 and 65535")?;
        if port == 0 {
            return Err("port must be between 1 and 65535".into());
        }
        let timeout_ms = request
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(5_000)
            .clamp(50, 10_000);
        let timeout = Duration::from_millis(timeout_ms);
        let secure = request.get("tls").and_then(Value::as_bool).unwrap_or(false);
        let nodelay = flag(request, "nodelay")?;
        let connector = if secure {
            Some(self.connector(&tls_options)?)
        } else {
            None
        };
        let addresses: Vec<SocketAddr> = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("[dns] name resolution failed: {e}"))?
            .collect();
        if addresses.is_empty() {
            return Err("[dns] hostname did not resolve to any address".into());
        }
        let mut last_error = None;
        for address in addresses {
            match TcpStream::connect_timeout(&address, timeout) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(timeout))
                        .map_err(|e| e.to_string())?;
                    stream
                        .set_write_timeout(Some(timeout))
                        .map_err(|e| e.to_string())?;
                    if nodelay {
                        stream
                            .set_nodelay(true)
                            .map_err(|e| tagged("cannot disable Nagle's algorithm", &e))?;
                    }
                    let mut tls_info = Value::Null;
                    let socket = match &connector {
                        Some(connector) => {
                            let stream = tls_options.handshake_with(connector, &host, stream)?;
                            tls_info = tls::describe(stream.ssl(), &tls_options, &host);
                            Socket::Tls(stream)
                        }
                        None => Socket::Tcp(stream),
                    };
                    let id = self.allocate_id();
                    self.sockets.insert(id, socket);
                    self.hosts.insert(id, host.clone());
                    let mut opened = serde_json::json!({
                        "socket": id,
                        "host": host,
                        "port": port,
                        "address": address.ip().to_string(),
                        "tls": secure,
                        "timeout_ms": timeout_ms,
                    });
                    if secure {
                        opened["tls_info"] = tls_info;
                    }
                    return Ok(opened.to_string());
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(match last_error {
            Some(error) => tagged("connection failed", &error),
            None => "[other] connection failed: no address available".into(),
        })
    }

    fn read(&mut self, request: &Value) -> Result<String, String> {
        let id = socket_id(request)?;
        let length = request
            .get("length")
            .and_then(Value::as_u64)
            .ok_or("request requires length")?;
        if length == 0 || length > MAX_IO_CHUNK as u64 {
            return Err(format!("socket reads must be 1-{MAX_IO_CHUNK} bytes"));
        }
        let timeout_ms = request
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(5_000)
            .clamp(50, 10_000);
        let cancelled = Arc::clone(&self.cancelled);
        let socket = self.sockets.get_mut(&id).ok_or("unknown socket")?;
        let mut bytes = vec![0; length as usize];
        let count = sliced(Duration::from_millis(timeout_ms), &cancelled, |slice| {
            socket.set_read_timeout(slice)?;
            socket.read(&mut bytes)
        })
        .ok_or(CANCELLED)?
        .map_err(|e| tagged("socket read failed", &e))?;
        bytes.truncate(count);
        use base64::Engine as _;
        Ok(serde_json::json!({"socket":id,"bytes_base64":base64::engine::general_purpose::STANDARD.encode(bytes),"eof":count==0,"timeout_ms":timeout_ms}).to_string())
    }

    fn write(&mut self, request: &Value) -> Result<String, String> {
        let id = socket_id(request)?;
        let encoded = string(request, "bytes_base64")?;
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| format!("invalid Base64 socket data: {e}"))?;
        if bytes.len() > MAX_IO_CHUNK {
            return Err(format!("socket writes are limited to {MAX_IO_CHUNK} bytes"));
        }
        let socket = self.sockets.get_mut(&id).ok_or("unknown socket")?;
        socket
            .write_all(&bytes)
            .map_err(|e| tagged("socket write failed", &e))?;
        socket
            .flush()
            .map_err(|e| tagged("socket flush failed", &e))?;
        Ok(serde_json::json!({"socket":id,"written":bytes.len()}).to_string())
    }

    fn close(&mut self, request: &Value) -> Result<String, String> {
        let id = socket_id(request)?;
        self.sockets.remove(&id).ok_or("unknown socket")?;
        self.hosts.remove(&id);
        Ok(serde_json::json!({"socket":id,"closed":true}).to_string())
    }

    /// Half-close or fully shut down a TCP or TLS stream without releasing
    /// its handle, so the tool can still read what the peer sends back.
    /// Input: `socket`, `how` (`write`, the default; `read`; or `both`).
    /// Shutting down the write side of a TLS stream first sends close_notify.
    fn shutdown(&mut self, request: &Value) -> Result<String, String> {
        let id = socket_id(request)?;
        let (how, label) = match request
            .get("how")
            .and_then(Value::as_str)
            .unwrap_or("write")
        {
            "write" => (std::net::Shutdown::Write, "write"),
            "read" => (std::net::Shutdown::Read, "read"),
            "both" => (std::net::Shutdown::Both, "both"),
            _ => return Err("how must be write, read, or both".into()),
        };
        let socket = self.sockets.get_mut(&id).ok_or("unknown socket")?;
        let result = match socket {
            Socket::Tcp(stream) => stream.shutdown(how),
            Socket::Tls(stream) => {
                if how != std::net::Shutdown::Read {
                    // Best effort: the peer may already have gone away.
                    let _ = stream.shutdown();
                }
                stream.get_ref().shutdown(how)
            }
        };
        match result {
            Ok(()) => {}
            // Shutting down a stream the peer already closed is not a failure.
            Err(error) if error.kind() == io::ErrorKind::NotConnected => {}
            Err(error) => return Err(tagged("socket shutdown failed", &error)),
        }
        Ok(serde_json::json!({"socket": id, "shutdown": label}).to_string())
    }

    /// Upgrade an open plaintext TCP socket to TLS in place (STARTTLS, or a
    /// tunnel through an HTTP proxy after `CONNECT`). Accepts the same TLS
    /// options as `socket.open`, plus `host` (the name to verify and send as
    /// SNI; defaults to the host the socket was opened to) and `timeout_ms`.
    /// The socket keeps its handle; a failed handshake closes it.
    fn starttls(&mut self, request: &Value) -> Result<String, String> {
        let id = socket_id(request)?;
        let tls_options = tls::TlsOptions::from_request(request)?;
        let timeout_ms = request
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(5_000)
            .clamp(50, 10_000);
        let host = match request.get("host") {
            None | Some(Value::Null) => self
                .hosts
                .get(&id)
                .cloned()
                .ok_or("request requires host")?,
            Some(_) => string(request, "host")?,
        };
        match self.sockets.get(&id) {
            None => return Err("unknown socket".into()),
            Some(Socket::Tls(_)) => return Err("[tls] the socket already uses TLS".into()),
            Some(Socket::Tcp(_)) => {}
        }
        let connector = self.connector(&tls_options)?;
        let Some(Socket::Tcp(stream)) = self.sockets.remove(&id) else {
            return Err("unknown socket".into());
        };
        let timeout = Duration::from_millis(timeout_ms);
        let prepared = stream
            .set_read_timeout(Some(timeout))
            .and_then(|()| stream.set_write_timeout(Some(timeout)));
        if let Err(error) = prepared {
            self.hosts.remove(&id);
            return Err(tagged("cannot prepare the socket for TLS", &error));
        }
        match tls_options.handshake_with(&connector, &host, stream) {
            Ok(stream) => {
                let tls_info = tls::describe(stream.ssl(), &tls_options, &host);
                self.sockets.insert(id, Socket::Tls(stream));
                self.hosts.insert(id, host);
                Ok(
                    serde_json::json!({"socket": id, "tls": true, "tls_info": tls_info})
                        .to_string(),
                )
            }
            Err(error) => {
                self.hosts.remove(&id);
                Err(format!("{error} (the socket was closed)"))
            }
        }
    }

    fn socket_count(&self) -> usize {
        self.sockets.len() + self.udp.len()
    }

    fn udp_open(&mut self, request: &Value) -> Result<String, String> {
        if self.socket_count() + self.pending.len() >= MAX_OPEN_SOCKETS {
            return Err(format!(
                "a tool may hold at most {MAX_OPEN_SOCKETS} sockets"
            ));
        }
        let family = request
            .get("family")
            .and_then(Value::as_str)
            .unwrap_or("v4");
        let bind_port = request
            .get("bind_port")
            .and_then(Value::as_u64)
            .map(|p| u16::try_from(p).map_err(|_| "bind_port must be between 0 and 65535"))
            .transpose()?
            .unwrap_or(0);
        let address: SocketAddr = match family {
            "v4" => (Ipv4Addr::UNSPECIFIED, bind_port).into(),
            "v6" => (Ipv6Addr::UNSPECIFIED, bind_port).into(),
            _ => return Err("family must be v4 or v6".into()),
        };
        let socket = UdpSocket::bind(address).map_err(|e| tagged("cannot open UDP socket", &e))?;
        if request
            .get("broadcast")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            socket
                .set_broadcast(true)
                .map_err(|e| tagged("cannot enable broadcast", &e))?;
        }
        if let Some(ttl) = request.get("ttl").and_then(Value::as_u64) {
            let ttl = u32::try_from(ttl).ok().filter(|t| (1..=255).contains(t));
            socket
                .set_ttl(ttl.ok_or("ttl must be between 1 and 255")?)
                .map_err(|e| tagged("cannot set TTL", &e))?;
        }
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        let local_port = socket.local_addr().map_err(|e| e.to_string())?.port();
        let id = self.allocate_id();
        self.udp.insert(id, socket);
        Ok(serde_json::json!({"socket":id,"family":family,"local_port":local_port}).to_string())
    }

    fn udp_send(&mut self, request: &Value) -> Result<String, String> {
        let id = socket_id(request)?;
        let host = string(request, "host")?;
        let port = request
            .get("port")
            .and_then(Value::as_u64)
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p != 0)
            .ok_or("port must be between 1 and 65535")?;
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(
                request
                    .get("bytes_base64")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
            .map_err(|e| format!("invalid Base64 datagram: {e}"))?;
        if bytes.len() > MAX_UDP_DATAGRAM {
            return Err(format!("datagrams are limited to {MAX_UDP_DATAGRAM} bytes"));
        }
        let socket = self.udp.get(&id).ok_or("unknown UDP socket")?;
        let wants_v4 = socket.local_addr().map_err(|e| e.to_string())?.is_ipv4();
        let target = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("[dns] name resolution failed: {e}"))?
            .find(|a| a.is_ipv4() == wants_v4)
            .ok_or("[dns] hostname has no address in this socket's family")?;
        let sent = socket
            .send_to(&bytes, target)
            .map_err(|e| tagged("UDP send failed", &e))?;
        Ok(
            serde_json::json!({"socket":id,"sent":sent,"address":target.ip().to_string()})
                .to_string(),
        )
    }

    fn udp_recv(&mut self, request: &Value) -> Result<String, String> {
        let id = socket_id(request)?;
        let length = request
            .get("length")
            .and_then(Value::as_u64)
            .unwrap_or(MAX_IO_CHUNK as u64);
        if length == 0 || length > MAX_IO_CHUNK as u64 {
            return Err(format!("UDP reads must be 1-{MAX_IO_CHUNK} bytes"));
        }
        let timeout_ms = request
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(2_000)
            .clamp(50, 10_000);
        let socket = self.udp.get(&id).ok_or("unknown UDP socket")?;
        // One spare byte tells a datagram of exactly `length` bytes apart from
        // a longer, truncated one.
        let length = length as usize;
        let mut buffer = vec![0u8; length + 1];
        use base64::Engine as _;
        let received = sliced(
            Duration::from_millis(timeout_ms),
            &self.cancelled,
            |slice| {
                socket.set_read_timeout(Some(slice))?;
                socket.recv_from(&mut buffer)
            },
        )
        .ok_or(CANCELLED)?;
        match received {
            Ok((count, from)) => Ok(serde_json::json!({
                "socket": id,
                "bytes_base64": base64::engine::general_purpose::STANDARD.encode(&buffer[..count.min(length)]),
                "from": {"address": from.ip().to_string(), "port": from.port()},
                "truncated": count > length,
                "timeout": false,
                "refused": false,
            })
            .to_string()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(serde_json::json!({"socket":id,"bytes_base64":"","timeout":true,"refused":false}).to_string())
            }
            // An ICMP "port unreachable" for an earlier datagram surfaces here.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionRefused
                ) =>
            {
                Ok(serde_json::json!({"socket":id,"bytes_base64":"","timeout":false,"refused":true}).to_string())
            }
            Err(error) => Err(tagged("UDP receive failed", &error)),
        }
    }

    fn udp_close(&mut self, request: &Value) -> Result<String, String> {
        let id = socket_id(request)?;
        self.udp.remove(&id).ok_or("unknown UDP socket")?;
        Ok(serde_json::json!({"socket":id,"closed":true}).to_string())
    }
}

fn string(request: &Value, key: &str) -> Result<String, String> {
    request
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("request requires {key}"))
}

/// An optional boolean field, `false` when absent.
fn flag(request: &Value, key: &str) -> Result<bool, String> {
    match request.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("{key} must be a boolean")),
    }
}

fn socket_id(request: &Value) -> Result<u64, String> {
    request
        .get("socket")
        .and_then(Value::as_u64)
        .ok_or_else(|| "request requires socket".into())
}

#[cfg(test)]
mod architecture_tests {
    use super::ALLOWED_OPERATIONS;

    #[test]
    fn network_capability_surface_stays_generic_and_fixed() {
        assert_eq!(
            ALLOWED_OPERATIONS,
            [
                "network.resolve",
                "dns.query",
                "socket.open",
                "socket.connect",
                "socket.poll",
                "socket.wait",
                "socket.read",
                "socket.write",
                "socket.close",
                "socket.shutdown",
                "socket.starttls",
                "udp.open",
                "udp.send",
                "udp.recv",
                "udp.close",
            ]
        );
    }
}

#[cfg(test)]
mod tcp_tests {
    use super::{NetworkState, error_kind};
    use serde_json::{Value, json};
    use std::{
        io::{self, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    fn call(state: &mut NetworkState, op: &str, body: Value) -> Result<Value, String> {
        state
            .request(op, &body)
            .map(|text| serde_json::from_str(&text).unwrap())
    }

    /// A loopback port with nothing listening on it.
    fn closed_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    #[test]
    fn classifies_io_errors_independently_of_the_message_language() {
        for (kind, expected) in [
            (io::ErrorKind::ConnectionRefused, "refused"),
            (io::ErrorKind::TimedOut, "timeout"),
            (io::ErrorKind::WouldBlock, "timeout"),
            (io::ErrorKind::ConnectionReset, "reset"),
            (io::ErrorKind::HostUnreachable, "unreachable"),
            (io::ErrorKind::Other, "other"),
        ] {
            let error = io::Error::new(kind, "message localisé");
            assert_eq!(error_kind(&error), expected, "{kind:?}");
        }
    }

    #[test]
    fn reports_a_refused_connection_with_a_structured_kind() {
        let port = closed_port();
        let mut state = NetworkState::default();
        let connection = call(
            &mut state,
            "socket.connect",
            json!({"host": "127.0.0.1", "port": port, "timeout_ms": 5000}),
        )
        .unwrap()["connection"]
            .as_u64()
            .unwrap();
        // Windows retries a refused loopback SYN for about two seconds.
        let started = Instant::now();
        let polled = loop {
            let polled = call(
                &mut state,
                "socket.poll",
                json!({"connection": connection, "wait_ms": 1000}),
            )
            .unwrap();
            if polled["state"] != "pending" || started.elapsed() > Duration::from_secs(8) {
                break polled;
            }
        };
        assert_eq!(polled["state"], "failed", "{polled}");
        assert_eq!(polled["error_kind"], "refused", "{polled}");
        assert!(call(&mut state, "socket.poll", json!({"connection": connection})).is_err());
    }

    #[test]
    fn waits_on_many_connections_and_reports_readable_sockets() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let open_port = listener.local_addr().unwrap().port();
        let refused_port = closed_port();
        let (send_tx, send_rx) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            send_rx.recv().unwrap();
            std::thread::sleep(Duration::from_millis(100));
            stream.write_all(b"hello").unwrap();
            std::thread::sleep(Duration::from_millis(500));
        });
        let mut state = NetworkState::default();
        let mut start = |port: u16| {
            call(
                &mut state,
                "socket.connect",
                json!({"host": "127.0.0.1", "port": port, "timeout_ms": 5000}),
            )
            .unwrap()["connection"]
                .as_u64()
                .unwrap()
        };
        let good = start(open_port);
        let bad = start(refused_port);

        let mut results = Vec::new();
        let started = Instant::now();
        while results.len() < 2 && started.elapsed() < Duration::from_secs(10) {
            let waited = call(&mut state, "socket.wait", json!({"timeout_ms": 1000})).unwrap();
            results.extend(waited["connections"].as_array().unwrap().iter().cloned());
        }
        let find = |id: u64| {
            results
                .iter()
                .find(|r| r["connection"] == id)
                .cloned()
                .unwrap()
        };
        let connected = find(good);
        assert_eq!(connected["state"], "connected", "{connected}");
        assert_eq!(connected["address"], "127.0.0.1");
        assert_eq!(find(bad)["error_kind"], "refused");

        let socket = connected["socket"].as_u64().unwrap();
        let quiet = call(
            &mut state,
            "socket.wait",
            json!({"sockets": [socket], "timeout_ms": 20}),
        )
        .unwrap();
        assert_eq!(quiet["readable"], json!([]), "{quiet}");
        send_tx.send(()).unwrap();
        let ready = call(
            &mut state,
            "socket.wait",
            json!({"sockets": [socket], "timeout_ms": 1000}),
        )
        .unwrap();
        assert_eq!(ready["readable"], json!([socket]), "{ready}");
        let read = call(
            &mut state,
            "socket.read",
            json!({"socket": socket, "length": 16, "timeout_ms": 1000}),
        )
        .unwrap();
        assert_eq!(read["bytes_base64"], "aGVsbG8=");
        assert_eq!(read["eof"], false);
        server.join().unwrap();
    }

    #[test]
    fn wait_rejects_unknown_handles_and_returns_at_once_with_nothing_to_watch() {
        let mut state = NetworkState::default();
        assert!(call(&mut state, "socket.wait", json!({"connections": [7]})).is_err());
        assert!(call(&mut state, "socket.wait", json!({"sockets": [7]})).is_err());
        assert!(call(&mut state, "socket.wait", json!({"sockets": "x"})).is_err());
        let started = Instant::now();
        let empty = call(&mut state, "socket.wait", json!({"timeout_ms": 1000})).unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(empty["pending"], 0);
    }

    #[test]
    fn a_read_timeout_is_tagged_and_cancellation_interrupts_it() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let held: Vec<_> = (0..2).map(|_| listener.accept().unwrap()).collect();
            std::thread::sleep(Duration::from_secs(3));
            drop(held);
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut state = NetworkState::new(Arc::clone(&cancelled));
        let open = |state: &mut NetworkState| {
            call(
                state,
                "socket.open",
                json!({"host": "127.0.0.1", "port": port, "timeout_ms": 2000}),
            )
            .unwrap()["socket"]
                .as_u64()
                .unwrap()
        };
        let first = open(&mut state);
        let error = call(
            &mut state,
            "socket.read",
            json!({"socket": first, "length": 8, "timeout_ms": 150}),
        )
        .unwrap_err();
        assert!(error.starts_with("[timeout] "), "{error}");

        let second = open(&mut state);
        let flag = Arc::clone(&cancelled);
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            flag.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let error = call(
            &mut state,
            "socket.read",
            json!({"socket": second, "length": 8, "timeout_ms": 10000}),
        )
        .unwrap_err();
        assert!(error.starts_with("[cancelled]"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
        canceller.join().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn connect_errors_from_open_are_tagged() {
        let port = closed_port();
        let mut state = NetworkState::default();
        let error = call(
            &mut state,
            "socket.open",
            json!({"host": "127.0.0.1", "port": port, "timeout_ms": 5000}),
        )
        .unwrap_err();
        assert!(error.starts_with("[refused] "), "{error}");
    }
}

#[cfg(test)]
mod udp_tests {
    use super::NetworkState;
    use base64::Engine as _;
    use serde_json::{Value, json};

    fn call(state: &mut NetworkState, op: &str, body: Value) -> Result<Value, String> {
        state
            .request(op, &body)
            .map(|text| serde_json::from_str(&text).unwrap())
    }

    #[test]
    fn exchanges_datagrams_with_a_local_echo_server() {
        let server = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let echo = std::thread::spawn(move || {
            let mut buffer = [0u8; 64];
            let (count, peer) = server.recv_from(&mut buffer).unwrap();
            server.send_to(&buffer[..count], peer).unwrap();
        });
        let mut state = NetworkState::default();
        let opened = call(&mut state, "udp.open", json!({"family": "v4"})).unwrap();
        let socket = opened["socket"].as_u64().unwrap();
        let payload = base64::engine::general_purpose::STANDARD.encode(b"ping");
        let sent = call(
            &mut state,
            "udp.send",
            json!({"socket": socket, "host": "127.0.0.1", "port": port, "bytes_base64": payload}),
        )
        .unwrap();
        assert_eq!(sent["sent"], 4);
        let received = call(
            &mut state,
            "udp.recv",
            json!({"socket": socket, "timeout_ms": 3000}),
        )
        .unwrap();
        assert_eq!(received["timeout"], false);
        assert_eq!(received["from"]["port"], port);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(received["bytes_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(bytes, b"ping");
        call(&mut state, "udp.close", json!({"socket": socket})).unwrap();
        assert!(call(&mut state, "udp.close", json!({"socket": socket})).is_err());
        echo.join().unwrap();
    }

    #[test]
    fn reports_a_timeout_instead_of_failing() {
        let mut state = NetworkState::default();
        let opened = call(&mut state, "udp.open", json!({})).unwrap();
        let socket = opened["socket"].as_u64().unwrap();
        let received = call(
            &mut state,
            "udp.recv",
            json!({"socket": socket, "timeout_ms": 100}),
        )
        .unwrap();
        assert_eq!(received["timeout"], true);
        assert_eq!(received["bytes_base64"], "");
    }

    #[test]
    fn validates_udp_arguments() {
        let mut state = NetworkState::default();
        assert!(call(&mut state, "udp.open", json!({"family": "v5"})).is_err());
        assert!(call(&mut state, "udp.open", json!({"ttl": 0})).is_err());
        assert!(
            call(
                &mut state,
                "udp.send",
                json!({"socket": 99, "host": "127.0.0.1", "port": 9, "bytes_base64": ""})
            )
            .is_err()
        );
        let socket = call(&mut state, "udp.open", json!({})).unwrap()["socket"]
            .as_u64()
            .unwrap();
        let oversized = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 70_000]);
        assert!(
            call(
                &mut state,
                "udp.send",
                json!({"socket": socket, "host": "127.0.0.1", "port": 9, "bytes_base64": oversized})
            )
            .is_err()
        );
        assert!(
            call(
                &mut state,
                "udp.send",
                json!({"socket": socket, "host": "127.0.0.1", "port": 0, "bytes_base64": ""})
            )
            .is_err()
        );
    }

    #[test]
    fn udp_sockets_count_toward_the_connection_limit() {
        let mut state = NetworkState::default();
        for _ in 0..128 {
            call(&mut state, "udp.open", json!({})).unwrap();
        }
        assert!(call(&mut state, "udp.open", json!({})).is_err());
    }
}

#[cfg(test)]
mod tls_tests {
    use crate::tls::TlsOptions;

    #[test]
    fn builds_a_peer_verifying_tls_connector() {
        let _connector = TlsOptions::default()
            .connector()
            .expect("TLS trust roots should load successfully");
    }
}
