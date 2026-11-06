# TLS Inspector

Connects to an HTTPS URL after interactive network approval and relies on rustls to verify the certificate chain and hostname. The package is `tools/tls-inspector/tls-inspector.tool` from the project root.

Test and build independently with `cargo test --manifest-path tools/tls-inspector/Cargo.toml` and `cargo build --manifest-path tools/tls-inspector/Cargo.toml --release --target wasm32-wasip2`.
