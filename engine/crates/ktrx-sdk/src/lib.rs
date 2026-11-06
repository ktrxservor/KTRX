//! Minimal guest SDK. Include the generated KTRX export bindings in each independent plugin.
pub use wit_bindgen;

/// Plugins implement `exports::ktrx::tool::tool::Guest` and export it with `export!`.
pub const API_VERSION: &str = "1";
