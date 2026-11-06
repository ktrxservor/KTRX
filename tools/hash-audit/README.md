# Hash Audit

Hash Audit is an offline helper for auditing hashes you provide. It checks raw, unsalted MD5, SHA-1, SHA-224/256/384/512, and SHA3-224/256/384/512 against a local wordlist or a selected brute-force alphabet. It does not connect to websites, accounts, password databases, or remote services.

`algorithm = auto` checks all algorithms compatible with a digest’s length, since SHA-256 and SHA3-256, for example, have the same length. Results identify the algorithm, digest and recovered candidate. A candidate match is limited to the chosen wordlist or finite search range.

Wordlist access goes through KTRX’s local-file approval. Files are read in 64 KiB chunks and capped at 8 MiB. Brute-force character sets are `digits`, `lower`, `lower-digit`, and `alnum`; lengths range from 1 to 16. `--max-candidates 0` means no candidate-count cutoff (the selected finite search can still take a long time). KTRX shows live progress; press `Ctrl+C` to cancel.

```sh
ktrx run hash-audit.tool 900150983cd24fb0d6963f7d28e17f72 --algorithm md5 --file ./wordlist.txt
ktrx run hash-audit.tool 900150983cd24fb0d6963f7d28e17f72 --algorithm md5 --bruteforce --charset lower --min-length 1 --max-length 4
```

## Build

```sh
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/ktrx_hash_audit.wasm module.wasm
ktrx pack . --output hash-audit.tool
ktrx validate hash-audit.tool
```
