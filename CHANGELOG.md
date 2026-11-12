# Changelog

## 0.4.0 — engine

Host API stays `1` (WIT `ktrx:tool@1.0.0` unchanged): every 0.3 tool runs unmodified. New capabilities are additive.

### Running programs — new `process` permission

- `process.exec` runs a program to completion: arguments passed verbatim (never through a shell), working directory, environment overrides or a clean environment, standard input, timeout, bounded output capture (Base64 or text), exit code, signal, duration.
- `process.spawn`, `process.read`, `process.write`, `process.wait`, `process.kill`, `process.close` drive a program interactively (streamed stdout/stderr, stdin with close, polling, termination). Up to 16 live processes per run.
- Each program runs in its own process tree — a kill-on-close job object on Windows (created suspended, resumed only inside the job), a process group on Unix. Kill, close, Ctrl+C, an `exec` timeout, and the end of the run terminate the whole tree; `exec` also ends background leftovers that would otherwise hold its output open.
- Manifests may list the programs a tool runs (`programs = ["git", "nmap"]`). The host then refuses any other program and any path, and resolves names on `PATH` (with `PATHEXT` on Windows).
- `ktrx run --allow-program NAME` (repeatable) narrows a run further. The approval prompt names the programs, or warns that any program may run.
- Every start is printed on the terminal (`K~TOOL ▸ exécute : …`) and published as a `process` event in `ktrx run --events`.

### Other host additions

- `host.info` (no permission): engine and API version, OS, architecture, CPU count, granted permissions and programs, and every operation with whether it is granted — capability negotiation for tools.
- `socket.starttls`: upgrade an open plaintext socket to TLS in place (STARTTLS, HTTPS through an HTTP proxy), with the same TLS options and `tls_info` report as `socket.open`.
- `socket.shutdown`: half-close a TCP/TLS stream (`close_notify` on TLS) without releasing it.
- `nodelay` option on `socket.open` and `socket.connect`.

### Manifest and CLI

- New permission `process`; new optional fields `programs` and `min_engine_version` (checked when an archive is opened, with a clear message on older-than-required engines).
- `ktrx capabilities [--json]` lists every host operation and the permission it needs.
- `ktrx info` shows a process tool's programs and its minimum engine version.
- `-y` documents that it also approves `process`, still bounded by `programs` and `--allow-program`.

### Quality

- 17 new end-to-end tests (`engine/crates/ktrx-runtime/tests/process.rs`) through the WebAssembly probe: execution, exit codes, verbatim arguments, stdin, cwd/env, truncation, timeouts, background leftovers, interactive sessions, process-tree termination on kill and at run end, manifest and user allow-lists, audit events, `host.info`, STARTTLS with half-close, failed upgrades.
- New unit tests for manifest validation (`programs`, `min_engine_version`, round-trip serialisation).

## ktrx-net-sdk 0.2.0 — web stack

- New `http` module: a complete HTTP client over the host's socket capabilities.
  - HTTP/2 (RFC 9113) negotiated with ALPN, or h2c by prior knowledge: flow control both ways, CONTINUATION, padding, PING, SETTINGS, GOAWAY and RST_STREAM handling, Content-Length checking, trailers, informational responses, server push disabled.
  - `Client::send_all` multiplexes many requests to one origin over a single HTTP/2 connection (up to the server's stream limit); responses come back in request order.
  - HTTP/1.1 (RFC 9112) with keep-alive pooling, chunked coding and trailers, 1xx handling, close-delimited bodies, obsolete line folding, strict status-line and Content-Length validation.
  - gzip/deflate decoding bounded against decompression bombs; redirects (opt-in) with RFC method rewriting and credential stripping across origins; HTTP proxies (absolute-form for `http://`, `CONNECT` + `socket.starttls` for `https://`).
  - Idempotent requests are retried once when a reused connection turns out to be closed, or the server refuses a stream (GOAWAY, REFUSED_STREAM).
  - Stable error kinds (`connect`, `tls`, `timeout`, `protocol`, `closed`, `too_large`, `cancelled`, `denied`, `invalid_request`, `other`).
- New `hpack` module (RFC 7541): full decoder with dynamic table and Huffman coding, stateless encoder; verified against the RFC's appendix C examples.
- The existing helpers (`http_get`, `open_socket`, …) are unchanged; every tool that uses the SDK builds as before.
- 35 tests, including end-to-end runs against local HTTP/1.1 and HTTP/2 servers.

## K~HERMES 2.0.0

Rebuilt on the new client: HTTP/2 or HTTP/1.1 (`--http-version`), `--compare` across both versions, methods, headers and bodies, redirects, body samples, decoding control, TLS summary, timing, proxies.
