# KTRX

KTRX is a Rust command-line host for portable WebAssembly Component Model tools. The engine lives in `engine/`; each independently built plugin lives under `tools/`. Plugins are packaged as ZIP-based `.tool` archives containing `manifest.toml` and `module.wasm`.

## Build

Install Rust and the WebAssembly component target:

```sh
rustup target add wasm32-wasip2
cargo build --manifest-path engine/Cargo.toml --release -p ktrx-cli
```

The executable is `engine/target/release/ktrx` (or `ktrx.exe` on Windows). Ready-to-run `.tool` packages are included in each plugin directory.

Build an individual plugin, then package it:

```sh
cargo build --manifest-path tools/base64/Cargo.toml --release --target wasm32-wasip2
cp tools/base64/target/wasm32-wasip2/release/ktrx_base64_tool.wasm tools/base64/module.wasm
engine/target/release/ktrx pack tools/base64 --output tools/base64/base64.tool
```

Build each plugin separately with the same pattern. Each has its own `Cargo.toml`; building the engine does not build plugins. Network tools ask for interactive approval on every run. The destination is selected by the tool arguments; private and reserved IP destinations remain blocked by the engine.

## Use

```sh
ktrx --help
ktrx run tools/base64/base64.tool --encode 'Hello World'
ktrx run tools/base64/base64.tool --help
ktrx run tools/hash/hash.tool 'Hello World'
ktrx run tools/port-scanner/port-scanner.tool example.com --ports 22,80,443,3306,8080,8443\nktrx run tools/tech-fingerprint/tech-fingerprint.tool https://example.com --max-requests 50 --depth 2
ktrx run tools/port-scanner/port-scanner.tool example.com --ports all --timeout-ms 200 --concurrency 128
ktrx run tools/hash-audit/hash-audit.tool 900150983cd24fb0d6963f7d28e17f72 --algorithm md5 --file ./wordlist.txt
ktrx info tools/hello-tool/hello-tool.tool
ktrx describe tools/hello-tool/hello-tool.tool --json
ktrx validate tools/hello-tool/hello-tool.tool
```

When a tool requests network or local-file access, KTRX asks for approval on an interactive terminal. Non-interactive runs deny these permissions. Network operations are routed through the engine and require approval on an interactive terminal for each run. Targets are chosen at run time; private and reserved IPs are permitted. Redirects are not followed. HTTP/TLS operations have a 10-second timeout and a 1 MiB response-body cap; tools may request a Base64 sample of up to 256 KiB from an HTTP response. DNS uses the operating system resolver and its timeout behavior. The permissioned `tcp.scan` host operation performs bounded-parallel TCP connect scans and lightweight banner probes.

Tools have no overall execution deadline by default. Long-running tools can send progress updates to the interactive terminal; press `Ctrl+C` to cancel them. A manifest can still set an explicit per-tool deadline when that is desired.

Port Explorer is an independently built plugin. It scans TCP ports 1–1000 by default, supports all 65,535 ports, and reports state, latency, service guesses, and basic application banners. Choose the target hostname at launch; KTRX prompts for network approval.

Hash Audit is an offline plugin for auditing supplied raw MD5, SHA-1, SHA-2, and SHA-3 digests against a local wordlist or a generated candidate set. It reports progress as candidates are checked. Wordlist access requires approval for that file; see `tools/hash-audit/README.md`.

## Tests

```sh
cargo test --manifest-path engine/Cargo.toml
cargo test --manifest-path tools/base64/Cargo.toml
cargo test --manifest-path tools/hash/Cargo.toml
cargo test --manifest-path tools/dns-inspector/Cargo.toml
cargo test --manifest-path tools/http-inspector/Cargo.toml
cargo test --manifest-path tools/tls-inspector/Cargo.toml
cargo test --manifest-path tools/hello-tool/Cargo.toml
cargo test --manifest-path tools/port-scanner/Cargo.toml
cargo test --manifest-path tools/hash-audit/Cargo.toml
```

See [architecture](engine/docs/ARCHITECTURE.md), [tool specification](engine/docs/TOOL_SPEC.md), and [plugin development](engine/docs/TOOL_DEVELOPMENT.md).

## HTTP/HTTPS interception proxy

KTRX also includes a native interactive request proxy. See engine/docs/PROXY_WORKBENCH.md for CA setup, browser configuration, editing and replay controls, and supported protocol limits.

Commands:

    engine/target/release/ktrx proxy init-ca
    engine/target/release/ktrx proxy start
\n\n## Technology fingerprinting\n\nTech Fingerprint inspects HTTP headers, cookies, HTML, scripts, styles, same-origin links, common discovery paths, DNS, and TLS metadata. It reports version evidence and detection confidence. See tools/tech-fingerprint/README.md for its limits and options.\n
