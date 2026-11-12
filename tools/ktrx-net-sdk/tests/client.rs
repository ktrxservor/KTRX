//! End-to-end tests of the HTTP client against local servers. A test bridge
//! implements the host's `socket.*` operations with real loopback sockets, so
//! the client runs exactly as it does inside a tool, only without TLS.

use base64::Engine as _;
use ktrx_net_sdk::{
    hpack,
    http::{Client, Config, ErrorKind, Request, Version, VersionPolicy},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

// ---------------------------------------------------------------- bridge --

#[derive(Default)]
struct Sockets {
    open: HashMap<u64, TcpStream>,
    next: u64,
    opened_total: u64,
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// The host's socket capabilities over std sockets, without TLS.
fn bridge(state: &RefCell<Sockets>, op: &str, input: &str) -> Result<String, String> {
    let request: Value = serde_json::from_str(input).unwrap();
    let mut state = state.borrow_mut();
    match op {
        "socket.open" => {
            if request["tls"] == true {
                return Err("[tls] the test bridge has no TLS".into());
            }
            let host = request["host"].as_str().unwrap();
            let port = request["port"].as_u64().unwrap();
            let stream = TcpStream::connect((host, port as u16))
                .map_err(|e| format!("[refused] connection failed: {e}"))?;
            state.next += 1;
            state.opened_total += 1;
            let id = state.next;
            state.open.insert(id, stream);
            Ok(json!({"socket": id, "address": "127.0.0.1", "tls": false}).to_string())
        }
        "socket.read" => {
            let id = request["socket"].as_u64().unwrap();
            let stream = state.open.get_mut(&id).ok_or("unknown socket")?;
            let timeout = request["timeout_ms"].as_u64().unwrap_or(5_000);
            stream
                .set_read_timeout(Some(Duration::from_millis(timeout)))
                .unwrap();
            let mut buffer = vec![0u8; request["length"].as_u64().unwrap() as usize];
            match stream.read(&mut buffer) {
                Ok(count) => Ok(
                    json!({"socket": id, "bytes_base64": b64(&buffer[..count]), "eof": count == 0})
                        .to_string(),
                ),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    Err(format!("[timeout] socket read failed: {e}"))
                }
                Err(e) => Err(format!("[reset] socket read failed: {e}")),
            }
        }
        "socket.write" => {
            let id = request["socket"].as_u64().unwrap();
            let stream = state.open.get_mut(&id).ok_or("unknown socket")?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(request["bytes_base64"].as_str().unwrap())
                .unwrap();
            stream
                .write_all(&bytes)
                .map_err(|e| format!("[reset] socket write failed: {e}"))?;
            Ok(json!({"socket": id, "written": bytes.len()}).to_string())
        }
        "socket.close" => {
            let id = request["socket"].as_u64().unwrap();
            state.open.remove(&id).ok_or("unknown socket")?;
            Ok(json!({"socket": id, "closed": true}).to_string())
        }
        other => Err(format!("unsupported host capability '{other}'")),
    }
}

/// Run `body` with a client wired to a fresh test bridge.
fn with_client<T>(config: Config, body: impl FnOnce(&mut Client<'_>, &RefCell<Sockets>) -> T) -> T {
    let state = RefCell::new(Sockets::default());
    let host = |op: &str, input: &str| bridge(&state, op, input);
    let mut client = Client::with_config(&host, config);
    let result = body(&mut client, &state);
    drop(client);
    result
}

// ------------------------------------------------------- HTTP/1.1 server --

struct H1Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_h1_request(stream: &mut TcpStream) -> Option<H1Request> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return None,
        }
    }
    let text = String::from_utf8(head).unwrap();
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap().split(' ');
    let method = first.next().unwrap().to_owned();
    let path = first.next().unwrap().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (n, v) = l.split_once(':').unwrap();
            (n.trim().to_ascii_lowercase(), v.trim().to_owned())
        })
        .collect();
    let length = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .map_or(0, |(_, v)| v.parse().unwrap());
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body).ok()?;
    Some(H1Request {
        method,
        path,
        headers,
        body,
    })
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// A scripted HTTP/1.1 server. Returns its port and a connection counter.
fn h1_server() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&connections);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            counter.fetch_add(1, Ordering::SeqCst);
            thread::spawn(move || {
                while let Some(request) = read_h1_request(&mut stream) {
                    let reply: Vec<u8> = match request.path.as_str() {
                        "/plain" => b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello".to_vec(),
                        "/chunked" => b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTrailer: X-Checksum\r\n\r\n4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\nX-Checksum: abc\r\n\r\n".to_vec(),
                        "/gzip" => {
                            let accepts = request.headers.iter().any(|(n, v)| n == "accept-encoding" && v.contains("gzip"));
                            assert!(accepts, "the client must advertise gzip");
                            let body = gzip(&b"compressed ".repeat(100));
                            let mut reply = format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
                            reply.extend(body);
                            reply
                        }
                        "/redirect" => b"HTTP/1.1 302 Found\r\nLocation: /plain\r\nContent-Length: 0\r\n\r\n".to_vec(),
                        "/loop" => b"HTTP/1.1 301 Moved\r\nLocation: /loop\r\nContent-Length: 0\r\n\r\n".to_vec(),
                        "/continue" => b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 103 Early Hints\r\nLink: </x>\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec(),
                        "/echo" => {
                            let mut body = format!("{} ", request.method).into_bytes();
                            body.extend(&request.body);
                            let mut reply = format!("HTTP/1.1 201 Created\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
                            reply.extend(body);
                            reply
                        }
                        "/big" => {
                            let mut reply = b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\n".to_vec();
                            reply.extend(vec![b'x'; 100_000]);
                            reply
                        }
                        "/head" => b"HTTP/1.1 200 OK\r\nContent-Length: 1234\r\n\r\n".to_vec(),
                        "/close" => {
                            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nuntil close");
                            let _ = stream.shutdown(Shutdown::Both);
                            return;
                        }
                        _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
                    };
                    if stream.write_all(&reply).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (port, connections)
}

#[test]
fn http1_reads_lengths_chunks_trailers_and_reuses_connections() {
    let (port, connections) = h1_server();
    with_client(Config::default(), |client, _| {
        let plain = client
            .send(Request::get(format!("http://127.0.0.1:{port}/plain")))
            .unwrap();
        assert_eq!((plain.version, plain.status), (Version::Http11, 200));
        assert_eq!(plain.reason.as_deref(), Some("OK"));
        assert_eq!(plain.body, b"hello");
        assert!(!plain.reused_connection);

        let chunked = client
            .send(Request::get(format!("http://127.0.0.1:{port}/chunked")))
            .unwrap();
        assert_eq!(chunked.text(), "Wikipedia");
        assert_eq!(
            chunked.trailers,
            [("X-Checksum".to_owned(), "abc".to_owned())]
        );
        assert!(chunked.reused_connection);

        let head = client
            .send(Request::head(format!("http://127.0.0.1:{port}/head")))
            .unwrap();
        assert!(head.body.is_empty());
        let after_head = client
            .send(Request::get(format!("http://127.0.0.1:{port}/plain")))
            .unwrap();
        assert_eq!(after_head.body, b"hello");
    });
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "keep-alive was not used"
    );
}

#[test]
fn http1_decodes_gzip_skips_informational_responses_and_sends_bodies() {
    let (port, _) = h1_server();
    with_client(Config::default(), |client, _| {
        let gz = client
            .send(Request::get(format!("http://127.0.0.1:{port}/gzip")))
            .unwrap();
        assert_eq!(gz.body, b"compressed ".repeat(100));
        assert_eq!(gz.content_encoding.as_deref(), Some("gzip"));
        assert!(gz.wire_bytes < gz.body.len() as u64);

        let interim = client
            .send(Request::get(format!("http://127.0.0.1:{port}/continue")))
            .unwrap();
        assert_eq!(interim.interim, [100, 103]);
        assert_eq!(interim.body, b"ok");

        let posted = client
            .send(Request::post(
                format!("http://127.0.0.1:{port}/echo"),
                "a=1&b=2",
            ))
            .unwrap();
        assert_eq!(posted.status, 201);
        assert_eq!(posted.text(), "POST a=1&b=2");
        let json = posted.to_json(true, 4);
        assert_eq!(json["version"], "HTTP/1.1");
        assert_eq!(json["body_sampled_bytes"], 4);
    });
}

#[test]
fn http1_handles_close_delimited_bodies_limits_and_redirects() {
    let (port, connections) = h1_server();
    let config = Config {
        max_body_bytes: 1_000,
        follow_redirects: 3,
        ..Config::default()
    };
    with_client(config, |client, _| {
        let closed = client
            .send(Request::get(format!("http://127.0.0.1:{port}/close")))
            .unwrap();
        assert_eq!(closed.text(), "until close");

        let big = client
            .send(Request::get(format!("http://127.0.0.1:{port}/big")))
            .unwrap();
        assert_eq!(big.body.len(), 1_000);
        assert!(big.body_truncated);

        let redirected = client
            .send(Request::get(format!("http://127.0.0.1:{port}/redirect")))
            .unwrap();
        assert_eq!(redirected.status, 200);
        assert_eq!(redirected.url, format!("http://127.0.0.1:{port}/plain"));
        assert_eq!(redirected.redirects.len(), 1);
        assert_eq!(redirected.redirects[0].status, 302);

        let looping = client
            .send(Request::get(format!("http://127.0.0.1:{port}/loop")))
            .unwrap();
        assert_eq!(looping.status, 301, "stops after the configured hops");
        assert_eq!(looping.redirects.len(), 3);
    });
    // The close-delimited and the truncated responses each cost a connection.
    assert!(connections.load(Ordering::SeqCst) >= 3);
}

#[test]
fn http1_retries_a_kept_alive_connection_the_server_closed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&served);
    thread::spawn(move || {
        for stream in listener.incoming().take(2) {
            let mut stream = stream.unwrap();
            // Answer one request, then drop the "kept-alive" connection.
            if read_h1_request(&mut stream).is_some() {
                count.fetch_add(1, Ordering::SeqCst);
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .unwrap();
            }
        }
    });
    with_client(Config::default(), |client, state| {
        for _ in 0..2 {
            let response = client
                .send(Request::get(format!("http://127.0.0.1:{port}/")))
                .unwrap();
            assert_eq!(response.body, b"ok");
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(state.borrow().opened_total, 2);
    });
    assert_eq!(served.load(Ordering::SeqCst), 2);
}

#[test]
fn reports_connection_failures_with_a_kind() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    with_client(Config::default(), |client, _| {
        let error = client
            .send(Request::get(format!("http://127.0.0.1:{port}/")))
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Connect, "{error}");
        let error = client.send(Request::get("gopher://x/")).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidRequest);
    });
}

// ---------------------------------------------------------- HTTP/2 server --

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

fn write_frame(stream: &mut TcpStream, kind: u8, flags: u8, id: u32, payload: &[u8]) {
    let mut out = (payload.len() as u32).to_be_bytes()[1..].to_vec();
    out.extend([kind, flags]);
    out.extend(id.to_be_bytes());
    out.extend(payload);
    stream.write_all(&out).unwrap();
}

fn read_frame(stream: &mut TcpStream) -> Option<(u8, u8, u32, Vec<u8>)> {
    let mut header = [0u8; 9];
    stream.read_exact(&mut header).ok()?;
    let length =
        (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload).ok()?;
    let id = u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff;
    Some((header[3], header[4], id, payload))
}

fn headers_block(fields: &[(&str, &str)]) -> Vec<u8> {
    let mut block = Vec::new();
    hpack::Encoder.encode(fields.iter().map(|(n, v)| (*n, v.as_bytes())), &mut block);
    block
}

struct H2Stream {
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// What the scripted HTTP/2 server observed.
#[derive(Default)]
struct H2Log {
    connections: usize,
    paths: Vec<String>,
    ping_acked: bool,
    settings_acked: bool,
    max_concurrent_seen: usize,
}

/// A scripted h2c (prior knowledge) server.
fn h2_server() -> (u16, Arc<Mutex<H2Log>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let log = Arc::new(Mutex::new(H2Log::default()));
    let shared = Arc::clone(&log);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let log = Arc::clone(&shared);
            log.lock().unwrap().connections += 1;
            thread::spawn(move || serve_h2(stream, log));
        }
    });
    (port, log)
}

fn serve_h2(mut stream: TcpStream, log: Arc<Mutex<H2Log>>) {
    let mut preface = [0u8; 24];
    if stream.read_exact(&mut preface).is_err() || preface != PREFACE {
        return;
    }
    // Small max frame size is the default; a modest concurrency limit.
    let mut settings = Vec::new();
    settings.extend(3u16.to_be_bytes());
    settings.extend(8u32.to_be_bytes());
    write_frame(&mut stream, 0x4, 0, 0, &settings);
    let mut decoder = hpack::Decoder::new(4096, 1 << 20);
    let mut open: HashMap<u32, H2Stream> = HashMap::new();
    let mut batch: Vec<u32> = Vec::new();
    let mut received_window = 0u32;
    while let Some((kind, flags, id, payload)) = read_frame(&mut stream) {
        match kind {
            0x4 => {
                if flags & 1 != 0 {
                    log.lock().unwrap().settings_acked = true;
                } else {
                    write_frame(&mut stream, 0x4, 1, 0, &[]);
                }
            }
            0x6 if flags & 1 != 0 => log.lock().unwrap().ping_acked = true,
            0x8 => {}
            0x1 => {
                assert_ne!(
                    flags & 0x4,
                    0,
                    "the test server expects single-frame headers"
                );
                let fields = decoder.decode(&payload).unwrap();
                let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
                let headers: Vec<(String, String)> =
                    fields.iter().map(|(n, v)| (text(n), text(v))).collect();
                let path = headers
                    .iter()
                    .find(|(n, _)| n == ":path")
                    .unwrap()
                    .1
                    .clone();
                log.lock().unwrap().paths.push(path.clone());
                open.insert(
                    id,
                    H2Stream {
                        path,
                        headers,
                        body: Vec::new(),
                    },
                );
                {
                    let mut log = log.lock().unwrap();
                    log.max_concurrent_seen = log.max_concurrent_seen.max(open.len());
                }
                if flags & 1 != 0 {
                    respond(&mut stream, id, &mut open, &mut batch);
                }
            }
            0x0 => {
                received_window += payload.len() as u32;
                // Return the credit so a large upload can continue.
                if received_window >= 16_384 {
                    write_frame(&mut stream, 0x8, 0, 0, &received_window.to_be_bytes());
                    write_frame(&mut stream, 0x8, 0, id, &received_window.to_be_bytes());
                    received_window = 0;
                }
                if let Some(s) = open.get_mut(&id) {
                    s.body.extend(&payload);
                }
                if flags & 1 != 0 {
                    respond(&mut stream, id, &mut open, &mut batch);
                }
            }
            0x7 => return,
            _ => {}
        }
    }
}

fn respond(
    stream: &mut TcpStream,
    id: u32,
    open: &mut HashMap<u32, H2Stream>,
    batch: &mut Vec<u32>,
) {
    let path = open[&id].path.clone();
    let done = |stream: &mut TcpStream, id: u32, open: &mut HashMap<u32, H2Stream>, body: &[u8]| {
        let length = body.len().to_string();
        write_frame(
            stream,
            0x1,
            0x4,
            id,
            &headers_block(&[(":status", "200"), ("content-length", &length)]),
        );
        write_frame(stream, 0x0, 0x1, id, body);
        open.remove(&id);
    };
    match path.as_str() {
        // Wait for all three, then answer in reverse order, padded and split.
        p if p.starts_with("/batch/") => {
            batch.push(id);
            if batch.len() == 3 {
                for id in batch.drain(..).rev() {
                    let body = open[&id].path.clone().into_bytes();
                    write_frame(
                        stream,
                        0x1,
                        0x4,
                        id,
                        &headers_block(&[(":status", "200"), ("server", "test")]),
                    );
                    let (first, second) = body.split_at(3);
                    let mut padded = vec![4u8];
                    padded.extend(first);
                    padded.extend([0u8; 4]);
                    write_frame(stream, 0x0, 0x8, id, &padded);
                    write_frame(stream, 0x0, 0x1, id, second);
                    open.remove(&id);
                }
            }
        }
        "/upload" => {
            let size = open[&id].body.len().to_string();
            done(stream, id, open, size.as_bytes());
        }
        "/agent" => {
            let agent = open[&id]
                .headers
                .iter()
                .find(|(n, _)| n == "user-agent")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            done(stream, id, open, agent.as_bytes());
        }
        "/ping" => {
            write_frame(stream, 0x6, 0, 0, b"12345678");
            done(stream, id, open, b"pong");
        }
        "/trailers" => {
            write_frame(stream, 0x1, 0x4, id, &headers_block(&[(":status", "200")]));
            write_frame(stream, 0x0, 0, id, b"data");
            write_frame(
                stream,
                0x1,
                0x5,
                id,
                &headers_block(&[("grpc-status", "0")]),
            );
            open.remove(&id);
        }
        "/interim" => {
            write_frame(
                stream,
                0x1,
                0x4,
                id,
                &headers_block(&[(":status", "103"), ("link", "</a>")]),
            );
            done(stream, id, open, b"final");
        }
        "/reset" => {
            write_frame(stream, 0x3, 0, id, &2u32.to_be_bytes());
            open.remove(&id);
        }
        "/gzip" => {
            let body = gzip(b"h2 compressed body");
            write_frame(
                stream,
                0x1,
                0x4,
                id,
                &headers_block(&[(":status", "200"), ("content-encoding", "gzip")]),
            );
            write_frame(stream, 0x0, 0x1, id, &body);
            open.remove(&id);
        }
        "/bad-length" => {
            write_frame(
                stream,
                0x1,
                0x4,
                id,
                &headers_block(&[(":status", "200"), ("content-length", "10")]),
            );
            write_frame(stream, 0x0, 0x1, id, b"short");
            open.remove(&id);
        }
        "/goaway" => {
            // Refuse this stream: a later one would be needed to process it.
            let mut payload = id.saturating_sub(2).to_be_bytes().to_vec();
            payload.extend(0u32.to_be_bytes());
            write_frame(stream, 0x7, 0, 0, &payload);
            let _ = stream.shutdown(Shutdown::Write);
        }
        _ => done(stream, id, open, b"default"),
    }
}

fn h2_config() -> Config {
    Config {
        version: VersionPolicy::Http2Only,
        ..Config::default()
    }
}

#[test]
fn http2_multiplexes_a_batch_over_one_connection() {
    let (port, log) = h2_server();
    with_client(h2_config(), |client, state| {
        let url = |p: &str| format!("http://127.0.0.1:{port}{p}");
        let results = client.send_all(vec![
            Request::get(url("/batch/one")),
            Request::get(url("/batch/two")),
            Request::get(url("/batch/three")),
        ]);
        let bodies: Vec<String> = results
            .into_iter()
            .map(|r| {
                let response = r.unwrap();
                assert_eq!(response.version, Version::Http2);
                assert_eq!(response.header("server"), Some("test"));
                response.text()
            })
            .collect();
        assert_eq!(bodies, ["/batch/one", "/batch/two", "/batch/three"]);
        // The connection stays open for later requests.
        let later = client.send(Request::get(url("/agent"))).unwrap();
        assert!(later.reused_connection);
        assert!(later.text().starts_with("KTRX/"));
        assert_eq!(state.borrow().opened_total, 1);
    });
    let log = log.lock().unwrap();
    assert_eq!(log.connections, 1);
    assert_eq!(log.max_concurrent_seen, 3);
    assert!(log.settings_acked, "the client must acknowledge SETTINGS");
}

#[test]
fn http2_uploads_with_flow_control_and_answers_pings() {
    let (port, log) = h2_server();
    with_client(h2_config(), |client, _| {
        let url = |p: &str| format!("http://127.0.0.1:{port}{p}");
        let upload = client
            .send(Request::post(url("/upload"), vec![7u8; 200_000]))
            .unwrap();
        assert_eq!(upload.text(), "200000");
        let pong = client.send(Request::get(url("/ping"))).unwrap();
        assert_eq!(pong.body, b"pong");
        // One more round trip lets the server read the PING acknowledgement.
        client.send(Request::get(url("/agent"))).unwrap();
    });
    assert!(log.lock().unwrap().ping_acked);
}

#[test]
fn http2_reads_trailers_interim_responses_and_compressed_bodies() {
    let (port, _) = h2_server();
    with_client(h2_config(), |client, _| {
        let url = |p: &str| format!("http://127.0.0.1:{port}{p}");
        let trailers = client.send(Request::get(url("/trailers"))).unwrap();
        assert_eq!(trailers.body, b"data");
        assert_eq!(
            trailers.trailers,
            [("grpc-status".to_owned(), "0".to_owned())]
        );
        let interim = client.send(Request::get(url("/interim"))).unwrap();
        assert_eq!(interim.interim, [103]);
        assert_eq!(interim.body, b"final");
        let gz = client.send(Request::get(url("/gzip"))).unwrap();
        assert_eq!(gz.body, b"h2 compressed body");
        assert_eq!(gz.content_encoding.as_deref(), Some("gzip"));
    });
}

#[test]
fn http2_reports_stream_errors_without_losing_the_connection() {
    let (port, log) = h2_server();
    with_client(h2_config(), |client, _| {
        let url = |p: &str| format!("http://127.0.0.1:{port}{p}");
        let reset = client.send(Request::get(url("/reset"))).unwrap_err();
        assert_eq!(reset.kind, ErrorKind::Closed, "{reset}");
        assert!(reset.message.contains("INTERNAL_ERROR"), "{reset}");
        let mismatch = client.send(Request::get(url("/bad-length"))).unwrap_err();
        assert_eq!(mismatch.kind, ErrorKind::Protocol, "{mismatch}");
        let fine = client.send(Request::get(url("/agent"))).unwrap();
        assert!(fine.reused_connection);
    });
    assert_eq!(log.lock().unwrap().connections, 1);
}

#[test]
fn http2_retries_an_idempotent_request_refused_by_goaway() {
    let (port, log) = h2_server();
    with_client(h2_config(), |client, _| {
        let url = |p: &str| format!("http://127.0.0.1:{port}{p}");
        client.send(Request::get(url("/agent"))).unwrap();
        // The server refuses /goaway on the first connection with GOAWAY; the
        // client retries it on a new connection, where it is refused again
        // (the script always refuses it), and reports the refusal.
        let error = client.send(Request::get(url("/goaway"))).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Closed, "{error}");
        assert!(error.message.contains("closing the connection"), "{error}");
        // A non-idempotent request is not retried.
        let posted = client.send(Request::post(url("/goaway"), "x")).unwrap_err();
        assert_eq!(posted.kind, ErrorKind::Closed);
    });
    let log = log.lock().unwrap();
    let goaways = log.paths.iter().filter(|p| *p == "/goaway").count();
    assert_eq!(
        goaways, 3,
        "GET once + one retry, POST once: {:?}",
        log.paths
    );
}

#[test]
fn http2_only_rejects_servers_without_http2() {
    let (port, _) = h1_server();
    with_client(h2_config(), |client, _| {
        let error = client
            .send(Request::get(format!("http://127.0.0.1:{port}/plain")))
            .unwrap_err();
        assert!(
            error.kind == ErrorKind::Protocol && error.message.contains("HTTP/1.x"),
            "{error}"
        );
    });
}
