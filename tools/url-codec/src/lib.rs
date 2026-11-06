wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
 fn run(input:String)->String { let result=(||->Result<serde_json::Value,String>{
  let v:serde_json::Value=serde_json::from_str(&input).map_err(|e|e.to_string())?;
  let s=v["text"].as_str().ok_or("missing text")?;
  let enc=v["encode"].as_bool()==Some(true); let dec=v["decode"].as_bool()==Some(true);
  if enc==dec{return Err("choose exactly one of --encode or --decode".into())}
  if enc { let mut out=String::new(); for b in s.bytes(){if b.is_ascii_alphanumeric()||b"-._~".contains(&b){out.push(b as char)}else{out.push_str(&format!("%{b:02X}"))}} Ok(serde_json::json!({"encoded":out})) }
  else { let bytes=s.as_bytes(); let mut out=Vec::new(); let mut i=0; while i<bytes.len(){if bytes[i]==b'%'{if i+2>=bytes.len(){return Err(format!("incomplete percent escape at byte {i}"))} let digit=|b:u8|->Option<u8>{match b{b'0'..=b'9'=>Some(b-b'0'),b'a'..=b'f'=>Some(b-b'a'+10),b'A'..=b'F'=>Some(b-b'A'+10),_=>None}};let hi=digit(bytes[i+1]).ok_or_else(||format!("invalid percent escape at byte {i}"))?;let lo=digit(bytes[i+2]).ok_or_else(||format!("invalid percent escape at byte {i}"))?;out.push((hi<<4)|lo);i+=3}else{out.push(bytes[i]);i+=1}} let decoded=String::from_utf8(out).map_err(|_|"decoded bytes are not valid UTF-8")?;Ok(serde_json::json!({"decoded":decoded})) }
 })(); match result{Ok(x)=>x.to_string(),Err(e)=>serde_json::json!({"error":e}).to_string()} }
}
export!(Plugin);
