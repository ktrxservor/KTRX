# KTRX architecture

## Strict boundary

- `ktrx-core` loads and validates manifests and archives and turns CLI arguments into JSON.
- `ktrx-runtime` compiles and executes WebAssembly components. It owns Wasmtime, per-run stores, memory limits, execution deadlines, cancellation, and the generic host-call bridge. It has no knowledge of tool names, protocols, discovery, scanning, fingerprints, CVEs, or hash algorithms.
- `ktrx-cli` handles run/inspect/validate/package/capabilities commands, asks the user to grant manifest-declared permissions (naming the programs a `process` tool may start), and displays results/events.
- `tools/` contains the independent tool implementations and the guest SDK (`ktrx-net-sdk`, including the HTTP/1.1 + HTTP/2 client). HTTP parsing, DNS lookup policy, TLS interpretation, port selection, scan scheduling, service detection, and result interpretation belong here.
- `proxy-app/` is a separate native program. It is not linked into the runner or runtime.

## Execution

The runner opens the `.tool` archive, validates its manifest and component, converts validated arguments to JSON, then asks the runtime to execute `ktrx:tool/tool.run`. The runtime caches compiled components by SHA-256, creates a fresh Wasmtime store per invocation, and enforces a 64 MiB memory limit. Tools have no deadline by default; a manifest may set one. Wasmtime receives an empty WASI context with no ambient sockets, stdio, environment, or filesystem access. User approval gates the runtime's generic host socket capabilities.

The engine does not schedule tool-specific work or interpret tool output. A tool returns JSON text. The engine forwards progress updates and the final output, and enforces generic resource and permission boundaries.

## Generic host capabilities

API 1 exposes only generic calls, grouped by permission: `host.info` and `progress.update` (none); `file.read`, `file.list`, `file.stat` (`file-read`); `file.create`, `file.write`, `file.append`, `file.mkdir` (`file-write`, confined to one directory chosen by the user for the run); `network.resolve`, `dns.query`, `socket.*` byte-stream operations (`open`, asynchronous `connect`/`poll`, multiplexed `wait`, `read`, `write`, `shutdown`, in-place TLS upgrade `starttls`, `close`), and `udp.*` datagram operations (`network`); `icmp.ping`, `arp.resolve`, `tcp.syn_probe` (`raw-net`, which may need elevated rights and is not available on every operating system); and `process.exec`, `process.spawn`, `process.read`, `process.write`, `process.wait`, `process.kill`, `process.close` (`process`). The runtime keeps one table of operations and the permissions that unlock them (`ktrx_runtime::capabilities`), which drives `host.info` and `ktrx capabilities`.

The process capability is the only one that leaves the sandbox, so it carries the strictest guarantees: programs are started directly (never through a shell) with arguments passed verbatim; the manifest's `programs` list and the user's `--allow-program` choices restrict them to bare names resolved on `PATH`; every program runs in its own process tree (a kill-on-close job object entered before the program's first instruction on Windows, a process group on Unix) that is terminated on kill, close, cancellation, and at the end of the run; pipes and handle counts are bounded; and every start is announced on the terminal and as a `process` event. What to run and how to read its output remain the tool's business. The bridge checks manifest declaration and per-run approval before any of them runs. `dns.query` and the certificate summary returned by TLS `socket.open` are the only places the host parses a wire format (DNS messages, X.509); both return plain structured data and no verdict. Socket I/O chunks and active connections are bounded. TLS may be requested when opening a socket; the host provides a verified encrypted byte stream, while the tool constructs requests and interprets protocol responses. On Windows, the runtime adds the current-user and local-machine trusted root certificates to OpenSSL's trust store so TLS uses the machine's installed trust anchors.

HTTP/1.1 and HTTP/2 (framing, HPACK, flow control, content decoding, redirects, proxy tunnels) live in the guest SDK `tools/ktrx-net-sdk`, on top of `socket.open` with ALPN, `socket.starttls`, and `socket.shutdown`. No HTTP operation, TLS inspection operation, TCP scan operation, scan concurrency policy, banner probe, tool-specific DNS cache, or service fingerprinting code exists in the engine. These are implemented by individual tools using the generic capabilities. The standalone proxy owns its listener, interception workflow, and request editing.

## Events and cancellation

Each invocation has a bounded event queue. `progress.update` produces structured `progress` events, every program a tool starts produces a `process` event, and `ktrx run --events` writes newline-delimited JSON. Intermediate progress snapshots can be dropped under backpressure; the final event is preserved. Cancellation and manifest deadlines are checked per Wasm store. Blocking socket reads and `socket.wait` observe the cancellation flag and return `[cancelled]` within about 100 ms; other in-flight host calls complete or error according to their own I/O timeout. Process reads, writes, waits, and `process.exec` observe cancellation too and terminate the program tree.

## Architecture guard

The runtime integration test scans its source and dependencies for protocol- and tool-specific APIs. It also checks that the primary CLI does not link the standalone proxy. When adding a host capability, keep it mechanism-level and reusable across unrelated tools; place parsing, policy, workflow, and interpretation in the guest tool or a separate application.
