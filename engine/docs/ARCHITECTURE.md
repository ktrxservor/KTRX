# KTRX architecture

## Boundaries

- `ktrx-core` parses and validates manifests and archives, enforces archive size limits, and converts CLI arguments to a JSON object.
- `ktrx-runtime` owns the Wasmtime engine, component compilation cache, per-call store, memory limit, execution deadline, and host capability implementation.
- `ktrx-cli` provides commands, user-facing permission prompts, packaging, and text or JSON output.
- `ktrx-sdk` exposes the guest-side WIT bindings dependency and API version.
- `tools/` contains independent guest crates and does not participate in the engine workspace.

## Execution

The host reads a `.tool` as a ZIP archive and accepts only `manifest.toml` and `module.wasm`. The manifest API version is checked before compiling the component. Arguments become a JSON object serialized as UTF-8 and passed to the exported `ktrx:tool/tool.run` function. The plugin returns JSON text, which KTRX emits without decoration in JSON mode.

Wasmtime compiles a component once per runtime process and caches it by SHA-256 of the component bytes. Each call receives a fresh store with a 64 MiB memory cap. Tools have no execution deadline by default; a manifest can set an explicit 30–180 second deadline. The runtime supplies WASI Preview 2 interfaces with an empty default context: no stdio inheritance, environment, preopened filesystem, or ambient sockets. KTRX `host.request` is the supported way for plugins to request network or file capabilities.

## Capabilities and network policy

Permissions are opt-in in the manifest and require interactive approval at each invocation. Non-interactive invocation denies them. Host calls check both the declared permission and runtime approval. Network requests require the manifest network permission and interactive approval for each invocation. Destinations are selected by tool arguments; DNS results are resolved once and pinned into the HTTP client to avoid resolving again at connect time. Redirects are disabled. HTTP/TLS requests time out after 10 seconds (5 seconds to connect), and response bodies are limited to 1 MiB. A 60-second DNS cache and a bounded pool of clients reuse connections. Network host operations are serialized per runtime. Local file reads are host-mediated in 64 KiB chunks, limited to regular files no larger than 64 MiB, and require the `file-read` permission and user approval.

The current DNS operation resolves A and AAAA addresses through the system resolver. That resolver's blocking timeout is platform-controlled. HTTP inspection performs a GET and reports status, response headers, and body byte count; a tool can request a Base64 body sample capped at 256 KiB while the host retains the 1 MiB full-response cap. TLS inspection verifies the TLS peer through rustls via reqwest, but this V1 host API does not expose full certificate fields or chain details. DNS record-type selection, HTTP methods, and TLS certificate detail inspection are future API extensions.

## Resource and error model

Malformed archives, manifests, arguments, unsupported API versions, missing imports, and execution traps become CLI errors with exit code 2. Tool-level errors are returned as JSON by plugins. Capability failures are returned through the host interface and never silently grant access.
