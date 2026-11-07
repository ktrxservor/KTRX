wit_bindgen::generate!({ path: "../../engine/wit", world: "ktrx-tool" });

use serde_json::{Value, json};
use std::collections::HashSet;

const MAX_WORDLIST_BYTES: u64 = 1_048_576;
const MAX_CANDIDATES: usize = 300;
const MAX_LABEL_LEN: usize = 63;
const BODY_SAMPLE_LIMIT: usize = 32_768;
const MAX_PREFIX_ERRORS: usize = 5;

/// Common subdomain prefixes seen across real deployments: infrastructure,
/// developer tooling, marketing microsites, and legacy/forgotten services —
/// the categories most often left behind with a dangling DNS record.
const BUILTIN_PREFIXES: &str = "www\nmail\nwebmail\nremote\nvpn\nftp\nsmtp\npop\nimap\nmx\nns1\nns2\nautodiscover\nautoconfig\nsecure\nportal\nsso\nauth\nlogin\nadmin\napi\napi2\napp\napps\ndev\ndev2\ndevel\nstaging\nstage\ntest\ntest2\nqa\nuat\nbeta\ndemo\nsandbox\npreview\nold\nold2\nnew\nlegacy\nbackup\narchive\nassets\nstatic\ncdn\ncdn2\nimg\nimages\nmedia\nvideo\nfiles\nupload\ndownload\ndocs\nhelp\nsupport\nstatus\nhealth\nmonitor\nmetrics\ngrafana\nkibana\nprometheus\nsentry\nblog\nnews\nshop\nstore\npay\npayment\nbilling\ncrm\nerp\nchat\nim\nvoice\nconsole\ndashboard\npanel\ncpanel\nwhm\nwebdisk\ngit\ngitlab\nsvn\nci\nbuild\njenkins\njira\nconfluence\nwiki\nconfig\ncdn-static\nm\nmobile\nweb\nweb2\nhost\nserver\nserver1\nserver2\nvps\ncloud\nbackend\nfrontend\ninternal\nintranet\nextranet\npartners\npartner\nclient\nclients\naccount\naccounts\nmy\nid\nsso2\nedge\nproxy\ngateway\nlb\nmta\nns3\nns4\nsmtp2\nmail2\nwww1\nwww2\nwww3\n";

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
    let domain = args
        .get("domain")
        .and_then(Value::as_str)
        .ok_or("domaine racine manquant")?;
    let domain = validate_domain(domain)?;

    let max_subdomains = args
        .get("max-subdomains")
        .and_then(Value::as_i64)
        .unwrap_or(100)
        .clamp(1, MAX_CANDIDATES as i64) as usize;
    let include_unresolved = args
        .get("include-unresolved")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let (prefixes, source) = if let Some(path) = args
        .get("file")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        (read_wordlist(path)?, "file")
    } else {
        (
            BUILTIN_PREFIXES.lines().map(str::to_owned).collect(),
            "builtin",
        )
    };

    let candidates = build_candidates(&domain, prefixes, max_subdomains);
    if candidates.is_empty() {
        return Err("la liste ne contient aucun préfixe de sous-domaine utilisable".into());
    }

    let host_request = |operation: &str, input: &str| ktrx::tool::host::request(operation, input);
    let mut results = Vec::with_capacity(candidates.len());
    let mut findings = Vec::new();
    let mut resolved_count = 0usize;
    let mut probed_count = 0usize;
    let mut prefix_errors = Vec::new();

    for (index, fqdn) in candidates.iter().enumerate() {
        let mut entry = json!({"subdomain": fqdn, "dns_status": "unresolved", "addresses": []});
        match ktrx_net_sdk::resolve(fqdn, 0, &host_request) {
            Ok(addresses) if !addresses.is_empty() => {
                resolved_count += 1;
                entry["dns_status"] = Value::String("resolved".into());
                entry["addresses"] = json!(addresses);
                if let Some((scheme, response)) = probe_http(fqdn, &host_request) {
                    probed_count += 1;
                    let status = response.get("status").and_then(Value::as_u64);
                    entry["http_status"] = json!(status);
                    entry["scheme_used"] = Value::String(scheme);
                    if let Some(signature) = match_takeover_signature(&response) {
                        let finding = json!({
                            "subdomain": fqdn,
                            "provider": signature.provider,
                            "confidence": signature.confidence,
                            "evidence": signature.note,
                            "http_status": status,
                        });
                        entry["takeover_signal"] = finding.clone();
                        findings.push(finding);
                    }
                }
            }
            Ok(_) => {
                if include_unresolved {
                    entry["note"] = Value::String(
                        "Aucune adresse retournée. Si ce sous-domaine existe par ailleurs (OSINT, certificat), cela peut indiquer un enregistrement CNAME orphelin.".into(),
                    );
                }
            }
            Err(error) => {
                if prefix_errors.len() < MAX_PREFIX_ERRORS {
                    prefix_errors.push(format!("{fqdn}: {error}"));
                }
                if include_unresolved {
                    entry["note"] = Value::String(
                        "Résolution DNS en échec. Si ce sous-domaine existe par ailleurs (OSINT, certificat), cela peut indiquer un enregistrement CNAME orphelin.".into(),
                    );
                }
            }
        }
        if include_unresolved || entry["dns_status"] == "resolved" {
            results.push(entry);
        }
        let _ = ktrx::tool::host::request(
            "progress.update",
            &json!({
                "phase": "enumeration",
                "current": index + 1,
                "total": candidates.len(),
                "recovered": findings.len(),
                "item_label": "sous-domaines testés",
                "result_label": "signaux de prise de contrôle",
                "done": index + 1 == candidates.len(),
            })
            .to_string(),
        );
        // Keep DNS and HTTP traffic deliberately low and sequential.
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(json!({
        "domain": domain,
        "candidates_source": source,
        "subdomains_tested": candidates.len(),
        "resolved": resolved_count,
        "http_probed": probed_count,
        "findings": findings,
        "results": results,
        "resolution_errors": prefix_errors,
        "limits": {
            "max_candidates": MAX_CANDIDATES,
            "max_wordlist_bytes": MAX_WORDLIST_BYTES,
            "body_sample_bytes": BODY_SAMPLE_LIMIT,
            "sequential": true,
            "minimum_delay_ms": 100,
        },
        "methodology": "Pour chaque sous-domaine, résolution DNS puis, si elle réussit, une requête GET (HTTPS puis HTTP en repli) dont la page est comparée à des empreintes publiques de pages d’erreur « ressource non revendiquée » de fournisseurs cloud connus (GitHub Pages, Heroku, AWS S3, Shopify, Fastly, Pantheon, Vercel, etc.).",
        "limitations": [
            "L’API KTRX ne fournit pas l’enregistrement CNAME brut, seulement l’adresse finale résolue; un échec de résolution ne prouve pas l’existence d’un CNAME orphelin, et une résolution réussie ne l’exclut pas non plus.",
            "Une correspondance d’empreinte est un indice fort à vérifier manuellement (confirmer l’enregistrement CNAME réel avec des outils DNS, puis la disponibilité de la ressource chez le fournisseur), pas une confirmation de prise de contrôle.",
            "La liste intégrée est volontairement compacte; les sous-domaines réels d’une organisation (trouvés via des journaux de certificats, de l’OSINT, etc.) peuvent être fournis via --file pour de bien meilleurs résultats.",
            "Aucune ressource n’est revendiquée ou enregistrée par cet outil : il observe uniquement l’état DNS et HTTP public.",
        ],
        "note": "À utiliser uniquement sur un domaine que vous possédez ou êtes autorisé à tester.",
    }))
}

struct Signature {
    provider: &'static str,
    needle: &'static str,
    confidence: f32,
    note: &'static str,
}

const SIGNATURES: &[Signature] = &[
    Signature {
        provider: "GitHub Pages",
        needle: "there isn't a github pages site here",
        confidence: 0.95,
        note: "Page d'erreur GitHub Pages par défaut: aucun site publié pour ce nom.",
    },
    Signature {
        provider: "Heroku",
        needle: "herokucdn.com/error-pages/no-such-app.html",
        confidence: 0.95,
        note: "Page d'erreur Heroku « no-such-app »: l'application associée n'existe plus.",
    },
    Signature {
        provider: "AWS S3",
        needle: "nosuchbucket",
        confidence: 0.93,
        note: "Erreur S3 NoSuchBucket: le bucket référencé par le CNAME n'existe plus.",
    },
    Signature {
        provider: "AWS S3",
        needle: "the specified bucket does not exist",
        confidence: 0.93,
        note: "Erreur S3: le bucket référencé par le CNAME n'existe plus.",
    },
    Signature {
        provider: "Shopify",
        needle: "sorry, this shop is currently unavailable",
        confidence: 0.9,
        note: "Page Shopify par défaut: aucune boutique active pour ce domaine.",
    },
    Signature {
        provider: "Fastly",
        needle: "fastly error: unknown domain",
        confidence: 0.92,
        note: "Erreur Fastly: le domaine n'est configuré sur aucun service Fastly actif.",
    },
    Signature {
        provider: "Pantheon",
        needle: "the gods are wise, but do not know of the site which you seek",
        confidence: 0.95,
        note: "Page d'erreur Pantheon par défaut: site introuvable chez ce fournisseur.",
    },
    Signature {
        provider: "Tumblr",
        needle: "whatever you were looking for doesn't currently exist at this address",
        confidence: 0.92,
        note: "Page d'erreur Tumblr: aucun blog associé à ce domaine personnalisé.",
    },
    Signature {
        provider: "Surge.sh",
        needle: "project not found",
        confidence: 0.75,
        note: "Message Surge.sh: le projet déployé n'existe plus.",
    },
    Signature {
        provider: "Bitbucket",
        needle: "repository not found",
        confidence: 0.7,
        note: "Message Bitbucket: le dépôt référencé n'existe plus.",
    },
    Signature {
        provider: "UserVoice",
        needle: "this uservoice subdomain is currently available",
        confidence: 0.95,
        note: "UserVoice indique explicitement que ce sous-domaine est libre à revendiquer.",
    },
    Signature {
        provider: "Help Scout",
        needle: "no settings were found for this company",
        confidence: 0.92,
        note: "Page d'erreur Help Scout: aucun compte associé à ce domaine.",
    },
    Signature {
        provider: "Intercom",
        needle: "this page is reserved for artistic dogs",
        confidence: 0.9,
        note: "Page d'erreur Intercom par défaut: domaine personnalisé non associé.",
    },
    Signature {
        provider: "Squarespace",
        needle: "no such account",
        confidence: 0.75,
        note: "Message Squarespace: aucun site associé à ce domaine personnalisé.",
    },
    Signature {
        provider: "Ghost(Pro)",
        needle: "the thing you were looking for is no longer here, or never was",
        confidence: 0.88,
        note: "Page d'erreur Ghost(Pro): aucun site associé à ce domaine.",
    },
    Signature {
        provider: "Vercel",
        needle: "the deployment could not be found on vercel",
        confidence: 0.92,
        note: "Erreur Vercel: aucun déploiement associé à ce domaine.",
    },
    Signature {
        provider: "Zendesk",
        needle: "help center closed",
        confidence: 0.85,
        note: "Message Zendesk: le centre d'aide associé est fermé ou supprimé.",
    },
    Signature {
        provider: "ReadMe",
        needle: "project doesnt exist... yet!",
        confidence: 0.85,
        note: "Page d'erreur ReadMe.io: aucun projet associé à ce domaine.",
    },
    Signature {
        provider: "Cargo",
        needle: "if you're moving your domain away from cargo",
        confidence: 0.85,
        note: "Message Cargo (cargocollective): domaine détaché du site.",
    },
    Signature {
        provider: "Strikingly",
        needle: "but if you're looking to build your own website",
        confidence: 0.75,
        note: "Page d'erreur Strikingly: aucun site associé à ce domaine.",
    },
    Signature {
        provider: "WordPress.com",
        needle: "do you want to register",
        confidence: 0.6,
        note: "Invite WordPress.com à enregistrer ce sous-domaine: aucun blog associé.",
    },
];

fn match_takeover_signature(response: &Value) -> Option<&'static Signature> {
    use base64::Engine;
    let encoded = response.get("body_base64").and_then(Value::as_str)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let body = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    SIGNATURES
        .iter()
        .find(|signature| body.contains(signature.needle))
}

fn probe_http<F>(fqdn: &str, request: &F) -> Option<(String, Value)>
where
    F: Fn(&str, &str) -> Result<String, String>,
{
    for scheme in ["https", "http"] {
        let url = format!("{scheme}://{fqdn}/");
        if let Ok(raw) = ktrx_net_sdk::http_get(&url, true, BODY_SAMPLE_LIMIT, request) {
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                return Some((scheme.to_owned(), value));
            }
        }
    }
    None
}

fn validate_domain(raw: &str) -> Result<String, String> {
    let domain = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() || domain.len() > 253 {
        return Err("le domaine doit faire entre 1 et 253 caractères".into());
    }
    if domain.contains("://") || domain.contains('/') || domain.contains(' ') {
        return Err("fournissez un nom de domaine, pas une URL (exemple: example.com)".into());
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|label| !is_valid_label(label)) {
        return Err(
            "le domaine doit être composé d’étiquettes DNS valides séparées par des points (exemple: example.com)"
                .into(),
        );
    }
    Ok(domain)
}

fn is_valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_LEN
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn build_candidates(domain: &str, prefixes: Vec<String>, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in prefixes {
        if out.len() >= limit {
            break;
        }
        let prefix = raw.trim().to_ascii_lowercase();
        if !is_valid_label(&prefix) {
            continue;
        }
        let fqdn = format!("{prefix}.{domain}");
        if fqdn.len() <= 253 && seen.insert(fqdn.clone()) {
            out.push(fqdn);
        }
    }
    out
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

export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_domain() {
        use exports::ktrx::tool::tool::Guest;
        assert_eq!(
            Plugin::run("{}".into()),
            r#"{"error":"domaine racine manquant"}"#
        );
    }

    #[test]
    fn validates_domain_format() {
        assert!(validate_domain("example.com").is_ok());
        assert_eq!(validate_domain("EXAMPLE.com").unwrap(), "example.com");
        assert!(validate_domain("example.com.").is_ok());
        assert!(validate_domain("").is_err());
        assert!(validate_domain("not a domain").is_err());
        assert!(validate_domain("https://example.com").is_err());
        assert!(validate_domain("example.com/path").is_err());
        assert!(validate_domain("onlylabel").is_err());
        assert!(validate_domain("-bad.example.com").is_err());
        assert!(validate_domain(&"a".repeat(300)).is_err());
    }

    #[test]
    fn builds_deduplicated_bounded_candidates() {
        let prefixes = vec![
            "www".to_owned(),
            "WWW".to_owned(),
            "dev".to_owned(),
            "-bad".to_owned(),
            "".to_owned(),
        ];
        let candidates = build_candidates("example.com", prefixes, 10);
        assert_eq!(
            candidates,
            vec!["www.example.com".to_owned(), "dev.example.com".to_owned()]
        );
    }

    #[test]
    fn respects_the_candidate_limit() {
        let prefixes: Vec<String> = (0..10).map(|i| format!("sub{i}")).collect();
        let candidates = build_candidates("example.com", prefixes, 3);
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn matches_a_known_takeover_fingerprint_case_insensitively() {
        use base64::Engine;
        let body = "<html>There isn't a GitHub Pages site here.</html>";
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);
        let response = json!({"status": 404, "body_base64": encoded});
        let matched = match_takeover_signature(&response).expect("should match");
        assert_eq!(matched.provider, "GitHub Pages");
    }

    #[test]
    fn does_not_match_an_ordinary_page() {
        use base64::Engine;
        let body = "<html><body>Welcome to our site</body></html>";
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);
        let response = json!({"status": 200, "body_base64": encoded});
        assert!(match_takeover_signature(&response).is_none());
    }

    #[test]
    fn builtin_prefixes_are_all_valid_dns_labels() {
        for prefix in BUILTIN_PREFIXES.lines() {
            assert!(is_valid_label(prefix), "invalid builtin prefix: {prefix}");
        }
    }
}
