wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        let name = serde_json::from_str::<serde_json::Value>(&input)
            .ok()
            .and_then(|v| v.get("name").and_then(|x| x.as_str()).map(str::to_owned))
            .unwrap_or_else(|| "world".into());
        serde_json::json!({"message":format!("Hello, {name}!"),"api_version":"1"}).to_string()
    }
}
export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn greets_name() {
        use exports::ktrx::tool::tool::Guest;
        assert_eq!(
            Plugin::run(r#"{"name":"Ada"}"#.into()),
            r#"{"api_version":"1","message":"Hello, Ada!"}"#
        );
    }
}
