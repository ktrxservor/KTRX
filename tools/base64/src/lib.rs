wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        use base64::Engine as _;
        let result = std::panic::catch_unwind(|| -> Result<serde_json::Value, String> {
            let v: serde_json::Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
            let text = v
                .get("text")
                .and_then(|x| x.as_str())
                .ok_or("missing text")?;
            let enc = base64::engine::general_purpose::STANDARD;
            let encode = v.get("encode").and_then(|x| x.as_bool()) == Some(true);
            let decode = v.get("decode").and_then(|x| x.as_bool()) == Some(true);
            if encode && decode {
                Err("choose either --encode or --decode".into())
            } else if encode {
                Ok(serde_json::json!({"encoded":enc.encode(text)}))
            } else if decode {
                let bytes = enc.decode(text).map_err(|e| e.to_string())?;
                let decoded = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                Ok(serde_json::json!({"decoded":decoded}))
            } else {
                Err("select --encode or --decode".into())
            }
        });
        match result {
            Ok(Ok(v)) => v.to_string(),
            Ok(Err(e)) => serde_json::json!({"error":e}).to_string(),
            Err(_) => "{\"error\":\"internal tool error\"}".into(),
        }
    }
}
export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encodes_and_decodes_text() {
        use exports::ktrx::tool::tool::Guest;
        assert_eq!(
            Plugin::run(r#"{"text":"Hello World","encode":true}"#.into()),
            r#"{"encoded":"SGVsbG8gV29ybGQ="}"#
        );
        assert_eq!(
            Plugin::run(r#"{"text":"SGVsbG8=","decode":true}"#.into()),
            r#"{"decoded":"Hello"}"#
        );
    }
}
