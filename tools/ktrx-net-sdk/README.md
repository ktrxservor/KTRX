# ktrx-net-sdk

Guest-side networking for KTRX tools. It is a library linked into `wasm32-wasip2` tool components, not a `.tool` plugin. Everything it does goes through the host's generic, permission-gated `socket.*` capabilities: the engine stays protocol-agnostic and this crate carries the protocols.

## Modules

- **`http`** (0.2) — a complete HTTP client:
  - HTTP/2 (RFC 9113) negotiated with ALPN over TLS, or h2c by prior knowledge; flow control, CONTINUATION, padding, PING, SETTINGS, GOAWAY, RST_STREAM, trailers, informational responses; server push disabled.
  - `Client::send_all` multiplexes requests to one origin over a single HTTP/2 connection and returns results in request order.
  - HTTP/1.1 (RFC 9112) with keep-alive pooling, chunked coding and trailers, 1xx, close-delimited bodies.
  - gzip/deflate decoding bounded against decompression bombs; opt-in redirects with method rewriting and credential stripping across origins; HTTP proxies (`CONNECT` + `socket.starttls` for HTTPS, engine 0.4+).
  - Idempotent requests are retried once on a kept-alive connection the server had closed, or after GOAWAY / REFUSED_STREAM.
  - Every size, count, and wait is bounded (`Config`). Errors carry a stable `ErrorKind`.
- **`hpack`** — RFC 7541 header compression (full decoder, stateless encoder), verified against the RFC's examples.
- Root functions (`http_get`, `open_socket`, `read_socket`, `wait`, `error_kind`, …) — the original helpers, unchanged.

```rust
use ktrx_net_sdk::http::{Client, Config, Request};

let host = |op: &str, input: &str| ktrx::tool::host::request(op, input);
let mut client = Client::with_config(&host, Config { follow_redirects: 3, ..Config::default() });
let response = client.send(Request::get("https://example.com/"))?;
println!("{} {} ({} bytes)", response.version.as_str(), response.status, response.body.len());
```

The tool needs the `network` permission. HTTPS through a proxy needs KTRX 0.4 or newer; everything else works on 0.3.

## Tests

```sh
cargo test --manifest-path tools/ktrx-net-sdk/Cargo.toml
```

`tests/client.rs` runs the client against local HTTP/1.1 and HTTP/2 servers through a test bridge that implements the host's socket operations with real loopback sockets.

Third-party data: the HPACK Huffman table (`src/huffman_table.rs`) is the RFC 7541 Appendix B table as published in the MIT-licensed `h2` crate.
