wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
use base64::Engine as _;
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
 fn run(input:String)->String { let result=(||->Result<serde_json::Value,String>{
  let v:serde_json::Value=serde_json::from_str(&input).map_err(|e|e.to_string())?; let token=v["token"].as_str().ok_or("missing token")?;let parts:Vec<&str>=token.split('.').collect();if parts.len()!=3{return Err("JWT must have three dot-separated parts".into())}
  let decode=|s:&str|->Result<serde_json::Value,String>{let bytes=base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).or_else(|_|base64::engine::general_purpose::URL_SAFE.decode(s)).map_err(|e|format!("invalid base64url segment: {e}"))?;serde_json::from_slice(&bytes).map_err(|e|format!("segment is not JSON: {e}"))};
  let header=decode(parts[0])?;let claims=decode(parts[1])?;let times=["exp","nbf","iat"].iter().filter_map(|k|claims.get(*k).map(|x|serde_json::json!({"claim":k,"value":x}))).collect::<Vec<_>>();Ok(serde_json::json!({"header":header,"claims":claims,"time_claims":times,"signature_present":!parts[2].is_empty(),"verified":false,"notice":"JWT contents decoded only; signature and claims are not verified."}))
 })();match result{Ok(x)=>x.to_string(),Err(e)=>serde_json::json!({"error":e}).to_string()} }
}
export!(Plugin);
