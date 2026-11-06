wit_bindgen::generate!({ path: "wit", world: "ktrx-tool" });

use base64::Engine as _;
use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::sync::OnceLock;
use url::Url;

const MAX_REQUESTS: usize = 150;
const MAX_HOSTS: usize = 8;
const MAX_FINDINGS: usize = 5000;
const MAX_ENDPOINTS: usize = 500;
const MAX_TEXT_BYTES: usize = 512 * 1024;

struct Plugin;

impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        match audit(&input) {
            Ok(report) => report.to_string(),
            Err(error) => json!({"error": error}).to_string(),
        }
    }
}

#[derive(Clone, Serialize)]
struct Finding {
    id: String,
    category: String,
    severity: String,
    title: String,
    description: String,
    resource: String,
    line: usize,
    confidence: f32,
    evidence: String,
    remediation: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResourceKind {
    Page,
    Script,
    SourceMap,
}

impl ResourceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Script => "script",
            Self::SourceMap => "source_map",
        }
    }
}

struct Resource {
    url: Url,
    kind: ResourceKind,
    inline: Option<String>,
}

struct Response {
    status: u16,
    content_type: String,
    text: String,
    truncated: bool,
}

struct Context {
    origin: Url,
    allow_external: bool,
    reveal_secrets: bool,
    max_requests: usize,
    max_body_bytes: usize,
    requests: usize,
    queue: VecDeque<Resource>,
    queued: HashSet<String>,
    visited: HashSet<String>,
    external_hosts: BTreeSet<String>,
    findings: Vec<Finding>,
    finding_keys: HashSet<String>,
    endpoints: BTreeSet<String>,
    source_files: usize,
    scripts: usize,
    maps: usize,
    truncated_resources: Vec<String>,
    errors: Vec<Value>,
}

fn audit(input: &str) -> Result<Value, String> {
    let args: Value = serde_json::from_str(input).map_err(|e| format!("entrée JSON invalide : {e}"))?;
    let raw_url = args.get("url").and_then(Value::as_str).ok_or("URL de départ manquante")?;
    let mut start = Url::parse(raw_url).map_err(|e| format!("URL invalide : {e}"))?;
    if !matches!(start.scheme(), "http" | "https") || start.host_str().is_none() {
        return Err("l’URL doit utiliser HTTP ou HTTPS et contenir un hôte".into());
    }
    if !start.username().is_empty() || start.password().is_some() {
        return Err("les identifiants intégrés dans l’URL ne sont pas acceptés".into());
    }
    start.set_fragment(None);
    let origin = Url::parse(&start.origin().ascii_serialization()).map_err(|e| e.to_string())?;
    let max_requests = bounded(&args, "max-requests", 80, 1, MAX_REQUESTS);
    let script_kb = bounded(&args, "script-kb", 512, 16, 512);
    let max_body_bytes = script_kb.saturating_mul(1024).min(MAX_TEXT_BYTES);
    let allow_external = args.get("external-scripts").and_then(Value::as_bool).unwrap_or(false);
    let reveal_secrets = args.get("reveal-secrets").and_then(Value::as_bool).unwrap_or(false);

    let mut context = Context {
        origin: origin.clone(), allow_external, reveal_secrets, max_requests, max_body_bytes,
        requests: 0, queue: VecDeque::new(), queued: HashSet::new(), visited: HashSet::new(),
        external_hosts: BTreeSet::new(), findings: Vec::new(), finding_keys: HashSet::new(),
        endpoints: BTreeSet::new(), source_files: 0, scripts: 0, maps: 0,
        truncated_resources: Vec::new(), errors: Vec::new(),
    };
    enqueue(&mut context, Resource { url: start.clone(), kind: ResourceKind::Page, inline: None });

    while let Some(resource) = context.queue.pop_front() {
        if context.findings.len() >= MAX_FINDINGS { break; }
        if resource.inline.is_none() && context.requests >= context.max_requests {
            context.queue.push_front(resource);
            break;
        }
        let key = resource.url.as_str().to_owned();
        if !context.visited.insert(key.clone()) { continue; }
        let (text, truncated) = if let Some(inline) = resource.inline {
            (inline, false)
        } else {
            context.requests += 1;
            match fetch(&resource.url, context.max_body_bytes) {
                Ok(response) => {
                    if !(200..300).contains(&response.status) {
                        if context.errors.len() < 25 { context.errors.push(json!({"resource":key,"status":response.status,"kind":resource.kind.label()})); }
                        progress(&context, false);
                        continue;
                    }
                    if !response.content_type.is_empty()
                        && !looks_like_kind(&resource.url, resource.kind, &response.content_type)
                    {
                        if context.errors.len() < 25 { context.errors.push(json!({"resource":key,"kind":resource.kind.label(),"error":"type de contenu non analysable"})); }
                        progress(&context, false);
                        continue;
                    }
                    (response.text, response.truncated)
                }
                Err(error) => {
                    if context.errors.len() < 25 { context.errors.push(json!({"resource":key,"kind":resource.kind.label(),"error":error})); }
                    progress(&context, false);
                    continue;
                }
            }
        };
        if truncated && context.truncated_resources.len() < 100 { context.truncated_resources.push(key.clone()); }
        context.source_files += 1;
        match resource.kind {
            ResourceKind::Page => {
                discover_inline_and_external_scripts(&text, &resource.url, &mut context);
            }
            ResourceKind::Script => {
                context.scripts += 1;
                analyze_source(&text, &key, &mut context);
                discover_modules_and_map(&text, &resource.url, &mut context);
            }
            ResourceKind::SourceMap => {
                context.maps += 1;
                analyze_source_map(&text, &key, &mut context);
            }
        }
        progress(&context, false);
    }
    let complete = context.queue.is_empty() && context.errors.is_empty();
    progress(&context, true);
    let severity_counts = json!({
        "critical":context.findings.iter().filter(|f| f.severity == "critical").count(),
        "high":context.findings.iter().filter(|f| f.severity == "high").count(),
        "medium":context.findings.iter().filter(|f| f.severity == "medium").count(),
        "low":context.findings.iter().filter(|f| f.severity == "low").count(),
        "info":context.findings.iter().filter(|f| f.severity == "info").count()
    });
    Ok(json!({
        "target":start.as_str(),
        "origin":context.origin.origin().ascii_serialization(),
        "status":if complete {"complete"} else {"partial"},
        "complete":complete,
        "limits":{"max_requests":context.max_requests,"max_body_bytes_per_resource":context.max_body_bytes,"external_scripts_enabled":context.allow_external,"secret_values_redacted":!context.reveal_secrets},
        "summary":{"requests":context.requests,"resources":context.source_files,"scripts":context.scripts,"source_maps":context.maps,"findings":context.findings.len(),"endpoints":context.endpoints.len(),"severity":severity_counts},
        "findings":context.findings,
        "endpoints":context.endpoints,
        "external_hosts_referenced":context.external_hosts,
        "truncated_resources":context.truncated_resources,
        "request_errors":context.errors,
        "methodology":{"systems":["règles de motifs pour identifiants et secrets connus","analyse d’entropie pour chaînes à forte aléa en contexte sensible","détection de sinks et APIs JavaScript à risque","analyse des indicateurs de logique métier et des routes exposées","analyse de source maps et sources originales accessibles"],"no_javascript_execution":true,"no_active_payloads":true,"no_forms_or_api_mutations":true,"same_origin_by_default":true},
        "limitations":["L’audit est statique : il ne lance pas le code, ne résout pas les branches d’exécution et ne teste pas la sécurité réelle des endpoints.","Un motif détecté est un indice à vérifier; il peut s’agir d’un faux positif ou d’un secret révoqué.","Les scripts tiers sont recensés mais non téléchargés sans --external-scripts.","Les bundles minifiés, obfusqués, générés après interaction ou chargés depuis des hôtes hors limite peuvent échapper à l’inspection.","Les valeurs de secrets sont masquées par défaut; --reveal-secrets les expose dans le rapport local."]
    }))
}

fn bounded(args: &Value, key: &str, default: usize, min: usize, max: usize) -> usize {
    args.get(key).and_then(Value::as_i64).unwrap_or(default as i64).clamp(min as i64, max as i64) as usize
}

fn enqueue(context: &mut Context, resource: Resource) {
    let key = resource.url.as_str().to_owned();
    if context.queued.contains(&key) || context.visited.contains(&key) { return; }
    if resource.url.origin() != context.origin.origin() {
        if let Some(host) = resource.url.host_str() { context.external_hosts.insert(host.to_owned()); }
        if !context.allow_external || context.external_hosts.len() > MAX_HOSTS { return; }
    }
    context.queued.insert(key);
    context.queue.push_back(resource);
}

fn resolve_url(raw: &str, base: &Url) -> Option<Url> {
    let cleaned = raw.trim().trim_matches(['\'', '"']).replace("&amp;", "&");
    if cleaned.is_empty() || cleaned.starts_with("#") || cleaned.starts_with("data:")
        || cleaned.starts_with("javascript:") || cleaned.starts_with("blob:") { return None; }
    let mut url = base.join(&cleaned).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() { return None; }
    url.set_fragment(None);
    Some(url)
}

fn discover_inline_and_external_scripts(html: &str, page: &Url, context: &mut Context) {
    static SCRIPT_TAG: OnceLock<Regex> = OnceLock::new();
    static SRC_ATTR: OnceLock<Regex> = OnceLock::new();
    static LINK_TAG: OnceLock<Regex> = OnceLock::new();
    static HREF_ATTR: OnceLock<Regex> = OnceLock::new();
    static EVENT_ATTR: OnceLock<Regex> = OnceLock::new();
    static JS_URL: OnceLock<Regex> = OnceLock::new();
    let tag_re = SCRIPT_TAG.get_or_init(|| Regex::new(r"(?is)<script\b([^>]*)>(.*?)</script\s*>").unwrap());
    let attr_re = SRC_ATTR.get_or_init(|| Regex::new(r#"(?is)\bsrc\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap());
    for (index, captures) in tag_re.captures_iter(html).enumerate() {
        let attrs = captures.get(1).map(|m| m.as_str()).unwrap_or("");
        let content = captures.get(2).map(|m| m.as_str()).unwrap_or("").to_owned();
        if let Some(attr) = attr_re.captures(attrs) {
            let src = attr.get(1).or_else(|| attr.get(2)).or_else(|| attr.get(3)).map(|m| m.as_str()).unwrap_or("");
            if let Some(url) = resolve_url(src, page) { enqueue(context, Resource { url, kind: ResourceKind::Script, inline: None }); }
        } else if !content.trim().is_empty() {
            let pseudo = page.join(&format!("#inline-script-{}", index + 1)).unwrap_or_else(|_| page.clone());
            enqueue(context, Resource { url: pseudo, kind: ResourceKind::Script, inline: Some(content) });
        }
    }
    let link_re = LINK_TAG.get_or_init(|| Regex::new(r"(?is)<link\b([^>]*)>").unwrap());
    let href_re = HREF_ATTR.get_or_init(|| Regex::new(r#"(?is)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap());
    for captures in link_re.captures_iter(html) {
        let attrs = captures.get(1).map(|m| m.as_str()).unwrap_or("");
        let lower = attrs.to_ascii_lowercase();
        if !(lower.contains("modulepreload") || (lower.contains("preload") && lower.contains("as=\"script\"")) || (lower.contains("preload") && lower.contains("as='script'"))) { continue; }
        if let Some(c) = href_re.captures(attrs) {
            let raw = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)).map(|m| m.as_str()).unwrap_or("");
            if let Some(url) = resolve_url(raw, page) { enqueue(context, Resource { url, kind: ResourceKind::Script, inline: None }); }
        }
    }
    let event_re = EVENT_ATTR.get_or_init(|| Regex::new(r#"(?is)\s(on[a-z0-9_-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap());
    for (index, c) in event_re.captures_iter(html).take(150).enumerate() {
        let code = c.get(2).or_else(|| c.get(3)).or_else(|| c.get(4)).map(|m| m.as_str()).unwrap_or("");
        if !code.is_empty() { analyze_source(code, &format!("{}#inline-{}-{}", page, c.get(1).map(|m|m.as_str()).unwrap_or("handler"), index + 1), context); }
    }
    let js_url_re = JS_URL.get_or_init(|| Regex::new(r#"(?is)\bhref\s*=\s*(?:"javascript:([^"]*)"|'javascript:([^']*)'|javascript:([^\s>]+))"#).unwrap());
    for (index, c) in js_url_re.captures_iter(html).take(100).enumerate() {
        let code = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)).map(|m| m.as_str()).unwrap_or("");
        if !code.is_empty() { analyze_source(code, &format!("{}#javascript-url-{}", page, index + 1), context); }
    }
}

fn discover_modules_and_map(source: &str, script_url: &Url, context: &mut Context) {
    static MAP_RE: OnceLock<Regex> = OnceLock::new();
    static MODULE_RE: OnceLock<Regex> = OnceLock::new();
    let map_re = MAP_RE.get_or_init(|| Regex::new(r"(?m)//[#@]\s*sourceMappingURL\s*=\s*([^\s*]+)|/\*[#@]\s*sourceMappingURL\s*=\s*([^\s*]+)\s*\*/").unwrap());
    if let Some(c) = map_re.captures(source) {
        let value = c.get(1).or_else(|| c.get(2)).map(|m| m.as_str()).unwrap_or("");
        if value.starts_with("data:application/json;base64,") {
            if let Some(encoded) = value.split_once(',').map(|(_, b)| b) {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    analyze_source_map(&text, &format!("{}#inline-source-map", script_url), context);
                }
            }
        } else if let Some(url) = resolve_url(value, script_url) {
            enqueue(context, Resource { url, kind: ResourceKind::SourceMap, inline: None });
        }
    }
    let module_re = MODULE_RE.get_or_init(|| Regex::new(r#"(?m)(?:\bfrom\s*|\bimport\s*\(|\brequire\s*\()\s*['\"]([^'\"]+)['\"]"#).unwrap());
    for captures in module_re.captures_iter(source).take(100) {
        let spec = captures.get(1).map(|m| m.as_str()).unwrap_or("");
        if spec.starts_with(".") || spec.starts_with("/") {
            if let Some(mut url) = resolve_url(spec, script_url) {
                if url.path().rsplit('/').next().is_some_and(|part| !part.contains('.')) {
                    let path = format!("{}.js", url.path());
                    url.set_path(&path);
                }
                enqueue(context, Resource { url, kind: ResourceKind::Script, inline: None });
            }
        }
    }
}

fn analyze_source_map(raw: &str, resource: &str, context: &mut Context) {
    let Ok(map) = serde_json::from_str::<Value>(raw) else { return; };
    let sources = map.get("sources").and_then(Value::as_array);
    let contents = map.get("sourcesContent").and_then(Value::as_array);
    if let (Some(sources), Some(contents)) = (sources, contents) {
        for (index, body) in contents.iter().enumerate().take(200) {
            let Some(body) = body.as_str() else { continue; };
            if body.trim().is_empty() { continue; }
            let name = sources.get(index).and_then(Value::as_str).unwrap_or("source map source");
            let label = format!("{} :: {}", resource, name);
            analyze_source(body, &label, context);
            if context.findings.len() >= MAX_FINDINGS { break; }
        }
    }
    let count = sources.map(Vec::len).unwrap_or(0);
    add_finding(context, "source_map", "info", "Source map avec sources originales", "Une source map est accessible et expose les noms ou le contenu des fichiers sources.", resource, 1, 0.99, &format!("{} source(s) dans la carte", count), "Si cette exposition n’est pas voulue en production, désactiver la publication de sourcesContent ou contrôler l’accès aux fichiers.");
}

fn analyze_source(source: &str, resource: &str, context: &mut Context) {
    let source = if source.len() > MAX_TEXT_BYTES { &source[..floor_char_boundary(source, MAX_TEXT_BYTES)] } else { source };
    let patterns = secret_patterns();
    for (category, severity, title, regex, confidence, description, remediation) in patterns {
        for captures in regex.captures_iter(source).take(100) {
            let Some(found) = captures.get(0) else { continue; };
            let value = if captures.len() > 1 { captures.get(1).unwrap_or(found).as_str() } else { found.as_str() };
            if is_placeholder(value) { continue; }
            if category == "entropy_secret" && !secret_context(source, found.start()) { continue; }
            let evidence = if context.reveal_secrets { value.to_owned() } else { redact(value) };
            let line = source[..found.start()].bytes().filter(|b| *b == b'\n').count() + 1;
            add_finding(context, category, severity, title, description, resource, line, confidence, &evidence, remediation);
        }
    }
    for (category, severity, title, regex, confidence, description, remediation) in risky_patterns() {
        for m in regex.find_iter(source).take(40) {
            let line = source[..m.start()].bytes().filter(|b| *b == b'\n').count() + 1;
            let excerpt = safe_excerpt(source, m.start(), m.end(), context.reveal_secrets);
            add_finding(context, category, severity, title, description, resource, line, *confidence, &excerpt, remediation);
        }
    }
    for (category, severity, title, regex, confidence, description) in logic_patterns() {
        if let Some(m) = regex.find(source) {
            let line = source[..m.start()].bytes().filter(|b| *b == b'\n').count() + 1;
            let excerpt = safe_excerpt(source, m.start(), m.end(), context.reveal_secrets);
            add_finding(context, category, severity, title, description, resource, line, *confidence, &excerpt, "Vérifier que la règle métier ou le contrôle d’accès critique est aussi imposé côté serveur.");
        }
    }
    extract_endpoints(source, context);
}

fn secret_patterns() -> Vec<(&'static str, &'static str, &'static str, Regex, f32, &'static str, &'static str)> {
    static PATTERNS: OnceLock<Vec<(&'static str, &'static str, &'static str, Regex, f32, &'static str, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let rows: &[(&str, &str, &str, &str, f32, &str, &str)] = &[
            ("secret", "critical", "Clé privée ou matériau de clé privée", r"(?i)-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----", 0.99, "Un bloc de clé privée est présent dans le JavaScript public.", "Révoquer/remplacer la clé et retirer le secret de l’artefact; le masquer dans l’historique de build si nécessaire."),
            ("secret", "high", "Identifiant AWS potentiellement exposé", r"\b(AKIA[0-9A-Z]{16})\b", 0.98, "Une clé d’accès AWS de format reconnu apparaît dans le code livré.", "Révoquer immédiatement l’identifiant, vérifier les journaux CloudTrail et utiliser une identité serveur à privilèges minimaux."),
            ("secret", "high", "Jeton GitHub potentiellement exposé", r"\b((?:gh[pousr]_[A-Za-z0-9_]{30,}|github_pat_[A-Za-z0-9_]{50,}))\b", 0.97, "Un jeton GitHub de format connu apparaît dans le bundle.", "Révoquer le jeton, vérifier ses portées et remplacer par un secret conservé côté serveur."),
            ("secret", "high", "Clé Stripe live potentiellement exposée", r"\b((?:sk|rk)_live_[A-Za-z0-9]{16,})\b", 0.98, "Une clé Stripe live apparaît dans le code client.", "Révoquer et renouveler la clé; ne jamais utiliser de clé secrète Stripe dans le navigateur."),
            ("secret", "high", "Jeton Slack potentiellement exposé", r"\b((?:xox[baprs]-)[A-Za-z0-9-]{20,})\b", 0.97, "Un jeton Slack de format reconnu apparaît dans le bundle.", "Révoquer le jeton dans Slack et examiner son utilisation."),
            ("secret", "high", "Clé API Google potentiellement exposée", r"\b(AIza[0-9A-Za-z_-]{35})\b", 0.91, "Une clé Google API de format courant apparaît. Certaines clés sont restreintes et conçues pour un usage client.", "Vérifier restrictions de référent, API autorisées et quotas; faire tourner la clé si elle a une portée sensible."),
            ("secret", "high", "Secret OpenAI potentiellement exposé", r"\b(sk-(?:proj-)?[A-Za-z0-9_-]{20,})\b", 0.93, "Une chaîne au format de clé OpenAI est incluse dans le code public.", "Révoquer la clé et déplacer l’appel authentifié côté serveur."),
            ("secret", "high", "Secret SendGrid potentiellement exposé", r"\b(SG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,})\b", 0.95, "Une clé SendGrid de format courant apparaît dans le bundle.", "Révoquer la clé et contrôler l’activité d’envoi."),
            ("secret", "medium", "Jeton JWT incorporé au code", r"\b(eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,})\b", 0.88, "Une valeur ressemblant à un JWT est incorporée au JavaScript.", "Vérifier audience, expiration et portée; ne pas livrer de jeton réutilisable dans un bundle."),
            ("entropy_secret", "medium", "Chaîne à forte entropie dans un contexte sensible", r#"(?i)(?:api[_-]?key|client[_-]?secret|access[_-]?token|password|passwd|authorization|secret|credential)[\w\s.'"`:=]{0,48}([A-Za-z0-9_+/=-]{24,})"#, 0.72, "Un nom de champ sensible est proche d’une valeur à forte entropie.", "Confirmer si la valeur est active; révoquer les identifiants réels et les sortir du code navigateur."),
        ];
        rows.iter().filter_map(|(category,severity,title,pattern,confidence,description,remediation)| Regex::new(pattern).ok().map(|re| (*category,*severity,*title,re,*confidence,*description,*remediation))).collect()
    }).clone()
}

type RiskyPattern = (&'static str, &'static str, &'static str, Regex, f32, &'static str, &'static str);

fn risky_patterns() -> &'static [RiskyPattern] {
    static PATTERNS: OnceLock<Vec<RiskyPattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let rows: &[(&str, &str, &str, &str, f32, &str, &str)] = &[
            ("dangerous_api","high","Exécution dynamique via eval()",r"\beval\s*\(",0.98,"eval() interprète une chaîne comme du code exécutable et peut transformer une injection en exécution de code.","Remplacer par un parseur ou une table de fonctions explicite; tracer toute donnée non fiable jusqu’à cette opération."),
            ("dangerous_api","high","Constructeur Function utilisé",r"\bnew\s+Function\s*\(",0.97,"Le constructeur Function compile du texte en code, comme eval().","Éviter la génération de code depuis des chaînes et valider les données avec une grammaire stricte."),
            ("dangerous_api","medium","Timer avec argument de code possible",r#"\b(?:setTimeout|setInterval)\s*\(\s*['"`]"#,0.95,"Un timer reçoit une chaîne interprétée comme du code dans certains environnements.","Passer une fonction callback plutôt qu’une chaîne."),
            ("dangerous_api","high","Écriture HTML directe dans le DOM",r"\b(?:document\.write|document\.writeln)\s*\(",0.96,"L’écriture HTML directe peut créer un point d’injection XSS.","Éviter document.write; construire le DOM avec des APIs sûres et échapper les données."),
            ("dangerous_api","high","Affectation à innerHTML/outerHTML",r"\.(?:innerHTML|outerHTML)\s*(?:\+?=)",0.86,"Un sink HTML est utilisé; le risque dépend de la provenance et de l’assainissement de la valeur.","Préférer textContent ou un assainisseur HTML éprouvé; examiner les données qui atteignent ce sink."),
            ("dangerous_api","high","Insertion HTML via insertAdjacentHTML()",r"\.insertAdjacentHTML\s*\(",0.91,"Cette API interprète le contenu fourni comme HTML.","Assainir avec une bibliothèque maintenue ou créer les nœuds DOM sans concaténer du HTML non fiable."),
            ("dangerous_api","medium","HTML injecté via dangerouslySetInnerHTML",r"\bdangerouslySetInnerHTML\b",0.93,"React désactive ici son échappement automatique du contenu.","N’utiliser que du HTML explicitement assaini avec une politique de confiance."),
            ("dangerous_api","medium","Message inter-origine envoyé avec wildcard",r#"\.postMessage\s*\([^;]{0,300},\s*['"]\*['"]\s*\)"#,0.82,"Le message peut être remis à toute origine; examiner aussi la validation côté récepteur.","Indiquer l’origine destinataire exacte et vérifier event.origin à la réception."),
            ("dangerous_api","medium","Contournement de validation TLS côté client possible",r#"NODE_TLS_REJECT_UNAUTHORIZED\s*[:=]\s*['"]?0\b"#,0.98,"Le code semble désactiver la validation de certificat dans un contexte Node intégré.","Supprimer ce réglage; ne jamais désactiver la vérification TLS en production."),
            ("dangerous_api","low","Usage de stockage navigateur à examiner",r"\b(?:localStorage|sessionStorage)\s*\.\s*(?:setItem|getItem)\s*\(",0.68,"Le stockage navigateur est accessible au JavaScript de la page en cas de XSS.","Ne pas stocker de secrets durables ou de données sensibles sans analyser leur portée et leur durée."),
        ];
        rows.iter().filter_map(|(category,severity,title,pattern,confidence,description,remediation)| Regex::new(pattern).ok().map(|regex| (*category,*severity,*title,regex,*confidence,*description,*remediation))).collect()
    })
}

type LogicPattern = (&'static str, &'static str, &'static str, Regex, f32, &'static str);

fn logic_patterns() -> &'static [LogicPattern] {
    static PATTERNS: OnceLock<Vec<LogicPattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let rows: &[(&str, &str, &str, &str, f32, &str)] = &[
            ("business_logic","info","Contrôle de rôle ou privilège côté client",r"(?i)\b(?:isAdmin|is_admin|hasRole|role|permission|isPremium|isStaff)\b\s*(?:===?|!==?|\(|\?|&&|\|\|)",0.78,"Une condition relative à un rôle, privilège ou niveau d’abonnement est visible dans le JavaScript public."),
            ("business_logic","info","Règle de prix, remise ou crédit côté client",r"(?i)\b(?:discount|coupon|promo(?:tion)?|credit|price|pricing|taxRate|freeTrial)\b\s*(?:[:=<>]|\(|\?|\+|-)",0.72,"Une partie de la logique commerciale semble incluse dans le bundle; cela ne prouve pas qu’elle soit utilisée comme autorité serveur."),
            ("business_logic","info","Drapeau de fonctionnalité ou mode debug exposé",r"(?i)\b(?:featureFlag|feature_flag|debugMode|isDebug|betaEnabled|enable[A-Z]\w*)\b\s*(?:[:=]|\(|\?|&&|\|\|)",0.72,"Un drapeau de déploiement ou mode debug est présent dans le code client."),
            ("business_logic","info","Route interne ou d’administration référencée",r#"(?i)['"`](/(?:admin|internal|debug|private|manage|backoffice)(?:/|['"`?#]))"#,0.75,"Une route souvent réservée à l’administration ou à l’usage interne est référencée dans le JavaScript."),
        ];
        rows.iter().filter_map(|(category,severity,title,pattern,confidence,description)| Regex::new(pattern).ok().map(|regex| (*category,*severity,*title,regex,*confidence,*description))).collect()
    })
}

fn extract_endpoints(source: &str, context: &mut Context) {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let re = URL_RE.get_or_init(|| Regex::new(r#"(?i)['\"`](https?://[^'\"`\s<>]{1,300}|/(?:api|graphql|admin|internal|auth|oauth|debug|v[0-9]+)(?:/[^'\"`\s<>]*)?)['\"`]"#).unwrap());
    for c in re.captures_iter(source).take(1000) {
        if let Some(value) = c.get(1) {
            let mut endpoint = value.as_str().trim_end_matches([';', ',', ')']).to_owned();
            if endpoint.starts_with("http") {
                if let Ok(mut url) = Url::parse(&endpoint) {
                    url.set_username("").ok();
                    url.set_password(None).ok();
                    url.set_query(None);
                    url.set_fragment(None);
                    endpoint = url.to_string();
                }
            } else if let Some((path, _)) = endpoint.split_once('?') {
                endpoint = path.to_owned();
            }
            if context.endpoints.len() < MAX_ENDPOINTS { context.endpoints.insert(endpoint); }
        }
    }
}

fn add_finding(context: &mut Context, category: &str, severity: &str, title: &str, description: &str, resource: &str, line: usize, confidence: f32, evidence: &str, remediation: &str) {
    if context.findings.len() >= MAX_FINDINGS { return; }
    let key = format!("{category}\0{title}\0{resource}\0{line}\0{evidence}");
    if !context.finding_keys.insert(key.clone()) { return; }
    let digest = key.bytes().fold(0xcbf29ce484222325u64, |hash, b| (hash ^ b as u64).wrapping_mul(0x100000001b3));
    context.findings.push(Finding {
        id: format!("JS-{digest:016x}"), category: category.into(), severity: severity.into(), title: title.into(),
        description: description.into(), resource: resource.into(), line, confidence, evidence: evidence.chars().take(300).collect(), remediation: remediation.into(),
    });
}

fn safe_excerpt(source: &str, start: usize, end: usize, reveal: bool) -> String {
    let lo = source[..start].char_indices().rev().nth(50).map(|(i,_)| i).unwrap_or(0);
    let hi = source[end..].char_indices().nth(100).map(|(i,_)| end+i).unwrap_or(source.len());
    let excerpt = source[lo..hi].replace('\n', " ").replace('\r', " ");
    if reveal { return excerpt.chars().take(300).collect(); }
    static SENSITIVE: OnceLock<Regex> = OnceLock::new();
    let re = SENSITIVE.get_or_init(|| Regex::new(r#"(?i)(api[_-]?key|client[_-]?secret|access[_-]?token|password|passwd|authorization|secret|credential)\s*[:=]\s*['"]?[^'"\s,;]{8,}"#).unwrap());
    re.replace_all(&excerpt, "$1=[masqué]").chars().take(300).collect()
}

fn redact(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 { return "[masqué]".into(); }
    format!("{}{}{}", chars[..4].iter().collect::<String>(), "•".repeat(chars.len().saturating_sub(8).min(32)), chars[chars.len()-4..].iter().collect::<String>())
}

fn is_placeholder(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    ["your_", "your-", "example", "changeme", "change_me", "placeholder", "dummy", "insert_", "<token", "xxxxxxxx", "abcdef123456"].iter().any(|s| v.contains(s))
}

fn secret_context(source: &str, offset: usize) -> bool {
    let start = floor_char_boundary(source, offset.saturating_sub(100));
    let mut end = (offset + 180).min(source.len());
    while end < source.len() && !source.is_char_boundary(end) { end += 1; }
    source[start..end].to_ascii_lowercase().contains("secret")
        || source[start..end].to_ascii_lowercase().contains("token")
        || source[start..end].to_ascii_lowercase().contains("api_key")
        || source[start..end].to_ascii_lowercase().contains("password")
}

fn floor_char_boundary(text: &str, mut n: usize) -> usize {
    while n > 0 && !text.is_char_boundary(n) { n -= 1; }
    n
}

fn looks_like_kind(url: &Url, kind: ResourceKind, content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    match kind {
        ResourceKind::Page => ct.contains("html") || url.path().ends_with('/') || url.path().ends_with(".html"),
        ResourceKind::Script => ct.contains("javascript") || ct.contains("ecmascript") || url.path().ends_with(".js") || url.path().ends_with(".mjs") || url.path().contains(".js?") || url.path().contains(".mjs?"),
        ResourceKind::SourceMap => ct.contains("json") || url.path().ends_with(".map") || url.path().contains(".map?"),
    }
}

fn fetch(url: &Url, max_body: usize) -> Result<Response, String> {
    let raw = ktrx::tool::host::request("http.get", &json!({"url":url.as_str(),"include_body":true,"max_body_bytes":max_body}).to_string()).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&raw).map_err(|e| format!("réponse HTTP illisible : {e}"))?;
    let status = value.get("status").and_then(Value::as_u64).unwrap_or(0) as u16;
    let content_type = value.get("headers").and_then(Value::as_array).into_iter().flatten().find_map(|h| {
        let name = h.get("name")?.as_str()?;
        name.eq_ignore_ascii_case("content-type").then(|| h.get("value").and_then(Value::as_str).unwrap_or("").to_owned())
    }).unwrap_or_default();
    let encoded = value.get("body_base64").and_then(Value::as_str).unwrap_or("");
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| format!("corps base64 invalide : {e}"))?;
    let truncated = value.get("body_truncated").and_then(Value::as_bool).unwrap_or(false);
    Ok(Response { status, content_type, text: String::from_utf8_lossy(&bytes).into_owned(), truncated })
}

fn progress(context: &Context, done: bool) {
    let _ = ktrx::tool::host::request("progress.update", &json!({
        "phase":"javascript-audit","current":context.requests,"recovered":context.findings.len(),
        "item_label":"ressources inspectées","result_label":"indices trouvés","done":done
    }).to_string());
}

export!(Plugin);
