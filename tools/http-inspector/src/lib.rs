wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        let v: serde_json::Value = match serde_json::from_str(&input) {
            Ok(v) => v,
            Err(e) => return serde_json::json!({"error":e.to_string()}).to_string(),
        };
        let url = match v.get("url").and_then(|x| x.as_str()) {
            Some(u) => u,
            None => return "{\"error\":\"missing URL\"}".into(),
        };
        match ktrx::tool::host::request("http.get", &serde_json::json!({"url":url}).to_string()) {
            Ok(v) => v,
            Err(e) => serde_json::json!({"error":e}).to_string(),
        }
    }
}
export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_missing_url() {
        use exports::ktrx::tool::tool::Guest;
        assert_eq!(Plugin::run("{}".into()), r#"{"error":"missing URL"}"#);
    }
}
