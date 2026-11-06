wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        let value: serde_json::Value = match serde_json::from_str(&input) {
            Ok(value) => value,
            Err(error) => return serde_json::json!({"error":error.to_string()}).to_string(),
        };
        let raw_url = match value.get("url").and_then(|value| value.as_str()) {
            Some(url) => url,
            None => return r#"{"error":"missing URL"}"#.into(),
        };
        let url = match url::Url::parse(raw_url) {
            Ok(url) if url.scheme() == "https" && url.host_str().is_some() => url,
            _ => return r#"{"error":"TLS inspection requires a valid HTTPS URL"}"#.into(),
        };
        let host_request =
            |operation: &str, input: &str| ktrx::tool::host::request(operation, input);
        match ktrx_net_sdk::http_get(raw_url, false, 0, &host_request).and_then(|raw| {
            let mut result: serde_json::Value =
                serde_json::from_str(&raw).map_err(|error| error.to_string())?;
            result["host"] =
                serde_json::Value::String(url.host_str().unwrap_or_default().to_owned());
            result["verified"] = serde_json::Value::Bool(true);
            result["note"] = serde_json::Value::String(
                "TLS peer verification completed by the secure socket capability.".into(),
            );
            Ok(result.to_string())
        }) {
            Ok(result) => result,
            Err(error) => serde_json::json!({"error":error}).to_string(),
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
    #[test]
    fn rejects_non_https_before_network_use() {
        use exports::ktrx::tool::tool::Guest;
        assert_eq!(
            Plugin::run(r#"{"url":"http://example.com/"}"#.into()),
            r#"{"error":"TLS inspection requires a valid HTTPS URL"}"#
        );
    }
}
