# Base64 Tool

Portable Base64 encoding and decoding for UTF-8 text. Example from the project root: `ktrx run tools/base64/base64.tool --encode 'Hello World'`.

Test and build independently:

```sh
cargo test --manifest-path tools/base64/Cargo.toml
cargo build --manifest-path tools/base64/Cargo.toml --release --target wasm32-wasip2
```
