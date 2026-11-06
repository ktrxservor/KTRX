wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
 fn run(input:String)->String{let r=(||->Result<serde_json::Value,String>{let a:serde_json::Value=serde_json::from_str(&input).map_err(|e|e.to_string())?;let raw=a["json"].as_str().ok_or("missing json")?;let value:serde_json::Value=serde_json::from_str(raw).map_err(|e|format!("invalid JSON: {e}"))?;let pretty=a["compact"].as_bool()!=Some(true);let formatted=if pretty{serde_json::to_string_pretty(&value)}else{serde_json::to_string(&value)}.map_err(|e|e.to_string())?;let kind=match &value{serde_json::Value::Null=>"null",serde_json::Value::Bool(_)=>"boolean",serde_json::Value::Number(_)=>"number",serde_json::Value::String(_)=>"string",serde_json::Value::Array(_)=>"array",serde_json::Value::Object(_)=>"object"};let keys=value.as_object().map(|o|o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();Ok(serde_json::json!({"valid":true,"formatted":formatted,"format":if pretty{"pretty"}else{"compact"},"root_type":kind,"top_level_keys":keys}))})();match r{Ok(x)=>x.to_string(),Err(e)=>serde_json::json!({"error":e}).to_string()}}
}
export!(Plugin);
