# KTRX architecture

## Strict boundary

- `ktrx-core` loads and validates manifests and archives and turns CLI arguments into JSON.
- `ktrx-runtime` compiles and executes WebAssembly components. It owns Wasmtime, per-run stores, memory limits, execution deadlines, cancellation, and the generic host-call bridge. It has no knowledge of tool names, protocols, discovery, scanning, fingerprints, CVEs, or hash algorithms.
- `ktrx-cli` handles run/inspect/validate/package commands, asks the user to grant manifest-declared permissions, and displays results/events.
- `tools/` contains the independent tool implementations. HTTP parsing, DNS lookup policy, TLS interpretation, port selection, scan scheduling, service detection, and result interpretation belong here.
- `proxy-app/` is a separate native program. It is not linked into the runner or runtime.

## Execution

The runner opens the `.tool` archive, validates its manifest and component, converts validated arguments to JSON, then asks the runtime to execute `ktrx:tool/tool.run`. The runtime caches compiled components by SHA-256, creates a fresh Wasmtime store per invocation, and enforces a 64 MiB memory limit. Tools have no deadline by default; a manifest may set one. Wasmtime receives an empty WASI context with no ambient sockets, stdio, environment, or filesystem access. User approval gates the runtime's generic host socket capabilities.

The engine does not schedule tool-specific work or interpret tool output. A tool returns JSON text. The engine forwards progress updates and the final output, and enforces generic resource and permission boundaries.

## Generic host capabilities

API 1 exposes only generic calls: `progress.update`, `file.read`, `network.resolve`, and `socket.*` byte-stream operations (`open`, asynchronous `connect`/`poll`, `read`, `write`, `close`). The bridge checks manifest declaration and per-run approval before network or file access. Socket I/O chunks and active connections are bounded. TLS may be requested when opening a socket; the host provides a verified encrypted byte stream, while the tool constructs requests and interprets protocol responses.

No HTTP operation, TLS inspection operation, TCP scan operation, scan concurrency policy, banner probe, tool-specific DNS cache, or service fingerprinting code exists in the engine. These are implemented by individual tools using the generic capabilities. The standalone proxy owns its listener, interception workflow, and request editing.

## Events and cancellation

Each invocation has a bounded event queue. `progress.update` produces structured events, and `ktrx run --events` writes newline-delimited JSON. Intermediate progress snapshots can be dropped under backpressure; the final event is preserved. Cancellation and manifest deadlines are checked per Wasm store. An in-flight host call completes or errors according to its own I/O timeout.

## Architecture guard

The runtime integration test scans its source and dependencies for protocol- and tool-specific APIs. It also checks that the primary CLI does not link the standalone proxy. When adding a host capability, keep it mechanism-level and reusable across unrelated tools; place parsing, policy, workflow, and interpretation in the guest tool or a separate application.
