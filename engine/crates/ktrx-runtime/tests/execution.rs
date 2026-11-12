use ktrx_core::{Manifest, ToolPackage};
use ktrx_runtime::{Approvals, Runtime};

#[test]
fn executes_a_wasi_component_through_the_ktrx_interface() {
    let manifest = Manifest {
        id: "hello-tool".into(),
        name: "Hello Tool".into(),
        description: "Runtime fixture".into(),
        version: "1.0.0".into(),
        author: "KTRX".into(),
        api_version: "1".into(),
        execution_timeout_seconds: None,
        permissions: vec![],
        arguments: vec![],
        examples: vec![],
        ..Manifest::default()
    };
    let package = ToolPackage {
        manifest,
        wasm: include_bytes!("../../../tests/fixtures/hello-tool.component.wasm").to_vec(),
    };
    let runtime = Runtime::new().unwrap();
    let output = runtime
        .run(&package, r#"{"name":"Ada"}"#, Approvals::default())
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(value["message"], "Hello, Ada!");
    assert_eq!(value["api_version"], "1");
}

#[test]
fn file_hashing_uses_the_permissioned_chunked_host_api() {
    use ktrx_core::Permission;
    use sha2::{Digest, Sha256};
    let path = std::env::temp_dir().join(format!("ktrx-hash-test-{}.bin", std::process::id()));
    let bytes = vec![b'x'; 140_000];
    std::fs::write(&path, &bytes).unwrap();
    let package = ToolPackage {
        manifest: Manifest {
            id: "hash".into(),
            name: "Hash Tool".into(),
            description: "File hash runtime fixture".into(),
            version: "1.0.0".into(),
            author: "KTRX".into(),
            api_version: "1".into(),
            execution_timeout_seconds: None,
            permissions: vec![Permission::FileRead],
            arguments: vec![],
            examples: vec![],
            ..Manifest::default()
        },
        wasm: include_bytes!("../../../tests/fixtures/hash-tool.component.wasm").to_vec(),
    };
    let runtime = Runtime::new().unwrap();
    let input = serde_json::json!({"file":path.to_string_lossy()}).to_string();

    let denied = runtime.run(&package, &input, Approvals::default()).unwrap();
    assert!(denied.contains("file-read permission was not granted"));

    let output = runtime
        .run(
            &package,
            &input,
            Approvals {
                file_read: true,
                ..Approvals::default()
            },
        )
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    let expected = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(value["hex"], expected);
    std::fs::remove_file(path).unwrap();
}
