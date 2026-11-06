# Creating a KTRX tool

1. Create a standalone Rust crate under `tools/<id>/` with `crate-type = ["cdylib"]`.
2. Add `wit-bindgen` and generate bindings from `../../engine/wit`, selecting world `tool`.
3. Implement `exports::ktrx::tool::tool::Guest::run`. Parse JSON input, perform the tool's own work, and return a JSON string.
4. Use `ktrx::tool::host::request` for network or file operations. Do not access the host OS directly. Declare each required capability in the manifest and handle denial errors. Use only generic `network.resolve` and `socket.*` operations. Implement DNS policy, protocol framing, concurrency, scanning, and result interpretation in your tool; the engine must not gain a tool-specific operation. The generic socket bridge caps active connections at 128.
5. Add `manifest.toml`, examples, and unit tests. Do not declare host allowlists. Declare the network permission when needed; KTRX prompts for approval on each invocation; private and reserved destinations are permitted.
6. Build and package:

```sh
rustup target add wasm32-wasip2
cargo test --manifest-path tools/my-tool/Cargo.toml
cargo build --manifest-path tools/my-tool/Cargo.toml --release --target wasm32-wasip2
cp tools/my-tool/target/wasm32-wasip2/release/my_tool.wasm tools/my-tool/module.wasm
cargo build --manifest-path engine/Cargo.toml --release -p ktrx-cli
engine/target/release/ktrx pack tools/my-tool --output tools/my-tool/my-tool.tool
engine/target/release/ktrx validate tools/my-tool/my-tool.tool
```

The output filename follows Cargo's library name conversion: hyphens become underscores. Tool source and engine source remain independent, and a compliant component can be built without adding it to the engine workspace.
