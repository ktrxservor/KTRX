//! Generic, permission-gated socket primitives exposed to sandboxed tools.
//! Protocols and scanning strategies belong in guest tools, not in this module.

use crate::{dns, tls};
use openssl::ssl::SslStream;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::Duration,
};

pub(crate) const ALLOWED_OPERATIONS: [&str; 12] = [
    "network.resolve",
    "dns.query",
    "socket.open",
    "socket.connect",
    "socket.poll",
    "socket.read",
    "socket.write",
    "socket.close",
    "udp.open",
    "udp.send",
    "udp.recv",
    "udp.close",
];
const MAX_UDP_DATAGRAM: usize = 65_507;
const MAX_RECORD_TYPES: usize = 8;
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
    udp: HashMap<u64, UdpSocket>,
    next_id: u64,
    pending: HashMap<u64, Receiver<Result<TcpStream, String>>>,
}

impl NetworkState {
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
        if let Some(types) = request.get("records") {
            return self.resolve_with_records(&host, port, types, request);
        }
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
                    let mut tls_info = Value::Null;
                    let socket = if secure {
                        let stream = tls_options.handshake(&host, stream)?;
                        tls_info = tls::describe(stream.ssl(), &tls_options, &host);
                        Socket::Tls(stream)
                    } else {
                        Socket::Tcp(stream)
                    };
                    self.next_id = self.next_id.wrapping_add(1).max(1);
                    let id = self.next_id;
                    self.sockets.insert(id, socket);
                    let mut opened = serde_json::json!({"socket":id,"host":host,"port":port,"tls":secure,"timeout_ms":timeout_ms});
                    if secure {
                        opened["tls_info"] = tls_info;
                    }
                    return Ok(opened.to_string());
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
        let socket =
            UdpSocket::bind(address).map_err(|e| format!("cannot open UDP socket: {e}"))?;
        if request
            .get("broadcast")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            socket
                .set_broadcast(true)
                .map_err(|e| format!("cannot enable broadcast: {e}"))?;
        }
        if let Some(ttl) = request.get("ttl").and_then(Value::as_u64) {
            let ttl = u32::try_from(ttl).ok().filter(|t| (1..=255).contains(t));
            socket
                .set_ttl(ttl.ok_or("ttl must be between 1 and 255")?)
                .map_err(|e| format!("cannot set TTL: {e}"))?;
        }
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        let local_port = socket.local_addr().map_err(|e| e.to_string())?.port();
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
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
            .map_err(|e| format!("name resolution failed: {e}"))?
            .find(|a| a.is_ipv4() == wants_v4)
            .ok_or("hostname has no address in this socket's family")?;
        let sent = socket
            .send_to(&bytes, target)
            .map_err(|e| format!("UDP send failed: {e}"))?;
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
        socket
            .set_read_timeout(Some(Duration::from_millis(timeout_ms)))
            .map_err(|e| e.to_string())?;
        let mut buffer = vec![0u8; length as usize];
        use base64::Engine as _;
        match socket.recv_from(&mut buffer) {
            Ok((count, from)) => Ok(serde_json::json!({
                "socket": id,
                "bytes_base64": base64::engine::general_purpose::STANDARD.encode(&buffer[..count]),
                "from": {"address": from.ip().to_string(), "port": from.port()},
                "truncated": count == buffer.len(),
                "timeout": false,
                "refused": false,
            })
            .to_string()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(serde_json::json!({"socket":id,"bytes_base64":"","timeout":true,"refused":false}).to_string())
            }
            // An ICMP "port unreachable" for an earlier datagram surfaces here.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                Ok(serde_json::json!({"socket":id,"bytes_base64":"","timeout":false,"refused":true}).to_string())
            }
            Err(error) => Err(format!("UDP receive failed: {error}")),
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
                "socket.read",
                "socket.write",
                "socket.close",
                "udp.open",
                "udp.send",
                "udp.recv",
                "udp.close",
            ]
        );
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
