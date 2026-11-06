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
        .clamp(1, MAX_CONCURRENCY);
    let service_detection = args
        .get("service-detection")
        .or_else(|| args.get("service_detection"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);

    let dns =
        ktrx::tool::host::request("dns.lookup", &serde_json::json!({"host": host}).to_string())
            .map_err(|e| access_error("DNS discovery failed", &e))?;
    let dns: serde_json::Value =
        serde_json::from_str(&dns).map_err(|e| format!("invalid DNS response: {e}"))?;
    let addresses = dns
        .get("addresses")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    let scan = ktrx::tool::host::request(
        "tcp.scan",
        &serde_json::json!({
            "host": host,
            "ports": ports,
            "timeout_ms": timeout_ms,
            "concurrency": concurrency,
            "service_detection": service_detection,
        })
        .to_string(),
    )
    .map_err(|e| access_error("TCP scan failed", &e))?;
    let mut scan: serde_json::Value =
        serde_json::from_str(&scan).map_err(|e| format!("invalid scan response: {e}"))?;
    let object = scan
        .as_object_mut()
        .ok_or("invalid scan response from KTRX host")?;
    object.insert("addresses".into(), serde_json::Value::Array(addresses));
    object.insert(
        "timeout_ms".into(),
        serde_json::Value::Number(timeout_ms.into()),
    );
    object.insert(
        "concurrency".into(),
        serde_json::Value::Number(concurrency.into()),
    );
    object.insert(
        "service_detection".into(),
        serde_json::Value::Bool(service_detection),
    );
    object.insert(
        "scope_note".into(),
        serde_json::Value::String("TCP connect scan. It completes a normal TCP handshake; closed and filtered are distinguished where the operating system reports connection refusal. This is not a raw SYN scan.".into()),
    );
    Ok(scan)
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
