# K~HERMES — HTTP Inspector
> Hermès, messager des dieux grecs : cet outil inspecte les échanges HTTP et en structure les informations utiles.

Performs a GET for an HTTP or HTTPS URL after interactive network approval and reports the status, headers, and response byte count. Redirects are disabled and the body is capped at 1 MiB. Example from the project root: `ktrx run 'tools/HERMES ( HTTP-Inspector )/K~HERMES.tool' https://example.com`.

Test and build independently with `cargo test --manifest-path 'tools/HERMES ( HTTP-Inspector )/Cargo.toml'` and `cargo build --manifest-path 'tools/HERMES ( HTTP-Inspector )/Cargo.toml' --release --target wasm32-wasip2`.
