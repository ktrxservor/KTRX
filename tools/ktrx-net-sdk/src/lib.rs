//! Guest-side HTTP and socket helpers. Protocol interpretation lives in the
//! tools; the runner exposes only permission-gated socket byte streams.

use base64::Engine as _;
use serde_json::{Value, json};

const IO_CHUNK: usize = 16 * 1024;
const MAX_RESPONSE: usize = 1024 * 1024;
const MAX_HEADERS: usize = 64 * 1024;

pub fn resolve<F>(host: &str, port: u16, request: &F) -> Result<Vec<String>, String>
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let raw = request(
        "network.resolve",
        &json!({"host":host,"port":port}).to_string(),
    )?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid DNS response: {e}"))?;
    value
        .get("addresses")
        .and_then(Value::as_array)
        .ok_or("DNS response has no addresses")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "invalid address in DNS response".into())
        })
        .collect()
}

pub fn start_connect<F>(host: &str, port: u16, timeout_ms: u64, request: &F) -> Result<u64, String>
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let raw = request(
        "socket.connect",
        &json!({"host":host,"port":port,"timeout_ms":timeout_ms}).to_string(),
    )?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid connect response: {e}"))?;
    value
        .get("connection")
        .and_then(Value::as_u64)
        .ok_or_else(|| "connect response has no handle".into())
}

/// Why a connection failed, as reported by the host. `kind` is
/// locale-independent (`refused`, `timeout`, `reset`, `unreachable`, `dns`,
/// `closed`, `denied`, `other`); `message` is the operating-system text, which
/// may be translated and must not be parsed.
#[derive(Clone, Debug)]
pub struct ConnectError {
    pub kind: String,
    pub message: String,
}

impl ConnectError {
    pub fn is_refused(&self) -> bool {
        self.kind == "refused"
    }
}

pub enum ConnectPoll {
    Pending,
    Connected(u64),
    Failed(ConnectError),
}

/// One finished connection reported by [`wait`].
pub struct ConnectResult {
    pub connection: u64,
    pub outcome: Result<u64, ConnectError>,
}

/// What [`wait`] observed.
pub struct WaitResult {
    pub connections: Vec<ConnectResult>,
    pub readable: Vec<u64>,
    pub pending: usize,
}

/// The `[kind]` tag that host error strings start with, if any.
pub fn error_kind(error: &str) -> Option<&str> {
    let rest = error.strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(&rest[..end])
}

fn connection_state(value: &Value) -> Result<ConnectPoll, String> {
    match value.get("state").and_then(Value::as_str) {
        Some("pending") => Ok(ConnectPoll::Pending),
        Some("connected") => Ok(ConnectPoll::Connected(
            value
                .get("socket")
                .and_then(Value::as_u64)
                .ok_or("connected response has no socket")?,
        )),
        Some("failed") => {
            let message = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("connection failed")
                .to_owned();
            // Hosts older than `error_kind` only sent English or translated
            // text; fall back to the best guess for them.
            let kind = value
                .get("error_kind")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if message.to_ascii_lowercase().contains("refused") {
                        "refused".into()
                    } else {
                        "other".into()
                    }
                });
            Ok(ConnectPoll::Failed(ConnectError { kind, message }))
        }
        _ => Err("unknown connection state".into()),
    }
}

pub fn poll_connect<F>(connection: u64, request: &F) -> Result<ConnectPoll, String>
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let raw = request(
        "socket.poll",
        &json!({"connection":connection,"wait_ms":1}).to_string(),
    )?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid poll response: {e}"))?;
    connection_state(&value)
}

/// Block up to `timeout_ms` (at most 1000) until at least one of the pending
/// `connections` finishes or one of `sockets` becomes readable. Pass `None`
/// for `connections` to watch every pending connection of the run.
pub fn wait<F>(
    connections: Option<&[u64]>,
    sockets: &[u64],
    timeout_ms: u64,
    request: &F,
) -> Result<WaitResult, String>
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let mut body = json!({"sockets": sockets, "timeout_ms": timeout_ms});
    if let Some(connections) = connections {
        body["connections"] = json!(connections);
    }
    let raw = request("socket.wait", &body.to_string())?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid wait response: {e}"))?;
    let connections = value
        .get("connections")
        .and_then(Value::as_array)
        .ok_or("wait response has no connections")?
        .iter()
        .map(|item| {
            let connection = item
                .get("connection")
                .and_then(Value::as_u64)
                .ok_or("wait result has no connection")?;
            let outcome = match connection_state(item)? {
                ConnectPoll::Connected(socket) => Ok(socket),
                ConnectPoll::Failed(error) => Err(error),
                ConnectPoll::Pending => return Err("wait returned a pending connection".into()),
            };
            Ok(ConnectResult {
                connection,
                outcome,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let readable = value
        .get("readable")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    let pending = value.get("pending").and_then(Value::as_u64).unwrap_or(0) as usize;
    Ok(WaitResult {
        connections,
        readable,
        pending,
    })
}

pub fn open_socket<F>(
    host: &str,
    port: u16,
    tls: bool,
    timeout_ms: u64,
    request: &F,
) -> Result<u64, String>
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let raw = request(
        "socket.open",
        &json!({"host":host,"port":port,"tls":tls,"timeout_ms":timeout_ms}).to_string(),
    )?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid socket response: {e}"))?;
    value
        .get("socket")
        .and_then(Value::as_u64)
        .ok_or_else(|| "socket response has no handle".into())
}

pub fn write_socket<F>(socket: u64, bytes: &[u8], request: &F) -> Result<(), String>
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    request(
        "socket.write",
        &json!({"socket":socket,"bytes_base64":encoded}).to_string(),
    )?;
    Ok(())
}

pub fn read_socket<F>(
    socket: u64,
    length: usize,
    timeout_ms: u64,
    request: &F,
) -> Result<Vec<u8>, String>
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let raw = request(
        "socket.read",
        &json!({"socket":socket,"length":length,"timeout_ms":timeout_ms}).to_string(),
    )?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("invalid socket data: {e}"))?;
    let encoded = value
        .get("bytes_base64")
        .and_then(Value::as_str)
        .ok_or("socket read response has no bytes")?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("invalid Base64 socket data: {e}"))
}

pub fn close_socket<F>(socket: u64, request: &F)
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let _ = request("socket.close", &json!({"socket":socket}).to_string());
}

/// Sends a simple HTTP/1.1 GET request and returns the response fields used by
/// the plugins. Redirect handling, detection rules, and reporting stay in each tool.
pub fn http_get<F>(
    raw_url: &str,
    include_body: bool,
    sample_limit: usize,
    request: &F,
) -> Result<String, String>
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let url = url::Url::parse(raw_url).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("only absolute HTTP and HTTPS URLs are supported".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("credentials in URLs are not supported".into());
    }
    let host = url.host_str().ok_or("URL has no hostname")?;
    let port = url.port_or_known_default().ok_or("URL has no valid port")?;
    let socket = open_socket(host, port, url.scheme() == "https", 5_000, request)?;
    let result = (|| {
        let mut path = url.path().to_owned();
        if path.is_empty() {
            path.push('/');
        }
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        let authority = url.host_str().unwrap_or_default();
        let host_header = if port == if url.scheme() == "https" { 443 } else { 80 } {
            authority.to_owned()
        } else {
            format!("{authority}:{port}")
        };
        let request_bytes = format!(
            "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUser-Agent: KTRX-Tool/1.0\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n"
        );
        write_socket(socket, request_bytes.as_bytes(), request)?;
        let mut response = Vec::new();
        loop {
            let chunk = read_socket(socket, IO_CHUNK, 10_000, request)?;
            if chunk.is_empty() {
                break;
            }
            if response.len() + chunk.len() > MAX_HEADERS + MAX_RESPONSE + 4 {
                return Err("HTTP response exceeds the 1 MiB body limit".into());
            }
            response.extend_from_slice(&chunk);
            if let Some(split) = find_header_end(&response) {
                if split > MAX_HEADERS {
                    return Err("HTTP headers exceed 64 KiB".into());
                }
                if response.len().saturating_sub(split) > MAX_RESPONSE {
                    return Err("HTTP response exceeds the 1 MiB body limit".into());
                }
            }
        }
        parse_response(raw_url, &response, include_body, sample_limit)
    })();
    close_socket(socket, request);
    result
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn parse_response(
    raw_url: &str,
    bytes: &[u8],
    include_body: bool,
    sample_limit: usize,
) -> Result<String, String> {
    let boundary = find_header_end(bytes).ok_or("HTTP response has no complete header block")?;
    if boundary > MAX_HEADERS {
        return Err("HTTP headers exceed 64 KiB".into());
    }
    let head = std::str::from_utf8(&bytes[..boundary - 4])
        .map_err(|_| "HTTP headers are not valid UTF-8")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("HTTP response has no status line")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or("HTTP status is missing")?
        .parse::<u16>()
        .map_err(|_| "invalid HTTP status")?;
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("malformed HTTP header")?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    let raw_body = &bytes[boundary..];
    let is_chunked = headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
    });
    let body = if is_chunked {
        decode_chunked(raw_body)?
    } else {
        raw_body.to_vec()
    };
    if body.len() > MAX_RESPONSE {
        return Err("HTTP response exceeds the 1 MiB body limit".into());
    }
    let mut result =
        json!({"url":raw_url,"status":status,"headers":headers,"body_bytes":body.len()});
    if include_body {
        let count = body.len().min(sample_limit.clamp(1, 262_144));
        result["body_base64"] =
            Value::String(base64::engine::general_purpose::STANDARD.encode(&body[..count]));
        result["body_sampled_bytes"] = Value::Number(count.into());
        result["body_truncated"] = Value::Bool(count < body.len());
    }
    Ok(result.to_string())
}

fn decode_chunked(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut cursor = 0;
    loop {
        let line_end = bytes[cursor..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("malformed chunked response")?
            + cursor;
        let size_text = std::str::from_utf8(&bytes[cursor..line_end])
            .map_err(|_| "invalid chunk size")?
            .split(';')
            .next()
            .unwrap_or_default();
        let size = usize::from_str_radix(size_text.trim(), 16).map_err(|_| "invalid chunk size")?;
        cursor = line_end + 2;
        if size == 0 {
            break;
        }
        let end = cursor.checked_add(size).ok_or("chunk size overflow")?;
        if end + 2 > bytes.len() || &bytes[end..end + 2] != b"\r\n" {
            return Err("truncated HTTP chunk".into());
        }
        output.extend_from_slice(&bytes[cursor..end]);
        if output.len() > MAX_RESPONSE {
            return Err("HTTP response exceeds the 1 MiB body limit".into());
        }
        cursor = end + 2;
    }
    Ok(output)
}

#[cfg(test)]
mod connect_tests {
    use super::{ConnectPoll, connection_state, error_kind, wait};
    use serde_json::json;

    #[test]
    fn reads_the_structured_failure_kind() {
        let failed = json!({"state":"failed","error":"Aucune connexion n'a pu être établie","error_kind":"refused"});
        let ConnectPoll::Failed(error) = connection_state(&failed).unwrap() else {
            panic!("expected a failure");
        };
        assert!(error.is_refused());
        let legacy = json!({"state":"failed","error":"Connection refused (os error 111)"});
        let ConnectPoll::Failed(error) = connection_state(&legacy).unwrap() else {
            panic!("expected a failure");
        };
        assert!(error.is_refused());
    }

    #[test]
    fn extracts_error_tags() {
        assert_eq!(error_kind("[timeout] socket read failed: x"), Some("timeout"));
        assert_eq!(error_kind("plain message"), None);
    }

    #[test]
    fn decodes_a_wait_response() {
        let bridge = |op: &str, body: &str| {
            assert_eq!(op, "socket.wait");
            let body: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(body["connections"], json!([4, 5]));
            Ok(json!({
                "connections": [
                    {"connection":4,"state":"connected","socket":9,"address":"127.0.0.1","port":80},
                    {"connection":5,"state":"failed","error":"x","error_kind":"timeout"}
                ],
                "readable": [9],
                "pending": 1
            })
            .to_string())
        };
        let result = wait(Some(&[4, 5]), &[9], 100, &bridge).unwrap();
        assert_eq!(result.connections[0].outcome.as_ref().unwrap(), &9);
        assert_eq!(result.connections[1].outcome.as_ref().unwrap_err().kind, "timeout");
        assert_eq!(result.readable, [9]);
        assert_eq!(result.pending, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_response;
    use base64::Engine as _;
    use serde_json::Value;

    #[test]
    fn parses_status_headers_and_a_sampled_body() {
        let response =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello";
        let parsed: Value = serde_json::from_str(
            &parse_response("http://example.test/", response, true, 3).unwrap(),
        )
        .unwrap();
        assert_eq!(parsed["status"], 200);
        assert_eq!(parsed["body_bytes"], 5);
        assert_eq!(parsed["body_sampled_bytes"], 3);
        assert_eq!(parsed["body_truncated"], true);
    }

    #[test]
    fn decodes_chunked_body() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        let parsed: Value = serde_json::from_str(
            &parse_response("http://example.test/", response, true, 64).unwrap(),
        )
        .unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(parsed["body_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(bytes, b"Wikipedia");
    }
}
