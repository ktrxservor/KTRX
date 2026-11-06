# KTRX plugin contributor instructions

- Follow the complete root-level `KTRX_TOOL_AUTHORING_GUIDE.md`.
- Keep each plugin independent under `tools/<MYTH> ( <Tool-Name> )/`; name its package `K~<MYTH>.tool` and its manifest display name `K~<MYTH>`. Keep the technical manifest `id` a stable lowercase slug.
- Add one concise mythology-and-purpose line beneath the title in every tool README. Keep mythological names unique. The shared SDK `tools/ktrx-net-sdk/` is not a `.tool` plugin and retains its technical name.
- Do not add plugins to the engine Cargo workspace or use native plugin execution. Target `wasm32-wasip2` and implement the versioned WIT interface in `engine/wit/ktrx.wit`; tool results are JSON text.
- Perform network and local-file operations only through `ktrx::tool::host::request`; declare needed permissions and handle denial. Network permission is approved for each run. Do not add host allowlists or destination restrictions to manifests.
- Include examples and tests. Do not commit generated `module.wasm` files or `target/` directories; do commit the current compiled `K~<MYTH>.tool` archive.
