# K~APOLLO — Hello Tool
> Apollon, dieu grec de la lumière : cet outil d’exemple accueille l’utilisateur et illustre le format minimal d’un plugin KTRX.

Minimal KTRX Component Model plugin. Example from the project root: `ktrx run 'tools/APOLLO ( Hello-Tool )/K~APOLLO.tool' --name Ada`.

Test and build independently:

```sh
cargo test --manifest-path 'tools/APOLLO ( Hello-Tool )/Cargo.toml'
cargo build --manifest-path 'tools/APOLLO ( Hello-Tool )/Cargo.toml' --release --target wasm32-wasip2
```
