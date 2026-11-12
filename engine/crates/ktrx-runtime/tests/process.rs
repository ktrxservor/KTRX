//! End-to-end checks of the `process` capability, `host.info`, and the
//! socket additions of engine 0.4: the WebAssembly probe forwards operations
//! through the real runtime, so permission gating, program allow-lists,
//! process-tree cleanup, and the audit trail are all exercised.

use base64::Engine as _;
use ktrx_core::{Manifest, Permission, ToolPackage};
use ktrx_runtime::{Approvals, ProcessApproval, RunEvent, Runtime};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn probe(permissions: Vec<Permission>, programs: Vec<&str>) -> ToolPackage {
    ToolPackage {
        manifest: Manifest {
            id: "host-probe".into(),
            name: "K~PROBE".into(),
            description: "Capability fixture".into(),
            version: "1.0.0".into(),
            author: "KTRX".into(),
            api_version: "1".into(),
            permissions,
            programs: programs.into_iter().map(String::from).collect(),
            ..Manifest::default()
        },
        wasm: include_bytes!("../../../tests/fixtures/host-probe.component.wasm").to_vec(),
    }
}

fn run(package: &ToolPackage, input: Value, approvals: Approvals) -> Value {
    let output = Runtime::new()
        .unwrap()
        .run(package, &input.to_string(), approvals)
        .unwrap();
    serde_json::from_str(&output).unwrap()
}

fn allow_processes() -> Approvals {
    Approvals {
        process: Some(ProcessApproval::default()),
        ..Approvals::default()
    }
}

fn exec(input: Value) -> Value {
    run(
        &probe(vec![Permission::Process], vec![]),
        json!({"op": "process.exec", "input": input}),
        allow_processes(),
    )
}

/// The platform shell and the arguments that make it run `script`.
fn shell(script: &str) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        ("cmd", vec!["/d".into(), "/c".into(), script.into()])
    } else {
        ("sh", vec!["-c".into(), script.into()])
    }
}

fn shell_exec(script: &str, extra: Value) -> Value {
    let (program, args) = shell(script);
    let mut input = json!({"program": program, "args": args, "output": "text"});
    for (key, value) in extra.as_object().unwrap() {
        input[key] = value.clone();
    }
    exec(input)
}

/// A command that runs for about 30 seconds and keeps its stdout open.
fn long_running() -> &'static str {
    if cfg!(windows) {
        "ping -n 30 127.0.0.1"
    } else {
        "sleep 30"
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap().replace('\r', "")
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unb64(value: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value.as_str().unwrap())
        .unwrap()
}

#[test]
fn runs_a_program_and_captures_its_output() {
    let result = shell_exec("echo hello", json!({}));
    let ok = &result["ok"];
    assert_eq!(ok["exit_code"], 0, "{result}");
    assert_eq!(ok["success"], true);
    assert_eq!(ok["timed_out"], false);
    assert_eq!(text(&ok["stdout"]).trim(), "hello");
    assert_eq!(ok["stderr"], "");
    assert!(ok["pid"].as_u64().unwrap() > 0);
    assert!(ok["program"].as_str().unwrap().len() > 3);

    // Base64 is the default output form.
    let (program, args) = shell("echo hi");
    let encoded = exec(json!({"program": program, "args": args}));
    let stdout = String::from_utf8(unb64(&encoded["ok"]["stdout_base64"])).unwrap();
    assert_eq!(stdout.trim(), "hi", "{encoded}");
    assert_eq!(encoded["ok"]["stdout_truncated"], false);
}

#[test]
fn reports_exit_codes_and_standard_error() {
    let script = if cfg!(windows) {
        "echo oops 1>&2 & exit /b 3"
    } else {
        "echo oops >&2; exit 3"
    };
    let result = shell_exec(script, json!({}));
    let ok = &result["ok"];
    assert_eq!(ok["exit_code"], 3, "{result}");
    assert_eq!(ok["success"], false);
    assert_eq!(text(&ok["stderr"]).trim(), "oops");
}

#[test]
fn passes_arguments_verbatim_without_a_shell() {
    // Shell metacharacters reach the program as plain text.
    let marker = "a & b | c ; $(d) `e` %PATH%";
    let (program, mut args) = if cfg!(windows) {
        // `cmd /c echo` prints the rest of its command line as received.
        (
            "cmd",
            vec!["/d".to_owned(), "/c".to_owned(), "echo".to_owned()],
        )
    } else {
        ("printf", vec!["%s".to_owned()])
    };
    args.push(marker.into());
    let result = exec(json!({"program": program, "args": args, "output": "text"}));
    let stdout = text(&result["ok"]["stdout"]);
    assert!(stdout.contains("a & b | c ; $(d) `e`"), "{result}");
}

#[test]
fn feeds_standard_input() {
    let result = exec(json!({
        "program": "sort",
        "stdin_base64": b64(b"pear\napple\nfig\n"),
        "output": "text",
    }));
    let lines: Vec<String> = text(&result["ok"]["stdout"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(lines, ["apple", "fig", "pear"], "{result}");
}

#[test]
fn applies_working_directory_and_environment() {
    let dir = std::env::temp_dir().join(format!("ktrx-proc-cwd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = if cfg!(windows) {
        "cd & echo %KTRX_TEST_VALUE%"
    } else {
        "pwd; echo $KTRX_TEST_VALUE"
    };
    let result = shell_exec(
        script,
        json!({"cwd": dir.to_string_lossy(), "env": {"KTRX_TEST_VALUE": "from-the-tool"}}),
    );
    let stdout = text(&result["ok"]["stdout"]);
    let canonical = std::fs::canonicalize(&dir).unwrap();
    let leaf = canonical
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(stdout.contains(&leaf), "{result}");
    assert!(stdout.contains("from-the-tool"), "{result}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn truncates_output_beyond_the_requested_limit() {
    let script = if cfg!(windows) {
        "for /l %i in (1,1,200) do @echo 0123456789"
    } else {
        "i=0; while [ $i -lt 200 ]; do echo 0123456789; i=$((i+1)); done"
    };
    let result = shell_exec(script, json!({"max_output": 100}));
    let ok = &result["ok"];
    assert_eq!(ok["stdout"].as_str().unwrap().len(), 100, "{result}");
    assert_eq!(ok["stdout_truncated"], true);
    assert!(ok["stdout_bytes"].as_u64().unwrap() >= 2_200);
    assert_eq!(ok["success"], true);
}

#[test]
fn a_timeout_terminates_the_program() {
    let started = Instant::now();
    let result = shell_exec(long_running(), json!({"timeout_ms": 500}));
    assert!(started.elapsed() < Duration::from_secs(10), "{result}");
    let ok = &result["ok"];
    assert_eq!(ok["timed_out"], true, "{result}");
    assert_eq!(ok["success"], false);
    assert_eq!(ok["killed"], true);
}

#[test]
fn exec_does_not_wait_for_programs_left_running_in_the_background() {
    let script = if cfg!(windows) {
        "start /b ping -n 30 127.0.0.1 & echo started"
    } else {
        "sleep 30 & echo started"
    };
    let started = Instant::now();
    let result = shell_exec(script, json!({}));
    assert!(started.elapsed() < Duration::from_secs(10), "{result}");
    assert!(
        text(&result["ok"]["stdout"]).contains("started"),
        "{result}"
    );
}

#[test]
fn drives_an_interactive_process() {
    let result = run(
        &probe(vec![Permission::Process], vec![]),
        json!({"steps": [
            {"op": "process.spawn", "input": {"program": "sort", "stdin": true}},
            {"op": "process.write", "input": {"process": 1, "bytes_base64": b64(b"zeta\nalpha\n")}},
            {"op": "process.write", "input": {"process": 1, "bytes_base64": b64(b"mu\n"), "close": true}},
            {"op": "process.wait", "input": {"process": 1, "timeout_ms": 10000}},
            {"op": "process.read", "input": {"process": 1, "stream": "stdout", "timeout_ms": 5000}},
            {"op": "process.read", "input": {"process": 1, "stream": "stdout", "timeout_ms": 5000}},
            {"op": "process.read", "input": {"process": 1, "stream": "stderr", "timeout_ms": 5000}},
            {"op": "process.write", "input": {"process": 1, "bytes_base64": b64(b"late")}},
            {"op": "process.close", "input": {"process": 1}},
            {"op": "process.wait", "input": {"process": 1}},
        ]}),
        allow_processes(),
    );
    let steps = result["results"].as_array().unwrap();
    assert_eq!(steps[0]["ok"]["process"], 1, "{result}");
    assert_eq!(steps[0]["ok"]["stdin"], true);
    assert_eq!(steps[1]["ok"]["written"], 11, "{result}");
    assert_eq!(steps[2]["ok"]["stdin_open"], false, "{result}");
    assert_eq!(steps[3]["ok"]["running"], false, "{result}");
    assert_eq!(steps[3]["ok"]["exit_code"], 0, "{result}");
    let mut stdout = unb64(&steps[4]["ok"]["bytes_base64"]);
    if steps[4]["ok"]["eof"] == false {
        stdout.extend(unb64(&steps[5]["ok"]["bytes_base64"]));
    }
    let stdout = String::from_utf8(stdout).unwrap().replace('\r', "");
    assert_eq!(stdout, "alpha\nmu\nzeta\n", "{result}");
    assert_eq!(steps[6]["ok"]["eof"], true, "{result}");
    assert!(steps[7]["error"].as_str().unwrap().starts_with("[closed]"));
    assert_eq!(steps[8]["ok"]["closed"], true);
    assert_eq!(steps[9]["error"], "unknown process");
}

#[test]
fn killing_a_process_ends_everything_it_started() {
    let (program, args) = shell(long_running());
    let result = run(
        &probe(vec![Permission::Process], vec![]),
        json!({"steps": [
            {"op": "process.spawn", "input": {"program": program, "args": args}},
            {"op": "process.wait", "input": {"process": 1, "timeout_ms": 200}},
            {"op": "process.kill", "input": {"process": 1}},
            // The grandchild held the stdout pipe; it closes only if the whole
            // tree is gone. Output written before the kill comes first.
            {"op": "process.read", "input": {"process": 1, "timeout_ms": 5000}},
            {"op": "process.read", "input": {"process": 1, "timeout_ms": 5000}},
            {"op": "process.read", "input": {"process": 1, "timeout_ms": 5000}},
            {"op": "process.read", "input": {"process": 1, "timeout_ms": 5000}},
            {"op": "process.read", "input": {"process": 1, "timeout_ms": 5000}},
            {"op": "process.read", "input": {"process": 1, "timeout_ms": 5000}},
            {"op": "process.read", "input": {"process": 1, "timeout_ms": 5000}},
            {"op": "process.read", "input": {"process": 1, "timeout_ms": 5000}},
        ]}),
        allow_processes(),
    );
    let steps = result["results"].as_array().unwrap();
    assert_eq!(steps[1]["ok"]["running"], true, "{result}");
    assert_eq!(steps[2]["ok"]["running"], false, "{result}");
    assert_eq!(steps[2]["ok"]["was_running"], true, "{result}");
    assert_eq!(steps[2]["ok"]["killed"], true);
    let reached_eof = steps[3..].iter().any(|step| step["ok"]["eof"] == true);
    assert!(reached_eof, "the process tree survived: {result}");
}

#[test]
fn the_run_ends_every_process_it_started() {
    let (program, args) = shell(long_running());
    let started = Instant::now();
    let result = run(
        &probe(vec![Permission::Process], vec![]),
        json!({"op": "process.spawn", "input": {"program": program, "args": args}}),
        allow_processes(),
    );
    let pid = result["ok"]["pid"].as_u64().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    // Give the operating system a moment to tear the tree down.
    let deadline = Instant::now() + Duration::from_secs(5);
    while process_exists(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert!(!process_exists(pid), "process {pid} outlived the run");
}

#[cfg(unix)]
fn process_exists(pid: u64) -> bool {
    // A reaped process no longer has a /proc entry (Linux) or answers `kill -0`.
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn process_exists(pid: u64) -> bool {
    let output = std::process::Command::new("tasklist")
        .args(["/fi", &format!("PID eq {pid}"), "/nh", "/fo", "csv"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
}

#[test]
fn the_manifest_program_list_is_enforced() {
    let package = probe(vec![Permission::Process], vec!["sort"]);
    let (shell_program, args) = shell("echo nope");
    let result = run(
        &package,
        json!({"steps": [
            {"op": "process.exec", "input": {"program": shell_program, "args": args}},
            {"op": "process.exec", "input": {"program": "./sort"}},
            {"op": "process.exec", "input": {"program": "sort", "stdin_base64": b64(b"b\na\n"), "output": "text"}},
        ]}),
        allow_processes(),
    );
    let steps = result["results"].as_array().unwrap();
    assert!(
        steps[0]["error"].as_str().unwrap().starts_with("[denied]"),
        "{result}"
    );
    assert!(
        steps[1]["error"].as_str().unwrap().contains("is a path"),
        "{result}"
    );
    assert_eq!(text(&steps[2]["ok"]["stdout"]), "a\nb\n", "{result}");
}

#[test]
fn the_user_can_narrow_the_programs_further() {
    let narrowed = Approvals {
        process: Some(ProcessApproval {
            programs: vec!["sort".into()],
        }),
        ..Approvals::default()
    };
    let (shell_program, args) = shell("echo nope");
    let step = json!({"op": "process.exec", "input": {"program": shell_program, "args": args}});
    // The manifest allows any program, the user only `sort`.
    let any = run(
        &probe(vec![Permission::Process], vec![]),
        step.clone(),
        narrowed.clone(),
    );
    assert!(
        any["error"].as_str().unwrap().starts_with("[denied]"),
        "{any}"
    );
    // The manifest allows the shell, the user only `sort`: nothing in common.
    let disjoint = run(
        &probe(vec![Permission::Process], vec![shell_program]),
        step,
        narrowed,
    );
    let error = disjoint["error"].as_str().unwrap();
    assert!(error.contains("(none)"), "{disjoint}");
}

#[test]
fn starting_a_program_publishes_an_audit_event() {
    let runtime = Arc::new(Runtime::new().unwrap());
    let (program, args) = shell("echo audited");
    let session = runtime.start_run(
        probe(vec![Permission::Process], vec![]),
        json!({"op": "process.exec", "input": {"program": program, "args": args}}).to_string(),
        allow_processes(),
    );
    let mut seen = None;
    for event in session.events.iter() {
        match event {
            RunEvent::Process {
                pid, program, args, ..
            } => seen = Some((pid, program, args)),
            RunEvent::Finished { .. } => break,
            RunEvent::Progress { .. } => {}
        }
    }
    session.join().unwrap();
    let (pid, program, args) = seen.expect("a process event");
    assert!(pid > 0);
    assert!(
        program
            .to_ascii_lowercase()
            .contains(if cfg!(windows) { "cmd" } else { "sh" })
    );
    assert_eq!(args.last().unwrap(), "echo audited");
}

#[test]
fn host_info_describes_the_engine_and_the_granted_operations() {
    let result = run(
        &probe(vec![Permission::Network, Permission::Process], vec!["git"]),
        json!({"op": "host.info", "input": {}}),
        Approvals {
            network: true,
            ..Approvals::default()
        },
    );
    let info = &result["ok"];
    assert_eq!(info["engine"], "ktrx", "{result}");
    assert_eq!(info["engine_version"], ktrx_core::ENGINE_VERSION);
    assert_eq!(info["api_version"], "1");
    assert_eq!(info["os"], std::env::consts::OS);
    assert_eq!(info["permissions"], json!(["network"]));
    assert_eq!(info["programs"], Value::Null);
    let operations = info["operations"].as_array().unwrap();
    let granted = |name: &str| {
        operations
            .iter()
            .find(|op| op["operation"] == name)
            .unwrap_or_else(|| panic!("{name} missing"))["granted"]
            .clone()
    };
    assert_eq!(granted("host.info"), true);
    assert_eq!(granted("socket.starttls"), true);
    assert_eq!(granted("process.exec"), false);
    assert_eq!(granted("file.read"), false);

    let with_processes = run(
        &probe(vec![Permission::Process], vec!["git"]),
        json!({"op": "host.info", "input": {}}),
        allow_processes(),
    );
    assert_eq!(with_processes["ok"]["programs"], json!(["git"]));
    assert_eq!(with_processes["ok"]["permissions"], json!(["process"]));
}

mod sockets {
    use super::*;
    use openssl::{
        asn1::Asn1Time,
        bn::BigNum,
        hash::MessageDigest,
        pkey::PKey,
        rsa::Rsa,
        ssl::{SslAcceptor, SslMethod},
        x509::{X509Builder, X509NameBuilder},
    };

    fn acceptor() -> SslAcceptor {
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "starttls.test").unwrap();
        let name = name.build();
        let mut builder = X509Builder::new().unwrap();
        builder.set_version(2).unwrap();
        builder
            .set_serial_number(&BigNum::from_u32(9).unwrap().to_asn1_integer().unwrap())
            .unwrap();
        builder.set_subject_name(&name).unwrap();
        builder.set_issuer_name(&name).unwrap();
        builder.set_pubkey(&key).unwrap();
        builder
            .set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        builder
            .set_not_after(&Asn1Time::days_from_now(2).unwrap())
            .unwrap();
        builder.sign(&key, MessageDigest::sha256()).unwrap();
        let mut acceptor = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).unwrap();
        acceptor.set_private_key(&key).unwrap();
        acceptor.set_certificate(&builder.build()).unwrap();
        acceptor.build()
    }

    #[test]
    fn upgrades_a_plaintext_socket_to_tls_and_half_closes_it() {
        let acceptor = acceptor();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"220 ready\r\n").unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert_eq!(line, "STARTTLS\r\n");
            stream.write_all(b"220 go\r\n").unwrap();
            let mut tls = acceptor.accept(stream).unwrap();
            // Read until the client half-closes, then answer.
            let mut received = Vec::new();
            tls.read_to_end(&mut received).unwrap();
            assert_eq!(received, b"ping");
            tls.write_all(b"pong").unwrap();
            let _ = tls.shutdown();
        });
        let result = run(
            &probe(vec![Permission::Network], vec![]),
            json!({"steps": [
                {"op": "socket.open", "input": {"host": "127.0.0.1", "port": port, "nodelay": true}},
                {"op": "socket.read", "input": {"socket": 1, "length": 64, "timeout_ms": 5000}},
                {"op": "socket.write", "input": {"socket": 1, "bytes_base64": b64(b"STARTTLS\r\n")}},
                {"op": "socket.read", "input": {"socket": 1, "length": 64, "timeout_ms": 5000}},
                {"op": "socket.starttls", "input": {"socket": 1, "host": "starttls.test", "verify": false}},
                {"op": "socket.write", "input": {"socket": 1, "bytes_base64": b64(b"ping")}},
                {"op": "socket.shutdown", "input": {"socket": 1, "how": "write"}},
                {"op": "socket.read", "input": {"socket": 1, "length": 64, "timeout_ms": 5000}},
                {"op": "socket.starttls", "input": {"socket": 1, "verify": false}},
                {"op": "socket.close", "input": {"socket": 1}},
            ]}),
            Approvals {
                network: true,
                ..Approvals::default()
            },
        );
        let steps = result["results"].as_array().unwrap();
        assert_eq!(
            unb64(&steps[1]["ok"]["bytes_base64"]),
            b"220 ready\r\n",
            "{result}"
        );
        assert_eq!(
            unb64(&steps[3]["ok"]["bytes_base64"]),
            b"220 go\r\n",
            "{result}"
        );
        let upgraded = &steps[4]["ok"];
        assert_eq!(upgraded["socket"], 1, "{result}");
        assert_eq!(upgraded["tls_info"]["sni"], "starttls.test");
        assert_eq!(upgraded["tls_info"]["verified"], false);
        assert_eq!(
            upgraded["tls_info"]["peer_certificates"][0]["subject"],
            "CN=starttls.test"
        );
        assert_eq!(steps[6]["ok"]["shutdown"], "write", "{result}");
        assert_eq!(unb64(&steps[7]["ok"]["bytes_base64"]), b"pong", "{result}");
        assert!(
            steps[8]["error"]
                .as_str()
                .unwrap()
                .contains("already uses TLS"),
            "{result}"
        );
        assert_eq!(steps[9]["ok"]["closed"], true);
        server.join().unwrap();
    }

    #[test]
    fn a_failed_upgrade_closes_the_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Not a TLS server: answer the ClientHello with plain text.
            let mut buffer = [0u8; 512];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
        });
        let result = run(
            &probe(vec![Permission::Network], vec![]),
            json!({"steps": [
                {"op": "socket.open", "input": {"host": "127.0.0.1", "port": port}},
                {"op": "socket.starttls", "input": {"socket": 1, "timeout_ms": 3000}},
                {"op": "socket.write", "input": {"socket": 1, "bytes_base64": b64(b"x")}},
                {"op": "socket.shutdown", "input": {"socket": 1, "how": "sideways"}},
            ]}),
            Approvals {
                network: true,
                ..Approvals::default()
            },
        );
        let steps = result["results"].as_array().unwrap();
        let error = steps[1]["error"].as_str().unwrap();
        assert!(error.contains("TLS handshake failed"), "{result}");
        assert!(error.contains("the socket was closed"), "{result}");
        assert_eq!(steps[2]["error"], "unknown socket", "{result}");
        assert_eq!(steps[3]["error"], "how must be write, read, or both");
        server.join().unwrap();
    }
}
