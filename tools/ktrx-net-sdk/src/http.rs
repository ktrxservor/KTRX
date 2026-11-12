//! A complete HTTP client for KTRX tools, built only on the host's generic,
//! permission-gated socket capabilities.
//!
//! * HTTP/1.1 (RFC 9112) with persistent connections, chunked transfer
//!   coding, trailers, informational responses, and lenient-but-safe parsing.
//! * HTTP/2 (RFC 9113) negotiated with ALPN over TLS, or by prior knowledge
//!   over cleartext; [`Client::send_all`] multiplexes many requests to one
//!   origin over a single connection.
//! * `gzip` and `deflate` content decoding with a bound on the decoded size.
//! * Redirects (opt-in), with method rewriting and credential stripping on
//!   cross-origin hops.
//! * HTTPS through an HTTP proxy (`CONNECT` + `socket.starttls`, engine 0.4+)
//!   and plain HTTP through a proxy (absolute-form requests).
//!
//! Every size, count, and wait is bounded. Response data is untrusted: the
//! client only frames it; interpreting it is the tool's job.
//!
//! ```no_run
//! # fn host(op: &str, input: &str) -> Result<String, String> { unimplemented!() }
//! use ktrx_net_sdk::http::{Client, Request};
//! let mut client = Client::new(&host);
//! let response = client.send(Request::get("https://example.com/")).unwrap();
//! println!("{} {} {}", response.version.as_str(), response.status, response.body.len());
//! ```

use crate::{
    conn::{Conn, ConnInfo, TlsSettings},
    decode,
    h1::{self, RawResponse},
    h2::H2Conn,
};
use base64::Engine as _;
use serde_json::{Value, json};
use std::{collections::HashMap, fmt, time::Instant};

/// The host bridge: `ktrx::tool::host::request` in a tool, a fake in tests.
pub type Bridge<'a> = &'a dyn Fn(&str, &str) -> Result<String, String>;

/// The HTTP version a response arrived with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    Http11,
    Http2,
}

impl Version {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http11 => "HTTP/1.1",
            Self::Http2 => "HTTP/2",
        }
    }
}

/// Which HTTP versions the client may use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VersionPolicy {
    /// HTTP/2 when the server offers it through ALPN, else HTTP/1.1.
    /// Cleartext `http://` always uses HTTP/1.1.
    #[default]
    Auto,
    /// Never use HTTP/2.
    Http1Only,
    /// Require HTTP/2: ALPN `h2` over TLS, prior knowledge (h2c) over
    /// cleartext. A server that does not speak it is an error.
    Http2Only,
}

/// An HTTP proxy, used for every request of a [`Client`].
#[derive(Clone, Debug)]
pub struct Proxy {
    pub host: String,
    pub port: u16,
    /// `Proxy-Authorization` value, for example `Basic dXNlcjpwYXNz`.
    pub authorization: Option<String>,
}

/// Client settings. Every limit applies per request.
#[derive(Clone, Debug)]
pub struct Config {
    pub version: VersionPolicy,
    pub connect_timeout_ms: u64,
    /// Longest wait for the next bytes from the server.
    pub read_timeout_ms: u64,
    /// Largest body kept (after decoding); longer bodies are cut and flagged.
    pub max_body_bytes: usize,
    /// Largest response header section.
    pub max_header_bytes: usize,
    /// Redirects followed automatically; `0` returns 3xx responses as is.
    pub follow_redirects: u8,
    /// Advertise and decode `gzip` and `deflate`.
    pub decompress: bool,
    /// Verify the server certificate (and host name) over HTTPS.
    pub verify_tls: bool,
    /// Sent unless the request sets its own `User-Agent`; empty sends none.
    pub user_agent: String,
    /// Keep connections open for later requests to the same origin.
    pub keep_alive: bool,
    pub proxy: Option<Proxy>,
    /// Most requests in flight at once on one HTTP/2 connection.
    pub max_concurrent_streams: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: VersionPolicy::Auto,
            connect_timeout_ms: 5_000,
            read_timeout_ms: 10_000,
            max_body_bytes: 8 * 1024 * 1024,
            max_header_bytes: 64 * 1024,
            follow_redirects: 0,
            decompress: true,
            verify_tls: true,
            user_agent: concat!("KTRX/", env!("CARGO_PKG_VERSION")).into(),
            keep_alive: true,
            proxy: None,
            max_concurrent_streams: 32,
        }
    }
}

/// An HTTP request. Header names are case-insensitive; HTTP/2 sends them in
/// lower case. `Host` overrides the authority derived from the URL.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Self::new("GET", url)
    }

    pub fn head(url: impl Into<String>) -> Self {
        Self::new("HEAD", url)
    }

    pub fn post(url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self::new("POST", url).body(body)
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
}

/// Broad category of a failure, stable for tools to branch on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The request itself is invalid (URL, method, header).
    InvalidRequest,
    /// No connection: refused, unreachable, DNS failure, proxy refusal.
    Connect,
    Tls,
    Timeout,
    /// The server broke the protocol.
    Protocol,
    /// The connection closed or was reset.
    Closed,
    /// A configured size limit was exceeded.
    TooLarge,
    Cancelled,
    /// The run was not granted the network permission.
    Denied,
    Other,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::Connect => "connect",
            Self::Tls => "tls",
            Self::Timeout => "timeout",
            Self::Protocol => "protocol",
            Self::Closed => "closed",
            Self::TooLarge => "too_large",
            Self::Cancelled => "cancelled",
            Self::Denied => "denied",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpError {
    pub kind: ErrorKind,
    pub message: String,
    /// Safe to retry on a new connection (nothing reached the server's
    /// application).
    pub(crate) retryable: bool,
}

impl HttpError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({"kind": self.kind.as_str(), "message": self.message})
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for HttpError {}

/// One redirect that was followed.
#[derive(Clone, Debug)]
pub struct Redirect {
    pub status: u16,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug)]
pub struct Response {
    /// The URL that produced this response (after redirects).
    pub url: String,
    pub version: Version,
    pub status: u16,
    /// Reason phrase (HTTP/1.1 only).
    pub reason: Option<String>,
    /// Header fields in received order (HTTP/2 names are lower case).
    pub headers: Vec<(String, String)>,
    pub trailers: Vec<(String, String)>,
    /// The body, decoded when `Content-Encoding` was gzip or deflate.
    pub body: Vec<u8>,
    /// The body was cut at `max_body_bytes` (or the transfer was incomplete).
    pub body_truncated: bool,
    /// Body bytes received on the wire, before content decoding.
    pub wire_bytes: u64,
    /// The content coding that was decoded, if any.
    pub content_encoding: Option<String>,
    /// Why content decoding failed; `body` then holds the raw bytes.
    pub decode_error: Option<String>,
    /// Informational (1xx) statuses received first.
    pub interim: Vec<u16>,
    pub remote_address: Option<String>,
    /// Protocol chosen by ALPN.
    pub alpn: Option<String>,
    /// The host's TLS report (`socket.open` `tls_info`) for HTTPS.
    pub tls: Option<Value>,
    /// The request reused an open connection.
    pub reused_connection: bool,
    pub redirects: Vec<Redirect>,
    pub elapsed_ms: u64,
}

impl Response {
    /// First value of a header, case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Every value of a header, case-insensitively, in order.
    pub fn header_all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// The body as text (invalid UTF-8 replaced).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// A JSON description of the response. The body is included (Base64,
    /// at most `body_limit` bytes) only when `include_body` is set.
    pub fn to_json(&self, include_body: bool, body_limit: usize) -> Value {
        let pairs = |list: &[(String, String)]| -> Vec<Value> {
            list.iter().map(|(n, v)| json!([n, v])).collect()
        };
        let mut value = json!({
            "url": self.url,
            "version": self.version.as_str(),
            "status": self.status,
            "reason": self.reason,
            "headers": pairs(&self.headers),
            "trailers": pairs(&self.trailers),
            "interim": self.interim,
            "body_bytes": self.body.len(),
            "wire_bytes": self.wire_bytes,
            "body_truncated": self.body_truncated,
            "content_encoding": self.content_encoding,
            "decode_error": self.decode_error,
            "remote_address": self.remote_address,
            "alpn": self.alpn,
            "reused_connection": self.reused_connection,
            "redirects": self.redirects.iter().map(|r| json!({"status": r.status, "from": r.from, "to": r.to})).collect::<Vec<_>>(),
            "elapsed_ms": self.elapsed_ms,
        });
        if include_body {
            let count = self.body.len().min(body_limit);
            value["body_base64"] =
                json!(base64::engine::general_purpose::STANDARD.encode(&self.body[..count]));
            value["body_sampled_bytes"] = json!(count);
        }
        value
    }
}

/// A validated request, ready to be written on a connection.
pub(crate) struct Prepared {
    pub(crate) method: String,
    pub(crate) url: url::Url,
    pub(crate) scheme: String,
    /// Host to connect to (IPv6 without brackets).
    pub(crate) host: String,
    /// Host as written in URLs (IPv6 in brackets).
    pub(crate) url_host: String,
    pub(crate) port: u16,
    /// `host[:port]`, the port omitted when it is the scheme's default.
    pub(crate) authority: String,
    /// Path and query; `/` at least.
    pub(crate) path: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl Prepared {
    pub(crate) fn absolute_url(&self) -> String {
        format!("{}://{}{}", self.scheme, self.authority, self.path)
    }

    fn origin(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.url_host, self.port)
    }
}

/// `tchar` from RFC 9110 §5.6.2.
pub(crate) fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn invalid(message: impl Into<String>) -> HttpError {
    HttpError::new(ErrorKind::InvalidRequest, message)
}

fn prepare(request: &Request) -> Result<Prepared, HttpError> {
    let method = request.method.trim().to_owned();
    if method.is_empty() || method.len() > 32 || !method.bytes().all(is_token_byte) {
        return Err(invalid(format!("invalid method {:?}", request.method)));
    }
    if method.eq_ignore_ascii_case("CONNECT") {
        return Err(invalid(
            "CONNECT requests are not supported; use the proxy setting",
        ));
    }
    let url = url::Url::parse(request.url.trim())
        .map_err(|e| invalid(format!("invalid URL {:?}: {e}", request.url)))?;
    let scheme = url.scheme().to_owned();
    if scheme != "http" && scheme != "https" {
        return Err(invalid("only http:// and https:// URLs are supported"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid(
            "credentials in URLs are not supported; send an Authorization header",
        ));
    }
    let (host, url_host) = match url.host() {
        Some(url::Host::Domain(domain)) => (domain.to_owned(), domain.to_owned()),
        Some(url::Host::Ipv4(address)) => (address.to_string(), address.to_string()),
        Some(url::Host::Ipv6(address)) => (address.to_string(), format!("[{address}]")),
        None => return Err(invalid("the URL has no host")),
    };
    let port = url
        .port_or_known_default()
        .ok_or_else(|| invalid("the URL has no port"))?;
    let authority = match url.port() {
        Some(port) => format!("{url_host}:{port}"),
        None => url_host.clone(),
    };
    let mut path = url.path().to_owned();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let mut headers = Vec::with_capacity(request.headers.len());
    for (name, value) in &request.headers {
        let name = name.trim();
        if name.is_empty() || !name.bytes().all(is_token_byte) {
            return Err(invalid(format!("invalid header name {name:?}")));
        }
        if value.bytes().any(|b| matches!(b, b'\r' | b'\n' | b'\0')) {
            return Err(invalid(format!(
                "the value of {name} contains a line break or NUL byte"
            )));
        }
        headers.push((name.to_owned(), value.trim().to_owned()));
    }
    Ok(Prepared {
        method,
        url,
        scheme,
        host,
        url_host,
        port,
        authority,
        path,
        headers,
        body: request.body.clone(),
    })
}

/// Methods that may be sent twice without a different effect (RFC 9110 §9.2.2).
fn idempotent(method: &str) -> bool {
    matches!(
        method,
        "GET" | "HEAD" | "OPTIONS" | "TRACE" | "PUT" | "DELETE"
    )
}

enum Fresh<'a> {
    H1(Conn<'a>),
    H2(H2Conn<'a>),
}

/// An HTTP client holding open connections between requests. Dropping it
/// closes them.
pub struct Client<'a> {
    bridge: Bridge<'a>,
    pub config: Config,
    idle: HashMap<String, Conn<'a>>,
    multiplexed: HashMap<String, H2Conn<'a>>,
}

impl<'a> Client<'a> {
    pub fn new(bridge: Bridge<'a>) -> Self {
        Self::with_config(bridge, Config::default())
    }

    pub fn with_config(bridge: Bridge<'a>, config: Config) -> Self {
        Self {
            bridge,
            config,
            idle: HashMap::new(),
            multiplexed: HashMap::new(),
        }
    }

    /// Close every open connection.
    pub fn close(&mut self) {
        self.idle.clear();
        self.multiplexed.clear();
    }

    /// Send one request, following redirects when configured.
    pub fn send(&mut self, request: Request) -> Result<Response, HttpError> {
        self.send_chain(request, Vec::new())
    }

    /// Send `request`, continuing a redirect chain already `redirects` long.
    fn send_chain(
        &mut self,
        request: Request,
        mut redirects: Vec<Redirect>,
    ) -> Result<Response, HttpError> {
        let started = Instant::now();
        let mut current = request;
        loop {
            let mut response = self.send_once(&current)?;
            if let Some(next) = self.redirect(&current, &response, redirects.len()) {
                redirects.push(Redirect {
                    status: response.status,
                    from: current.url.clone(),
                    to: next.url.clone(),
                });
                current = next;
                continue;
            }
            response.redirects = redirects;
            response.elapsed_ms = started.elapsed().as_millis() as u64;
            return Ok(response);
        }
    }

    /// Send several requests and return their results in the same order.
    /// Requests to one origin that speaks HTTP/2 travel together over one
    /// connection; others are sent one after another on kept-alive
    /// connections.
    pub fn send_all(&mut self, requests: Vec<Request>) -> Vec<Result<Response, HttpError>> {
        let started = Instant::now();
        let mut results: Vec<Option<Result<Response, HttpError>>> =
            requests.iter().map(|_| None).collect();
        let mut groups: Vec<(String, Vec<(usize, Prepared)>)> = Vec::new();
        for (index, request) in requests.iter().enumerate() {
            match prepare(request) {
                Ok(prepared) => {
                    let origin = prepared.origin();
                    match groups.iter_mut().find(|(o, _)| *o == origin) {
                        Some((_, list)) => list.push((index, prepared)),
                        None => groups.push((origin, vec![(index, prepared)])),
                    }
                }
                Err(error) => results[index] = Some(Err(error)),
            }
        }
        for (origin, list) in groups {
            let use_h2 = if self.multiplexed.contains_key(&origin) {
                true
            } else if self.idle.contains_key(&origin) || list.len() < 2 {
                false
            } else {
                match self.connect(&list[0].1) {
                    Ok(Fresh::H2(conn)) => {
                        self.multiplexed.insert(origin.clone(), conn);
                        true
                    }
                    Ok(Fresh::H1(conn)) => {
                        self.idle.insert(origin.clone(), conn);
                        false
                    }
                    Err(error) => {
                        for (index, _) in &list {
                            results[*index] = Some(Err(error.clone()));
                        }
                        continue;
                    }
                }
            };
            if use_h2 {
                let prepared: Vec<&Prepared> = list.iter().map(|(_, p)| p).collect();
                let outcomes = self.run_h2(&origin, &prepared);
                for ((index, request), outcome) in list.iter().zip(outcomes) {
                    let outcome = match outcome {
                        Err(error) if error.retryable && idempotent(&request.method) => {
                            self.send_prepared(request)
                        }
                        other => other,
                    };
                    results[*index] = Some(outcome);
                }
            } else {
                for (index, request) in &list {
                    results[*index] = Some(self.send_prepared(request));
                }
            }
        }
        let mut finished = Vec::with_capacity(results.len());
        for (request, result) in requests.into_iter().zip(results) {
            let result = result.unwrap_or_else(|| Err(HttpError::new(ErrorKind::Other, "not run")));
            let result = result.and_then(|response| {
                let mut response = match self.redirect(&request, &response, 0) {
                    // Follow the rest of the chain one request at a time.
                    Some(next) => {
                        let first = Redirect {
                            status: response.status,
                            from: request.url.clone(),
                            to: next.url.clone(),
                        };
                        self.send_chain(next, vec![first])?
                    }
                    None => response,
                };
                response.elapsed_ms = started.elapsed().as_millis() as u64;
                Ok(response)
            });
            finished.push(result);
        }
        finished
    }

    /// One request, no redirects.
    fn send_once(&mut self, request: &Request) -> Result<Response, HttpError> {
        let started = Instant::now();
        let prepared = prepare(request)?;
        let mut response = self.send_prepared(&prepared)?;
        response.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(response)
    }

    fn send_prepared(&mut self, request: &Prepared) -> Result<Response, HttpError> {
        let origin = request.origin();
        let retry_allowed = idempotent(&request.method);
        if self.multiplexed.contains_key(&origin) {
            let outcome = self
                .run_h2(&origin, &[request])
                .pop()
                .unwrap_or_else(|| Err(HttpError::new(ErrorKind::Other, "no response")));
            match outcome {
                Err(error) if error.retryable && retry_allowed => {}
                other => return other,
            }
        }
        if let Some(mut conn) = self.idle.remove(&origin) {
            let info = conn.info.clone();
            // A connection opened by `send_all` but not used yet is fresh.
            let reused = conn.received > 0;
            match h1::exchange(
                &mut conn,
                request,
                &self.config,
                self.absolute_form(request),
            ) {
                Ok((raw, reusable)) => {
                    if reusable {
                        self.idle.insert(origin, conn);
                    }
                    return Ok(self.finish(raw, request, &info, reused));
                }
                Err(error) if error.retryable && retry_allowed => {}
                Err(error) => return Err(error),
            }
        }
        match self.connect(request)? {
            Fresh::H1(mut conn) => {
                let info = conn.info.clone();
                let (raw, reusable) = h1::exchange(
                    &mut conn,
                    request,
                    &self.config,
                    self.absolute_form(request),
                )?;
                if reusable {
                    self.idle.insert(origin, conn);
                }
                Ok(self.finish(raw, request, &info, false))
            }
            Fresh::H2(conn) => {
                self.multiplexed.insert(origin.clone(), conn);
                self.run_h2(&origin, &[request])
                    .pop()
                    .unwrap_or_else(|| Err(HttpError::new(ErrorKind::Other, "no response")))
            }
        }
    }

    /// Run requests on the pooled HTTP/2 connection for `origin`.
    fn run_h2(&mut self, origin: &str, requests: &[&Prepared]) -> Vec<Result<Response, HttpError>> {
        let Some(conn) = self.multiplexed.get_mut(origin) else {
            return requests
                .iter()
                .map(|_| Err(HttpError::new(ErrorKind::Other, "no HTTP/2 connection")))
                .collect();
        };
        let info = conn.info();
        let reused = conn.streams_opened() > 0;
        let outcomes = conn.execute(requests, &self.config);
        if !conn.reusable() {
            self.multiplexed.remove(origin);
        }
        outcomes
            .into_iter()
            .zip(requests)
            .map(|(outcome, request)| outcome.map(|raw| self.finish(raw, request, &info, reused)))
            .collect()
    }

    fn absolute_form(&self, request: &Prepared) -> bool {
        self.config.proxy.is_some() && request.scheme == "http"
    }

    /// Open a connection for `request` and settle the HTTP version.
    fn connect(&self, request: &Prepared) -> Result<Fresh<'a>, HttpError> {
        let config = &self.config;
        let https = request.scheme == "https";
        let alpn = match config.version {
            VersionPolicy::Auto => vec!["h2", "http/1.1"],
            VersionPolicy::Http1Only => vec!["http/1.1"],
            VersionPolicy::Http2Only => vec!["h2"],
        };
        let tls = TlsSettings {
            verify: config.verify_tls,
            alpn,
        };
        let conn = match (&config.proxy, https) {
            (None, _) => Conn::open(
                self.bridge,
                &request.host,
                request.port,
                https.then_some(&tls),
                config.connect_timeout_ms,
                config.read_timeout_ms,
            )?,
            (Some(proxy), true) => Conn::open_tunnel(
                self.bridge,
                proxy,
                &request.host,
                &request.url_host,
                request.port,
                &tls,
                config,
            )?,
            (Some(proxy), false) => {
                if config.version == VersionPolicy::Http2Only {
                    return Err(invalid(
                        "HTTP/2 to an http:// URL through a proxy is not supported",
                    ));
                }
                Conn::open(
                    self.bridge,
                    &proxy.host,
                    proxy.port,
                    None,
                    config.connect_timeout_ms,
                    config.read_timeout_ms,
                )?
            }
        };
        let h2 = if https {
            conn.info.alpn.as_deref() == Some("h2")
        } else {
            config.version == VersionPolicy::Http2Only
        };
        if https && config.version == VersionPolicy::Http2Only && !h2 {
            return Err(HttpError::new(
                ErrorKind::Protocol,
                format!(
                    "the server did not negotiate HTTP/2 (ALPN: {})",
                    conn.info.alpn.as_deref().unwrap_or("none")
                ),
            ));
        }
        if h2 {
            Ok(Fresh::H2(H2Conn::handshake(conn, config)?))
        } else {
            Ok(Fresh::H1(conn))
        }
    }

    /// Build the public response: decode the content and attach metadata.
    fn finish(
        &self,
        raw: RawResponse,
        request: &Prepared,
        info: &ConnInfo,
        reused: bool,
    ) -> Response {
        let encoding = raw
            .header("content-encoding")
            .map(str::to_owned)
            .unwrap_or_default();
        let mut body = raw.body;
        let mut truncated = raw.truncated;
        let mut content_encoding = None;
        let mut decode_error = None;
        if self.config.decompress && !body.is_empty() {
            match decode::content(&body, &encoding, self.config.max_body_bytes, truncated) {
                Ok(Some((decoded, cut))) => {
                    body = decoded;
                    truncated |= cut;
                    content_encoding = Some(encoding.trim().to_ascii_lowercase());
                }
                Ok(None) => {}
                Err(error) => decode_error = Some(error),
            }
        }
        Response {
            url: request.url.to_string(),
            version: raw.version.unwrap_or(Version::Http11),
            status: raw.status,
            reason: raw.reason,
            headers: raw.headers,
            trailers: raw.trailers,
            body,
            body_truncated: truncated,
            wire_bytes: raw.wire_bytes,
            content_encoding,
            decode_error,
            interim: raw.interim,
            remote_address: info.address.clone(),
            alpn: info.alpn.clone(),
            tls: info.tls.clone(),
            reused_connection: reused,
            redirects: Vec::new(),
            elapsed_ms: 0,
        }
    }

    /// The next request when `response` is a redirect that should be followed.
    fn redirect(&self, request: &Request, response: &Response, followed: usize) -> Option<Request> {
        if followed >= usize::from(self.config.follow_redirects)
            || !matches!(response.status, 301 | 302 | 303 | 307 | 308)
        {
            return None;
        }
        let location = response.header("location")?;
        let base = url::Url::parse(&request.url).ok()?;
        let target = base.join(location).ok()?;
        if !matches!(target.scheme(), "http" | "https") {
            return None;
        }
        let method = request.method.to_ascii_uppercase();
        let becomes_get = response.status == 303 && method != "HEAD"
            || matches!(response.status, 301 | 302) && method == "POST";
        let mut next = request.clone();
        next.url = target.to_string();
        if becomes_get {
            next.method = "GET".into();
            next.body.clear();
            next.headers.retain(|(name, _)| {
                ![
                    "content-type",
                    "content-length",
                    "content-encoding",
                    "transfer-encoding",
                ]
                .iter()
                .any(|h| name.eq_ignore_ascii_case(h))
            });
        }
        let same_origin = base.scheme() == target.scheme()
            && base.host_str() == target.host_str()
            && base.port_or_known_default() == target.port_or_known_default();
        if !same_origin {
            next.headers.retain(|(name, _)| {
                !["authorization", "cookie", "proxy-authorization", "host"]
                    .iter()
                    .any(|h| name.eq_ignore_ascii_case(h))
            });
        }
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_requests_and_rejects_invalid_ones() {
        let prepared = prepare(&Request::get("https://Example.com:8443/a/b?c=d#frag")).unwrap();
        assert_eq!(prepared.host, "example.com");
        assert_eq!(prepared.authority, "example.com:8443");
        assert_eq!(prepared.path, "/a/b?c=d");
        assert_eq!(prepared.origin(), "https://example.com:8443");
        let ipv6 = prepare(&Request::get("http://[::1]/")).unwrap();
        assert_eq!(ipv6.host, "::1");
        assert_eq!(ipv6.authority, "[::1]");
        assert_eq!(ipv6.absolute_url(), "http://[::1]/");
        for bad in [
            Request::get("ftp://example.com/"),
            Request::get("https://user:pw@example.com/"),
            Request::get("not a url"),
            Request::new("GE T", "https://example.com/"),
            Request::new("CONNECT", "https://example.com/"),
            Request::get("https://example.com/").header("X-Bad", "a\r\nInjected: 1"),
            Request::get("https://example.com/").header("Bad Name", "x"),
        ] {
            assert!(prepare(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn rewrites_redirected_requests() {
        let host = |_: &str, _: &str| -> Result<String, String> { Err("unused".into()) };
        let client = Client::with_config(
            &host,
            Config {
                follow_redirects: 5,
                ..Config::default()
            },
        );
        let response = |status: u16, location: &str| Response {
            url: String::new(),
            version: Version::Http11,
            status,
            reason: None,
            headers: vec![("Location".into(), location.into())],
            trailers: vec![],
            body: vec![],
            body_truncated: false,
            wire_bytes: 0,
            content_encoding: None,
            decode_error: None,
            interim: vec![],
            remote_address: None,
            alpn: None,
            tls: None,
            reused_connection: false,
            redirects: vec![],
            elapsed_ms: 0,
        };
        let post = Request::post("https://a.test/form", "x=1")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Authorization", "Bearer t");
        let next = client.redirect(&post, &response(303, "/done"), 0).unwrap();
        assert_eq!(
            (next.method.as_str(), next.url.as_str()),
            ("GET", "https://a.test/done")
        );
        assert!(next.body.is_empty());
        assert_eq!(
            next.headers.len(),
            1,
            "only Authorization stays on the same origin"
        );
        let kept = client
            .redirect(&post, &response(307, "https://a.test/again"), 0)
            .unwrap();
        assert_eq!(
            (kept.method.as_str(), kept.body.as_slice()),
            ("POST", &b"x=1"[..])
        );
        let away = client
            .redirect(&post, &response(308, "https://b.test/"), 0)
            .unwrap();
        assert!(away.headers.iter().all(|(n, _)| n != "Authorization"));
        assert!(
            client
                .redirect(&post, &response(302, "javascript:alert(1)"), 0)
                .is_none()
        );
        assert!(client.redirect(&post, &response(200, "/x"), 0).is_none());
        assert!(client.redirect(&post, &response(301, "/x"), 5).is_none());
    }
}
