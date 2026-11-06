# K~HADES — Hash
> Hadès, gardien du monde souterrain grec : cet outil calcule des empreintes cryptographiques à sens unique pour les données fournies.

Computes SHA-256 for UTF-8 text or a local file. File reads are chunked through KTRX and require `file-read` permission approval. Examples from the project root: `ktrx run 'tools/HADES ( Hash )/K~HADES.tool' 'Hello World'` and `ktrx run 'tools/HADES ( Hash )/K~HADES.tool' --file ./document.pdf`.

Test and build independently:

```sh
cargo test --manifest-path 'tools/HADES ( Hash )/Cargo.toml'
cargo build --manifest-path 'tools/HADES ( Hash )/Cargo.toml' --release --target wasm32-wasip2
```
