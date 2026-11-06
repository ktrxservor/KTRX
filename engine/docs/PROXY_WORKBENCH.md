# KTRX HTTP/HTTPS request proxy

KTRX's local proxy is a native CLI feature because a sandboxed `.tool` component cannot open a listener or hold a live browser session. It captures HTTP/1.1 requests, including decrypted HTTPS requests when the test browser trusts the local KTRX CA. Each intercepted request can be forwarded, edited, dropped, or replaced by replaying one of the last 100 in-memory requests.

## Start

```sh
ktrx proxy init-ca
ktrx proxy start
```

The default CA files are stored in `~/.ktrx/proxy-ca/ca.crt` and `ca.key`. Import only `ca.crt` into the trust store of the browser or test device. Then configure that browser to use `127.0.0.1:8080` as its HTTP proxy. KTRX binds only to a loopback address. Startup requires an interactive confirmation.

The CA private key is created with owner-only file permissions on Unix. Keep it private. Remove the CA from the test device's trust store and delete the key after use.

## Per-request controls

- `f`: forward the intercepted request unchanged.
- `e`: enter the request editor. Use `m METHOD /path`, `h Header: value`, `x Header`, or `s old text => replacement`, then `done`.
- `r N`: replay an earlier captured request to the origin (index `0` is the newest) and show its response in the terminal. The current browser request stays paused for its own decision.
- `d`: drop the request and return HTTP 403 to the client.
- `q`: stop the proxy.

Requests and response summaries appear in the terminal; short UTF-8 body previews may include cookies, authorization values, or other test data. The proxy keeps history in memory only and does not write captures to disk. Upstream TLS certificates are still verified by the HTTP client.

## Current protocol limits

The proxy handles HTTP/1.1 requests and HTTPS through CONNECT plus a locally signed per-host certificate. Request bodies up to 8 MiB are supported, including chunked uploads. Upstream response bodies are capped at 32 MiB. It does not handle HTTP/2, WebSocket upgrades, response editing, or persistent project/history files. The CA must be trusted by the test client for HTTPS interception; without that trust, the TLS handshake fails.
