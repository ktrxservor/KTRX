use anyhow::{Context, Result, bail};
use openssl::{
    asn1::Asn1Time,
    bn::{BigNum, MsbOption},
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkey::{PKey, Private},
    rsa::Rsa,
    ssl::{SslAcceptor, SslMethod},
    x509::{
        X509, X509NameBuilder,
        extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName},
    },
};
use reqwest::{Method, StatusCode, blocking::Client};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, IsTerminal, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

const MAX_LINE: usize = 16 * 1024;
const MAX_HEADERS: usize = 64 * 1024;
const MAX_REQUEST_BODY: usize = 8 * 1024 * 1024;
const MAX_RESPONSE_BODY: usize = 32 * 1024 * 1024;
const HISTORY_LIMIT: usize = 100;

pub fn default_ca_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".ktrx")
        .join("proxy-ca")
}

pub fn init_ca(directory: &Path) -> Result<()> {
    let cert_path = directory.join("ca.crt");
    let key_path = directory.join("ca.key");
    if cert_path.exists() || key_path.exists() {
        bail!(
            "a proxy CA already exists in {}; choose another directory to create a new one",
            directory.display()
        );
    }
    fs::create_dir_all(directory).with_context(|| format!("creating {}", directory.display()))?;
    let key = PKey::from_rsa(Rsa::generate(3072)?)?;
    let mut name = X509NameBuilder::new()?;
    name.append_entry_by_text("CN", "KTRX Local Interception CA")?;
    let name = name.build();
    let mut builder = X509::builder()?;
    builder.set_version(2)?;
    let mut serial = BigNum::new()?;
    serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
    let serial = serial.to_asn1_integer()?;
    builder.set_serial_number(&serial)?;
    builder.set_subject_name(&name)?;
    builder.set_issuer_name(&name)?;
    builder.set_pubkey(&key)?;
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(3_650)?;
    builder.set_not_before(&not_before)?;
    builder.set_not_after(&not_after)?;
    builder.append_extension(BasicConstraints::new().critical().ca().pathlen(0).build()?)?;
    builder.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()?,
    )?;
    builder.append_extension(
        SubjectAlternativeName::new()
            .dns("ktrx-local-ca.invalid")
            .build(&builder.x509v3_context(None, None))?,
    )?;
    builder.sign(&key, MessageDigest::sha256())?;
    let cert = builder.build();
    write_private_file(&key_path, &key.private_key_to_pem_pkcs8()?)?;
    fs::write(&cert_path, cert.to_pem()?)
        .with_context(|| format!("writing {}", cert_path.display()))?;
    println!(
        "Created local proxy CA:\n  Certificate: {}\n  Private key: {}\n\nImport only ca.crt into the test browser/device trust store. Keep ca.key private.",
        cert_path.display(),
        key_path.display()
    );
    Ok(())
}

#[cfg(unix)]
fn write_private_file(path: &Path, data: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(data)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(data)?;
    Ok(())
}

pub fn start(listen: SocketAddr, ca_dir: &Path) -> Result<()> {
    if !listen.ip().is_loopback() {
        bail!("the intercepting proxy can bind only to a loopback address");
    }
    if !io::stdin().is_terminal() {
        bail!("the interactive proxy requires a terminal");
    }
    let cert_path = ca_dir.join("ca.crt");
    let key_path = ca_dir.join("ca.key");
    let ca_cert = X509::from_pem(
        &fs::read(&cert_path).with_context(|| format!("reading {}", cert_path.display()))?,
    )?;
    let ca_key = PKey::private_key_from_pem(
        &fs::read(&key_path).with_context(|| format!("reading {}", key_path.display()))?,
    )?;
    let cert_key = ca_cert.public_key()?;
    if !ca_key.public_eq(&cert_key) {
        bail!("the proxy CA certificate and key do not match");
    }
    println!(
        "KTRX request proxy intercepts local HTTP and HTTPS traffic. HTTPS plaintext is visible in this terminal."
    );
    println!(
        "Trust the generated CA only on a test browser/device. CA: {}",
        cert_path.display()
    );
    eprint!("Start proxy on {listen}? [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        bail!("proxy startup cancelled");
    }
    let listener =
        TcpListener::bind(listen).with_context(|| format!("binding proxy at {listen}"))?;
    listener.set_nonblocking(true)?;
    let state = Arc::new(ProxyState::new(ca_cert, ca_key)?);
    println!("Listening at http://{listen}. Configure the test browser to use this HTTP proxy.");
    println!(
        "Commands per request: [f]orward, [e]dit, [r N] replay a saved request, [d]rop, [q]uit."
    );
    while !state.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                stream.set_nonblocking(false)?;
                let state = state.clone();
                thread::spawn(move || {
                    if let Err(error) = handle_client(stream, state) {
                        eprintln!("[{peer}] proxy error: {error:#}");
                    }
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20))
            }
            Err(error) => return Err(error.into()),
        }
    }
    println!("Proxy stopped.");
    Ok(())
}

struct ProxyState {
    ca: Mutex<(X509, PKey<Private>)>,
    tls_cache: Mutex<HashMap<String, Arc<SslAcceptor>>>,
    client: Client,
    history: Mutex<Vec<ProxyRequest>>,
    prompt: Mutex<()>,
    stop: AtomicBool,
}

impl ProxyState {
    fn new(cert: X509, key: PKey<Private>) -> Result<Self> {
        let client = Client::builder()
            .http1_only()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            ca: Mutex::new((cert, key)),
            tls_cache: Mutex::new(HashMap::new()),
            client,
            history: Mutex::new(Vec::new()),
            prompt: Mutex::new(()),
            stop: AtomicBool::new(false),
        })
    }

    fn tls_acceptor(&self, host: &str) -> Result<Arc<SslAcceptor>> {
        if let Some(acceptor) = self
            .tls_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("TLS cache lock poisoned"))?
            .get(host)
        {
            return Ok(acceptor.clone());
        }
        let ca = self
            .ca
            .lock()
            .map_err(|_| anyhow::anyhow!("CA lock poisoned"))?;
        let (cert, key) = make_leaf_cert(host, &ca.0, &ca.1)?;
        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        builder.set_certificate(&cert)?;
        builder.set_private_key(&key)?;
        builder.check_private_key()?;
        let acceptor = Arc::new(builder.build());
        let mut cache = self
            .tls_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("TLS cache lock poisoned"))?;
        if cache.len() >= 256 {
            if let Some(key) = cache.keys().next().cloned() {
                cache.remove(&key);
            }
        }
        cache.insert(host.to_owned(), acceptor.clone());
        Ok(acceptor)
    }
}

fn make_leaf_cert(host: &str, ca: &X509, ca_key: &PKey<Private>) -> Result<(X509, PKey<Private>)> {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)?;
    let key = PKey::from_ec_key(EcKey::generate(&group)?)?;
    let mut subject = X509NameBuilder::new()?;
    subject.append_entry_by_text("CN", &host.chars().take(200).collect::<String>())?;
    let subject = subject.build();
    let mut builder = X509::builder()?;
    builder.set_version(2)?;
    let mut serial = BigNum::new()?;
    serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
    let serial = serial.to_asn1_integer()?;
    builder.set_serial_number(&serial)?;
    builder.set_subject_name(&subject)?;
    builder.set_issuer_name(ca.subject_name())?;
    builder.set_pubkey(&key)?;
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(30)?;
    builder.set_not_before(&not_before)?;
    builder.set_not_after(&not_after)?;
    builder.append_extension(BasicConstraints::new().critical().build()?)?;
    builder.append_extension(
        KeyUsage::new()
            .critical()
            .digital_signature()
            .key_encipherment()
            .build()?,
    )?;
    builder.append_extension(ExtendedKeyUsage::new().server_auth().build()?)?;
    let mut san = SubjectAlternativeName::new();
    if IpAddr::from_str(host).is_ok() {
        san.ip(host);
    } else {
        san.dns(host);
    }
    builder.append_extension(san.build(&builder.x509v3_context(Some(ca), None))?)?;
    builder.sign(ca_key, MessageDigest::sha256())?;
    Ok((builder.build(), key))
}

#[derive(Clone, Debug)]
struct ProxyRequest {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    scheme: String,
    fallback_host: Option<String>,
}

#[derive(Debug)]
struct ProxyResponse {
    status: StatusCode,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

enum Decision {
    Forward(ProxyRequest),
    Drop,
    Quit,
}

fn handle_client(stream: TcpStream, state: Arc<ProxyState>) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(Duration::from_secs(120)))?;
    let mut reader = BufReader::new(stream);
    let Some(mut first) = read_request(&mut reader)? else {
        return Ok(());
    };
    if first.method.eq_ignore_ascii_case("CONNECT") {
        let authority = first.target.clone();
        let host = authority_host(&authority).context("invalid CONNECT authority")?;
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: KTRX\r\n\r\n")?;
        let tcp = reader.into_inner();
        let acceptor = state.tls_acceptor(&host)?;
        let tls = acceptor
            .accept(tcp)
            .context("TLS handshake failed; check that the test client trusts the KTRX CA")?;
        serve_requests(BufReader::new(tls), None, "https", Some(authority), state)
    } else {
        first.scheme = "http".into();
        serve_requests(reader, Some(first), "http", None, state)
    }
}

fn serve_requests<S: Read + Write>(
    mut reader: BufReader<S>,
    first: Option<ProxyRequest>,
    scheme: &str,
    fallback_host: Option<String>,
    state: Arc<ProxyState>,
) -> Result<()> {
    let mut pending = first;
    loop {
        if state.stop.load(Ordering::Relaxed) {
            break;
        }
        let mut request = match pending.take() {
            Some(request) => request,
            None => match read_request(&mut reader)? {
                Some(request) => request,
                None => break,
            },
        };
        request.scheme = scheme.to_owned();
        request.fallback_host = fallback_host.clone();
        let keep_alive = !header(&request.headers, "connection")
            .or_else(|| header(&request.headers, "proxy-connection"))
            .is_some_and(|v| v.eq_ignore_ascii_case("close"));
        let decision = intercept(request, &state)?;
        let response = match decision {
            Decision::Forward(request) => {
                remember(&state, request.clone());
                match send_upstream(&state.client, &request) {
                    Ok(response) => response,
                    Err(error) => error_response(502, &format!("KTRX upstream error: {error:#}")),
                }
            }
            Decision::Drop => error_response(403, "Request dropped by KTRX intercept decision."),
            Decision::Quit => {
                state.stop.store(true, Ordering::Relaxed);
                error_response(503, "KTRX proxy is stopping.")
            }
        };
        show_response(&response);
        write_response(reader.get_mut(), response, keep_alive)?;
        if !keep_alive || state.stop.load(Ordering::Relaxed) {
            break;
        }
    }
    Ok(())
}

fn intercept(mut request: ProxyRequest, state: &ProxyState) -> Result<Decision> {
    let _guard = state
        .prompt
        .lock()
        .map_err(|_| anyhow::anyhow!("prompt lock poisoned"))?;
    loop {
        print_request(&request);
        eprint!("[f]orward [e]dit [r N] replay [d]rop [q]uit > ");
        io::stderr().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let mut words = input.split_whitespace();
        match words.next().unwrap_or("").to_ascii_lowercase().as_str() {
            "f" | "forward" => return Ok(Decision::Forward(request)),
            "e" | "edit" => {
                edit_request(&mut request)?;
            }
            "r" | "replay" => {
                let index = words.next().and_then(|n| n.parse::<usize>().ok());
                let history = state
                    .history
                    .lock()
                    .map_err(|_| anyhow::anyhow!("history lock poisoned"))?;
                if let Some(replay) = index.and_then(|i| history.iter().rev().nth(i).cloned()) {
                    drop(history);
                    eprintln!("Replaying {} {}", replay.method, request_url(&replay)?);
                    remember(state, replay.clone());
                    match send_upstream(&state.client, &replay) {
                        Ok(response) => show_response(&response),
                        Err(error) => eprintln!("Replay failed: {error:#}"),
                    }
                    continue;
                }
                eprintln!("History index unavailable; recent requests:");
                for (i, item) in history.iter().rev().take(10).enumerate() {
                    eprintln!("  {i}: {} {}", item.method, item.target);
                }
            }
            "d" | "drop" => return Ok(Decision::Drop),
            "q" | "quit" => return Ok(Decision::Quit),
            _ => eprintln!("Choose f, e, r N, a, d or q."),
        }
    }
}

fn edit_request(request: &mut ProxyRequest) -> Result<()> {
    eprintln!(
        "Edit commands: m METHOD /path | h Header: value | x Header-Name | s old => new | done"
    );
    loop {
        eprint!("edit> ");
        io::stderr().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.eq_ignore_ascii_case("done") || line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("m ") {
            let mut parts = value.splitn(2, char::is_whitespace);
            if let (Some(method), Some(target)) = (parts.next(), parts.next()) {
                request.method = method.to_owned();
                request.target = target.trim().to_owned();
            } else {
                eprintln!("Use: m METHOD /path");
            }
        } else if let Some(value) = line.strip_prefix("h ") {
            if let Some((name, value)) = value.split_once(':') {
                let name = name.trim().to_owned();
                request
                    .headers
                    .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
                request.headers.push((name, value.trim().to_owned()));
            } else {
                eprintln!("Use: h Header-Name: value");
            }
        } else if let Some(name) = line.strip_prefix("x ") {
            request
                .headers
                .retain(|(existing, _)| !existing.eq_ignore_ascii_case(name.trim()));
        } else if let Some(value) = line.strip_prefix("s ") {
            if let Some((old, new)) = value.split_once(" => ") {
                if let Ok(body) = std::str::from_utf8(&request.body) {
                    request.body = body.replace(old, new).into_bytes();
                } else {
                    eprintln!("Body is not UTF-8; edit binary data outside this console editor.");
                }
            } else {
                eprintln!("Use: s old text => replacement");
            }
        } else {
            eprintln!("Unknown edit command.");
        }
    }
    Ok(())
}

fn print_request(request: &ProxyRequest) {
    let url =
        request_url(request).unwrap_or_else(|_| format!("{} {}", request.scheme, request.target));
    eprintln!("\n--- intercepted {} {} ---", request.method, url);
    for (name, value) in &request.headers {
        eprintln!("{name}: {value}");
    }
    if !request.body.is_empty() {
        eprintln!("\nBody: {} bytes", request.body.len());
        if let Ok(text) = std::str::from_utf8(&request.body) {
            eprintln!("{}", text.chars().take(700).collect::<String>());
        }
    }
}

fn remember(state: &ProxyState, request: ProxyRequest) {
    if let Ok(mut history) = state.history.lock() {
        history.push(request);
        if history.len() > HISTORY_LIMIT {
            history.remove(0);
        }
    }
}

fn send_upstream(client: &Client, request: &ProxyRequest) -> Result<ProxyResponse> {
    let url = request_url(request)?;
    let method = Method::from_bytes(request.method.as_bytes()).context("invalid HTTP method")?;
    let mut builder = client.request(method, &url);
    for (name, value) in &request.headers {
        if is_hop_header(name) || name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .context("invalid header name")?;
        let value =
            reqwest::header::HeaderValue::from_str(value).context("invalid header value")?;
        builder = builder.header(name, value);
    }
    let mut response = builder
        .body(request.body.clone())
        .send()
        .context("sending upstream request")?;
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().unwrap_or("<binary>").to_owned(),
            )
        })
        .collect();
    let mut body = Vec::new();
    response
        .by_ref()
        .take((MAX_RESPONSE_BODY + 1) as u64)
        .read_to_end(&mut body)?;
    if body.len() > MAX_RESPONSE_BODY {
        bail!("upstream response exceeded 32 MiB proxy limit");
    }
    Ok(ProxyResponse {
        status,
        headers,
        body,
    })
}

fn request_url(request: &ProxyRequest) -> Result<String> {
    if request.target.starts_with("http://") || request.target.starts_with("https://") {
        return Ok(request.target.clone());
    }
    let host = header(&request.headers, "host")
        .map(str::to_owned)
        .or_else(|| request.fallback_host.clone())
        .context("request has no Host header")?;
    let target = if request.target.starts_with('/') {
        request.target.clone()
    } else {
        format!("/{}", request.target)
    };
    Ok(format!("{}://{}{}", request.scheme, host, target))
}

fn write_response<S: Write>(
    stream: &mut S,
    response: ProxyResponse,
    keep_alive: bool,
) -> Result<()> {
    let reason = response.status.canonical_reason().unwrap_or("Response");
    write!(
        stream,
        "HTTP/1.1 {} {}\r\n",
        response.status.as_u16(),
        reason
    )?;
    for (name, value) in &response.headers {
        if is_hop_header(name) || name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        if value.contains(['\r', '\n']) {
            continue;
        }
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(
        stream,
        "Content-Length: {}\r\nConnection: {}\r\n\r\n",
        response.body.len(),
        if keep_alive { "keep-alive" } else { "close" }
    )?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}

fn show_response(response: &ProxyResponse) {
    eprintln!(
        "--- upstream response: {} {}, {} bytes ---",
        response.status.as_u16(),
        response.status.canonical_reason().unwrap_or(""),
        response.body.len()
    );
    for (name, value) in &response.headers {
        eprintln!("{name}: {value}");
    }
    if let Ok(text) = std::str::from_utf8(&response.body) {
        eprintln!("\n{}", text.chars().take(500).collect::<String>());
    }
}

fn error_response(code: u16, message: &str) -> ProxyResponse {
    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = format!("{message}\n").into_bytes();
    ProxyResponse {
        status,
        headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
        body,
    }
}

fn is_hop_header(name: &str) -> bool {
    [
        "connection",
        "proxy-connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ]
    .iter()
    .any(|hop| name.eq_ignore_ascii_case(hop))
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn authority_host(authority: &str) -> Option<String> {
    let host = if let Some(ipv6) = authority.strip_prefix('[') {
        ipv6.split(']').next()?
    } else {
        authority
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(authority)
    };
    (!host.is_empty()).then(|| host.to_owned())
}

fn read_request<S: Read + Write>(reader: &mut BufReader<S>) -> Result<Option<ProxyRequest>> {
    let Some(line) = read_line_limited(reader, MAX_LINE)? else {
        return Ok(None);
    };
    if line.trim().is_empty() {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().context("missing HTTP method")?.to_owned();
    let target = parts.next().context("missing request target")?.to_owned();
    let _version = parts.next().unwrap_or("HTTP/1.1");
    let mut headers = Vec::new();
    let mut header_bytes = 0;
    let mut headers_complete = false;
    while let Some(line) = read_line_limited(reader, MAX_LINE)? {
        header_bytes += line.len();
        if header_bytes > MAX_HEADERS {
            bail!("request headers exceed 64 KiB");
        }
        if line == "\r\n" || line == "\n" {
            headers_complete = true;
            break;
        }
        let (name, value) = line
            .trim_end_matches(['\r', '\n'])
            .split_once(':')
            .context("malformed HTTP header")?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    if !headers_complete {
        bail!("truncated HTTP request headers");
    }
    if header(&headers, "expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue")) {
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let mut body = Vec::new();
    if header(&headers, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    {
        read_chunked_body(reader, &mut body)?;
        headers.retain(|(name, _)| {
            !name.eq_ignore_ascii_case("transfer-encoding")
                && !name.eq_ignore_ascii_case("content-length")
        });
    } else if let Some(length) = header(&headers, "content-length") {
        let length = length.parse::<usize>().context("invalid Content-Length")?;
        if length > MAX_REQUEST_BODY {
            bail!("request body exceeds 8 MiB proxy limit");
        }
        body.resize(length, 0);
        reader.read_exact(&mut body)?;
    }
    Ok(Some(ProxyRequest {
        method,
        target,
        headers,
        body,
        scheme: "http".into(),
        fallback_host: None,
    }))
}

fn read_chunked_body<R: BufRead>(reader: &mut R, body: &mut Vec<u8>) -> Result<()> {
    loop {
        let line = read_line_limited(reader, 128)?.context("truncated chunked request")?;
        let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or("").trim(), 16)
            .context("invalid chunk size")?;
        if size == 0 {
            while let Some(trailer) = read_line_limited(reader, MAX_LINE)? {
                if trailer == "\r\n" || trailer == "\n" {
                    break;
                }
            }
            return Ok(());
        }
        if body.len().saturating_add(size) > MAX_REQUEST_BODY {
            bail!("chunked request body exceeds 8 MiB proxy limit");
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        let mut crlf = [0; 2];
        reader.read_exact(&mut crlf)?;
        if crlf != *b"\r\n" {
            bail!("invalid chunk delimiter");
        }
    }
}

fn read_line_limited<R: BufRead>(reader: &mut R, limit: usize) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                bail!("truncated HTTP line")
            };
        }
        let amount = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|i| i + 1)
            .unwrap_or(available.len());
        if bytes.len() + amount > limit {
            bail!("HTTP line exceeds configured limit");
        }
        let ended = available.get(amount.wrapping_sub(1)) == Some(&b'\n');
        bytes.extend_from_slice(&available[..amount]);
        reader.consume(amount);
        if ended {
            break;
        }
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}
