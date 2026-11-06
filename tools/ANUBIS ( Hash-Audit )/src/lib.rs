wit_bindgen::generate!({ path: "wit", world: "ktrx-tool" });

use base64::Engine as _;
use md5::Md5;
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512};
use sha3::{Sha3_224, Sha3_256, Sha3_384, Sha3_512};
use std::collections::{BTreeMap, HashMap, HashSet};
use sha2::Digest;

const MAX_WORDLIST_BYTES: usize = 8 * 1024 * 1024;
const MAX_CANDIDATES: usize = 2_000_000;
const READ_CHUNK: usize = 65_536;

struct Plugin;

impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        match run_audit(&input) {
            Ok(value) => value.to_string(),
            Err(error) => json!({"error":error}).to_string(),
        }
    }
}

fn run_audit(input: &str) -> Result<Value, String> {
    let args: Value = serde_json::from_str(input).map_err(|e| format!("invalid input JSON: {e}"))?;
    let raw_hashes = args.get("hashes").and_then(Value::as_str).ok_or("hashes argument is required")?;
    let mut hashes = Vec::new();
    for raw in raw_hashes.split(',') {
        let hash = raw.trim().to_ascii_lowercase();
        if !valid_hex(&hash) { return Err(format!("invalid hexadecimal digest: {raw}")); }
        if !hashes.contains(&hash) { hashes.push(hash); }
    }
    if hashes.is_empty() { return Err("at least one hash is required".into()); }
    let requested_algorithm = args.get("algorithm").and_then(Value::as_str).unwrap_or("auto");
    let algorithm = normalize_algorithm(requested_algorithm)?;
    let active_algorithms = choose_algorithms(&hashes, &algorithm)?;
    let mut targets: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for hash in &hashes {
        for alg in &active_algorithms {
            if hash.len() == digest_hex_len(alg) { targets.entry(alg.clone()).or_default().insert(hash.clone()); }
        }
    }
    if targets.is_empty() { return Err("no digest length is compatible with the selected algorithm".into()); }

    let brute_force = args.get("bruteforce").and_then(Value::as_bool).unwrap_or(false);
    let max_candidates = args.get("max-candidates").and_then(Value::as_i64).unwrap_or(0).clamp(0, MAX_CANDIDATES as i64) as usize;
    let mut recovered: HashMap<(String,String), String> = HashMap::new();
    let mut candidates_tested = 0usize;
    let mut complete = true;
    let mut last_progress = 0usize;

    if brute_force {
        let charset_name = args.get("charset").and_then(Value::as_str).unwrap_or("lower-digit");
        let alphabet = charset(charset_name)?;
        let min_len = args.get("min-length").and_then(Value::as_i64).unwrap_or(1).clamp(1,16) as usize;
        let max_len = args.get("max-length").and_then(Value::as_i64).unwrap_or(4).clamp(1,16) as usize;
        if min_len > max_len { return Err("min-length must not exceed max-length".into()); }
        'lengths: for len in min_len..=max_len {
            let mut positions = vec![0usize; len];
            loop {
                if max_candidates > 0 && candidates_tested >= max_candidates { complete = false; break 'lengths; }
                let candidate: Vec<u8> = positions.iter().map(|i| alphabet[*i]).collect();
                check_candidate(&candidate, &active_algorithms, &targets, &mut recovered);
                candidates_tested += 1;
                if candidates_tested - last_progress >= 10_000 {
                    progress(candidates_tested, recovered.len(), false);
                    last_progress = candidates_tested;
                }
                if recovered.len() == target_count(&targets) { break 'lengths; }
                let mut position = len;
                while position > 0 {
                    position -= 1;
                    positions[position] += 1;
                    if positions[position] < alphabet.len() { break; }
                    positions[position] = 0;
                }
                if position == 0 && positions[0] == 0 { break; }
            }
        }
    } else {
        let path = args.get("file").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("provide --file or enable --bruteforce")?;
        let wordlist = read_wordlist(path)?;
        for line in wordlist.lines() {
            if max_candidates > 0 && candidates_tested >= max_candidates { complete = false; break; }
            let candidate = line.trim_end_matches('\r').as_bytes();
            check_candidate(candidate, &active_algorithms, &targets, &mut recovered);
            candidates_tested += 1;
            if candidates_tested - last_progress >= 10_000 {
                progress(candidates_tested, recovered.len(), false);
                last_progress = candidates_tested;
            }
            if recovered.len() == target_count(&targets) { break; }
        }
    }

    progress(candidates_tested, recovered.len(), true);
    let results: Vec<Value> = hashes.iter().map(|hash| {
        let matches: Vec<Value> = active_algorithms.iter().filter(|alg| hash.len() == digest_hex_len(alg))
            .map(|alg| {
                let key = (alg.clone(), hash.clone());
                if let Some(plaintext) = recovered.get(&key) {
                    json!({"algorithm":alg,"found":true,"plaintext":plaintext})
                } else {
                    json!({"algorithm":alg,"found":false})
                }
            }).collect();
        json!({"hash":hash,"matches":matches,"found":matches.iter().any(|m|m.get("found").and_then(Value::as_bool)==Some(true))})
    }).collect();
    let found_hashes = hashes.iter().filter(|hash| active_algorithms.iter().any(|alg| recovered.contains_key(&(alg.clone(), (*hash).clone())))).count();
    let unmatched: Vec<&String> = hashes.iter().filter(|hash| !active_algorithms.iter().any(|alg| recovered.contains_key(&(alg.clone(), (*hash).clone())))).collect();
    Ok(json!({
        "status":if complete {"complete"} else {"partial"},
        "mode":if brute_force {"bruteforce"} else {"wordlist"},
        "algorithms_checked":active_algorithms,
        "candidates_tested":candidates_tested,
        "hashes_total":hashes.len(),
        "hashes_recovered":found_hashes,
        "complete":complete,
        "results":results,
        "unmatched_hashes":unmatched
    }))
}

fn check_candidate(candidate: &[u8], algorithms: &[String], targets: &BTreeMap<String,HashSet<String>>, recovered: &mut HashMap<(String,String),String>) {
    for algorithm in algorithms {
        if !targets.contains_key(algorithm) { continue; }
        let digest = digest_hex(algorithm, candidate);
        if targets.get(algorithm).is_some_and(|set| set.contains(&digest)) {
            recovered.entry((algorithm.clone(),digest)).or_insert_with(|| String::from_utf8_lossy(candidate).into_owned());
        }
    }
}

fn read_wordlist(path: &str) -> Result<String,String> {
    let mut offset = 0usize;
    let mut bytes = Vec::new();
    loop {
        let raw = ktrx::tool::host::request("file.read", &json!({"path":path,"offset":offset,"length":READ_CHUNK}).to_string()).map_err(|e| format!("wordlist read denied or failed: {e}"))?;
        let response: Value = serde_json::from_str(&raw).map_err(|e| format!("invalid file.read response: {e}"))?;
        let encoded = response.get("bytes_base64").and_then(Value::as_str).ok_or("file.read response lacks bytes_base64")?;
        let chunk = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| format!("invalid Base64 wordlist chunk: {e}"))?;
        if chunk.is_empty() { break; }
        if bytes.len() + chunk.len() > MAX_WORDLIST_BYTES { return Err("wordlist exceeds the 8 MiB limit".into()); }
        offset += chunk.len();
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| "wordlist must be valid UTF-8".into())
}

fn progress(current: usize, recovered: usize, done: bool) {
    let _ = ktrx::tool::host::request("progress.update", &json!({"phase":"hash-audit","current":current,"recovered":recovered,"item_label":"candidats testés","result_label":"hashes retrouvés","done":done}).to_string());
}

fn normalize_algorithm(raw: &str) -> Result<String,String> {
    let norm = raw.to_ascii_lowercase().replace('_', "-");
    let algorithm = match norm.as_str() {
        "auto" => "auto",
        "md5" => "md5",
        "sha1" | "sha-1" => "sha1",
        "sha224" | "sha-224" => "sha224",
        "sha256" | "sha-256" => "sha256",
        "sha384" | "sha-384" => "sha384",
        "sha512" | "sha-512" => "sha512",
        "sha3-224" | "sha3224" => "sha3-224",
        "sha3-256" | "sha3256" => "sha3-256",
        "sha3-384" | "sha3384" => "sha3-384",
        "sha3-512" | "sha3512" => "sha3-512",
        _ => return Err(format!("unsupported hash algorithm: {raw}")),
    };
    Ok(algorithm.into())
}

fn choose_algorithms(hashes: &[String], requested: &str) -> Result<Vec<String>,String> {
    let supported = ["md5","sha1","sha224","sha256","sha384","sha512","sha3-224","sha3-256","sha3-384","sha3-512"];
    if requested != "auto" {
        if hashes.iter().any(|h| h.len() != digest_hex_len(requested)) { return Err(format!("digest length does not match {requested}")); }
        return Ok(vec![requested.to_owned()]);
    }
    let algs: Vec<String> = supported.iter().filter(|alg| hashes.iter().any(|hash| hash.len()==digest_hex_len(alg))).map(|s|(*s).to_owned()).collect();
    if algs.is_empty() { return Err("unsupported digest length; expected MD5, SHA-1, SHA-224/256/384/512 or SHA3-224/256/384/512".into()); }
    Ok(algs)
}

fn digest_hex_len(algorithm: &str) -> usize {
    match algorithm { "md5"=>32,"sha1"=>40,"sha224"|"sha3-224"=>56,"sha256"|"sha3-256"=>64,"sha384"|"sha3-384"=>96,"sha512"|"sha3-512"=>128,_=>0 }
}

fn digest_hex(algorithm: &str, bytes: &[u8]) -> String {
    let digest: Vec<u8> = match algorithm {
        "md5" => Md5::digest(bytes).to_vec(),
        "sha1" => Sha1::digest(bytes).to_vec(),
        "sha224" => Sha224::digest(bytes).to_vec(),
        "sha256" => Sha256::digest(bytes).to_vec(),
        "sha384" => Sha384::digest(bytes).to_vec(),
        "sha512" => Sha512::digest(bytes).to_vec(),
        "sha3-224" => Sha3_224::digest(bytes).to_vec(),
        "sha3-256" => Sha3_256::digest(bytes).to_vec(),
        "sha3-384" => Sha3_384::digest(bytes).to_vec(),
        "sha3-512" => Sha3_512::digest(bytes).to_vec(),
        _ => Vec::new(),
    };
    let mut text = String::with_capacity(digest.len()*2);
    for byte in digest { use std::fmt::Write; let _ = write!(&mut text,"{byte:02x}"); }
    text
}

fn valid_hex(value: &str) -> bool { !value.is_empty() && value.len()%2==0 && value.bytes().all(|b| b.is_ascii_hexdigit()) }
fn target_count(targets: &BTreeMap<String,HashSet<String>>) -> usize { targets.values().map(HashSet::len).sum() }

fn charset(name: &str) -> Result<Vec<u8>,String> {
    let bytes = match name {
        "digits" => b"0123456789".as_slice(),
        "lower" => b"abcdefghijklmnopqrstuvwxyz".as_slice(),
        "lower-digit" => b"abcdefghijklmnopqrstuvwxyz0123456789".as_slice(),
        "alnum" => b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".as_slice(),
        _ => return Err("charset must be digits, lower, lower-digit, or alnum".into()),
    };
    Ok(bytes.to_vec())
}

export!(Plugin);
