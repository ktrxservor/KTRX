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

`id` uses lowercase ASCII letters, digits, and hyphens. `api_version` must match the host API. Argument types are `string`, `integer`, `boolean`, and `path`; `positional` assigns the argument to the next non-option CLI value. API 1 permissions are `network` and `file-read`.

## Component interface and capabilities

`engine/wit/ktrx.wit` declares package `ktrx:tool@1.0.0`: `tool.run(input: string) -> string` receives validated JSON arguments and returns JSON text. `host.request(operation, input)` provides only generic capabilities:

- `progress.update`: publish a tool-authored progress snapshot.
- `file.read`: read a user-approved regular file in bounded chunks.
- `network.resolve`: resolve a hostname through the operating system resolver.
- `socket.open`, `socket.connect`, `socket.poll`, `socket.read`, `socket.write`, `socket.close`: open permission-gated byte streams and transfer bounded byte chunks. `socket.connect` is asynchronous so a guest tool can manage concurrent connections; connection concurrency and scan policy remain the tool's responsibility. TLS on `socket.open` gives the tool a verified encrypted byte stream.

The host does not expose HTTP requests, TLS inspection, TCP scans, service identification, or other tool-specific operations. Tools implement protocol framing, limits, scheduling, and interpretation themselves. Do not add tool names or protocol workflows to the engine host API.

Network and file calls require the corresponding manifest permission and interactive approval. Non-interactive invocations deny these permissions. Network access is available only through the permission-checked host socket bridge; tools do not receive ambient WASI sockets or operating-system access. Socket calls enforce bounded active connections, chunk sizes, and I/O timeouts. File reads are limited to regular files no larger than 64 MiB and 64 KiB per chunk.

Tools may call `progress.update` to report live progress. The host displays throttled terminal updates and publishes structured events. `ktrx run <file.tool> --events` emits progress and the final result as newline-delimited JSON. The queue holds at most 256 events; intermediate snapshots may be dropped under backpressure. Ctrl+C requests cancellation. Tools have no execution deadline by default; a manifest may request an explicit 30–180 second deadline.

Capability failures return errors and never grant access silently. Plugins should handle returned errors and return structured JSON errors rather than trap.
