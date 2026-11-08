# KTRX `.tool` specification — API 1

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

`id` uses lowercase ASCII letters, digits, and hyphens. `api_version` must match the host API. Argument types are `string`, `integer`, `boolean`, and `path`; `positional` assigns the argument to the next non-option CLI value. API 1 permissions are `network` and `file-read`.

## Component interface and capabilities

`engine/wit/ktrx.wit` declares package `ktrx:tool@1.0.0`: `tool.run(input: string) -> string` receives validated JSON arguments and returns JSON text. `host.request(operation, input)` provides only generic capabilities:

- `progress.update`: publish a tool-authored progress snapshot.
- `file.read`, `file.list`, `file.stat` (`file-read`): read a user-approved regular file in bounded chunks, list a directory (up to 5000 entries), and inspect a path without failing when it does not exist.
- `file.create`, `file.write`, `file.append`, `file.mkdir` (`file-write`): create a file (never overwriting unless `overwrite` is true), write at an offset into a file created during the run, append to a file, and create directories. Every path resolves inside the directory the user chose for the run (`ktrx run --write-dir DIR`, default the current directory): `..`, absolute paths elsewhere, symbolic links out of the tree, and Windows device names are refused. Chunks are 64 KiB, a file is at most 256 MiB, a run writes at most 1 GiB, and at most eight files are open for writing. `file.close` flushes and closes a handle.
- `network.resolve`: resolve a hostname through the operating system resolver. With a `records` array (for example `["CNAME","MX"]`) it also returns those DNS records and the response codes, and a failed system lookup is reported instead of being fatal.
- `dns.query` (`network`): send one DNS question (`name`, `type`, optional `server`, `port`, `timeout_ms`, `recurse`, `dnssec`, `tcp`) and return the structured answer: response code, flags, and the answer, authority, and additional records. A, AAAA, CNAME, NS, PTR, MX, TXT, SOA, SRV, and CAA are decoded; other types return `raw_base64`. Without `server` the system's configured resolvers are used. Truncated UDP answers are retried over TCP.
- `socket.open`, `socket.connect`, `socket.poll`, `socket.read`, `socket.write`, `socket.close`: open permission-gated byte streams and transfer bounded byte chunks. `socket.connect` is asynchronous so a guest tool can manage concurrent connections; connection concurrency and scan policy remain the tool's responsibility. TLS on `socket.open` gives the tool an encrypted byte stream and reports what was negotiated in `tls_info`: version, cipher, ALPN protocol, SNI, whether verification succeeded and why not, and the certificate chain the server sent (subject, issuer, serial, validity, subject alternative names, key type and size, signature algorithm, SHA-256 fingerprint, DER). Optional `sni`, `alpn`, `tls_min`, `tls_max`, `ciphers`, `ciphersuites`, and `verify` shape the handshake; `verify: false` completes it against an expired or self-signed certificate while still reporting the verification result, and requesting TLS 1.0 or 1.1 relaxes OpenSSL's security level for that connection only.
- `udp.open`, `udp.send`, `udp.recv`, `udp.close` (`network`): datagram sockets (IPv4 or IPv6, optional broadcast and TTL). `udp.recv` reports a timeout, or `refused` when the host returned an ICMP port-unreachable, as data instead of an error. Sockets count toward the same 128-connection limit.
- `icmp.ping` (`raw-net`): one ICMP echo with a settable `ttl`, returning `reply`, `ttl_expired` (with the router's address, which is all a traceroute needs), `unreachable`, or `timeout`. IPv4 only. `arp.resolve` returns the MAC address of a host on the local segment (Windows and Linux). `tcp.syn_probe` sends a SYN and reports `open`, `closed`, or `filtered` (Linux with root or CAP_NET_RAW only; other systems return an error pointing to `socket.connect`).

The host does not expose HTTP requests, scan loops, service identification, vulnerability checks, or other tool-specific operations; a traceroute, a port scan, or a certificate grade is built by a tool from the primitives above. Tools implement protocol framing, limits, scheduling, and interpretation themselves. Do not add tool names or protocol workflows to the engine host API.

Every call requires the corresponding manifest permission (`network`, `file-read`, `file-write`, `raw-net`) and per-run approval, which `ktrx run` asks for interactively; `-y` approves everything the manifest declares. Non-interactive invocations deny these permissions. Network access is available only through the permission-checked host socket bridge; tools do not receive ambient WASI sockets or operating-system access. Socket calls enforce bounded active connections, chunk sizes, and I/O timeouts. File reads are limited to regular files no larger than 64 MiB and 64 KiB per chunk.

Tools may call `progress.update` to report live progress. The host displays throttled terminal updates and publishes structured events. `ktrx run <file.tool> --events` emits progress and the final result as newline-delimited JSON. The queue holds at most 256 events; intermediate snapshots may be dropped under backpressure. Ctrl+C requests cancellation. Tools have no execution deadline by default; a manifest may request an explicit 30–180 second deadline.

Capability failures return errors and never grant access silently. Plugins should handle returned errors and return structured JSON errors rather than trap.
