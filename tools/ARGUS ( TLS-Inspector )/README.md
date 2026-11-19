# K~ARGUS — TLS Inspector
> Argos Panoptès, gardien mythique aux cent yeux : cet outil examine les certificats et paramètres TLS exposés par un serveur.

Connects to an HTTPS URL after interactive network approval and uses the permission-gated secure socket transport to verify the certificate chain and hostname. The package is `tools/ARGUS ( TLS-Inspector )/K~ARGUS.tool` from the project root.

Test and build independently with `cargo test --manifest-path 'tools/ARGUS ( TLS-Inspector )/Cargo.toml'` and `cargo build --manifest-path 'tools/ARGUS ( TLS-Inspector )/Cargo.toml' --release --target wasm32-wasip2`.
