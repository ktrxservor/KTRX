# KTRX plugin contributor instructions

- Follow the complete root-level `KTRX_TOOL_AUTHORING_GUIDE.md`.
- Keep each plugin independent under `tools/<MYTH> ( <Tool-Name> )/`; name its package `K~<MYTH>.tool` and its manifest display name `K~<MYTH>`. Keep the technical manifest `id` a stable lowercase slug.
- Add one concise mythology-and-purpose line beneath the title in every tool README. Keep mythological names unique. The shared SDK `tools/ktrx-net-sdk/` is not a `.tool` plugin and retains its technical name.
- Give every plugin an `icon.png` emblem (square 1:1 PNG, 1024 × 1024, no text) at the root of its folder, in the style defined by `docs/ICON_STYLE_GUIDE.md`. Start the tool README with the header block from that guide (above the title), and add the tool to the gallery in the root `README.md`. Until the tool has its own emblem, copy `docs/images/default-icon.png` to `icon.png`. The emblem is not packaged in the `.tool` archive.
- Do not add plugins to the engine Cargo workspace or use native plugin execution. Target `wasm32-wasip2` and implement the versioned WIT interface in `engine/wit/ktrx.wit`; tool results are JSON text.
- Perform network and local-file operations only through `ktrx::tool::host::request`; declare needed permissions and handle denial. Network permission is approved for each run. Do not add host allowlists or destination restrictions to manifests.
- Include examples and tests. Do not commit generated `module.wasm` files or `target/` directories; do commit the current compiled `K~<MYTH>.tool` archive.
