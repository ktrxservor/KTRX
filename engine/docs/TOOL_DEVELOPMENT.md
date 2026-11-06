# Developing a KTRX tool

Use the complete, standalone requirements in the repository root file [KTRX_TOOL_AUTHORING_GUIDE.md](../../KTRX_TOOL_AUTHORING_GUIDE.md). That guide defines the tool architecture, generic host capabilities, manifest schema, quality requirements, and mandatory mythological directory and `K~NAME.tool` naming convention.

At a high level, each plugin is an independent Rust `cdylib` component targeting `wasm32-wasip2`. It implements `ktrx:tool/tool.run`, receives JSON arguments, returns JSON text, and declares only the generic host permissions it needs. Do not add tool-specific behavior to the engine. Build and validate every package from its source and manifest before delivery.
