//! Engine test fixture: forwards host operations so the engine's permission
//! checks and capabilities can be exercised end to end from WebAssembly.
//!
//! Input is either `{"op": "...", "input": {...}}` for one operation, or
//! `{"steps": [{"op": "...", "input": {...}}, ...]}` for a sequence. Each
//! result is `{"ok": <host response>}` or `{"error": "<host error>"}`.

wit_bindgen::generate!({ path: "../../wit", world: "ktrx-tool" });

use serde_json::{Value, json};

struct Plugin;

fn call(step: &Value) -> Value {
    let Some(op) = step.get("op").and_then(Value::as_str) else {
        return json!({"error": "step requires op"});
    };
    let payload = step.get("input").cloned().unwrap_or_else(|| json!({}));
    match ktrx::tool::host::request(op, &payload.to_string()) {
        Ok(text) => {
            json!({"ok": serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text))})
        }
        Err(error) => json!({"error": error}),
    }
}

impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        let Ok(request) = serde_json::from_str::<Value>(&input) else {
            return json!({"error": "input must be JSON"}).to_string();
        };
        // The CLI passes JSON documents as strings; accept both forms.
        let decode = |value: &Value| match value {
            Value::String(text) => serde_json::from_str(text).unwrap_or_else(|_| value.clone()),
            other => other.clone(),
        };
        if let Some(steps) = request.get("steps") {
            let steps = decode(steps);
            let results: Vec<Value> = steps
                .as_array()
                .map(|steps| steps.iter().map(call).collect())
                .unwrap_or_default();
            return json!({"results": results}).to_string();
        }
        let mut step = request.clone();
        if let Some(payload) = request.get("input") {
            step["input"] = decode(payload);
        }
        call(&step).to_string()
    }
}

export!(Plugin);
