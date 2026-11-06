wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        use base64::Engine as _;
        use sha2::{Digest, Sha256};
        let r = std::panic::catch_unwind(|| -> Result<String, String> {
            let v: serde_json::Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
            if v.get("text").is_some() && v.get("file").is_some() {
                return Err("choose either text or --file <path>".into());
            }
            let mut hasher = Sha256::new();
            let source = if let Some(text) = v.get("text").and_then(|x| x.as_str()) {
                hasher.update(text.as_bytes());
                "text"
            } else if let Some(path) = v.get("file").and_then(|x| x.as_str()) {
                let mut offset = 0u64;
                loop {
                    let response = ktrx::tool::host::request(
                        "file.read",
                        &serde_json::json!({"path":path,"offset":offset,"length":65536})
                            .to_string(),
                    )?;
                    let response: serde_json::Value =
                        serde_json::from_str(&response).map_err(|e| e.to_string())?;
                    let encoded = response
                        .get("bytes_base64")
                        .and_then(|x| x.as_str())
                        .ok_or("host returned an invalid file response")?;
                    let chunk = base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .map_err(|e| e.to_string())?;
                    if chunk.is_empty() {
                        break;
                    }
                    offset += chunk.len() as u64;
                    hasher.update(chunk);
                }
                "file"
            } else {
                return Err("provide text or --file <path>".into());
            };
            let digest = hasher.finalize();
            Ok(serde_json::json!({"algorithm":"SHA-256","source":source,"hex":format!("{digest:x}")}).to_string())
        });
        match r {
            Ok(Ok(v)) => v,
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
    fn hashes_text_with_sha256() {
        use exports::ktrx::tool::tool::Guest;
        assert_eq!(
            Plugin::run(r#"{"text":"abc"}"#.into()),
            r#"{"algorithm":"SHA-256","hex":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","source":"text"}"#
        );
    }
}
