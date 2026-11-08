<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~ARGUS — TLS Inspector">
</p>

<p align="center"><sub><code>DOSSIER K~ARGUS</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~ARGUS — TLS Inspector
> Argos Panoptès, gardien mythique aux cent yeux : cet outil examine les certificats et paramètres TLS exposés par un serveur.

Connects to an HTTPS URL after interactive network approval and uses the permission-gated secure socket transport to verify the certificate chain and hostname. The package is `tools/ARGUS ( TLS-Inspector )/K~ARGUS.tool` from the project root.

Test and build independently with `cargo test --manifest-path 'tools/ARGUS ( TLS-Inspector )/Cargo.toml'` and `cargo build --manifest-path 'tools/ARGUS ( TLS-Inspector )/Cargo.toml' --release --target wasm32-wasip2`.
