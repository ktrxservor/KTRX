//! K~HERMES — HTTP inspector built on the KTRX SDK's HTTP/1.1 + HTTP/2 client.
//!
//! The tool sends one request (or the same request over HTTP/1.1 and HTTP/2
//! with `--compare`) and reports what came back: protocol, status, headers,
//! trailers, redirects, timing, decoding, and a summary of the TLS session.

wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });

use ktrx_net_sdk::http::{Client, Config, Proxy, Request, Response, VersionPolicy};
use serde_json::{Value, json};

struct Plugin;

const MAX_BODY_LIMIT: i64 = 1024 * 1024;
const MAX_HEADERS: usize = 64;

/// Validated arguments.
struct Options {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    data: Option<String>,
    version: VersionPolicy,
    follow: u8,
    insecure: bool,
    show_body: bool,
    body_limit: usize,
    timeout_ms: u64,
    proxy: Option<Proxy>,
    decompress: bool,
    compare: bool,
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn integer(
    args: &Value,
    key: &str,
    default: i64,
    range: std::ops::RangeInclusive<i64>,
) -> Result<i64, String> {
    let value = args.get(key).and_then(Value::as_i64).unwrap_or(default);
    if range.contains(&value) {
        Ok(value)
    } else {
        Err(format!(
            "--{key} must be between {} and {}",
            range.start(),
            range.end()
        ))
    }
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// `Name: value` lines, from `--header` and the newline-separated `--headers`.
fn parse_headers(args: &Value) -> Result<Vec<(String, String)>, String> {
    let mut lines: Vec<&str> = Vec::new();
    if let Some(one) = text(args, "header") {
        lines.push(one);
    }
    if let Some(many) = text(args, "headers") {
        lines.extend(many.split('\n'));
    }
    let mut headers = Vec::new();
    for line in lines
        .into_iter()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty())
    {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| format!("header {line:?} must look like 'Name: value'"))?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    if headers.len() > MAX_HEADERS {
        return Err(format!("at most {MAX_HEADERS} headers may be sent"));
    }
    Ok(headers)
}

fn parse_proxy(value: &str) -> Result<Proxy, String> {
    let address = value
        .strip_prefix("http://")
        .unwrap_or(value)
        .trim_end_matches('/');
    let (host, port) = address
        .rsplit_once(':')
        .ok_or("--proxy must be host:port")?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|p| *p != 0)
        .ok_or("--proxy port must be between 1 and 65535")?;
    if host.is_empty() {
        return Err("--proxy needs a host".into());
    }
    Ok(Proxy {
        host: host.to_owned(),
        port,
        authorization: None,
    })
}

fn parse(args: &Value) -> Result<Options, String> {
    let url = text(args, "url").ok_or("missing URL")?.to_owned();
    let version = match text(args, "http-version").unwrap_or("auto") {
        "auto" => VersionPolicy::Auto,
        "1.1" | "1" | "http1" => VersionPolicy::Http1Only,
        "2" | "h2" | "http2" => VersionPolicy::Http2Only,
        other => {
            return Err(format!(
                "--http-version must be auto, 1.1, or 2 (got {other})"
            ));
        }
    };
    let data = text(args, "data").map(str::to_owned);
    let method = text(args, "method")
        .map(str::to_ascii_uppercase)
        .unwrap_or_else(|| {
            if data.is_some() {
                "POST".into()
            } else {
                "GET".into()
            }
        });
    Ok(Options {
        url,
        method,
        headers: parse_headers(args)?,
        data,
        version,
        follow: integer(args, "follow", 0, 0..=20)? as u8,
        insecure: flag(args, "insecure"),
        show_body: flag(args, "body"),
        body_limit: integer(args, "body-limit", 4096, 1..=MAX_BODY_LIMIT)? as usize,
        timeout_ms: integer(args, "timeout-ms", 10_000, 100..=120_000)? as u64,
        proxy: text(args, "proxy").map(parse_proxy).transpose()?,
        decompress: !flag(args, "no-decompress"),
        compare: flag(args, "compare"),
    })
}

fn request(options: &Options) -> Request {
    let mut request = Request::new(options.method.clone(), options.url.clone());
    request.headers = options.headers.clone();
    if let Some(data) = &options.data {
        request.body = data.clone().into_bytes();
    }
    request
}

fn config(options: &Options, version: VersionPolicy) -> Config {
    Config {
        version,
        connect_timeout_ms: options.timeout_ms.min(10_000),
        read_timeout_ms: options.timeout_ms,
        max_body_bytes: 8 * 1024 * 1024,
        follow_redirects: options.follow,
        decompress: options.decompress,
        verify_tls: !options.insecure,
        proxy: options.proxy.clone(),
        ..Config::default()
    }
}

/// The interesting part of the host's TLS report, without certificate DER.
fn tls_summary(tls: &Value) -> Value {
    let leaf = &tls["peer_certificates"][0];
    json!({
        "version": tls["version"],
        "cipher": tls["cipher"],
        "alpn": tls["alpn"],
        "verified": tls["verified"],
        "verify_error": tls["verify_error"],
        "certificate": if leaf.is_null() { Value::Null } else { json!({
            "subject": leaf["subject"],
            "issuer": leaf["issuer"],
            "not_after": leaf["not_after"],
            "subject_alt_names": leaf["subject_alt_names"],
            "sha256": leaf["sha256"],
        })},
    })
}

fn describe(response: &Response, options: &Options) -> Value {
    let mut value = response.to_json(options.show_body, options.body_limit);
    if let Value::Object(fields) = &mut value {
        fields.remove("tls");
        if options.show_body {
            let shown = &response.body[..response.body.len().min(options.body_limit)];
            if let Ok(text) = std::str::from_utf8(shown) {
                fields.insert("body_text".into(), json!(text));
            }
        }
    }
    value["tls"] = response.tls.as_ref().map_or(Value::Null, tls_summary);
    value
}

fn inspect(options: &Options, host: &dyn Fn(&str, &str) -> Result<String, String>) -> Value {
    let run = |version: VersionPolicy| -> Value {
        let mut client = Client::with_config(host, config(options, version));
        match client.send(request(options)) {
            Ok(response) => describe(&response, options),
            Err(error) => json!({"error": error.to_json()}),
        }
    };
    let request_summary = json!({
        "method": options.method,
        "url": options.url,
        "headers": options.headers.iter().map(|(n, v)| json!([n, v])).collect::<Vec<_>>(),
        "body_bytes": options.data.as_ref().map_or(0, String::len),
        "proxy": options.proxy.as_ref().map(|p| format!("{}:{}", p.host, p.port)),
    });
    if !options.compare {
        let mut result = run(options.version);
        result["request"] = request_summary;
        return result;
    }
    let http1 = run(VersionPolicy::Http1Only);
    let http2 = run(VersionPolicy::Http2Only);
    json!({
        "request": request_summary,
        "comparison": compare(&http1, &http2),
        "http1": http1,
        "http2": http2,
    })
}

/// Differences between the HTTP/1.1 and HTTP/2 answers to the same request.
fn compare(http1: &Value, http2: &Value) -> Value {
    let names = |v: &Value| -> Vec<String> {
        let mut names: Vec<String> = v["headers"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|pair| pair[0].as_str().map(str::to_ascii_lowercase))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names.dedup();
        names
    };
    let (one, two) = (names(http1), names(http2));
    let only = |a: &[String], b: &[String]| {
        a.iter()
            .filter(|n| !b.contains(n))
            .cloned()
            .collect::<Vec<_>>()
    };
    json!({
        "http2_supported": http2.get("error").is_none(),
        "same_status": http1["status"] == http2["status"],
        "same_body_size": http1["body_bytes"] == http2["body_bytes"],
        "headers_only_in_http1": only(&one, &two),
        "headers_only_in_http2": only(&two, &one),
    })
}

impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        let args: Value = match serde_json::from_str(&input) {
            Ok(args) => args,
            Err(e) => return json!({"error": format!("invalid input: {e}")}).to_string(),
        };
        let options = match parse(&args) {
            Ok(options) => options,
            Err(error) => return json!({"error": error}).to_string(),
        };
        let host = |operation: &str, input: &str| ktrx::tool::host::request(operation, input);
        inspect(&options, &host).to_string()
    }
}

export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    use exports::ktrx::tool::tool::Guest;

    #[test]
    fn rejects_missing_url() {
        assert_eq!(Plugin::run("{}".into()), r#"{"error":"missing URL"}"#);
    }

    #[test]
    fn parses_options() {
        let options = parse(&json!({
            "url": "https://example.com",
            "data": "a=1",
            "headers": "Accept: text/html\nX-Test: 1",
            "header": "Cookie: a=b; c=d",
            "http-version": "2",
            "follow": 3,
            "proxy": "127.0.0.1:8080",
        }))
        .unwrap();
        assert_eq!(options.method, "POST");
        assert_eq!(options.headers.len(), 3);
        assert_eq!(options.headers[0], ("Cookie".into(), "a=b; c=d".into()));
        assert_eq!(options.version, VersionPolicy::Http2Only);
        assert_eq!(options.follow, 3);
        assert_eq!(options.proxy.unwrap().port, 8080);
        assert!(options.decompress);
    }

    #[test]
    fn rejects_invalid_options() {
        for bad in [
            json!({"url": "https://x", "http-version": "3"}),
            json!({"url": "https://x", "follow": 50}),
            json!({"url": "https://x", "headers": "no colon"}),
            json!({"url": "https://x", "proxy": "nohost"}),
            json!({"url": "https://x", "proxy": "h:0"}),
            json!({"url": "https://x", "body-limit": 0}),
        ] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn reports_request_errors_as_json() {
        let options = parse(&json!({"url": "ftp://example.com"})).unwrap();
        let host = |_: &str, _: &str| -> Result<String, String> { Err("unused".into()) };
        let result = inspect(&options, &host);
        assert_eq!(result["error"]["kind"], "invalid_request");
        assert_eq!(result["request"]["method"], "GET");
    }

    #[test]
    fn compares_header_sets() {
        let one = json!({"status": 200, "body_bytes": 5, "headers": [["Server", "x"], ["Connection", "keep-alive"]]});
        let two = json!({"status": 200, "body_bytes": 5, "headers": [["server", "x"], ["alt-svc", "h3"]]});
        let diff = compare(&one, &two);
        assert_eq!(diff["same_status"], true);
        assert_eq!(diff["headers_only_in_http1"], json!(["connection"]));
        assert_eq!(diff["headers_only_in_http2"], json!(["alt-svc"]));
    }
}
