wit_bindgen::generate!({ path: "wit", world: "ktrx-tool" });

use serde_json::{Value, json};
use std::collections::HashSet;
use url::Url;

const MAX_WORDLIST_BYTES: u64 = 1_048_576;
const MAX_CANDIDATES: usize = 5_000;
const MAX_RESULTS: usize = 500;
const BUILTIN_WORDS: &str = "admin\napi\nassets\nauth\nbackup\nbin\nblog\ncache\nconfig\nconsole\ncss\ndashboard\ndata\ndebug\ndocs\ndownload\nexport\nfiles\nhealth\nhelp\nhome\nimages\nimg\nincludes\njs\nlogin\nlogout\nmedia\nold\nprivate\nprofile\npublic\nrobots.txt\nsitemap.xml\nstatic\nstatus\nstorage\ntest\ntmp\nuser\nusers\nversion\nwebmail\nwp-admin\nwp-content\nwp-includes\n.env\n.git\n.htaccess\nserver-status\nactuator\nactuator/health\napi/v1\napi/docs\napi/swagger\nbackup.zip\nconfig.php\nconfig.json\ndatabase.sql\nindex.php\nindex.html\nphpinfo.php\nserver-info\n";

struct Plugin;

impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        match scan(&input) {
            Ok(result) => result.to_string(),
            Err(error) => json!({"error": error}).to_string(),
        }
    }
}

fn scan(input: &str) -> Result<Value, String> {
    let args: Value =
        serde_json::from_str(input).map_err(|e| format!("entrée JSON invalide: {e}"))?;
    let raw_url = args
        .get("url")
        .and_then(Value::as_str)
        .ok_or("URL de base manquante")?;
    let mut base = Url::parse(raw_url).map_err(|e| format!("URL invalide: {e}"))?;
    if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
        return Err("l’URL doit utiliser HTTP ou HTTPS et contenir un nom d’hôte".into());
    }
    if !base.username().is_empty() || base.password().is_some() {
        return Err("les identifiants intégrés dans l’URL ne sont pas acceptés".into());
    }
    base.set_query(None);
    base.set_fragment(None);
    let base_path = base.path().trim_end_matches('/').to_owned();

    let max_words = args
        .get("max-words")
        .and_then(Value::as_i64)
        .unwrap_or(500)
        .clamp(1, MAX_CANDIDATES as i64) as usize;
    let statuses = parse_statuses(
        args.get("include-status")
            .and_then(Value::as_str)
            .unwrap_or("200,204,301,302,307,401,403"),
    )?;
    let extensions =
        parse_extensions(args.get("extensions").and_then(Value::as_str).unwrap_or(""))?;
    let words = if let Some(path) = args
        .get("file")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        read_wordlist(path)?
    } else {
        BUILTIN_WORDS.lines().map(str::to_owned).collect()
    };
    let candidates = build_candidates(words, &extensions, max_words);
    if candidates.is_empty() {
        return Err("la liste ne contient aucun chemin utilisable".into());
    }

    // A baseline helps suppress soft-404 responses that otherwise look like discoveries.
    let baseline_url = candidate_url(&base, &base_path, ".ktrx-not-found-probe-7d91c4");
    let baseline = fetch(&baseline_url)?;
    let mut findings = Vec::new();
    let mut match_count = 0usize;
    let mut errors = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let relative = candidate.trim_start_matches('/');
        let probe = candidate_url(&base, &base_path, relative);
        let response = match fetch(&probe) {
            Ok(response) => response,
            Err(error) => {
                if errors.len() < 5 {
                    errors.push(error);
                }
                progress(
                    index + 1,
                    candidates.len(),
                    findings.len(),
                    index + 1 == candidates.len(),
                );
                continue;
            }
        };
        let soft_404 =
            response.status == baseline.status && response.body_bytes == baseline.body_bytes;
        if !soft_404 && statuses.contains(&response.status) {
            match_count += 1;
            let finding = json!({
                "path": format!("/{relative}"),
                "url": probe,
                "status": response.status,
                "body_bytes": response.body_bytes,
                "content_type": response.content_type,
                "redirect": response.location,
            });
            if findings.len() < MAX_RESULTS {
                findings.push(finding);
            }
        }
        progress(
            index + 1,
            candidates.len(),
            match_count,
            index + 1 == candidates.len(),
        );
        // Keep request rate deliberately low and sequential.
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(json!({
        "target": base.as_str(),
        "paths_tested": candidates.len(),
        "matches_found": match_count,
        "matches_returned": findings.len(),
        "matches_truncated": match_count > findings.len(),
        "soft_404_baseline": {"status": baseline.status, "body_bytes": baseline.body_bytes},
        "findings": findings,
        "request_errors": errors,
        "limits": {"max_candidates": MAX_CANDIDATES, "max_wordlist_bytes": MAX_WORDLIST_BYTES, "sequential": true, "minimum_delay_ms": 100},
        "note": "À utiliser uniquement sur un site que vous possédez ou êtes autorisé à tester. Les redirections ne sont pas suivies."
    }))
}

#[derive(Debug)]
struct Response {
    status: u16,
    body_bytes: u64,
    content_type: Option<String>,
    location: Option<String>,
}

fn fetch(url: &str) -> Result<Response, String> {
    let raw = ktrx::tool::host::request("http.get", &json!({"url": url}).to_string())
        .map_err(|e| format!("requête refusée ou échouée: {e}"))?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("réponse HTTP invalide: {e}"))?;
    let headers = v
        .get("headers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let header = |name: &str| -> Option<String> {
        headers
            .iter()
            .find(|h| {
                h.as_array()
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .and_then(|h| h.as_array())
            .and_then(|a| a.get(1))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Ok(Response {
        status: v
            .get("status")
            .and_then(Value::as_u64)
            .ok_or("code HTTP absent")? as u16,
        body_bytes: v.get("body_bytes").and_then(Value::as_u64).unwrap_or(0),
        content_type: header("content-type"),
        location: header("location"),
    })
}

fn candidate_url(base: &Url, base_path: &str, relative: &str) -> String {
    let mut url = base.clone();
    let path = format!(
        "{}/{}",
        base_path.trim_end_matches('/'),
        relative.trim_start_matches('/')
    );
    url.set_path(&path);
    url.to_string()
}

fn progress(current: usize, total: usize, found: usize, done: bool) {
    let _ = ktrx::tool::host::request("progress.update", &json!({"phase":"discovery", "current":current, "total":total, "recovered":found, "done":done}).to_string());
}

fn read_wordlist(path: &str) -> Result<Vec<String>, String> {
    use base64::Engine;
    let mut offset = 0u64;
    let mut bytes = Vec::new();
    loop {
        let response = ktrx::tool::host::request(
            "file.read",
            &json!({"path":path,"offset":offset,"length":65_536}).to_string(),
        )
        .map_err(|e| format!("lecture de la liste impossible: {e}"))?;
        let v: Value = serde_json::from_str(&response)
            .map_err(|e| format!("réponse fichier invalide: {e}"))?;
        let chunk = v
            .get("bytes_base64")
            .and_then(Value::as_str)
            .ok_or("bloc fichier sans données")?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(chunk)
            .map_err(|e| format!("bloc base64 invalide: {e}"))?;
        if decoded.is_empty() {
            break;
        }
        offset += decoded.len() as u64;
        if offset > MAX_WORDLIST_BYTES {
            return Err("la liste dépasse 1 Mio".into());
        }
        bytes.extend_from_slice(&decoded);
    }
    let text = String::from_utf8(bytes).map_err(|_| "la liste doit être un fichier UTF-8")?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect())
}

fn build_candidates(words: Vec<String>, extensions: &[String], limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    'words: for raw in words {
        let word = raw.trim().trim_start_matches('/');
        if word.is_empty()
            || word.len() > 200
            || word.contains("..")
            || !word
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
        {
            continue;
        }
        if seen.insert(word.to_owned()) {
            out.push(word.to_owned());
            if out.len() >= limit {
                break;
            }
        }
        for ext in extensions {
            let candidate = format!("{word}.{ext}");
            if seen.insert(candidate.clone()) {
                out.push(candidate);
                if out.len() >= limit {
                    break 'words;
                }
            }
        }
    }
    out
}

fn parse_extensions(value: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for raw in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let ext = raw.trim_start_matches('.');
        if ext.len() > 12 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(format!("extension invalide: {raw}"));
        }
        if !out.contains(&ext.to_owned()) {
            out.push(ext.to_owned());
        }
    }
    if out.len() > 10 {
        return Err("10 extensions au maximum".into());
    }
    Ok(out)
}

fn parse_statuses(value: &str) -> Result<HashSet<u16>, String> {
    let mut out = HashSet::new();
    for part in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let code: u16 = part
            .parse()
            .map_err(|_| format!("code HTTP invalide: {part}"))?;
        if !(100..=599).contains(&code) {
            return Err(format!("code HTTP hors limites: {code}"));
        }
        out.insert(code);
    }
    if out.is_empty() {
        return Err("fournir au moins un code HTTP à inclure".into());
    }
    Ok(out)
}

export!(Plugin);
