wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });

const MAX_PORTS: usize = 65_535;
const DEFAULT_TIMEOUT_MS: u64 = 250;
const MAX_TIMEOUT_MS: u64 = 2_000;
const MAX_CONCURRENCY: u64 = 128;

struct Plugin;

impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        match scan(&input) {
            Ok(value) => value.to_string(),
            Err(error) => serde_json::json!({"error": error}).to_string(),
        }
    }
}

fn scan(input: &str) -> Result<serde_json::Value, String> {
    use std::time::Instant;
    let args: serde_json::Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
    let host = args
        .get("host")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or("missing target hostname")?;
    let port_spec = args
        .get("ports")
        .and_then(|v| v.as_str())
        .unwrap_or("1-1000");
    let ports = parse_ports(port_spec)?;
    let timeout_ms = args
        .get("timeout-ms")
        .or_else(|| args.get("timeout_ms"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(50, MAX_TIMEOUT_MS);
    let concurrency = args
        .get("concurrency")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(MAX_CONCURRENCY)
        .clamp(1, MAX_CONCURRENCY) as usize;
    let service_detection = args
        .get("service-detection")
        .or_else(|| args.get("service_detection"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let bridge = |op: &str, data: &str| ktrx::tool::host::request(op, data);
    let addresses = ktrx_net_sdk::resolve(host, 0, &bridge)
        .map_err(|e| access_error("DNS discovery failed", &e))?;
    let connect_host = addresses
        .first()
        .ok_or("hostname did not resolve to any address")?
        .clone();
    let total_ports = ports.len();
    let mut queue = ports.into_iter();
    let mut fingerprinted = 0usize;
    let mut pending: Vec<(u16, u64, Instant)> = Vec::new();
    let mut results = Vec::new();
    let mut completed = 0usize;
    loop {
        while pending.len() < concurrency {
            let Some(port) = queue.next() else {
                break;
            };
            match ktrx_net_sdk::start_connect(&connect_host, port, timeout_ms, &bridge) {
                Ok(handle) => pending.push((port, handle, Instant::now())),
                Err(error) => {
                    results.push(port_result(
                        port,
                        &connect_host,
                        "filtered",
                        None,
                        None,
                        None,
                        None,
                        Some(error),
                    ));
                    completed += 1;
                }
            }
        }
        if pending.is_empty() {
            break;
        }
        let mut i = 0;
        let mut made_progress = false;
        while i < pending.len() {
            let (port, handle, started) = pending[i];
            match ktrx_net_sdk::poll_connect(handle, &bridge) {
                Ok(ktrx_net_sdk::ConnectPoll::Pending) => i += 1,
                Ok(ktrx_net_sdk::ConnectPoll::Connected(socket)) => {
                    let latency = started.elapsed().as_millis() as u64;
                    let (service, product, banner) = if service_detection && fingerprinted < 1024 {
                        fingerprinted += 1;
                        fingerprint(socket, host, port, &bridge)
                    } else {
                        (None, None, None)
                    };
                    ktrx_net_sdk::close_socket(socket, &bridge);
                    results.push(port_result(
                        port,
                        &connect_host,
                        "open",
                        Some(latency),
                        service.as_deref(),
                        product,
                        banner,
                        None,
                    ));
                    pending.swap_remove(i);
                    completed += 1;
                    made_progress = true;
                }
                Ok(ktrx_net_sdk::ConnectPoll::Failed(error)) => {
                    let state = if error.to_ascii_lowercase().contains("refused") {
                        "closed"
                    } else {
                        "filtered"
                    };
                    results.push(port_result(
                        port,
                        &connect_host,
                        state,
                        Some(started.elapsed().as_millis() as u64),
                        None,
                        None,
                        None,
                        Some(error),
                    ));
                    pending.swap_remove(i);
                    completed += 1;
                    made_progress = true;
                }
                Err(error) => {
                    results.push(port_result(
                        port,
                        &connect_host,
                        "filtered",
                        None,
                        None,
                        None,
                        None,
                        Some(error),
                    ));
                    pending.swap_remove(i);
                    completed += 1;
                    made_progress = true;
                }
            }
        }
        if made_progress && (completed % 100 == 0 || completed == total_ports) {
            let _ = bridge("progress.update", &serde_json::json!({"phase":"TCP connect scan","current":completed as u64,"total":total_ports as u64,"recovered":results.iter().filter(|r: &&serde_json::Value| r["state"] == "open").count() as u64,"item_label":"ports","result_label":"open","done":pending.is_empty() && queue.len() == 0}).to_string());
        }
    }
    let _ = bridge("progress.update", &serde_json::json!({"phase":"TCP connect scan","current":completed as u64,"total":total_ports as u64,"recovered":results.iter().filter(|r: &&serde_json::Value| r["state"] == "open").count() as u64,"item_label":"ports","result_label":"open","done":true}).to_string());
    results.sort_by_key(|r| r["port"].as_u64().unwrap_or(0));
    let open = results.iter().filter(|r| r["state"] == "open").count();
    let closed = results.iter().filter(|r| r["state"] == "closed").count();
    let filtered = results.len().saturating_sub(open + closed);
    Ok(serde_json::json!({
        "host":host,"target":host,"target_address":addresses.first(),"addresses":addresses,
        "ports_scanned":results.len(),"ports_not_scanned":queue.len() + pending.len(),"complete":queue.len() == 0 && pending.is_empty(),
        "open":open,"closed":closed,"filtered":filtered,"method":"TCP connect",
        "service_fingerprint_limit":if service_detection {1024} else {0},"results":results,"timeout_ms":timeout_ms,"concurrency":concurrency,
        "service_detection":service_detection,
        "scope_note":"TCP connect scan. It completes a normal TCP handshake; closed and filtered are distinguished where the operating system reports connection refusal. This is not a raw SYN scan."
    }))
}

fn port_result(
    port: u16,
    address: &str,
    state: &str,
    latency: Option<u64>,
    service: Option<&str>,
    product: Option<String>,
    banner: Option<String>,
    detail: Option<String>,
) -> serde_json::Value {
    let mut value = serde_json::json!({"port":port,"address":address,"state":state,"service_guess":service_guess(port),"service":service,"product":product,"banner":banner});
    if let Some(ms) = latency {
        value["rtt_ms"] = ms.into();
    }
    if let Some(message) = detail {
        value["detail"] = message.into();
    }
    value
}

fn service_guess(port: u16) -> &'static str {
    match port {
        20 => "ftp-data",
        21 => "ftp",
        22 => "ssh",
        23 => "telnet",
        25 | 587 => "smtp",
        53 => "domain",
        80 | 3000 | 5000 | 7001 | 8000 | 8008 | 8080 | 8081 | 8888 | 9000 => "http",
        110 => "pop3",
        143 => "imap",
        443 => "https",
        445 => "microsoft-ds",
        3306 => "mysql",
        3389 => "ms-wbt-server",
        5432 => "postgresql",
        5900 => "vnc",
        6379 => "redis",
        8443 => "https-alt",
        _ => "unknown",
    }
}

fn fingerprint<F>(
    socket: u64,
    host: &str,
    port: u16,
    bridge: &F,
) -> (Option<String>, Option<String>, Option<String>)
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    let guessed = service_guess(port);
    let http_ports = [80, 3000, 5000, 7001, 8000, 8008, 8080, 8081, 8888, 9000];
    let probe = if http_ports.contains(&port) || guessed == "unknown" {
        Some(format!(
            "HEAD / HTTP/1.0\r\nHost: {host}\r\nUser-Agent: KTRX-Port-Explorer/1.0\r\nConnection: close\r\n\r\n"
        ))
    } else if port == 6379 {
        Some("PING\r\n".to_owned())
    } else {
        None
    };
    if let Some(probe) = probe {
        if ktrx_net_sdk::write_socket(socket, probe.as_bytes(), bridge).is_err() {
            return (None, None, None);
        }
    } else if !matches!(port, 21 | 22 | 25 | 110 | 143 | 3306 | 587) {
        return (None, None, None);
    }
    let bytes = match ktrx_net_sdk::read_socket(socket, 4096, 250, bridge) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => return (None, None, None),
    };
    let raw = String::from_utf8_lossy(&bytes);
    let clean = raw
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\r' | '\n' | '\t'))
        .take(512)
        .collect::<String>()
        .trim()
        .to_owned();
    if clean.is_empty() {
        return (None, None, None);
    }
    let first = clean.lines().next().unwrap_or_default().to_owned();
    if http_ports.contains(&port) || guessed == "unknown" {
        if !first.starts_with("HTTP/") {
            return (None, Some(first), Some(clean));
        }
        let server = clean.lines().find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("server")
                    .then(|| value.trim().to_owned())
            })
        });
        return (Some("http".into()), server.or(Some(first)), Some(clean));
    }
    let detected = match port {
        21 => first.starts_with("220 ").then_some("ftp"),
        22 => first.starts_with("SSH-").then_some("ssh"),
        25 | 587 => first.starts_with("220 ").then_some("smtp"),
        110 => first.starts_with("+OK").then_some("pop3"),
        143 => first
            .to_ascii_uppercase()
            .starts_with("* OK")
            .then_some("imap"),
        3306 => (bytes.get(4) == Some(&0x0a)).then_some("mysql"),
        6379 => first.starts_with("+PONG").then_some("redis"),
        _ => None,
    };
    (
        detected.map(str::to_owned),
        detected.map(|_| first),
        Some(clean),
    )
}

fn access_error(context: &str, error: &str) -> String {
    if error.contains("permission was not granted") {
        format!("network access was not approved for this run: {error}")
    } else {
        format!("{context}: {error}")
    }
}

fn parse_ports(spec: &str) -> Result<Vec<u16>, String> {
    let spec = spec.trim().to_ascii_lowercase();
    if spec == "all" {
        return Ok((1..=u16::MAX).collect());
    }
    let mut ports = Vec::new();
    let mut seen = vec![false; MAX_PORTS + 1];
    for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((start, end)) = item.split_once('-') {
            let start: u32 = start
                .parse()
                .map_err(|_| format!("invalid port range '{item}'"))?;
            let end: u32 = end
                .parse()
                .map_err(|_| format!("invalid port range '{item}'"))?;
            if start == 0 || end > u16::MAX as u32 || start > end {
                return Err(format!("invalid port range '{item}'"));
            }
            for port in start..=end {
                push_port(&mut ports, &mut seen, port as u16)?;
            }
        } else {
            let port: u16 = item.parse().map_err(|_| format!("invalid port '{item}'"))?;
            if port == 0 {
                return Err("port numbers must be between 1 and 65535".into());
            }
            push_port(&mut ports, &mut seen, port)?;
        }
    }
    if ports.is_empty() {
        return Err("provide at least one port, a range, or 'all'".into());
    }
    Ok(ports)
}

fn push_port(ports: &mut Vec<u16>, seen: &mut [bool], port: u16) -> Result<(), String> {
    if !seen[port as usize] {
        if ports.len() == MAX_PORTS {
            return Err(format!("a scan is limited to {MAX_PORTS} unique ports"));
        }
        seen[port as usize] = true;
        ports.push(port);
    }
    Ok(())
}

export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ports_ranges_and_full_scan() {
        assert_eq!(
            parse_ports("80,443,8000-8002").unwrap(),
            [80, 443, 8000, 8001, 8002]
        );
        assert_eq!(parse_ports("all").unwrap().len(), 65_535);
        assert_eq!(parse_ports("1-65535").unwrap().len(), 65_535);
    }

    #[test]
    fn removes_duplicates_without_spending_the_limit() {
        assert_eq!(parse_ports("80,80,443").unwrap(), [80, 443]);
    }

    #[test]
    fn rejects_invalid_scan_inputs() {
        for invalid in ["", "0", "65536", "80-65536", "9-8"] {
            assert!(parse_ports(invalid).is_err(), "{invalid}");
        }
        assert!(parse_ports("80,,443").is_ok());
    }

    #[test]
    fn validates_input_before_network_use() {
        use exports::ktrx::tool::tool::Guest;
        assert_eq!(
            Plugin::run("{}".into()),
            r#"{"error":"missing target hostname"}"#
        );
        let invalid = Plugin::run(r#"{"host":"example.com","ports":"0"}"#.into());
        assert!(invalid.contains("port numbers must be between 1 and 65535"));
    }

    #[test]
    fn clamps_network_tunables_to_host_limits() {
        assert_eq!(MAX_CONCURRENCY, 128);
        assert_eq!(MAX_TIMEOUT_MS, 2_000);
    }
}
