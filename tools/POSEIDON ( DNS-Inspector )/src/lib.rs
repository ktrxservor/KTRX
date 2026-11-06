wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        let v: serde_json::Value = match serde_json::from_str(&input) {
            Ok(v) => v,
            Err(e) => return serde_json::json!({"error":e.to_string()}).to_string(),
        };
        let host = match v.get("host").and_then(|x| x.as_str()) {
            Some(h) => h,
            None => return "{\"error\":\"missing hostname\"}".into(),
        };
        let host_request = |operation: &str, input: &str| ktrx::tool::host::request(operation, input);
        match ktrx_net_sdk::resolve(host, 0, &host_request) {
            Ok(addresses) => serde_json::json!({"host":host,"addresses":addresses}).to_string(),
            Err(e) => serde_json::json!({"error":e}).to_string(),
        }
    }
}
export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_missing_hostname() {
        use exports::ktrx::tool::tool::Guest;
        assert_eq!(Plugin::run("{}".into()), r#"{"error":"missing hostname"}"#);
    }
}
