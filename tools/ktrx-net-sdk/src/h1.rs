//! HTTP/1.1 message exchange (RFC 9112) over a [`Conn`].

use crate::{
    conn::Conn,
    decode::ACCEPT_ENCODING,
    http::{Config, ErrorKind, HttpError, Prepared, Version},
};

const MAX_LINE: usize = 16 * 1024;
const MAX_TRAILER_LINES: usize = 128;

/// A response as read from the wire, before content decoding.
#[derive(Debug, Default)]
pub(crate) struct RawResponse {
    pub(crate) version: Option<Version>,
    pub(crate) status: u16,
    pub(crate) reason: Option<String>,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) trailers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
    /// The body was cut at the configured limit.
    pub(crate) truncated: bool,
    /// Informational (1xx) statuses received before the final response.
    pub(crate) interim: Vec<u16>,
    /// Body bytes received on the wire (after transfer decoding).
    pub(crate) wire_bytes: u64,
}

impl RawResponse {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn header_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.headers
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Send `request` and read its response. Returns the response and whether
/// the connection may carry another request.
pub(crate) fn exchange(
    conn: &mut Conn<'_>,
    request: &Prepared,
    config: &Config,
    absolute_form: bool,
) -> Result<(RawResponse, bool), HttpError> {
    let received_before = conn.received;
    let mark = |mut error: HttpError, conn: &Conn<'_>| {
        // Nothing came back: the request may be retried on a new connection
        // (a kept-alive connection the server had already closed).
        if conn.received == received_before && error.kind == ErrorKind::Closed {
            error.retryable = true;
        }
        error
    };
    let head = serialize(request, config, absolute_form);
    let mut message = head.into_bytes();
    message.extend_from_slice(&request.body);
    conn.write_all(&message).map_err(|e| mark(e, conn))?;
    read_response(conn, request, config).map_err(|e| mark(e, conn))
}

fn serialize(request: &Prepared, config: &Config, absolute_form: bool) -> String {
    let has = |name: &str| {
        request
            .headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
    };
    let target = if absolute_form {
        request.absolute_url()
    } else {
        request.path.clone()
    };
    let mut head = format!("{} {target} HTTP/1.1\r\n", request.method);
    if !has("host") {
        head.push_str(&format!("Host: {}\r\n", request.authority));
    }
    for (name, value) in &request.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !has("user-agent") && !config.user_agent.is_empty() {
        head.push_str(&format!("User-Agent: {}\r\n", config.user_agent));
    }
    if !has("accept") {
        head.push_str("Accept: */*\r\n");
    }
    if config.decompress && !has("accept-encoding") {
        head.push_str(&format!("Accept-Encoding: {ACCEPT_ENCODING}\r\n"));
    }
    let body_expected = matches!(request.method.as_str(), "POST" | "PUT" | "PATCH");
    if !has("content-length")
        && !has("transfer-encoding")
        && (!request.body.is_empty() || body_expected)
    {
        head.push_str(&format!("Content-Length: {}\r\n", request.body.len()));
    }
    if !config.keep_alive && !has("connection") {
        head.push_str("Connection: close\r\n");
    }
    if absolute_form
        && !has("proxy-authorization")
        && let Some(credentials) = config.proxy.as_ref().and_then(|p| p.authorization.as_ref())
    {
        head.push_str(&format!("Proxy-Authorization: {credentials}\r\n"));
    }
    head.push_str("\r\n");
    head
}

fn protocol(message: impl Into<String>) -> HttpError {
    HttpError::new(ErrorKind::Protocol, message)
}

fn read_response(
    conn: &mut Conn<'_>,
    request: &Prepared,
    config: &Config,
) -> Result<(RawResponse, bool), HttpError> {
    let mut response = RawResponse::default();
    let minor = loop {
        let head = conn.read_head(config.max_header_bytes)?;
        let (minor, status, reason, headers) = parse_head(&head)?;
        if (100..200).contains(&status) && status != 101 {
            if response.interim.len() >= 16 {
                return Err(protocol("too many informational responses"));
            }
            response.interim.push(status);
            continue;
        }
        response.status = status;
        response.reason = reason;
        response.headers = headers;
        break minor;
    };
    response.version = Some(Version::Http11);
    let status = response.status;
    let connection_close = response
        .header_values("connection")
        .flat_map(|v| v.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case("close"));
    let mut reusable = config.keep_alive && minor == 1 && !connection_close && status != 101;
    let no_body = request.method == "HEAD" || matches!(status, 101 | 204 | 304);
    if no_body {
        return Ok((response, reusable));
    }
    let transfer: Vec<String> = response
        .header_values("transfer-encoding")
        .flat_map(|v| v.split(','))
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty())
        .collect();
    let limit = config.max_body_bytes;
    if !transfer.is_empty() {
        if transfer.last().map(String::as_str) == Some("chunked") {
            read_chunked(conn, &mut response, limit)?;
        } else {
            read_to_close(conn, &mut response, limit)?;
            reusable = false;
        }
    } else if let Some(length) = content_length(&response)? {
        read_length(conn, &mut response, length, limit)?;
    } else {
        read_to_close(conn, &mut response, limit)?;
        reusable = false;
    }
    if response.truncated {
        reusable = false;
    }
    Ok((response, reusable))
}

type Head = (u8, u16, Option<String>, Vec<(String, String)>);

fn parse_head(head: &[u8]) -> Result<Head, HttpError> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
    let status_line = lines.next().unwrap_or_default();
    let rest = status_line
        .strip_prefix("HTTP/1.")
        .ok_or_else(|| protocol(format!("not an HTTP/1.x response: {:.60}", status_line)))?;
    let minor = rest
        .bytes()
        .next()
        .filter(u8::is_ascii_digit)
        .map(|d| d - b'0')
        .ok_or_else(|| protocol("invalid HTTP version"))?;
    let rest = &rest[1..];
    let rest = rest
        .strip_prefix(' ')
        .ok_or_else(|| protocol("invalid status line"))?;
    let code = rest.get(..3).unwrap_or_default();
    let after = rest.get(3..).unwrap_or_default();
    let status = code
        .parse::<u16>()
        .ok()
        .filter(|s| {
            (100..=999).contains(s)
                && code.bytes().all(|b| b.is_ascii_digit())
                && (after.is_empty() || after.starts_with(' '))
        })
        .ok_or_else(|| protocol(format!("invalid status code in {:.40}", status_line)))?;
    let reason = Some(after)
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .map(str::to_owned);
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            // Obsolete line folding: join with a single space (RFC 9112 §5.2).
            let (_, value) = headers
                .last_mut()
                .ok_or_else(|| protocol("folded header line without a header"))?;
            value.push(' ');
            value.push_str(line.trim());
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| protocol(format!("malformed header line {:.60}", line)))?;
        if name.is_empty() || !name.bytes().all(crate::http::is_token_byte) {
            return Err(protocol(format!("invalid header name {name:?}")));
        }
        headers.push((name.to_owned(), value.trim_matches([' ', '\t']).to_owned()));
    }
    Ok((minor, status, reason, headers))
}

/// The message length from `Content-Length`; differing values are an error.
fn content_length(response: &RawResponse) -> Result<Option<u64>, HttpError> {
    let mut length = None;
    for value in response
        .header_values("content-length")
        .flat_map(|v| v.split(','))
    {
        let value = value.trim();
        let parsed = value
            .parse::<u64>()
            .ok()
            .filter(|_| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| protocol(format!("invalid Content-Length {value:?}")))?;
        match length {
            Some(previous) if previous != parsed => {
                return Err(protocol("conflicting Content-Length values"));
            }
            _ => length = Some(parsed),
        }
    }
    Ok(length)
}

/// Move up to `count` buffered-or-read bytes into the body, respecting `limit`.
/// Returns how many bytes were taken from the connection.
fn take_body(
    conn: &mut Conn<'_>,
    response: &mut RawResponse,
    count: usize,
    limit: usize,
) -> Result<usize, HttpError> {
    if conn.available().is_empty() && !conn.read_more()? {
        return Ok(0);
    }
    let take = conn.available().len().min(count);
    let room = limit.saturating_sub(response.body.len());
    let keep = take.min(room);
    response.body.extend_from_slice(&conn.available()[..keep]);
    response.wire_bytes += take as u64;
    if keep < take {
        response.truncated = true;
    }
    conn.consume(take);
    Ok(take)
}

fn read_length(
    conn: &mut Conn<'_>,
    response: &mut RawResponse,
    length: u64,
    limit: usize,
) -> Result<(), HttpError> {
    let mut remaining = length;
    while remaining > 0 {
        if response.body.len() >= limit {
            // Stop reading: the rest is not kept and the connection is dropped.
            response.truncated = true;
            return Ok(());
        }
        let wanted = remaining.min(usize::MAX as u64) as usize;
        let taken = take_body(conn, response, wanted, limit)?;
        if taken == 0 {
            return Err(HttpError::new(
                ErrorKind::Closed,
                format!("the connection closed {remaining} bytes before the end of the body"),
            ));
        }
        remaining -= taken as u64;
    }
    Ok(())
}

fn read_to_close(
    conn: &mut Conn<'_>,
    response: &mut RawResponse,
    limit: usize,
) -> Result<(), HttpError> {
    loop {
        if response.body.len() >= limit {
            response.truncated = true;
            return Ok(());
        }
        if take_body(conn, response, usize::MAX, limit)? == 0 {
            return Ok(());
        }
    }
}

fn read_chunked(
    conn: &mut Conn<'_>,
    response: &mut RawResponse,
    limit: usize,
) -> Result<(), HttpError> {
    loop {
        let line = conn.read_line(MAX_LINE)?;
        let line = String::from_utf8_lossy(&line);
        let size_text = line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16)
            .ok()
            .filter(|_| !size_text.is_empty() && size_text.len() <= 15)
            .ok_or_else(|| protocol(format!("invalid chunk size {size_text:?}")))?;
        if size == 0 {
            break;
        }
        let mut remaining = size;
        while remaining > 0 {
            if response.body.len() >= limit {
                response.truncated = true;
                return Ok(());
            }
            let taken = take_body(conn, response, remaining, limit)?;
            if taken == 0 {
                return Err(HttpError::new(
                    ErrorKind::Closed,
                    "the connection closed in the middle of a chunk",
                ));
            }
            remaining -= taken;
        }
        let end = conn.read_line(MAX_LINE)?;
        if !end.is_empty() {
            return Err(protocol("a chunk is not followed by CRLF"));
        }
    }
    // Trailer section, ended by an empty line.
    for _ in 0..MAX_TRAILER_LINES {
        let line = conn.read_line(MAX_LINE)?;
        if line.is_empty() {
            return Ok(());
        }
        let line = String::from_utf8_lossy(&line);
        if let Some((name, value)) = line.split_once(':') {
            response
                .trailers
                .push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }
    Err(protocol("too many trailer fields"))
}

#[cfg(test)]
mod tests {
    use super::parse_head;

    #[test]
    fn parses_status_lines_and_folded_headers() {
        let (minor, status, reason, headers) =
            parse_head(b"HTTP/1.1 404 Not Found\r\nServer: x\r\nX-Long: a\r\n  b\r\n\r\n").unwrap();
        assert_eq!((minor, status), (1, 404));
        assert_eq!(reason.as_deref(), Some("Not Found"));
        assert_eq!(headers[1], ("X-Long".into(), "a b".into()));
        let (_, status, reason, _) = parse_head(b"HTTP/1.0 200\r\n\r\n").unwrap();
        assert_eq!(status, 200);
        assert_eq!(reason, None);
    }

    #[test]
    fn rejects_malformed_heads() {
        for bad in [
            &b"HTTP/2 200 OK\r\n\r\n"[..],
            b"HTTP/1.1 2000 OK\r\n\r\n",
            b"HTTP/1.1 20x OK\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nBad Header: x\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nNoColon\r\n\r\n",
            b"HTTP/1.1 200 OK\r\n continued\r\n\r\n",
        ] {
            assert!(parse_head(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }
}
