<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~HERMES — HTTP Inspector">
</p>

<p align="center"><sub><code>DOSSIER K~HERMES</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~HERMES — HTTP Inspector
> Hermès, messager des dieux grecs : cet outil inspecte les échanges HTTP et en structure les informations utiles.


Sends one HTTP request and reports exactly what came back: the protocol actually used (HTTP/2 when the server offers it through ALPN, otherwise HTTP/1.1), status and reason, headers in received order, trailers, informational (1xx) responses, redirects followed, timing, connection reuse, content decoding, and a summary of the TLS session (version, cipher, ALPN, verification result, leaf certificate). Built on the HTTP/1.1 + HTTP/2 client of `ktrx-net-sdk` 0.2.

## Examples

From the project root:

```sh
ktrx run 'tools/HERMES ( HTTP-Inspector )/K~HERMES.tool' https://example.com
ktrx run 'tools/HERMES ( HTTP-Inspector )/K~HERMES.tool' https://example.com --http-version 2 --body --body-limit 2048
ktrx run 'tools/HERMES ( HTTP-Inspector )/K~HERMES.tool' https://example.com --compare
ktrx run 'tools/HERMES ( HTTP-Inspector )/K~HERMES.tool' http://example.com --follow 5
ktrx run 'tools/HERMES ( HTTP-Inspector )/K~HERMES.tool' https://httpbin.org/post --data 'a=1' --header 'Content-Type: application/x-www-form-urlencoded'
ktrx run 'tools/HERMES ( HTTP-Inspector )/K~HERMES.tool' https://example.com --proxy 127.0.0.1:8080 --insecure
```

## Options

| Option | Meaning |
|---|---|
| `<url>` | `http://` or `https://` URL (required). |
| `--method` | Request method; default `GET`, or `POST` with `--data`. |
| `--header 'Name: value'` | One request header. `--headers` takes several, one per line. |
| `--data` | Request body, sent as is. |
| `--http-version` | `auto` (default), `1.1`, or `2`. `2` requires HTTP/2: ALPN `h2` over TLS, prior knowledge (h2c) for `http://`. |
| `--compare` | Send the request over HTTP/1.1 and over HTTP/2 and report the differences (HTTP/2 support, status, body size, header names present in only one version). |
| `--follow N` | Follow up to N redirects (0–20, default 0). Credentials are dropped on cross-origin hops. |
| `--body` | Include a sample of the body (`body_base64`, and `body_text` when it is UTF-8). `--body-limit` sets its size (default 4096 bytes). |
| `--no-decompress` | Do not advertise or decode gzip/deflate. |
| `--insecure` | Finish the TLS handshake even if the certificate does not verify; the verification result is still reported. |
| `--timeout-ms` | Read timeout (100–120000, default 10000). |
| `--proxy host:port` | Use an HTTP proxy; HTTPS goes through `CONNECT` (requires KTRX 0.4 or newer). |

## Output

A JSON object with `request` (method, URL, headers, body size, proxy) and the response: `version`, `status`, `reason`, `headers` and `trailers` (arrays of `[name, value]`; HTTP/2 names are lower case), `interim`, `body_bytes` (after decoding), `wire_bytes` (as transferred), `body_truncated`, `content_encoding`, `decode_error`, `remote_address`, `alpn`, `reused_connection`, `redirects` (`status`, `from`, `to`), `elapsed_ms`, and `tls`. Failures are reported as `{"error": {"kind": …, "message": …}}` with a stable `kind` (`connect`, `tls`, `timeout`, `protocol`, `closed`, `too_large`, `invalid_request`, `denied`, `cancelled`, `other`). With `--compare`, the result holds `comparison`, `http1`, and `http2`.

## Limits

Bodies are kept up to 8 MiB after decoding (cut and flagged beyond), headers up to 64 KiB. Only `gzip` and `deflate` are requested and decoded; `br` is never advertised. HTTP/3 and WebSocket are not supported. Server push is disabled. The tool reports observations; it does not grade security headers (see K~THEMIS).

## Permissions

`network`, approved for each run. The destination is the URL you pass (and the proxy, when given).

## Build and test

```sh
cargo test --manifest-path 'tools/HERMES ( HTTP-Inspector )/Cargo.toml'
cargo build --manifest-path 'tools/HERMES ( HTTP-Inspector )/Cargo.toml' --release --target wasm32-wasip2
```
