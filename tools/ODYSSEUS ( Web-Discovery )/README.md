# K~ODYSSEUS — Web Discovery
> Ulysse, voyageur de l’Odyssée : cet outil explore les chemins web accessibles selon les mots et limites fournis.

Web Discovery is a KTRX path and file discovery tool inspired by Gobuster. It sends sequential HTTP GET requests for a built-in list or a local newline-separated wordlist, reports progress in the terminal, and returns matching status codes, response sizes, content types, and redirect locations. It also probes a missing path once to help filter soft-404 pages. Redirects are not followed.

Use it only on a website you own or have permission to test. Requests are sequential with a 100 ms pause, limited to 5,000 generated candidates and 1 MiB of wordlist data. The host enforces its own 10-second request timeout including private and reserved destinations. This is a content discovery tool, not an exploit tool.

## Build and package

From this directory, build for WASI Preview 2 and package using the KTRX CLI:

```sh
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/ktrx_web_discovery.wasm module.wasm
ktrx pack . --output K~ODYSSEUS.tool
ktrx validate K~ODYSSEUS.tool
```

Choose the target URL when launching the tool; KTRX asks for network approval for each run including private and reserved destinations. A custom wordlist also requires file-read approval.

## Usage

```sh
ktrx run 'tools/ODYSSEUS ( Web-Discovery )/K~ODYSSEUS.tool' https://example.com --max-words 500
ktrx run 'tools/ODYSSEUS ( Web-Discovery )/K~ODYSSEUS.tool' https://example.com --file ./paths.txt --extensions php,txt --max-words 3000
```

The built-in list is intentionally compact. A custom file uses one path per line; blank lines and `#` comments are ignored. Up to 10 alphanumeric extensions may be supplied. The `--include-status` option accepts comma-separated HTTP codes and defaults to `200,204,301,302,307,401,403`. Exact soft-404 matches are suppressed using a single random-looking baseline path. KTRX returns status, headers, and byte count by default. Tools that opt into body sampling can receive a bounded Base64 sample; Web Discovery needs only status and byte count.
