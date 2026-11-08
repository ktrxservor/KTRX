<p align="center">
  <img src="icon.png" width="220" alt="Emblème de K~POSEIDON — DNS Inspector">
</p>

<p align="center"><sub><code>DOSSIER K~POSEIDON</code> &nbsp;·&nbsp; <code>KTRX SYSTEMS ARCHIVE</code> &nbsp;·&nbsp; <code>ARCHIVE PUBLIQUE</code></sub></p>

# K~POSEIDON — DNS Inspector
> Poséidon, dieu grec des mers : cet outil explore les réponses DNS associées aux noms de domaine.

Resolves A and AAAA addresses through the KTRX host API after interactive network approval. Private and reserved DNS results are returned too. Example from the project root: `ktrx run 'tools/POSEIDON ( DNS-Inspector )/K~POSEIDON.tool' example.com`.

Test and build independently with `cargo test --manifest-path 'tools/POSEIDON ( DNS-Inspector )/Cargo.toml'` and `cargo build --manifest-path 'tools/POSEIDON ( DNS-Inspector )/Cargo.toml' --release --target wasm32-wasip2`.
