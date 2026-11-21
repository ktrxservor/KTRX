# KTRX

KTRX is a Rust command-line host for portable WebAssembly Component Model tools. The engine lives in `engine/`; each independently built plugin lives under `tools/`. Plugins are packaged as ZIP-based `.tool` archives containing `manifest.toml` and `module.wasm`.

## Catalogue des outils

Le [site officiel KTRX](https://ktrx-site.tiagoantunes2009.workers.dev) présente chaque outil (documentation, arguments, permissions, versions) et permet d'installer le moteur `ktrx` sur Windows, macOS et Linux. Les outils s'installent aussi en ligne de commande avec `ktrx install <outil>` (voir « Installing tools from the registry »). Le tableau ci-dessous est généré par `scripts/readme_tools.py`.

<!-- KTRX:TOOLS:START -->

| Outil | Description | Permissions | |
|---|---|---|---|
| [`K~ANUBIS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=hash-audit) | Offline hash audit with live progress, wordlists and configurable brute-force search. | lecture de fichiers | [source](tools/ANUBIS%20%28%20Hash-Audit%20%29/) |
| [`K~APATE`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=webshell-forge) | Forge de webshells furtifs PHP/JSP/ASPX : transports et chiffrements empilables, obfuscation par graine, linter de signatures et score d'exposition. | aucune | [source](tools/APATE%20%28%20Webshell-Forge%20%29/) |
| [`K~ARGUS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=tls-inspector) | Verify TLS connectivity and inspect the HTTPS response for a user-selected target. | réseau | [source](tools/ARGUS%20%28%20TLS-Inspector%20%29/) |
| [`K~ASCLEPIUS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=cve-radar) | Détection CVE par base JSON configurable, empreinte de version, preuves et recours NVD limité sans charge d’exploitation. | réseau, lecture de fichiers | [source](tools/ASCLEPIUS%20%28%20CVE-Radar%20%29/) |
| [`K~ATHENA`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=js-audit) | Audit statique approfondi du JavaScript front-end : secrets, fonctions dangereuses, logique exposée et source maps. | réseau | [source](tools/ATHENA%20%28%20JS-Audit%20%29/) |
| [`K~CERBERUS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=subdomain-sentinel) | Énumère des sous-domaines à partir d’une liste intégrée ou fournie et repère les signes de prise de contrôle : échec de résolution DNS ou page d’erreur caractéristique d’un fournisseur cloud connu. | réseau, lecture de fichiers | [source](tools/CERBERUS%20%28%20Subdomain-Sentinel%20%29/) |
| [`K~HADES`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=hash) | Calculate SHA-256 digests for UTF-8 text. | lecture de fichiers | [source](tools/HADES%20%28%20Hash%20%29/) |
| [`K~HECATE`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=hex-codec) | Convert UTF-8 text to hexadecimal and hexadecimal bytes back to UTF-8. | aucune | [source](tools/HECATE%20%28%20Hex-Codec%20%29/) |
| [`K~HEPHAESTUS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=html-entities) | Encode text as HTML entities or decode common named and numeric entities. | aucune | [source](tools/HEPHAESTUS%20%28%20HTML-Entities%20%29/) |
| [`K~HERMES`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=http-inspector) | Inspect an HTTP or HTTPS exchange over HTTP/1.1 or HTTP/2: status, headers, trailers, redirects, timing, content decoding, and TLS session. | réseau | [source](tools/HERMES%20%28%20HTTP-Inspector%20%29/) |
| [`K~HORUS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=port-scanner) | Concurrent TCP connect scanning with DNS discovery and lightweight service fingerprinting. | réseau | [source](tools/HORUS%20%28%20Port-Scanner%20%29/) |
| [`K~IRIS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=url-codec) | Percent-encode or decode URL components and form values. | aucune | [source](tools/IRIS%20%28%20URL-Codec%20%29/) |
| [`K~JANUS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=jwt-inspector) | Decode JWT header and claims locally and report common time claims. Does not verify signatures. | aucune | [source](tools/JANUS%20%28%20JWT-Inspector%20%29/) |
| [`K~METIS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=json-inspector) | Validate, pretty-print, or compact JSON and summarize its top-level structure. | aucune | [source](tools/METIS%20%28%20JSON-Inspector%20%29/) |
| [`K~ODYSSEUS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=web-discovery) | Découverte prudente de chemins web à partir d’une liste de mots, inspirée de Gobuster. | réseau, lecture de fichiers | [source](tools/ODYSSEUS%20%28%20Web-Discovery%20%29/) |
| [`K~POSEIDON`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=dns-inspector) | Resolve a hostname through the permissioned KTRX host network API. | réseau | [source](tools/POSEIDON%20%28%20DNS-Inspector%20%29/) |
| [`K~PROMETHEUS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=tech-fingerprint) | Fingerprint technologique web multicouche avec versions, ressources, DNS, TLS, cookies et navigation same-origin. | réseau | [source](tools/PROMETHEUS%20%28%20Tech-Fingerprint%20%29/) |
| [`K~SPHINX`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=regex-lab) | Search pasted text with a Rust regular expression and inspect matches and capture groups. | aucune | [source](tools/SPHINX%20%28%20Regex-Lab%20%29/) |
| [`K~THEMIS`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=header-audit) | Review pasted HTTP response headers for common security controls and cookie flags. | aucune | [source](tools/THEMIS%20%28%20Header-Audit%20%29/) |
| [`K~THOTH`](https://ktrx-site.tiagoantunes2009.workers.dev/tool?id=base64) | Encode and decode UTF-8 text using standard Base64. | aucune | [source](tools/THOTH%20%28%20Base64%20%29/) |

<!-- KTRX:TOOLS:END -->

## Install

Nothing else needs to be installed: the binaries are self-contained (OpenSSL built in, static Linux builds, static C runtime on Windows, CA certificates embedded as a fallback).

```sh
# macOS (Apple Silicon or Intel) and Linux (x86_64 or arm64)
curl -fsSL https://ktrx-site.tiagoantunes2009.workers.dev/install.sh | sh
```

```powershell
# Windows 10/11, in PowerShell
irm https://ktrx-site.tiagoantunes2009.workers.dev/install.ps1 | iex
```

The installer picks the build for the machine, verifies its SHA-256, puts `ktrx` in `~/.ktrx/bin` without administrator rights and adds it to the PATH. Run it again to update. `KTRX_VERSION=0.5.3` installs a given version, `KTRX_INSTALL=DIR` another folder, `KTRX_NO_MODIFY_PATH=1` leaves shell files alone. Archives for manual installation are on the site and in the GitHub releases.

## Build

Install Rust 1.97 or newer (required by the locked Wasmtime 50 dependencies) and the WebAssembly component target:

```sh
rustup target add wasm32-wasip2
cargo build --manifest-path engine/Cargo.toml --release -p ktrx-cli
```

OpenSSL is built from source on every platform, so a C compiler, `make` and Perl are needed (Xcode command line tools on macOS, `build-essential` and `perl` on Linux). On Windows this adds three requirements:

- **The MSVC build tools and a native Perl must be on `PATH`.** Build from a "x64 Native Tools Command Prompt for VS" (or after running `vcvars64.bat`) so that `cl`, `nmake`, and `link` resolve to MSVC, and install a native Perl such as [Strawberry Perl](https://strawberryperl.com/) (which also provides the NASM assembler OpenSSL needs). A Cygwin, MSYS2, or Git-for-Windows Perl cannot configure OpenSSL for the MSVC target and will fail; likewise make sure their `link.exe` does not shadow the MSVC linker.
- **The build path must be ASCII.** OpenSSL's build script cannot decode non-ASCII characters in its output directory, so a user name or checkout path containing non-ASCII characters (for example an accented letter or `µ`) breaks the build with a `FromUtf8Error`. Point the build at an ASCII directory, for example `set "CARGO_TARGET_DIR=C:\ktrx-build"`, before running `cargo build`.

Released binaries need nothing installed on the target machine (OpenSSL is built in, Linux is a static musl build, Windows links its C runtime statically, TLS falls back to embedded root certificates when the system has none). A plain `cargo build` produces a binary for your own machine; the portable Linux build is made in the release workflow with `cargo zigbuild --target x86_64-unknown-linux-musl`.

The executable is `engine/target/release/ktrx` (`ktrx.exe` on Windows, or under `%CARGO_TARGET_DIR%\release` when that is set). Ready-to-run `.tool` packages are included in each plugin directory.

## Installing tools from the registry

```sh
ktrx search hash
ktrx install anubis            # or K~ANUBIS, or the id hash-audit
ktrx install anubis@1.1.0      # pin a version
ktrx list
ktrx update                    # or: ktrx update anubis
ktrx uninstall anubis
ktrx run anubis --help         # installed tools resolve by name
```

Tools go to `~/.ktrx/tools` (`$KTRX_HOME/tools`, or `--dir`). Every download is checked against the SHA-256 published by the registry and opened as a real `.tool` archive (manifest, engine version) before it is written; on any failure nothing is installed. The registry is `$KTRX_REGISTRY` or `--registry URL` and must use HTTPS.

## Engine CI and releases

Changes to `engine/` are tested on Linux, Windows, and macOS by GitHub Actions. To build and publish platform binaries, bump the version in `engine/Cargo.toml` (and `engine/Cargo.lock`), add a `CHANGELOG.md` entry, then push a version tag beginning with `v` (for example, `v0.5.3`). The release workflow builds `ktrx-linux.tar.gz` and `ktrx-linux-arm64.tar.gz` (static musl), `ktrx-windows.zip` (x64, static C runtime) and `ktrx-macos.tar.gz` (universal: Apple Silicon and Intel), checks that none of them depends on anything outside the operating system, and smoke-tests each one, including on a bare container without CA certificates. A push to a `release-test/*` branch runs the same builds and checks without publishing. The archives are attached to the GitHub Release and, when the `KTRX_REGISTRY_KEY` secret is set, uploaded to the registry, which serves them at `/dl/engine/<windows|linux|linux-arm64|macos>` and through the installers. After a release, the `Installers` workflow (`installers.yml`) checks the one-line installers on clean machines.

```sh
git tag -a v0.5.3 -m "KTRX 0.5.3"
git push origin v0.5.3
```

Pushing tools to `main` publishes them to the registry through `.github/workflows/registry-publish.yml` (same secret). See [docs/REGISTRY.md](docs/REGISTRY.md).

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
