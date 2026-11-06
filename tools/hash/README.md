# Hash Tool

Computes SHA-256 for UTF-8 text or a local file. File reads are chunked through KTRX and require `file-read` permission approval. Examples from the project root: `ktrx run tools/hash/hash.tool 'Hello World'` and `ktrx run tools/hash/hash.tool --file ./document.pdf`.

Test and build independently:

```sh
cargo test --manifest-path tools/hash/Cargo.toml
cargo build --manifest-path tools/hash/Cargo.toml --release --target wasm32-wasip2
```
