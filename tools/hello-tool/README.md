# Hello Tool

Minimal KTRX Component Model plugin. Example from the project root: `ktrx run tools/hello-tool/hello-tool.tool --name Ada`.

Test and build independently:

```sh
cargo test --manifest-path tools/hello-tool/Cargo.toml
cargo build --manifest-path tools/hello-tool/Cargo.toml --release --target wasm32-wasip2
```
