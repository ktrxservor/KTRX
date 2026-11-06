# KTRX `.tool` specification — API 1

## Archive format

A `.tool` is a ZIP archive with root entries `manifest.toml` and `module.wasm`. KTRX accepts archives up to 64 MiB, manifests up to 256 KiB, and Wasm components up to 48 MiB. The module must be a WebAssembly Component Model component compatible with the KTRX WIT world and WASI Preview 2 toolchain. Archive paths are not extracted to disk.

## Manifest

```toml
id = "hello-tool"
name = "Hello Tool"
description = "Example."
version = "1.0.0"
author = "Example Author"
api_version = "1"
permissions = []
examples = ["ktrx run hello-tool.tool --name Ada"]

[[arguments]]
name = "name"
description = "Name to greet."
value_type = "string"
required = false
positional = false
default = "world"
```

`id` uses lowercase ASCII letters, digits, and hyphens. `api_version` must match the host API. Argument types are `string`, `integer`, `boolean`, and `path`; `positional` assigns the argument to the next non-option CLI value. Defaults are TOML scalar values. API 1 permissions are `network` and `file-read`. Network destinations come from tool arguments at run time. The engine requests interactive approval for each network invocation and allows destinations returned by DNS resolution, including private and reserved addresses.

## Component interface

`engine/wit/ktrx.wit` declares package `ktrx:tool@1.0.0`:

- `tool.run(input: string) -> string`: receives a JSON object with validated argument values and returns JSON text.
- host.request(operation: string, input: string) -> result<string, string>: requests permissioned host operations. http.get returns status, headers, and body byte count; include_body=true adds an optional Base64 sample up to 256 KiB, reducible with max_body_bytes. Full responses remain capped at 1 MiB. Other operations include dns.lookup, tls.inspect, tcp.scan, chunked file.read, and terminal-only progress.update.

`tcp.scan` accepts a user-selected hostname after interactive network approval, a unique array of TCP ports, a connect timeout, bounded concurrency (up to 128), and a service-detection flag. It performs full TCP connect handshakes concurrently, returns open/closed/filtered classifications, connect latency and optional lightweight application banners (capped at 1,024 services per run). A runtime permits up to four scans at once. It does not implement raw SYN, UDP, or OS fingerprinting. Tools have no automatic execution deadline by default; a manifest may request 30–180 seconds explicitly. Long-running tools should report progress where useful and should remain cancellable by the user.

Tools may call `host.request("progress.update", JSON)` to report live progress. The host displays throttled updates on an interactive terminal and also publishes structured progress events. `ktrx run <file.tool> --events` emits progress and the final result as newline-delimited JSON. The queue holds at most 256 events; intermediate progress snapshots may be dropped under backpressure. Updates may include total, item_label, and result_label fields for tool-specific live counters. Progress requests do not require network or file permissions. Ctrl+C requests cancellation; Wasm is interrupted at its next engine checkpoint, while a host network call may take up to its own timeout to return.

Host calls remain subject to manifest permissions, user approval, engine limits. Plugins must handle returned errors and should return structured JSON errors rather than trap.
