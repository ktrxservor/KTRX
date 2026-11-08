//! End-to-end checks of the engine's host capabilities: a WebAssembly fixture
//! forwards operations through the real runtime, so manifest declarations,
//! per-run approvals, and the capabilities themselves are all exercised.

use base64::Engine as _;
use ktrx_core::{Manifest, Permission, ToolPackage};
use ktrx_runtime::{Approvals, Runtime};
use serde_json::{Value, json};
use std::{
    net::{TcpListener, UdpSocket},
    path::PathBuf,
    thread,
};

fn probe(permissions: Vec<Permission>) -> ToolPackage {
    ToolPackage {
        manifest: Manifest {
            id: "host-probe".into(),
            name: "K~PROBE".into(),
            description: "Capability fixture".into(),
            version: "1.0.0".into(),
            author: "KTRX".into(),
            api_version: "1".into(),
            execution_timeout_seconds: None,
            permissions,
            arguments: vec![],
            examples: vec![],
        },
        wasm: include_bytes!("../../../tests/fixtures/host-probe.component.wasm").to_vec(),
    }
}

fn all_permissions() -> Vec<Permission> {
    vec![
        Permission::Network,
        Permission::FileRead,
        Permission::FileWrite,
        Permission::RawNet,
    ]
}

fn run(package: &ToolPackage, input: Value, approvals: Approvals) -> Value {
    let runtime = Runtime::new().unwrap();
    let output = runtime.run(package, &input.to_string(), approvals).unwrap();
    serde_json::from_str(&output).unwrap()
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ktrx-cap-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[test]
fn writes_files_and_directories_inside_the_approved_directory() {
    let dir = scratch("write");
    let package = probe(all_permissions());
    let approvals = Approvals {
        file_read: true,
        file_write: Some(dir.clone()),
        ..Approvals::default()
    };
    let result = run(
        &package,
        json!({"steps": [
            {"op": "file.mkdir", "input": {"path": "reports/2026"}},
            {"op": "file.create", "input": {"path": "reports/2026/scan.json", "bytes_base64": b64(b"{\"a\":")}},
            {"op": "file.write", "input": {"path": "reports/2026/scan.json", "offset": 5, "bytes_base64": b64(b"1}")}},
            {"op": "file.append", "input": {"path": "reports/log.ndjson", "bytes_base64": b64(b"one\n")}},
            {"op": "file.append", "input": {"path": "reports/log.ndjson", "bytes_base64": b64(b"two\n")}},
            {"op": "file.close", "input": {"path": "reports/2026/scan.json"}},
            {"op": "file.close", "input": {"path": "reports/log.ndjson"}},
            {"op": "file.list", "input": {"path": dir.join("reports").to_string_lossy()}},
            {"op": "file.stat", "input": {"path": dir.join("reports/log.ndjson").to_string_lossy()}},
        ]}),
        approvals,
    );
    let steps = result["results"].as_array().unwrap();
    for (index, step) in steps.iter().enumerate() {
        assert!(step.get("ok").is_some(), "step {index} failed: {step}");
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("reports/2026/scan.json")).unwrap(),
        "{\"a\":1}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("reports/log.ndjson")).unwrap(),
        "one\ntwo\n"
    );
    let names: Vec<&str> = steps[7]["ok"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["2026", "log.ndjson"]);
    assert_eq!(steps[8]["ok"]["size"], 8);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn refuses_writes_outside_the_directory_and_overwrites_without_consent() {
    let dir = scratch("confine");
    let outside = scratch("confine-outside");
    std::fs::write(dir.join("existing.txt"), "keep").unwrap();
    let package = probe(all_permissions());
    let approvals = Approvals {
        file_write: Some(dir.clone()),
        ..Approvals::default()
    };
    let result = run(
        &package,
        json!({"steps": [
            {"op": "file.create", "input": {"path": "../escape.txt"}},
            {"op": "file.create", "input": {"path": outside.join("abs.txt").to_string_lossy()}},
            {"op": "file.create", "input": {"path": "existing.txt"}},
            {"op": "file.mkdir", "input": {"path": "../../elsewhere"}},
        ]}),
        approvals,
    );
    for step in result["results"].as_array().unwrap() {
        assert!(step.get("error").is_some(), "expected a refusal: {step}");
    }
    assert!(!outside.join("abs.txt").exists());
    assert!(!dir.parent().unwrap().join("escape.txt").exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("existing.txt")).unwrap(),
        "keep"
    );
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn every_capability_needs_both_a_manifest_declaration_and_an_approval() {
    let dir = scratch("gating");
    let write = json!({"op": "file.create", "input": {"path": "x.txt"}});
    let ping = json!({"op": "icmp.ping", "input": {"host": "127.0.0.1"}});
    let resolve = json!({"op": "network.resolve", "input": {"host": "localhost"}});
    let udp = json!({"op": "udp.open", "input": {}});
    let list = json!({"op": "file.list", "input": {"path": dir.to_string_lossy()}});

    // Declared but not approved.
    let nothing = Approvals::default();
    for (request, permission) in [
        (&write, "file-write"),
        (&ping, "raw-net"),
        (&resolve, "network"),
        (&udp, "network"),
        (&list, "file-read"),
    ] {
        let result = run(&probe(all_permissions()), request.clone(), nothing.clone());
        let error = result["error"].as_str().unwrap_or_default().to_owned();
        assert!(
            error.contains(&format!("{permission} permission was not granted")),
            "{request}: {result}"
        );
    }

    // Approved but not declared by the manifest.
    let everything = Approvals {
        network: true,
        file_read: true,
        file_write: Some(dir.clone()),
        raw_net: true,
    };
    for (request, permission) in [
        (&write, "file-write"),
        (&ping, "raw-net"),
        (&resolve, "network"),
        (&list, "file-read"),
    ] {
        let result = run(&probe(vec![]), request.clone(), everything.clone());
        let error = result["error"].as_str().unwrap_or_default().to_owned();
        assert!(
            error.contains(&format!("{permission} permission was not granted")),
            "{request}: {result}"
        );
    }
    assert!(!dir.join("x.txt").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unknown_operations_are_rejected() {
    let approvals = Approvals {
        network: true,
        file_read: true,
        raw_net: true,
        ..Approvals::default()
    };
    for op in ["process.exec", "file.delete", "tcp.scan", "icmp.flood"] {
        let result = run(
            &probe(all_permissions()),
            json!({"op": op, "input": {}}),
            approvals.clone(),
        );
        let error = result["error"].as_str().unwrap();
        assert!(
            error.contains("unsupported") || error.contains("not granted"),
            "{op}: {error}"
        );
    }
}

#[test]
fn exchanges_udp_datagrams_through_the_host() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let echo = thread::spawn(move || {
        let mut buffer = [0u8; 128];
        let (count, peer) = server.recv_from(&mut buffer).unwrap();
        let mut reply = b"echo:".to_vec();
        reply.extend_from_slice(&buffer[..count]);
        server.send_to(&reply, peer).unwrap();
    });
    let package = probe(all_permissions());
    // The socket id is allocated by the host, so open first and reuse it.
    let first = run(
        &package,
        json!({"steps": [
            {"op": "udp.open", "input": {}},
            {"op": "udp.send", "input": {"socket": 1, "host": "127.0.0.1", "port": port, "bytes_base64": b64(b"hello")}},
            {"op": "udp.recv", "input": {"socket": 1, "timeout_ms": 3000}},
            {"op": "udp.close", "input": {"socket": 1}},
        ]}),
        Approvals {
            network: true,
            ..Approvals::default()
        },
    );
    let steps = first["results"].as_array().unwrap();
    assert_eq!(steps[0]["ok"]["socket"], 1, "{first}");
    assert_eq!(steps[1]["ok"]["sent"], 5, "{first}");
    assert_eq!(steps[2]["ok"]["timeout"], false, "{first}");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(steps[2]["ok"]["bytes_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(bytes, b"echo:hello");
    assert_eq!(steps[3]["ok"]["closed"], true);
    echo.join().unwrap();
}

/// A one-shot DNS server that answers any question with a CNAME and an A record.
fn fake_dns_server() -> (u16, thread::JoinHandle<()>) {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let mut buffer = [0u8; 512];
        let (count, peer) = server.recv_from(&mut buffer).unwrap();
        // Keep the header and question; drop the EDNS record the client added.
        let question_end = 12 + buffer[12..count].iter().position(|b| *b == 0).unwrap() + 1 + 4;
        let mut response = buffer[..question_end].to_vec();
        response[2] = 0x81;
        response[3] = 0x80;
        response[6..8].copy_from_slice(&2u16.to_be_bytes());
        response[10..12].copy_from_slice(&0u16.to_be_bytes());
        // www.<name> CNAME -> "dangling" label + the question's parent via pointer.
        let mut cname = vec![8];
        cname.extend_from_slice(b"orphaned");
        // Point at the question name minus its first label.
        let pointer = 12 + 1 + usize::from(buffer[12]);
        cname.extend_from_slice(&[0xc0 | (pointer >> 8) as u8, pointer as u8]);
        for (rtype, rdata) in [(5u16, cname), (1u16, vec![203, 0, 113, 77])] {
            response.extend_from_slice(&[0xc0, 0x0c]);
            response.extend_from_slice(&rtype.to_be_bytes());
            response.extend_from_slice(&[0, 1]);
            response.extend_from_slice(&120u32.to_be_bytes());
            response.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
            response.extend_from_slice(&rdata);
        }
        server.send_to(&response, peer).unwrap();
    });
    (port, handle)
}

#[test]
fn queries_dns_records_through_the_host() {
    let (port, server) = fake_dns_server();
    let result = run(
        &probe(all_permissions()),
        json!({"op": "dns.query", "input": {
            "name": "www.example.com", "type": "CNAME",
            "server": "127.0.0.1", "port": port, "timeout_ms": 3000,
        }}),
        Approvals {
            network: true,
            ..Approvals::default()
        },
    );
    let answer = &result["ok"];
    assert_eq!(answer["rcode"], "NOERROR", "{result}");
    assert_eq!(answer["transport"], "udp");
    assert_eq!(answer["answers"][0]["type"], "CNAME");
    assert_eq!(
        answer["answers"][0]["data"]["target"],
        "orphaned.example.com"
    );
    assert_eq!(answer["answers"][1]["data"]["address"], "203.0.113.77");
    server.join().unwrap();
}

#[test]
fn resolves_with_record_types_even_when_the_system_lookup_fails() {
    let (port, server) = fake_dns_server();
    let result = run(
        &probe(all_permissions()),
        json!({"op": "network.resolve", "input": {
            "host": "dangling.invalid", "records": ["CNAME"],
            "server": "127.0.0.1", "dns_port": port,
        }}),
        Approvals {
            network: true,
            ..Approvals::default()
        },
    );
    let resolved = &result["ok"];
    assert_eq!(
        resolved["records"][0]["data"]["target"], "orphaned.invalid",
        "{result}"
    );
    assert_eq!(resolved["rcodes"]["CNAME"], "NOERROR");
    assert!(resolved["resolution_error"].is_string(), "{result}");
    server.join().unwrap();
}

#[test]
fn raw_net_pings_loopback_or_explains_why_it_cannot() {
    let result = run(
        &probe(all_permissions()),
        json!({"op": "icmp.ping", "input": {"host": "127.0.0.1", "timeout_ms": 2000}}),
        Approvals {
            raw_net: true,
            ..Approvals::default()
        },
    );
    if let Some(ok) = result.get("ok") {
        assert_eq!(ok["status"], "reply", "{result}");
        assert_eq!(ok["address"], "127.0.0.1");
    } else {
        assert!(
            result["error"].as_str().unwrap().contains("ICMP"),
            "{result}"
        );
    }
}

mod tls {
    use super::*;
    use openssl::{
        asn1::Asn1Time,
        bn::BigNum,
        hash::MessageDigest,
        pkey::PKey,
        rsa::Rsa,
        ssl::{AlpnError, SslAcceptor, SslMethod, select_next_proto},
        x509::{X509Builder, X509NameBuilder, extension::SubjectAlternativeName},
    };

    fn self_signed_server(connections: usize) -> (u16, thread::JoinHandle<()>) {
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "ktrx-test.local").unwrap();
        let name = name.build();
        let mut builder = X509Builder::new().unwrap();
        builder.set_version(2).unwrap();
        builder
            .set_serial_number(&BigNum::from_u32(7).unwrap().to_asn1_integer().unwrap())
            .unwrap();
        builder.set_subject_name(&name).unwrap();
        builder.set_issuer_name(&name).unwrap();
        builder.set_pubkey(&key).unwrap();
        builder
            .set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        builder
            .set_not_after(&Asn1Time::days_from_now(7).unwrap())
            .unwrap();
        let san = SubjectAlternativeName::new()
            .dns("ktrx-test.local")
            .build(&builder.x509v3_context(None, None))
            .unwrap();
        builder.append_extension(san).unwrap();
        builder.sign(&key, MessageDigest::sha256()).unwrap();
        let cert = builder.build();

        let mut acceptor = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).unwrap();
        acceptor.set_private_key(&key).unwrap();
        acceptor.set_certificate(&cert).unwrap();
        acceptor.set_alpn_select_callback(|_, client| {
            select_next_proto(b"\x02h2", client).ok_or(AlpnError::NOACK)
        });
        let acceptor = acceptor.build();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            for _ in 0..connections {
                let (stream, _) = listener.accept().unwrap();
                // Failed handshakes (an untrusting client) are expected.
                let _ = acceptor.accept(stream);
            }
        });
        (port, handle)
    }

    fn open(port: u16, extra: Value) -> Value {
        let mut input = json!({"host": "127.0.0.1", "port": port, "tls": true, "timeout_ms": 5000});
        for (key, value) in extra.as_object().unwrap() {
            input[key] = value.clone();
        }
        run(
            &probe(all_permissions()),
            json!({"op": "socket.open", "input": input}),
            Approvals {
                network: true,
                ..Approvals::default()
            },
        )
    }

    #[test]
    fn reports_negotiated_parameters_and_the_certificate_when_verification_is_off() {
        let (port, server) = self_signed_server(1);
        let result = open(
            port,
            json!({"verify": false, "alpn": ["h2", "http/1.1"], "sni": "ktrx-test.local", "tls_max": "1.2"}),
        );
        let info = &result["ok"]["tls_info"];
        assert_eq!(info["version"], "TLSv1.2", "{result}");
        assert_eq!(info["alpn"], "h2");
        assert_eq!(info["sni"], "ktrx-test.local");
        assert_eq!(info["verification_enforced"], false);
        assert_eq!(info["verified"], false);
        assert!(
            info["verify_error"]
                .as_str()
                .unwrap()
                .contains("self-signed")
                || info["verify_error"]
                    .as_str()
                    .unwrap()
                    .contains("self signed"),
            "{info}"
        );
        assert!(info["cipher"].is_string());
        let leaf = &info["peer_certificates"][0];
        assert_eq!(leaf["subject"], "CN=ktrx-test.local");
        assert_eq!(leaf["self_issued"], true);
        assert_eq!(leaf["subject_alt_names"][0], "DNS:ktrx-test.local");
        assert_eq!(leaf["public_key"]["type"], "RSA");
        assert_eq!(leaf["serial"], "07");
        server.join().unwrap();
    }

    #[test]
    fn rejects_an_untrusted_certificate_by_default() {
        let (port, server) = self_signed_server(1);
        let result = open(port, json!({}));
        let error = result["error"].as_str().unwrap();
        assert!(error.contains("TLS handshake failed"), "{result}");
        server.join().unwrap();
    }

    #[test]
    fn honours_the_protocol_version_bounds() {
        let (port, server) = self_signed_server(1);
        // The server only speaks TLS 1.2 and newer, so a TLS 1.0-1.1 ceiling fails.
        let result = open(
            port,
            json!({"verify": false, "tls_min": "1.0", "tls_max": "1.1"}),
        );
        assert!(
            result["error"]
                .as_str()
                .unwrap()
                .contains("TLS handshake failed"),
            "{result}"
        );
        server.join().unwrap();
    }
}
