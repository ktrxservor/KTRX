# KTRX

KTRX is a Rust command-line host for portable WebAssembly Component Model tools. The engine lives in `engine/`; each independently built plugin lives under `tools/`. Plugins are packaged as ZIP-based `.tool` archives containing `manifest.toml` and `module.wasm`.

## Catalogue des outils

Découvrez les outils disponibles et téléchargez directement leurs archives `.tool` depuis le [catalogue officiel KTRX](https://ktrxservor.github.io/KTRX/). Le catalogue se met à jour automatiquement lorsque les outils sont ajoutés ou modifiés sur `main`.

<!-- KTRX:GALLERY:START -->

#### Réseau & reconnaissance

<table>
  <tr>
    <td align="center" width="20%">
      <a href="tools/ARGUS%20(%20TLS-Inspector%20)/"><img src="tools/ARGUS%20(%20TLS-Inspector%20)/icon.png" width="140" alt="K~ARGUS"><br><b>K~ARGUS</b></a><br><sub>TLS Inspector</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/ASCLEPIUS%20(%20CVE-Radar%20)/"><img src="tools/ASCLEPIUS%20(%20CVE-Radar%20)/icon.png" width="140" alt="K~ASCLEPIUS"><br><b>K~ASCLEPIUS</b></a><br><sub>CVE Radar</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/ATHENA%20(%20JS-Audit%20)/"><img src="tools/ATHENA%20(%20JS-Audit%20)/icon.png" width="140" alt="K~ATHENA"><br><b>K~ATHENA</b></a><br><sub>JS Audit</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/CERBERUS%20(%20Subdomain-Sentinel%20)/"><img src="tools/CERBERUS%20(%20Subdomain-Sentinel%20)/icon.png" width="140" alt="K~CERBERUS"><br><b>K~CERBERUS</b></a><br><sub>Subdomain Sentinel</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/HERMES%20(%20HTTP-Inspector%20)/"><img src="tools/HERMES%20(%20HTTP-Inspector%20)/icon.png" width="140" alt="K~HERMES"><br><b>K~HERMES</b></a><br><sub>HTTP Inspector</sub>
    </td>
  </tr>
  <tr>
    <td align="center" width="20%">
      <a href="tools/HORUS%20(%20Port-Scanner%20)/"><img src="tools/HORUS%20(%20Port-Scanner%20)/icon.png" width="140" alt="K~HORUS"><br><b>K~HORUS</b></a><br><sub>Port Scanner</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/ODYSSEUS%20(%20Web-Discovery%20)/"><img src="tools/ODYSSEUS%20(%20Web-Discovery%20)/icon.png" width="140" alt="K~ODYSSEUS"><br><b>K~ODYSSEUS</b></a><br><sub>Web Discovery</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/POSEIDON%20(%20DNS-Inspector%20)/"><img src="tools/POSEIDON%20(%20DNS-Inspector%20)/icon.png" width="140" alt="K~POSEIDON"><br><b>K~POSEIDON</b></a><br><sub>DNS Inspector</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/PROMETHEUS%20(%20Tech-Fingerprint%20)/"><img src="tools/PROMETHEUS%20(%20Tech-Fingerprint%20)/icon.png" width="140" alt="K~PROMETHEUS"><br><b>K~PROMETHEUS</b></a><br><sub>Tech Fingerprint</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/THEMIS%20(%20Header-Audit%20)/"><img src="tools/THEMIS%20(%20Header-Audit%20)/icon.png" width="140" alt="K~THEMIS"><br><b>K~THEMIS</b></a><br><sub>Header Audit</sub>
    </td>
  </tr>
</table>

#### Analyse

<table>
  <tr>
    <td align="center" width="20%">
      <a href="tools/ANUBIS%20(%20Hash-Audit%20)/"><img src="tools/ANUBIS%20(%20Hash-Audit%20)/icon.png" width="140" alt="K~ANUBIS"><br><b>K~ANUBIS</b></a><br><sub>Hash Audit</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/HADES%20(%20Hash%20)/"><img src="tools/HADES%20(%20Hash%20)/icon.png" width="140" alt="K~HADES"><br><b>K~HADES</b></a><br><sub>Hash</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/JANUS%20(%20JWT-Inspector%20)/"><img src="tools/JANUS%20(%20JWT-Inspector%20)/icon.png" width="140" alt="K~JANUS"><br><b>K~JANUS</b></a><br><sub>JWT Inspector</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/METIS%20(%20JSON-Inspector%20)/"><img src="tools/METIS%20(%20JSON-Inspector%20)/icon.png" width="140" alt="K~METIS"><br><b>K~METIS</b></a><br><sub>JSON Inspector</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/SPHINX%20(%20Regex-Lab%20)/"><img src="tools/SPHINX%20(%20Regex-Lab%20)/icon.png" width="140" alt="K~SPHINX"><br><b>K~SPHINX</b></a><br><sub>Regex Lab</sub>
    </td>
  </tr>
</table>

#### Utilitaires

<table>
  <tr>
    <td align="center" width="20%">
      <a href="tools/HECATE%20(%20Hex-Codec%20)/"><img src="tools/HECATE%20(%20Hex-Codec%20)/icon.png" width="140" alt="K~HECATE"><br><b>K~HECATE</b></a><br><sub>Hex Codec</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/HEPHAESTUS%20(%20HTML-Entities%20)/"><img src="tools/HEPHAESTUS%20(%20HTML-Entities%20)/icon.png" width="140" alt="K~HEPHAESTUS"><br><b>K~HEPHAESTUS</b></a><br><sub>HTML Entities</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/IRIS%20(%20URL-Codec%20)/"><img src="tools/IRIS%20(%20URL-Codec%20)/icon.png" width="140" alt="K~IRIS"><br><b>K~IRIS</b></a><br><sub>URL Codec</sub>
    </td>
    <td align="center" width="20%">
      <a href="tools/THOTH%20(%20Base64%20)/"><img src="tools/THOTH%20(%20Base64%20)/icon.png" width="140" alt="K~THOTH"><br><b>K~THOTH</b></a><br><sub>Base64</sub>
    </td>
    <td width="20%"></td>
  </tr>
</table>

<!-- KTRX:GALLERY:END -->

## Build

Install Rust 1.97 or newer (required by the locked Wasmtime 50 dependencies) and the WebAssembly component target:

```sh
rustup target add wasm32-wasip2
cargo build --manifest-path engine/Cargo.toml --release -p ktrx-cli
```

On Windows the native TLS dependency builds OpenSSL from source, which adds three requirements:

- **The MSVC build tools and a native Perl must be on `PATH`.** Build from a "x64 Native Tools Command Prompt for VS" (or after running `vcvars64.bat`) so that `cl`, `nmake`, and `link` resolve to MSVC, and install a native Perl such as [Strawberry Perl](https://strawberryperl.com/) (which also provides the NASM assembler OpenSSL needs). A Cygwin, MSYS2, or Git-for-Windows Perl cannot configure OpenSSL for the MSVC target and will fail; likewise make sure their `link.exe` does not shadow the MSVC linker.
- **The build path must be ASCII.** OpenSSL's build script cannot decode non-ASCII characters in its output directory, so a user name or checkout path containing non-ASCII characters (for example an accented letter or `µ`) breaks the build with a `FromUtf8Error`. Point the build at an ASCII directory, for example `set "CARGO_TARGET_DIR=C:\ktrx-build"`, before running `cargo build`.

The executable is `engine/target/release/ktrx` (`ktrx.exe` on Windows, or under `%CARGO_TARGET_DIR%\release` when that is set). Ready-to-run `.tool` packages are included in each plugin directory.

## Engine CI and releases

Changes to `engine/` are tested on Linux, Windows, and macOS by GitHub Actions. To build and publish platform binaries, push a version tag beginning with `v` (for example, `v0.1.0`). The resulting GitHub Release contains a `.tar.gz` archive for Linux, a `.zip` for Windows, and a `.tar.gz` archive for macOS.

```sh
git tag v0.1.0
git push origin v0.1.0
```

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
ktrx run 'tools/CERBERUS ( Subdomain-Sentinel )/K~CERBERUS.tool' example.com --max-subdomains 250
ktrx run 'tools/ANUBIS ( Hash-Audit )/K~ANUBIS.tool' 900150983cd24fb0d6963f7d28e17f72 --algorithm md5 --file ./wordlist.txt
ktrx info 'tools/THOTH ( Base64 )/K~THOTH.tool'
ktrx describe 'tools/THOTH ( Base64 )/K~THOTH.tool' --json
ktrx validate 'tools/THOTH ( Base64 )/K~THOTH.tool'
ktrx run 'tools/HERMES ( HTTP-Inspector )/K~HERMES.tool' https://example.com --compare
ktrx capabilities
```

When a tool requests network, local-file, file-write, privileged-network (`raw-net`), or program-execution (`process`) access, KTRX asks for approval on an interactive terminal. Non-interactive runs deny these permissions unless `-y` is passed before the tool path (`ktrx run -y <tool> ...`), which approves everything the manifest declares. Each approval applies to one run. A tool with `file-write` can only write inside one directory: `--write-dir DIR` (created if missing), or the current directory by default. `raw-net` (ICMP ping and TTL-limited traceroute, ARP, SYN probes) may need administrator rights, and SYN probes are only available on Linux. Tools use generic resolver and socket byte-stream capabilities; protocol handling, scan behavior, service identification, and response limits belong to each tool. The runtime does not implement scanning or HTTP/TLS workflows.

Since engine 0.4, a tool with the `process` permission can run programs on the machine, outside the sandbox and with your rights. The prompt names the programs the manifest lists (or warns that any program may run when it lists none); `--allow-program NAME` (repeatable, before the tool path) restricts a run further. Programs are started without a shell, every start is shown as `K~TOOL ▸ exécute : …` and published as a `process` event, and everything a tool started is terminated when it is killed, when you press `Ctrl+C`, and when the run ends. `ktrx capabilities` lists every host operation and the permission it needs; tools can ask the same question at run time with `host.info`.

For the web, the shared SDK (`tools/ktrx-net-sdk`) provides an HTTP/1.1 and HTTP/2 client (ALPN, multiplexing, keep-alive, gzip/deflate, redirects, HTTP proxies) built on the generic socket capabilities; K~HERMES uses it. See [CHANGELOG.md](CHANGELOG.md) for everything new in 0.4.0.

Tools have no overall execution deadline by default. The runner streams progress events while a tool runs; use `ktrx run <tool.tool> --events` for newline-delimited JSON. Press `Ctrl+C` to request cancellation. Wasm execution stops at the next engine checkpoint; socket reads and waits stop within about 100 ms, while other in-flight network calls (connection attempts, DNS queries) may take up to their configured timeout to return. A manifest can set an explicit per-tool deadline.

K~HORUS (Port Scanner) is an independently built plugin. It scans TCP ports 1–1000 by default, supports all 65,535 ports, and reports state, latency, service guesses, and basic application banners. Choose the target hostname at launch; KTRX prompts for network approval.

Hash Audit is an offline plugin for auditing supplied raw MD5, SHA-1, SHA-2, and SHA-3 digests against a local wordlist or a generated candidate set. It reports progress as candidates are checked. Reading the wordlist requires the run's `file-read` approval, which covers any file the tool asks for during that run, not only the wordlist; see `tools/ANUBIS ( Hash-Audit )/README.md`.

Seven offline analysis helpers are also available: URL Codec, JWT Inspector, HTML Entity Codec, Hex Codec, HTTP Header Audit, JSON Inspector, and Regex Lab. JWT Inspector only decodes token contents and never verifies signatures; HTTP Header Audit reports context-dependent observations rather than vulnerability verdicts. See each tool's README for details.

## Tests

```sh
cargo test --manifest-path engine/Cargo.toml
cargo test --manifest-path tools/ktrx-net-sdk/Cargo.toml
cargo test --manifest-path 'tools/THOTH ( Base64 )/Cargo.toml'
cargo test --manifest-path 'tools/HADES ( Hash )/Cargo.toml'
cargo test --manifest-path 'tools/POSEIDON ( DNS-Inspector )/Cargo.toml'
cargo test --manifest-path 'tools/HERMES ( HTTP-Inspector )/Cargo.toml'
cargo test --manifest-path 'tools/ARGUS ( TLS-Inspector )/Cargo.toml'
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

Every tool ships an `icon.png` emblem next to its README. The visual direction (mythological relic, secret-archive mood), file specifications and ready-to-use image prompts are in [docs/ICON_STYLE_GUIDE.md](docs/ICON_STYLE_GUIDE.md).
