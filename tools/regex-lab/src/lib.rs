wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });
use regex::Regex;
struct Plugin;
impl exports::ktrx::tool::tool::Guest for Plugin {
 fn run(input:String)->String{let r=(||->Result<serde_json::Value,String>{let a:serde_json::Value=serde_json::from_str(&input).map_err(|e|e.to_string())?;let pattern=a["pattern"].as_str().ok_or("missing pattern")?;let text=a["text"].as_str().ok_or("missing text")?;let re=Regex::new(pattern).map_err(|e|format!("invalid regex: {e}"))?;let matches=re.captures_iter(text).take(1000).map(|caps|{let m=caps.get(0).unwrap();let groups=(0..caps.len()).map(|i|caps.get(i).map(|g|serde_json::json!({"index":i,"text":g.as_str(),"start":g.start(),"end":g.end()})).unwrap_or_else(||serde_json::json!({"index":i,"text":null}))).collect::<Vec<_>>();serde_json::json!({"text":m.as_str(),"start":m.start(),"end":m.end(),"captures":groups})}).collect::<Vec<_>>();let truncated=matches.len()==1000&&re.find_iter(text).count()>1000;Ok(serde_json::json!({"pattern":pattern,"matches":matches,"count_returned":matches.len(),"truncated":truncated}))})();match r{Ok(x)=>x.to_string(),Err(e)=>serde_json::json!({"error":e}).to_string()}}
}
export!(Plugin);
