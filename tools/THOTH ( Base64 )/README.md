<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~THOTH — Base64">
</p>

<p align="center"><sub><code>DOSSIER K~THOTH</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~THOTH — Base64
> Thot, dieu égyptien de l’écriture et du savoir : cet outil encode et décode du texte en Base64 pour faciliter son transport.

Portable Base64 encoding and decoding for UTF-8 text. Example from the project root: `ktrx run 'tools/THOTH ( Base64 )/K~THOTH.tool' --encode 'Hello World'`.

Test and build independently:

```sh
cargo test --manifest-path 'tools/THOTH ( Base64 )/Cargo.toml'
cargo build --manifest-path 'tools/THOTH ( Base64 )/Cargo.toml' --release --target wasm32-wasip2
```
