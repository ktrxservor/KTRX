<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~CERBERUS — Subdomain Sentinel">
</p>

<p align="center"><sub><code>DOSSIER K~CERBERUS</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~CERBERUS — Subdomain Sentinel

> Cerbère, le chien à trois têtes qui garde les portes des Enfers dans la mythologie grecque : cet outil veille sur les « portes » DNS d’un domaine et signale celles laissées sans garde.

Subdomain Sentinel enumerates subdomains of a root domain from a built-in list or a local wordlist, resolves each one, and for those that resolve, fetches the HTTP(S) response and compares it against public fingerprints of "unclaimed resource" error pages from well-known cloud providers (GitHub Pages, Heroku, AWS S3, Shopify, Fastly, Pantheon, Vercel, and more). A match is a strong signal that a DNS record (typically a CNAME) still points at a resource — a GitHub Pages site, an S3 bucket, a Heroku app — that was since deleted, meaning a third party could potentially claim it and serve content under the organization's own subdomain: a classic, frequently paid, subdomain-takeover finding.

Use it only on a domain you own or are explicitly authorized to test. This tool only observes public DNS and HTTP state; it never registers, claims, or configures any resource, and sends no exploitation payload.

## Capabilities and limitations

- Sequential, polite probing: one DNS resolution and up to two HTTP requests (HTTPS, then HTTP as a fallback) per candidate, with a 100 ms pause between candidates. Bounded to 300 candidates and a 1 MiB wordlist.
- The KTRX host API returns only the final resolved address for a name, not the raw CNAME chain. A resolution failure does not by itself prove a dangling CNAME exists, and a successful resolution does not rule one out — the fingerprint match against the fetched page is the actual signal this tool relies on.
- A fingerprint match is strong supporting evidence, not a confirmed vulnerability: always verify the real DNS record with a dedicated DNS lookup tool and confirm the resource is genuinely unclaimed at the provider before reporting a takeover.
- The built-in prefix list is intentionally compact and generic. Real subdomains discovered through other reconnaissance (certificate transparency logs, search engines, other KTRX tools) and supplied via `--file` will find far more than guessing ever will.
- `--include-unresolved` surfaces every candidate whose DNS resolution failed outright, not just fingerprint matches. It is noisy with the built-in list (most guessed prefixes simply do not exist) but useful when you already know a subdomain exists from other recon and want to confirm it no longer resolves.

## Build and package

From this directory, build for WASI Preview 2 and package using the KTRX CLI:

```sh
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/ktrx_subdomain_sentinel.wasm module.wasm
ktrx pack . --output K~CERBERUS.tool
ktrx validate K~CERBERUS.tool
```

Choose the target domain when launching the tool; KTRX asks for network approval for each run, including private and reserved destinations. A custom prefix list also requires file-read approval.

## Usage

```sh
ktrx run 'tools/CERBERUS ( Subdomain-Sentinel )/K~CERBERUS.tool' example.com
ktrx run 'tools/CERBERUS ( Subdomain-Sentinel )/K~CERBERUS.tool' example.com --file ./subdomains.txt --max-subdomains 250
ktrx run 'tools/CERBERUS ( Subdomain-Sentinel )/K~CERBERUS.tool' example.com --include-unresolved
```

A custom file uses one subdomain prefix per line (for example `dev`, not `dev.example.com`); blank lines and `#` comments are ignored. `--max-subdomains` accepts 1 to 300. The output lists `findings` (fingerprint matches, the actionable signal), the fuller per-candidate `results`, resolution counts, and the methodology and limitations above.
