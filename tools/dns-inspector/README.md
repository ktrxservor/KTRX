# DNS Inspector

Resolves A and AAAA addresses through the KTRX host API after interactive network approval. Private and reserved DNS results are returned too. Example from the project root: `ktrx run tools/dns-inspector/dns-inspector.tool example.com`.

Test and build independently with `cargo test --manifest-path tools/dns-inspector/Cargo.toml` and `cargo build --manifest-path tools/dns-inspector/Cargo.toml --release --target wasm32-wasip2`.
