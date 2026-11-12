//! A buffered byte stream over the host's `socket.*` capabilities, shared by
//! the HTTP/1.1 and HTTP/2 code.

use crate::http::{Bridge, Config, ErrorKind, HttpError, Proxy};
use base64::Engine as _;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Largest chunk the host moves per `socket.read` or `socket.write`.
const IO_CHUNK: usize = 64 * 1024;
/// Longest single blocking read the host accepts.
const HOST_MAX_READ_MS: u64 = 10_000;

/// TLS parameters for a new connection.
pub(crate) struct TlsSettings {
    pub(crate) verify: bool,
    pub(crate) alpn: Vec<&'static str>,
}

/// What a response reports about the connection that carried it.
#[derive(Clone, Debug, Default)]
pub(crate) struct ConnInfo {
    pub(crate) address: Option<String>,
    pub(crate) alpn: Option<String>,
    pub(crate) tls: Option<Value>,
}

pub(crate) struct Conn<'a> {
    bridge: Bridge<'a>,
    socket: Option<u64>,
    buffer: Vec<u8>,
    start: usize,
    read_timeout_ms: u64,
    pub(crate) info: ConnInfo,
    /// Total bytes received; lets callers tell whether a peer answered at all.
    pub(crate) received: u64,
}

/// Turn a host error string (`[kind] message`) into an [`HttpError`].
pub(crate) fn host_error(context: &str, message: String) -> HttpError {
    let kind = match crate::error_kind(&message) {
        Some("refused" | "unreachable" | "dns") => ErrorKind::Connect,
        Some("denied") => ErrorKind::Denied,
        Some("timeout") => ErrorKind::Timeout,
        Some("tls") => ErrorKind::Tls,
        Some("cancelled") => ErrorKind::Cancelled,
        Some("reset" | "closed") => ErrorKind::Closed,
        _ if message.contains("permission was not granted") => ErrorKind::Denied,
        _ => ErrorKind::Other,
    };
    HttpError::new(kind, format!("{context}: {message}"))
}

impl<'a> Conn<'a> {
    /// Open a TCP (or TLS) connection to `host:port`.
    pub(crate) fn open(
        bridge: Bridge<'a>,
        host: &str,
        port: u16,
        tls: Option<&TlsSettings>,
        connect_timeout_ms: u64,
        read_timeout_ms: u64,
    ) -> Result<Self, HttpError> {
        let mut request = json!({
            "host": host,
            "port": port,
            "timeout_ms": connect_timeout_ms.clamp(50, 10_000),
            "nodelay": true,
        });
        if let Some(tls) = tls {
            request["tls"] = json!(true);
            request["verify"] = json!(tls.verify);
            if !tls.alpn.is_empty() {
                request["alpn"] = json!(tls.alpn);
            }
        }
        let raw = bridge("socket.open", &request.to_string())
            .map_err(|e| host_error(&format!("cannot connect to {host}:{port}"), e))?;
        let opened: Value = serde_json::from_str(&raw).map_err(|e| {
            HttpError::new(ErrorKind::Other, format!("invalid socket.open reply: {e}"))
        })?;
        let socket = opened
            .get("socket")
            .and_then(Value::as_u64)
            .ok_or_else(|| HttpError::new(ErrorKind::Other, "socket.open reply has no socket"))?;
        let mut conn = Self {
            bridge,
            socket: Some(socket),
            buffer: Vec::new(),
            start: 0,
            read_timeout_ms,
            info: ConnInfo {
                address: opened
                    .get("address")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                ..ConnInfo::default()
            },
            received: 0,
        };
        if let Some(tls_info) = opened.get("tls_info").filter(|v| !v.is_null()) {
            conn.set_tls(tls_info.clone());
        }
        Ok(conn)
    }

    /// Open a tunnel to `host:port` through an HTTP proxy (`CONNECT`), then
    /// upgrade it to TLS with `socket.starttls`.
    pub(crate) fn open_tunnel(
        bridge: Bridge<'a>,
        proxy: &Proxy,
        host: &str,
        authority: &str,
        port: u16,
        tls: &TlsSettings,
        config: &Config,
    ) -> Result<Self, HttpError> {
        let connect_timeout_ms = config.connect_timeout_ms;
        let mut conn = Self::open(
            bridge,
            &proxy.host,
            proxy.port,
            None,
            connect_timeout_ms,
            config.read_timeout_ms,
        )?;
        let target = format!("{authority}:{port}");
        let mut head = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
        if let Some(credentials) = &proxy.authorization {
            head.push_str(&format!("Proxy-Authorization: {credentials}\r\n"));
        }
        head.push_str("\r\n");
        conn.write_all(head.as_bytes())?;
        let reply = conn.read_head(config.max_header_bytes)?;
        let text = String::from_utf8_lossy(&reply);
        let status_line = text.lines().next().unwrap_or_default();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| {
                HttpError::new(
                    ErrorKind::Protocol,
                    "the proxy sent an invalid CONNECT reply",
                )
            })?;
        if !(200..300).contains(&status) {
            return Err(HttpError::new(
                ErrorKind::Connect,
                format!("the proxy refused the tunnel: {}", status_line.trim()),
            ));
        }
        if !conn.available().is_empty() {
            return Err(HttpError::new(
                ErrorKind::Protocol,
                "the proxy sent data before the TLS handshake",
            ));
        }
        let mut request = json!({
            "socket": conn.socket,
            "host": host,
            "verify": tls.verify,
            "timeout_ms": connect_timeout_ms.clamp(50, 10_000),
        });
        if !tls.alpn.is_empty() {
            request["alpn"] = json!(tls.alpn);
        }
        let raw = match bridge("socket.starttls", &request.to_string()) {
            Ok(raw) => raw,
            Err(error) => {
                // A failed upgrade already closed the socket on the host.
                conn.socket = None;
                return Err(host_error("TLS through the proxy failed", error));
            }
        };
        let upgraded: Value = serde_json::from_str(&raw).map_err(|e| {
            HttpError::new(
                ErrorKind::Other,
                format!("invalid socket.starttls reply: {e}"),
            )
        })?;
        conn.set_tls(upgraded.get("tls_info").cloned().unwrap_or(Value::Null));
        Ok(conn)
    }

    fn set_tls(&mut self, tls_info: Value) {
        self.info.alpn = tls_info
            .get("alpn")
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.info.tls = Some(tls_info);
    }

    pub(crate) fn is_open(&self) -> bool {
        self.socket.is_some()
    }

    /// Buffered bytes not consumed yet.
    pub(crate) fn available(&self) -> &[u8] {
        &self.buffer[self.start..]
    }

    pub(crate) fn consume(&mut self, count: usize) {
        self.start = (self.start + count).min(self.buffer.len());
        if self.start == self.buffer.len() {
            self.buffer.clear();
            self.start = 0;
        } else if self.start > 64 * 1024 && self.start * 2 > self.buffer.len() {
            self.buffer.drain(..self.start);
            self.start = 0;
        }
    }

    /// Read one more chunk into the buffer. Returns `false` at end of stream.
    pub(crate) fn read_more(&mut self) -> Result<bool, HttpError> {
        let socket = self
            .socket
            .ok_or_else(|| HttpError::new(ErrorKind::Closed, "the connection is closed"))?;
        let deadline = Instant::now() + Duration::from_millis(self.read_timeout_ms.max(50));
        loop {
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64;
            let request = json!({
                "socket": socket,
                "length": IO_CHUNK,
                "timeout_ms": remaining.clamp(50, HOST_MAX_READ_MS),
            });
            match (self.bridge)("socket.read", &request.to_string()) {
                Ok(raw) => {
                    let reply: Value = serde_json::from_str(&raw).map_err(|e| {
                        HttpError::new(ErrorKind::Other, format!("invalid socket.read reply: {e}"))
                    })?;
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(
                            reply
                                .get("bytes_base64")
                                .and_then(Value::as_str)
                                .unwrap_or(""),
                        )
                        .map_err(|e| {
                            HttpError::new(ErrorKind::Other, format!("invalid socket data: {e}"))
                        })?;
                    if bytes.is_empty() {
                        return Ok(false);
                    }
                    self.received += bytes.len() as u64;
                    self.buffer.extend_from_slice(&bytes);
                    return Ok(true);
                }
                // The host caps one read at ten seconds; keep waiting until
                // the configured read timeout has really elapsed.
                Err(error)
                    if crate::error_kind(&error) == Some("timeout")
                        && Instant::now() < deadline =>
                {
                    continue;
                }
                Err(error) => return Err(host_error("read failed", error)),
            }
        }
    }

    /// Exactly `count` bytes, failing at end of stream.
    pub(crate) fn read_exact(&mut self, count: usize) -> Result<Vec<u8>, HttpError> {
        while self.available().len() < count {
            if !self.read_more()? {
                return Err(HttpError::new(
                    ErrorKind::Closed,
                    "the connection closed in the middle of a message",
                ));
            }
        }
        let bytes = self.available()[..count].to_vec();
        self.consume(count);
        Ok(bytes)
    }

    /// One line without its line ending (`\n` or `\r\n`).
    pub(crate) fn read_line(&mut self, max: usize) -> Result<Vec<u8>, HttpError> {
        let mut searched = 0;
        loop {
            if let Some(index) = self.available()[searched..]
                .iter()
                .position(|b| *b == b'\n')
            {
                let end = searched + index;
                let mut line = self.available()[..end].to_vec();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.consume(end + 1);
                return Ok(line);
            }
            searched = self.available().len();
            if searched > max {
                return Err(HttpError::new(
                    ErrorKind::Protocol,
                    "a protocol line is too long",
                ));
            }
            if !self.read_more()? {
                return Err(HttpError::new(
                    ErrorKind::Closed,
                    "the connection closed in the middle of a line",
                ));
            }
        }
    }

    /// A message head up to and including the empty line that ends it.
    pub(crate) fn read_head(&mut self, max: usize) -> Result<Vec<u8>, HttpError> {
        let received_before = self.received;
        let mut searched = 0usize;
        loop {
            let data = self.available();
            if let Some(end) = head_end(data, searched.saturating_sub(3)) {
                let head = data[..end].to_vec();
                self.consume(end);
                return Ok(head);
            }
            searched = data.len();
            if searched > max {
                return Err(HttpError::new(
                    ErrorKind::TooLarge,
                    format!("the response headers exceed {max} bytes"),
                ));
            }
            if !self.read_more()? {
                let nothing = self.received == received_before && self.available().is_empty();
                let mut error = HttpError::new(
                    ErrorKind::Closed,
                    if nothing {
                        "the server closed the connection without answering"
                    } else {
                        "the connection closed in the middle of the response headers"
                    },
                );
                error.retryable = nothing;
                return Err(error);
            }
        }
    }

    pub(crate) fn write_all(&mut self, bytes: &[u8]) -> Result<(), HttpError> {
        let socket = self
            .socket
            .ok_or_else(|| HttpError::new(ErrorKind::Closed, "the connection is closed"))?;
        for chunk in bytes.chunks(IO_CHUNK) {
            let request = json!({
                "socket": socket,
                "bytes_base64": base64::engine::general_purpose::STANDARD.encode(chunk),
            });
            (self.bridge)("socket.write", &request.to_string())
                .map_err(|e| host_error("write failed", e))?;
        }
        Ok(())
    }

    pub(crate) fn close(&mut self) {
        if let Some(socket) = self.socket.take() {
            let _ = (self.bridge)("socket.close", &json!({ "socket": socket }).to_string());
        }
    }
}

impl Drop for Conn<'_> {
    fn drop(&mut self) {
        self.close();
    }
}

/// Index just past the blank line ending a message head (`\r\n\r\n` or the
/// lenient `\n\n`), searching from `from`.
fn head_end(data: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while index < data.len() {
        if data[index] == b'\n' {
            if data.get(index + 1) == Some(&b'\n') {
                return Some(index + 2);
            }
            if data.get(index + 1) == Some(&b'\r') && data.get(index + 2) == Some(&b'\n') {
                return Some(index + 3);
            }
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::head_end;

    #[test]
    fn finds_the_end_of_a_message_head() {
        assert_eq!(
            head_end(b"HTTP/1.1 200 OK\r\nA: b\r\n\r\nbody", 0),
            Some(25)
        );
        assert_eq!(head_end(b"HTTP/1.1 200 OK\nA: b\n\nbody", 0), Some(22));
        assert_eq!(head_end(b"HTTP/1.1 200 OK\r\nA: b\r\n", 0), None);
    }
}
