//! Generic, permission-gated socket primitives exposed to sandboxed tools.
//! Protocols and scanning strategies belong in guest tools, not in this module.

use openssl::ssl::{SslConnector, SslMethod, SslStream, SslVerifyMode};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::Duration,
};

pub(crate) const ALLOWED_OPERATIONS: [&str; 7] = [
    "network.resolve",
    "socket.open",
    "socket.connect",
    "socket.poll",
    "socket.read",
    "socket.write",
    "socket.close",
];
const MAX_OPEN_SOCKETS: usize = 128;
const MAX_IO_CHUNK: usize = 64 * 1024;

enum Socket {
    Tcp(TcpStream),
    Tls(SslStream<TcpStream>),
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buf),
            Self::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buf),
            Self::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

impl Socket {
    fn set_read_timeout(&self, timeout: Duration) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.set_read_timeout(Some(timeout)),
            Self::Tls(stream) => stream.get_ref().set_read_timeout(Some(timeout)),
        }
    }
}

#[derive(Default)]
pub(crate) struct NetworkState {
    sockets: HashMap<u64, Socket>,
    next_id: u64,
    pending: HashMap<u64, Receiver<Result<TcpStream, String>>>,
}

impl NetworkState {
    pub(crate) fn request(&mut self, operation: &str, input: &Value) -> Result<String, String> {
        match operation {
            "network.resolve" => self.resolve(input),
            "socket.open" => self.open(input),
            "socket.connect" => self.connect(input),
            "socket.poll" => self.poll(input),
            "socket.read" => self.read(input),
            "socket.write" => self.write(input),
            "socket.close" => self.close(input),
            other => Err(format!("unsupported network capability '{other}'")),
        }
    }

    fn resolve(&self, request: &Value) -> Result<String, String> {
        let host = string(request, "host")?;
        let port = request.get("port").and_then(Value::as_u64).unwrap_or(0);
        let port = u16::try_from(port).map_err(|_| "port must be between 0 and 65535")?;
        let addresses: Vec<IpAddr> = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("name resolution failed: {e}"))?
            .map(|addr| addr.ip())
            .collect();
        if addresses.is_empty() {
            return Err("hostname did not resolve to any address".into());
        }
        let mut unique = addresses;
        unique.sort();
        unique.dedup();
        Ok(serde_json::json!({"host":host,"addresses":unique}).to_string())
    }

    fn connect(&mut self, request: &Value) -> Result<String, String> {
        if self.sockets.len() + self.pending.len() >= MAX_OPEN_SOCKETS {
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
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map_err(|e| format!("name resolution failed: {e}"))
                .and_then(|addresses| {
                    let mut last_error = None;
                    for address in addresses {
                        match TcpStream::connect_timeout(
                            &address,
                            Duration::from_millis(timeout_ms),
                        ) {
                            Ok(stream) => return Ok(stream),
                            Err(error) => last_error = Some(error),
                        }
                    }
                    Err(last_error.map_or_else(
                        || "hostname did not resolve to any address".into(),
                        |e| e.to_string(),
                    ))
                });
            let _ = tx.send(result);
        });
        self.pending.insert(id, rx);
        Ok(serde_json::json!({"connection":id}).to_string())
    }

    fn poll(&mut self, request: &Value) -> Result<String, String> {
        let id = request
            .get("connection")
            .and_then(Value::as_u64)
            .ok_or("request requires connection")?;
        let wait_ms = request
            .get("wait_ms")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .min(2);
        let receiver = self.pending.get(&id).ok_or("unknown pending connection")?;
        match receiver.recv_timeout(Duration::from_millis(wait_ms)) {
            Err(RecvTimeoutError::Timeout) => {
                Ok(serde_json::json!({"state":"pending"}).to_string())
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.pending.remove(&id);
                Ok(
                    serde_json::json!({"state":"failed","error":"connection worker stopped"})
                        .to_string(),
                )
            }
            Ok(Err(error)) => {
                self.pending.remove(&id);
                Ok(serde_json::json!({"state":"failed","error":error}).to_string())
            }
            Ok(Ok(stream)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                self.pending.remove(&id);
                self.next_id = self.next_id.wrapping_add(1).max(1);
                let socket = self.next_id;
                self.sockets.insert(socket, Socket::Tcp(stream));
                Ok(serde_json::json!({"state":"connected","socket":socket}).to_string())
            }
        }
    }

    fn open(&mut self, request: &Value) -> Result<String, String> {
        if self.sockets.len() >= MAX_OPEN_SOCKETS {
            return Err(format!(
                "a tool may hold at most {MAX_OPEN_SOCKETS} sockets"
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
            .unwrap_or(5_000)
            .clamp(50, 10_000);
        let timeout = Duration::from_millis(timeout_ms);
        let addresses: Vec<SocketAddr> = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("name resolution failed: {e}"))?
            .collect();
        if addresses.is_empty() {
            return Err("hostname did not resolve to any address".into());
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
                    let secure = request.get("tls").and_then(Value::as_bool).unwrap_or(false);
                    let socket = if secure {
                        let mut builder = SslConnector::builder(SslMethod::tls())
                            .map_err(|e| format!("TLS setup failed: {e}"))?;
                        builder.set_verify(SslVerifyMode::PEER);
                        let connector = builder.build();
                        Socket::Tls(
                            connector
                                .connect(&host, stream)
                                .map_err(|e| format!("TLS handshake failed: {e}"))?,
                        )
                    } else {
                        Socket::Tcp(stream)
                    };
                    self.next_id = self.next_id.wrapping_add(1).max(1);
                    let id = self.next_id;
                    self.sockets.insert(id, socket);
                    return Ok(serde_json::json!({"socket":id,"host":host,"port":port,"tls":secure,"timeout_ms":timeout_ms}).to_string());
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(format!(
            "connection failed: {}",
            last_error.map_or_else(|| "no address available".into(), |e| e.to_string())
        ))
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
        let socket = self.sockets.get_mut(&id).ok_or("unknown socket")?;
        socket
            .set_read_timeout(Duration::from_millis(timeout_ms))
            .map_err(|e| format!("cannot set socket read timeout: {e}"))?;
        let mut bytes = vec![0; length as usize];
        let count = socket
            .read(&mut bytes)
            .map_err(|e| format!("socket read failed: {e}"))?;
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
            .map_err(|e| format!("socket write failed: {e}"))?;
        socket
            .flush()
            .map_err(|e| format!("socket flush failed: {e}"))?;
        Ok(serde_json::json!({"socket":id,"written":bytes.len()}).to_string())
    }

    fn close(&mut self, request: &Value) -> Result<String, String> {
        let id = socket_id(request)?;
        self.sockets.remove(&id).ok_or("unknown socket")?;
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
                "socket.open",
                "socket.connect",
                "socket.poll",
                "socket.read",
                "socket.write",
                "socket.close",
            ]
        );
    }
}
