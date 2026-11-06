use std::{fs, path::PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn runtime_has_no_protocol_or_tool_specific_operations() {
    let root = repo_root();
    let runtime = fs::read_to_string(root.join("engine/crates/ktrx-runtime/src/lib.rs")).unwrap();
    let network =
        fs::read_to_string(root.join("engine/crates/ktrx-runtime/src/network.rs")).unwrap();
    let runtime_manifest =
        fs::read_to_string(root.join("engine/crates/ktrx-runtime/Cargo.toml")).unwrap();
    let source = format!("{runtime}\n{network}").to_ascii_lowercase();
    assert!(
        !source.contains("allow_tcp"),
        "tools must not receive ambient WASI sockets"
    );
    assert!(
        !source.contains("allow_ip_name_lookup"),
        "DNS must pass through the permissioned host bridge"
    );
    for forbidden in [
        "tcp.scan",
        "http.get",
        "tls.inspect",
        "service_guess",
        "fingerprint_tcp_service",
        "reqwest",
    ] {
        assert!(
            !source.contains(forbidden),
            "runtime source must not contain {forbidden}"
        );
    }
    assert!(
        !runtime_manifest.to_ascii_lowercase().contains("reqwest"),
        "runtime must not depend on an HTTP client"
    );
}

#[test]
fn tool_runner_does_not_embed_the_interception_proxy() {
    let root = repo_root();
    let cli = fs::read_to_string(root.join("engine/crates/ktrx-cli/src/main.rs")).unwrap();
    let manifest = fs::read_to_string(root.join("engine/crates/ktrx-cli/Cargo.toml")).unwrap();
    assert!(!cli.contains("mod proxy"));
    assert!(!cli.contains("url_target_host"));
    assert!(!cli.contains("value.get(\"error\")"));
    assert!(!manifest.contains("reqwest"));
    assert!(!root.join("engine/crates/ktrx-cli/src/proxy.rs").exists());
    assert!(root.join("proxy-app/src/proxy.rs").exists());
}
