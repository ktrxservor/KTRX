# KTRX plugin contributor instructions

- Keep each plugin independent under its own `tools/<id>/` directory and Cargo package.
- Do not add plugins to the engine Cargo workspace or use native plugin execution.
- Implement the versioned WIT interface in `engine/wit/ktrx.wit`, target `wasm32-wasip2`, and keep tool results as JSON text.
- Perform network and local-file operations only through `ktrx::tool::host::request`; declare needed permissions and handle denial.
- Declare the network permission for network tools; the engine asks for approval each run and permits the user-selected destination. Do not add hostname allowlists to manifests.
- Include examples and tests in each plugin. Do not commit generated `module.wasm` files for normal development.
