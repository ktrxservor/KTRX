<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~HORUS — Port Scanner">
</p>

<p align="center"><sub><code>DOSSIER K~HORUS</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~HORUS — Port Scanner
> Horus, dieu égyptien à tête de faucon associé à l’œil protecteur : cet outil repère les ports TCP accessibles et les services qu’ils révèlent.

Port Explorer is a KTRX TCP connect scanner. The engine resolves the selected hostname after interactive approval, scans TCP ports with bounded parallel connections. It reports open, closed and filtered results, target IP, connect latency, conventional service guesses, and (when enabled) lightweight banner/version information for common HTTP, SSH, SMTP, FTP, POP3, IMAP, MySQL and Redis services. Version probes are capped at 1,024 open ports per run to keep large scans responsive.

This is a full TCP handshake scan. It is not a raw SYN scan and does not implement UDP scanning or OS fingerprinting. Service names inferred only from a port number are marked as guesses. A timeout is classified as filtered because a connect scan cannot distinguish packet filtering from an unreachable service with certainty.

Network access requires interactive approval for each run. Choose the target hostname when launching the tool. Private and reserved targets can be scanned after network approval.

## Usage

```sh
# Default range: TCP ports 1-1000, 250 ms connect timeout, service detection enabled
ktrx run 'tools/HORUS ( Port-Scanner )/K~HORUS.tool' example.com

# Select common services or custom ranges
ktrx run 'tools/HORUS ( Port-Scanner )/K~HORUS.tool' example.com --ports 22,80,443,3306,8080,8443
ktrx run 'tools/HORUS ( Port-Scanner )/K~HORUS.tool' example.com --ports 8000-8100

# Full TCP range, 1-65535. Large or filtered scans can take several minutes.
ktrx run 'tools/HORUS ( Port-Scanner )/K~HORUS.tool' example.com --ports all --timeout-ms 200 --concurrency 128

# Skip application probes for a faster connect-only scan
ktrx run 'tools/HORUS ( Port-Scanner )/K~HORUS.tool' example.com --service-detection false
```

Per-port timeouts are bounded to 50–2000 ms and concurrency to 1–128. There is no overall scan or tool deadline; interactive terminals show progress and `Ctrl+C` cancels the scan.

## Develop and package

```sh
cargo test --manifest-path 'tools/HORUS ( Port-Scanner )/Cargo.toml'
cargo build --manifest-path 'tools/HORUS ( Port-Scanner )/Cargo.toml' --release --target wasm32-wasip2
cp 'tools/HORUS ( Port-Scanner )/target/wasm32-wasip2/release/ktrx_port_scanner.wasm' 'tools/HORUS ( Port-Scanner )/module.wasm'
cargo build --manifest-path engine/Cargo.toml --release -p ktrx-cli
engine/target/release/ktrx pack 'tools/HORUS ( Port-Scanner )' --output 'tools/HORUS ( Port-Scanner )/K~HORUS.tool'
engine/target/release/ktrx validate 'tools/HORUS ( Port-Scanner )/K~HORUS.tool'
```
