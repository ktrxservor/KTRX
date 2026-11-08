# Engine verification

The `ktrx-core` crate includes unit tests for manifest IDs, typed arguments, required values, and invalid input. Each independent plugin has tests for its primary behavior or argument errors.

End-to-end package validation requires the `wasm32-wasip2` Rust target and the engine build. Build a plugin such as THOTH, package it with `ktrx pack 'tools/THOTH ( Base64 )' --output 'tools/THOTH ( Base64 )/K~THOTH.tool'`, then run `ktrx validate 'tools/THOTH ( Base64 )/K~THOTH.tool'` and `ktrx run 'tools/THOTH ( Base64 )/K~THOTH.tool' --encode 'Hello World'`.

## Host capability tests

`engine/tests/host-probe/` is a tiny `wasm32-wasip2` fixture that forwards `host.request` operations, so `crates/ktrx-runtime/tests/capabilities.rs` can exercise the real runtime end to end: manifest declarations and per-run approvals, write confinement, UDP, DNS (against a local fake server), TLS (against a local self-signed server), and ICMP. It is not part of the engine workspace. To rebuild the committed fixture:

```sh
cargo build --manifest-path engine/tests/host-probe/Cargo.toml --release --target wasm32-wasip2
cp engine/tests/host-probe/target/wasm32-wasip2/release/ktrx_host_probe.wasm engine/tests/fixtures/host-probe.component.wasm
```

Package it with `engine/tests/host-probe/manifest.toml` (and `module.wasm`) using `ktrx pack` to try any operation from the command line: `ktrx run -y host-probe.tool --op dns.query --input '{"name":"example.com","type":"MX"}'`.

The Unix-only paths (raw ICMP, TCP SYN probes, `/proc/net/arp`) are exercised on Linux by building the `rawnet`, `dns`, and `fs` modules for `x86_64-unknown-linux-musl` and running them as root.
