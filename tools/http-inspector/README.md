# HTTP Inspector

Performs a GET for an HTTP or HTTPS URL after interactive network approval and reports the status, headers, and response byte count. Redirects are disabled and the body is capped at 1 MiB. Example from the project root: `ktrx run tools/http-inspector/http-inspector.tool https://example.com`.

Test and build independently with `cargo test --manifest-path tools/http-inspector/Cargo.toml` and `cargo build --manifest-path tools/http-inspector/Cargo.toml --release --target wasm32-wasip2`.
