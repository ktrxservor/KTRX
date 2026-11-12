# KTRX `.tool` specification — API 1 (engine 0.4)

## Archive format

A `.tool` is a ZIP archive with root entries `manifest.toml` and `module.wasm`. KTRX accepts archives up to 64 MiB, manifests up to 256 KiB, and Wasm components up to 48 MiB. The module must be a WebAssembly Component Model component compatible with the KTRX WIT world and WASI Preview 2 toolchain. Archive paths are not extracted to disk.

## Manifest

```toml
id = "hello-tool"
name = "K~EXAMPLE"
description = "Example."
version = "1.0.0"
author = "Example Author"
api_version = "1"
permissions = []
examples = ["ktrx run 'K~EXAMPLE.tool' --name Ada"]

[[arguments]]
name = "name"
description = "Name to greet."
value_type = "string"
required = false
positional = false
default = "world"
```

`id` uses lowercase ASCII letters, digits, and hyphens. `api_version` must match the host API. Argument types are `string`, `integer`, `boolean`, and `path`; `positional` assigns the argument to the next non-option CLI value. API 1 permissions are `network`, `file-read`, `file-write`, `raw-net`, and `process` (engine 0.4 and newer).

Two optional fields were added in engine 0.4; older engines ignore them:

- `programs`: the programs a tool with the `process` permission may start, as bare names looked up on `PATH` (for example `["git", "nmap"]`; at most 32 names of letters, digits, `.`, `_`, `-`, `+`). Listing programs requires the `process` permission. When the list is present the host refuses any other program and any path. Without a list the tool may start any program, and the approval prompt says so.
- `min_engine_version`: the oldest engine able to run the tool, as `MAJOR.MINOR.PATCH`. A newer requirement is refused with a clear message when the archive is opened. Set it when a tool depends on an operation added in a later engine, for example `"0.4.0"` for `process.*` or `socket.starttls`.

## Component interface and capabilities

`engine/wit/ktrx.wit` declares package `ktrx:tool@1.0.0`: `tool.run(input: string) -> string` receives validated JSON arguments and returns JSON text. `host.request(operation, input)` provides only generic capabilities:

- `host.info` (no permission, engine 0.4+): describe the host so a tool can adapt instead of probing: `engine_version`, `api_version`, `os`, `family`, `arch`, `cpus`, the `permissions` granted for this run, the `programs` the run may start (`"any"`, a list, or `null` without the process permission), and `operations` — every operation with the permissions that unlock it and whether it is `granted` now. On an older engine the call fails as an unsupported operation, which is itself the answer. `ktrx capabilities [--json]` prints the same operation list.
- `progress.update`: publish a tool-authored progress snapshot.
- `file.read`, `file.list`, `file.stat` (`file-read`): read a user-approved regular file in bounded chunks, list a directory (up to 5000 entries), and inspect a path without failing when it does not exist.
- `file.create`, `file.write`, `file.append`, `file.mkdir` (`file-write`): create a file (never overwriting unless `overwrite` is true), write at an offset into a file created during the run, append to a file, and create directories. Every path resolves inside the directory the user chose for the run (`ktrx run --write-dir DIR`, default the current directory): `..`, absolute paths elsewhere, symbolic links out of the tree, and Windows device names are refused. Chunks are 64 KiB, a file is at most 256 MiB, a run writes at most 1 GiB, and at most eight files are open for writing. `file.close` flushes and closes a handle.
- `network.resolve`: resolve a hostname through the operating system resolver. With a `records` array (for example `["CNAME","MX"]`) it also returns those DNS records and the response codes, and a failed system lookup is reported instead of being fatal.
- `dns.query` (`network`): send one DNS question (`name`, `type`, optional `server`, `port`, `timeout_ms`, `recurse`, `dnssec`, `tcp`) and return the structured answer: response code, flags, and the answer, authority, and additional records. A, AAAA, CNAME, NS, PTR, MX, TXT, SOA, SRV, and CAA are decoded; other types return `raw_base64`. Without `server` the system's configured resolvers are used. Truncated UDP answers are retried over TCP.
- `socket.open`, `socket.connect`, `socket.poll`, `socket.wait`, `socket.read`, `socket.write`, `socket.close`: open permission-gated byte streams and transfer bounded byte chunks. `socket.connect` is asynchronous so a guest tool can manage concurrent connections; connection concurrency and scan policy remain the tool's responsibility. `socket.poll` checks one pending connection (waiting up to `wait_ms`, at most 1000). `socket.wait` takes `connections` (pending ids; omitted means all), `sockets` (open TCP, TLS, or UDP ids), and `timeout_ms` (0–1000, default 100), and returns as soon as at least one connection finished or one socket is readable: `connections` (results shaped like `socket.poll`), `readable`, and `pending`. Connected results and `socket.open` report the peer `address` that was reached. Failures carry a locale-independent kind — `error_kind` on `socket.poll`/`socket.wait` results and a `[kind]` prefix on other network error strings — drawn from `refused`, `timeout`, `reset`, `unreachable`, `closed`, `denied`, `dns`, `tls`, `cancelled`, and `other`; operating-system messages may be translated and must not be parsed. Blocking reads stop within about 100 ms of cancellation, and TLS connectors are reused within a run for identical options. TLS on `socket.open` gives the tool an encrypted byte stream and reports what was negotiated in `tls_info`: version, cipher, ALPN protocol, SNI, whether verification succeeded and why not, and the certificate chain the server sent (subject, issuer, serial, validity, subject alternative names, key type and size, signature algorithm, SHA-256 fingerprint, DER). Optional `sni`, `alpn`, `tls_min`, `tls_max`, `ciphers`, `ciphersuites`, and `verify` shape the handshake; `verify: false` completes it against an expired or self-signed certificate while still reporting the verification result, and requesting TLS 1.0 or 1.1 relaxes OpenSSL's security level for that connection only.
- `socket.open` and `socket.connect` accept `nodelay: true` to disable Nagle's algorithm (useful for request/response protocols such as HTTP/2). Engine 0.4 adds two stream operations (`network`):
  - `socket.shutdown` (`socket`, `how`: `write` (default), `read`, or `both`) half-closes a TCP or TLS stream without releasing the handle, so the tool can still read the peer's answer; on TLS, shutting down the write side sends `close_notify` first.
  - `socket.starttls` (`socket`, optional `host`, `timeout_ms`, and the TLS options of `socket.open`) upgrades an open plaintext TCP socket to TLS in place and returns `tls_info`: STARTTLS for SMTP, IMAP, POP3, FTP, or PostgreSQL, and HTTPS through an HTTP proxy after `CONNECT`. `host` is the name verified and sent as SNI; it defaults to the host the socket was opened to, so a proxy tunnel must name the target. The socket keeps its id; a failed handshake closes it.
- `udp.open`, `udp.send`, `udp.recv`, `udp.close` (`network`): datagram sockets (IPv4 or IPv6, optional broadcast and TTL). `udp.recv` reports a timeout, or `refused` when the host returned an ICMP port-unreachable, as data instead of an error. Sockets count toward the same 128-connection limit.
- `icmp.ping` (`raw-net`): one ICMP echo with a settable `ttl`, returning `reply`, `ttl_expired` (with the router's address, which is all a traceroute needs), `unreachable`, or `timeout`. IPv4 only. `arp.resolve` returns the MAC address of a host on the local segment (Windows and Linux). `tcp.syn_probe` sends a SYN and reports `open`, `closed`, or `filtered` (Linux with root or CAP_NET_RAW only; other systems return an error pointing to `socket.connect`).

- `process.exec`, `process.spawn`, `process.read`, `process.write`, `process.wait`, `process.kill`, `process.close` (`process`, engine 0.4+): start programs on the host, outside the sandbox, with the rights of the user running KTRX.
  - Common start options: `program` (a bare name looked up on the child's `PATH`, with `PATHEXT` on Windows, or — only when the manifest lists no programs — a path, relative to `cwd`), `args` (array of strings, passed verbatim: programs are never started through a shell), `cwd` (default: the runner's current directory), `env` (object of names to a string, or `null` to remove a variable), `clear_env` (start from an empty environment). At most 1024 arguments, 32 KiB of command line, 256 variables.
  - `process.exec` runs the program to completion. Optional `stdin_base64` (≤16 MiB, then end of input), `timeout_ms` (0 = no limit, the default; at most one day), `max_output` (bytes kept per stream, default 1 MiB, at most 16 MiB), and `output` (`base64`, the default, or `text` for lossy UTF-8). The reply holds `pid`, `program` (the resolved path), `exit_code`, `signal` (Unix), `success`, `timed_out`, `killed`, `duration_ms`, and per stream `stdout_base64`/`stdout` (and stderr), `stdout_bytes` (total produced), and `stdout_truncated`. When the program exits but something it started still holds its output open, that leftover is terminated after 0.5 s instead of blocking the call.
  - `process.spawn` starts the program and returns `process` (a handle), `pid`, `program`, and `stdin` (whether input is piped: pass `stdin: true`; otherwise input is the null device). `process.read` (`process`, `stream`: `stdout` or `stderr`, `length` 1–65536, `timeout_ms` 0–10000, default 1000) returns `bytes_base64`, `eof`, and `timeout` (no data yet, which is not an error). `process.write` (`process`, `bytes_base64` ≤64 KiB, `close` to end input, `timeout_ms`) fails with `[closed]` once input is closed and `[timeout]` when the program does not read. `process.wait` (`process`, `timeout_ms` 0–10000) reports `running`, and once ended `exit_code`, `signal`, `success`, `duration_ms`. `process.kill` terminates the program and everything it started (`was_running` tells whether it was still running). `process.close` releases the handle, terminating the program if needed. Output pipes buffer about 1 MiB each; read both streams of a chatty program or it blocks, as with any pipe.
  - Errors carry a `[kind]` prefix: `denied` (program not allowed), `not_found`, `closed`, `timeout`, `cancelled`, `other`. A run may keep 16 processes alive at once.
  - Every program runs in its own process tree — a kill-on-close job object on Windows (the program is created suspended and only resumed inside the job), a process group on Unix. Killing, closing, cancelling (Ctrl+C), and the end of the run terminate the whole tree, so nothing a tool starts outlives the run.
  - Every start is published as a `process` event (`pid`, `program`, `args`, `cwd`) and, on a terminal, printed as `K~TOOL ▸ exécute : <command line>` for the audit trail.

The host does not expose HTTP requests, scan loops, service identification, vulnerability checks, or other tool-specific operations; a traceroute, a port scan, or a certificate grade is built by a tool from the primitives above. Tools implement protocol framing, limits, scheduling, and interpretation themselves. Do not add tool names or protocol workflows to the engine host API.

An HTTP/1.1 and HTTP/2 client built on these primitives ships with the guest SDK (`tools/ktrx-net-sdk`, module `http`); it is ordinary tool-side code, not a host operation.

Every call requires the corresponding manifest permission (`network`, `file-read`, `file-write`, `raw-net`, `process`) and per-run approval, which `ktrx run` asks for interactively; `-y` approves everything the manifest declares. Non-interactive invocations deny these permissions. The process prompt names the programs the run may start, or warns that it may start any program; `ktrx run --allow-program NAME` (repeatable) narrows a run further: only programs on both the manifest's list (when it has one) and the user's list may start. Network access is available only through the permission-checked host socket bridge; tools do not receive ambient WASI sockets or operating-system access. Socket calls enforce bounded active connections, chunk sizes, and I/O timeouts. File reads are limited to regular files no larger than 64 MiB and 64 KiB per chunk.

Tools may call `progress.update` to report live progress. The host displays throttled terminal updates and publishes structured events. `ktrx run <file.tool> --events` emits `progress` events, a `process` event for every program the tool starts, and the final result as newline-delimited JSON; when the run fails, the final `finished` event carries an `error` message. The queue holds at most 256 events; intermediate snapshots may be dropped under backpressure. Ctrl+C requests cancellation. Tools have no execution deadline by default; a manifest may request an explicit 30–180 second deadline.

Capability failures return errors and never grant access silently. Plugins should handle returned errors and return structured JSON errors rather than trap.
