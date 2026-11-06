# Engine verification

The `ktrx-core` crate includes unit tests for manifest IDs, typed arguments, required values, and invalid input. Each independent plugin has tests for its primary behavior or argument errors.

End-to-end package validation requires the `wasm32-wasip2` Rust target and the engine build. Build `hello-tool`, package it with `ktrx pack 'tools/APOLLO ( Hello-Tool )' --output 'tools/APOLLO ( Hello-Tool )/K~APOLLO.tool'`, then run `ktrx validate 'tools/APOLLO ( Hello-Tool )/K~APOLLO.tool'` and `ktrx run 'tools/APOLLO ( Hello-Tool )/K~APOLLO.tool' --name Ada`.
