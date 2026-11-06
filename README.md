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
cargo build --manifest-path 'tools/THOTH ( Base64 )/Cargo.toml' --release --target wasm32-wasip2
cp 'tools/THOTH ( Base64 )/target/wasm32-wasip2/release/ktrx_base64_tool.wasm' 'tools/THOTH ( Base64 )/module.wasm'
engine/target/release/ktrx pack 'tools/THOTH ( Base64 )' --output 'tools/THOTH ( Base64 )/K~THOTH.tool'
```

Build each plugin separately with the same pattern. Each has its own `Cargo.toml`; building the engine does not build plugins. Network tools ask for interactive approval on every run. The destination is selected by the tool arguments; approved private and reserved destinations are allowed.

## Use

```sh
ktrx --help
ktrx run 'tools/THOTH ( Base64 )/K~THOTH.tool' --encode 'Hello World'
ktrx run 'tools/IRIS ( URL-Codec )/K~IRIS.tool' --encode 'admin/test?x=1'
ktrx run 'tools/JANUS ( JWT-Inspector )/K~JANUS.tool' '<header>.<payload>.<signature>'
ktrx run 'tools/HEPHAESTUS ( HTML-Entities )/K~HEPHAESTUS.tool' --encode '<script>alert(1)</script>'
ktrx run 'tools/HECATE ( Hex-Codec )/K~HECATE.tool' --encode 'GET / HTTP/1.1'
ktrx run 'tools/THEMIS ( Header-Audit )/K~THEMIS.tool' --headers $'HTTP/1.1 200 OK\nSet-Cookie: sid=abc'
ktrx run 'tools/THOTH ( Base64 )/K~THOTH.tool' --help
ktrx run 'tools/HADES ( Hash )/K~HADES.tool' 'Hello World'
ktrx run 'tools/HORUS ( Port-Scanner )/K~HORUS.tool' example.com --ports 22,80,443,3306,8080,8443
ktrx run 'tools/PROMETHEUS ( Tech-Fingerprint )/K~PROMETHEUS.tool' https://example.com --max-requests 50 --depth 2
ktrx run 'tools/HORUS ( Port-Scanner )/K~HORUS.tool' example.com --ports all --timeout-ms 200 --concurrency 128
ktrx run 'tools/ANUBIS ( Hash-Audit )/K~ANUBIS.tool' 900150983cd24fb0d6963f7d28e17f72 --algorithm md5 --file ./wordlist.txt
ktrx info 'tools/APOLLO ( Hello-Tool )/K~APOLLO.tool'
ktrx describe 'tools/APOLLO ( Hello-Tool )/K~APOLLO.tool' --json
ktrx validate 'tools/APOLLO ( Hello-Tool )/K~APOLLO.tool'
```

When a tool requests network or local-file access, KTRX asks for approval on an interactive terminal. Non-interactive runs deny these permissions. Network and file access require an interactive approval for each run. Tools use generic resolver and socket byte-stream capabilities; protocol handling, scan behavior, service identification, and response limits belong to each tool. The runtime does not implement scanning or HTTP/TLS workflows.

Tools have no overall execution deadline by default. The runner streams progress events while a tool runs; use `ktrx run <tool.tool> --events` for newline-delimited JSON. Press `Ctrl+C` to request cancellation. Wasm execution stops at the next engine checkpoint; an in-flight network call may take up to its configured timeout to return. A manifest can set an explicit per-tool deadline.

Port Explorer is an independently built plugin. It scans TCP ports 1–1000 by default, supports all 65,535 ports, and reports state, latency, service guesses, and basic application banners. Choose the target hostname at launch; KTRX prompts for network approval.

Hash Audit is an offline plugin for auditing supplied raw MD5, SHA-1, SHA-2, and SHA-3 digests against a local wordlist or a generated candidate set. It reports progress as candidates are checked. Wordlist access requires approval for that file; see `tools/ANUBIS ( Hash-Audit )/README.md`.

Seven offline analysis helpers are also available: URL Codec, JWT Inspector, HTML Entity Codec, Hex Codec, HTTP Header Audit, JSON Inspector, and Regex Lab. JWT Inspector only decodes token contents and never verifies signatures; HTTP Header Audit reports context-dependent observations rather than vulnerability verdicts. See each tool's README for details.

## Tests

```sh
cargo test --manifest-path engine/Cargo.toml
cargo test --manifest-path 'tools/THOTH ( Base64 )/Cargo.toml'
cargo test --manifest-path 'tools/HADES ( Hash )/Cargo.toml'
cargo test --manifest-path 'tools/POSEIDON ( DNS-Inspector )/Cargo.toml'
cargo test --manifest-path 'tools/HERMES ( HTTP-Inspector )/Cargo.toml'
cargo test --manifest-path 'tools/ARGUS ( TLS-Inspector )/Cargo.toml'
cargo test --manifest-path 'tools/APOLLO ( Hello-Tool )/Cargo.toml'
cargo test --manifest-path 'tools/HORUS ( Port-Scanner )/Cargo.toml'
cargo test --manifest-path 'tools/ANUBIS ( Hash-Audit )/Cargo.toml'
```

See [architecture](engine/docs/ARCHITECTURE.md), [tool specification](engine/docs/TOOL_SPEC.md), and [plugin development](engine/docs/TOOL_DEVELOPMENT.md).

## HTTP/HTTPS interception proxy

The interception proxy is a separate native program outside the tool runner. See engine/docs/PROXY_WORKBENCH.md for CA setup, browser configuration, editing and replay controls, and supported protocol limits. Build it from `proxy-app/`.

Commands:

    cargo run --manifest-path proxy-app/Cargo.toml -- init-ca
    cargo run --manifest-path proxy-app/Cargo.toml -- start


## Technology fingerprinting

Tech Fingerprint inspects HTTP headers, cookies, HTML, scripts, styles, same-origin links, common discovery paths, DNS, and TLS metadata. It reports version evidence and detection confidence. See 'tools/PROMETHEUS ( Tech-Fingerprint )/README.md' for its limits and options.

## Creating tools

The complete standalone requirements for new plugins are in [KTRX_TOOL_AUTHORING_GUIDE.md](KTRX_TOOL_AUTHORING_GUIDE.md). It is the source of truth for architecture, packaging, quality checks, and the `K~MYTH` naming convention.
