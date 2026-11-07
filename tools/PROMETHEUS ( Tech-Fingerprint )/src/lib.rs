wit_bindgen::generate!({ path: "wit", world: "ktrx-tool" });

use base64::Engine as _;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use url::Url;

const MAX_REQUESTS: usize = 150;
const PROBE_PATHS: &[&str] = &[
    "/robots.txt",
    "/sitemap.xml",
    "/manifest.json",
    "/security.txt",
    "/.well-known/security.txt",
    "/wp-json/",
    "/wp-json/wp/v2/",
    "/.well-known/assetlinks.json",
    "/openapi.json",
    "/swagger.json",
    "/favicon.ico",
];

struct Plugin;

impl exports::ktrx::tool::tool::Guest for Plugin {
    fn run(input: String) -> String {
        match scan(&input) {
            Ok(report) => report.to_string(),
            Err(error) => json!({"error": error}).to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResourceKind {
    Page,
    Script,
    Style,
    Probe,
}

impl ResourceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Script => "script",
            Self::Style => "style",
            Self::Probe => "probe",
        }
    }
}

#[derive(Clone)]
struct WorkItem {
    url: Url,
    depth: usize,
    kind: ResourceKind,
}

#[derive(Default, Clone, Serialize)]
struct Technology {
    name: String,
    category: String,
    version: Option<String>,
    confidence: f32,
    evidence: Vec<String>,
}

fn scan(input: &str) -> Result<Value, String> {
    let args: Value =
        serde_json::from_str(input).map_err(|e| format!("entrée JSON invalide: {e}"))?;
    let raw_url = args
        .get("url")
        .and_then(Value::as_str)
        .ok_or("URL de départ manquante")?;
    let mut start = Url::parse(raw_url).map_err(|e| format!("URL invalide: {e}"))?;
    if !matches!(start.scheme(), "http" | "https") || start.host_str().is_none() {
        return Err("l’URL doit utiliser HTTP ou HTTPS et inclure un hôte".into());
    }
    if !start.username().is_empty() || start.password().is_some() {
        return Err("les identifiants intégrés dans l’URL ne sont pas acceptés".into());
    }
    start.set_fragment(None);

    let max_requests = args
        .get("max-requests")
        .and_then(Value::as_i64)
        .unwrap_or(50)
        .clamp(1, MAX_REQUESTS as i64) as usize;
    let max_depth = args
        .get("depth")
        .and_then(Value::as_i64)
        .unwrap_or(2)
        .clamp(0, 4) as usize;
    let sample_limit = args
        .get("body-kb")
        .and_then(Value::as_i64)
        .unwrap_or(256)
        .clamp(16, 256) as usize
        * 1024;

    let host = start.host_str().ok_or("hôte absent de l’URL")?.to_owned();
    let port = start.port_or_known_default().unwrap_or(443);
    let dns = host_request("dns.lookup", &json!({"host":host,"port":port}))
        .and_then(|raw| serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string()))
        .unwrap_or_else(|error| json!({"error":error}));

    if let Some(error) = dns.get("error").and_then(Value::as_str) {
        let message = friendly_dns_error(error);
        return Ok(json!({
            "target":start.as_str(),
            "origin":start.origin().ascii_serialization(),
            "status":"failed",
            "complete":false,
            "dns":{"host":host,"error":message},
            "tls":{"verified":false,"skipped":true,"reason":"la résolution DNS a échoué"},
            "summary":{"requests":0,"pages":0,"assets":0,"technologies":0,"versions_identified":0,"external_hosts_referenced":0},
            "technologies":[],
            "external_hosts_referenced":[],
            "resource_fingerprints":{},
            "resources":[],
            "request_errors":[{"stage":"dns","host":host,"error":message}],
            "limitations":["Aucune requête HTTP n’a été envoyée, car l’hôte n’a pas pu être résolu."]
        }));
    }

    let tls = if start.scheme() == "https" {
        host_request("tls.inspect", &json!({"url":start.as_str()}))
            .and_then(|raw| serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string()))
            .unwrap_or_else(|error| json!({"verified":false,"error":error}))
    } else {
        json!({"verified":false,"note":"la cible utilise HTTP sans TLS"})
    };

    let mut origin_root = start.clone();
    origin_root.set_path("/");
    origin_root.set_query(None);
    origin_root.set_fragment(None);

    let mut queue = VecDeque::new();
    queue.push_back(WorkItem {
        url: start.clone(),
        depth: 0,
        kind: ResourceKind::Page,
    });
    let mut visited = HashSet::new();
    let mut requests_attempted = 0usize;
    let mut technologies: BTreeMap<String, Technology> = BTreeMap::new();
    let mut external_hosts = BTreeSet::new();
    let mut resource_fingerprints = BTreeMap::new();
    let mut observations = Vec::new();
    let mut errors = Vec::new();
    let mut page_count = 0usize;
    let mut asset_count = 0usize;
    let mut probes_added = false;

    while let Some(item) = queue.pop_front() {
        if requests_attempted >= max_requests {
            break;
        }
        let item_url = clean_url(item.url);
        let key = item_url.as_str().to_owned();
        if !visited.insert(key) {
            continue;
        }
        requests_attempted += 1;

        let response = host_request(
            "http.get",
            &json!({
                "url": item_url.as_str(),
                "include_body": true,
                "max_body_bytes": sample_limit
            }),
        );
        let Ok(raw_response) = response else {
            if errors.len() < 20 {
                errors.push(
                    json!({"url":item_url.as_str(),"kind":item.kind.label(),"error":response.unwrap_err()}),
                );
            }
            progress(requests_attempted, max_requests, technologies.len(), false);
            continue;
        };
        let parsed: Value = match serde_json::from_str(&raw_response) {
            Ok(value) => value,
            Err(error) => {
                if errors.len() < 20 {
                    errors.push(
                        json!({"url":item_url.as_str(),"error":format!("invalid host response: {error}")}),
                    );
                }
                progress(requests_attempted, max_requests, technologies.len(), false);
                continue;
            }
        };
        let status = parsed.get("status").and_then(Value::as_u64).unwrap_or(0) as u16;
        let body_bytes = parsed
            .get("body_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let body_sampled_bytes = parsed
            .get("body_sampled_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let body_truncated = parsed
            .get("body_truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let headers = parse_headers(parsed.get("headers"));
        let body = parsed
            .get("body_base64")
            .and_then(Value::as_str)
            .and_then(|encoded| {
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .ok()
            })
            .unwrap_or_default();
        let text = String::from_utf8_lossy(&body).into_owned();
        let content_type = header_first(&headers, "content-type").unwrap_or_default();
        if item_url.path().to_ascii_lowercase().contains("favicon")
            || content_type.to_ascii_lowercase().contains("icon")
        {
            let digest = Sha256::digest(&body);
            resource_fingerprints.insert(
                item_url.as_str().to_owned(),
                digest
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            );
        }

        detect_url(&mut technologies, item_url.as_str());
        detect_headers(&mut technologies, &headers, item_url.as_str());
        detect_body(&mut technologies, &text, item_url.as_str());
        if !text.is_empty() {
            let tags = parse_tags(&text);
            for tag in &tags {
                if tag.name == "meta"
                    && tag
                        .attrs
                        .get("name")
                        .is_some_and(|name| name.eq_ignore_ascii_case("generator"))
                {
                    if let Some(generator) = tag.attrs.get("content") {
                        detect_generator(&mut technologies, generator, item_url.as_str());
                    }
                }
            }
            let mut discovered = Vec::new();
            for tag in &tags {
                collect_tag_urls(
                    tag,
                    &item_url,
                    &origin_root,
                    item.depth,
                    max_depth,
                    &mut discovered,
                    &mut external_hosts,
                    &mut technologies,
                );
            }
            for next in discovered {
                if !visited.contains(next.url.as_str()) {
                    queue.push_back(next);
                }
            }
        }

        if item.kind == ResourceKind::Page {
            page_count += 1;
        } else if matches!(item.kind, ResourceKind::Script | ResourceKind::Style) {
            asset_count += 1;
        }
        observations.push(json!({
            "url":item_url.as_str(),
            "kind":item.kind.label(),
            "status":status,
            "content_type":content_type,
            "body_bytes":body_bytes,
            "body_sampled_bytes":body_sampled_bytes,
            "body_truncated":body_truncated
        }));

        if !probes_added && item.depth == 0 && item.kind == ResourceKind::Page {
            probes_added = true;
            for path in PROBE_PATHS {
                if let Ok(url) = origin_root.join(path) {
                    queue.push_back(WorkItem {
                        url,
                        depth: 0,
                        kind: ResourceKind::Probe,
                    });
                }
            }
        }
        if item.kind == ResourceKind::Probe && item_url.path().ends_with("robots.txt") {
            enqueue_robots_sitemaps(&text, &origin_root, &mut queue);
        }
        progress(requests_attempted, max_requests, technologies.len(), false);
    }

    let complete = queue.is_empty() && errors.is_empty();
    let status = if observations.is_empty() && !errors.is_empty() {
        "failed"
    } else if complete {
        "complete"
    } else {
        "partial"
    };
    let technologies: Vec<_> = technologies.into_values().collect();
    progress(requests_attempted, max_requests, technologies.len(), true);
    let versions_known = technologies
        .iter()
        .filter(|tech| tech.version.is_some())
        .count();
    let mut limitations = vec![
        "Les scripts ne sont pas exécutés; seuls les marqueurs présents dans les ressources reçues sont inspectés.".to_owned(),
        "Les ressources externes sont identifiées par leur URL mais ne sont pas téléchargées.".to_owned(),
        "L’API KTRX ne fournit pas le détail du certificat TLS ni les enregistrements DNS CNAME.".to_owned(),
    ];
    if observations.iter().any(|observation| {
        observation
            .get("body_truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }) {
        limitations.push("Certains corps HTTP ont été tronqués à la limite configurée.".to_owned());
    }

    Ok(json!({
        "target": start.as_str(),
        "origin": origin_root.origin().ascii_serialization(),
        "dns": dns,
        "tls": tls,
        "complete": complete,
        "status": status,
        "limits": {"max_requests":max_requests,"depth":max_depth,"body_sample_bytes_per_resource":sample_limit},
        "summary": {
            "requests":requests_attempted,
            "pages":page_count,
            "assets":asset_count,
            "technologies":technologies.len(),
            "versions_identified":versions_known,
            "external_hosts_referenced":external_hosts.len()
        },
        "technologies":technologies,
        "external_hosts_referenced":external_hosts,
        "resource_fingerprints":resource_fingerprints,
        "resources":observations,
        "request_errors":errors,
        "limitations":limitations
    }))
}

fn friendly_dns_error(error: &str) -> String {
    if error.contains("nodename nor servname provided, or not known")
        || error.contains("failed to lookup address information")
    {
        "Résolution DNS impossible : vérifiez le nom de domaine et la disponibilité du réseau."
            .into()
    } else {
        format!("Échec de la résolution DNS : {error}")
    }
}

fn clean_url(mut url: Url) -> Url {
    url.set_fragment(None);
    url
}

fn host_request(operation: &str, request: &Value) -> Result<String, String> {
    let bridge = |name: &str, input: &str| ktrx::tool::host::request(name, input);
    match operation {
        "dns.lookup" => {
            let host = request.get("host").and_then(Value::as_str).ok_or("request requires host")?;
            let port = request.get("port").and_then(Value::as_u64).unwrap_or(443) as u16;
            let addresses = ktrx_net_sdk::resolve(host, port, &bridge)?;
            Ok(json!({"host":host,"addresses":addresses}).to_string())
        }
        "http.get" | "tls.inspect" => {
            let url = request.get("url").and_then(Value::as_str).ok_or("request requires url")?;
            let include_body = request.get("include_body").and_then(Value::as_bool).unwrap_or(false);
            let limit = request.get("max_body_bytes").and_then(Value::as_u64).unwrap_or(262_144) as usize;
            let mut value: Value = serde_json::from_str(&ktrx_net_sdk::http_get(url, include_body, limit, &bridge)?)
                .map_err(|e| format!("invalid HTTP response: {e}"))?;
            if operation == "tls.inspect" {
                if !url.starts_with("https://") { return Err("TLS inspection requires HTTPS".into()); }
                value["host"] = Value::String(Url::parse(url).ok().and_then(|u|u.host_str().map(str::to_owned)).unwrap_or_default());
                value["verified"] = Value::Bool(true);
                value["note"] = Value::String("TLS peer verification completed by the secure socket capability.".into());
            }
            Ok(value.to_string())
        }
        _ => ktrx::tool::host::request(operation, &request.to_string()),
    }
}

fn progress(current: usize, _maximum: usize, found: usize, done: bool) {
    let _ = ktrx::tool::host::request(
        "progress.update",
        &json!({
            "phase":"fingerprint",
            "current":current,
            "recovered":found,
            "item_label":"ressources inspectées",
            "result_label":"technologies détectées",
            "done":done
        })
        .to_string(),
    );
}

fn parse_headers(value: Option<&Value>) -> Vec<(String, String)> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|pair| {
            let pair = pair.as_array()?;
            Some((
                pair.first()?.as_str()?.to_owned(),
                pair.get(1)?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn header_first<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn add_technology(
    technologies: &mut BTreeMap<String, Technology>,
    name: &str,
    category: &str,
    version: Option<String>,
    confidence: f32,
    evidence: String,
) {
    let key = name.to_ascii_lowercase();
    let tech = technologies.entry(key).or_insert_with(|| Technology {
        name: name.to_owned(),
        category: category.to_owned(),
        version: None,
        confidence,
        evidence: Vec::new(),
    });
    if tech.version.is_none() {
        tech.version = version;
    }
    tech.confidence = tech.confidence.max(confidence);
    if tech.evidence.len() < 8 && !tech.evidence.contains(&evidence) {
        tech.evidence.push(evidence.chars().take(240).collect());
    }
}

fn detect_headers(
    technologies: &mut BTreeMap<String, Technology>,
    headers: &[(String, String)],
    source: &str,
) {
    for (name, value) in headers {
        let lower_name = name.to_ascii_lowercase();
        let lower_value = value.to_ascii_lowercase();
        let evidence = format!("HTTP header {name}: {value}");
        if lower_name == "server" {
            for (marker, product, category) in [
                ("nginx", "nginx", "web-server"),
                ("apache", "Apache HTTP Server", "web-server"),
                ("microsoft-iis", "Microsoft IIS", "web-server"),
                ("caddy", "Caddy", "web-server"),
                ("litespeed", "LiteSpeed", "web-server"),
                ("openresty", "OpenResty", "web-server"),
                ("cloudflare", "Cloudflare", "cdn-and-edge"),
                ("envoy", "Envoy", "proxy-and-gateway"),
                ("varnish", "Varnish", "cache"),
                ("gunicorn", "Gunicorn", "application-server"),
                ("uvicorn", "Uvicorn", "application-server"),
                ("kestrel", "Kestrel", "application-server"),
            ] {
                if lower_value.contains(marker) {
                    add_technology(
                        technologies,
                        product,
                        category,
                        version_after(&lower_value, marker),
                        0.98,
                        evidence.clone(),
                    );
                }
            }
        }
        if lower_name == "x-powered-by" || lower_name == "x-aspnet-version" {
            for (marker, product, category) in [
                ("php", "PHP", "programming-language"),
                ("express", "Express", "backend-framework"),
                ("asp.net", "ASP.NET", "backend-framework"),
                ("servlet", "Java Servlet", "backend-framework"),
                ("next.js", "Next.js", "frontend-framework"),
                ("plesk", "Plesk", "hosting-panel"),
            ] {
                if lower_value.contains(marker) {
                    add_technology(
                        technologies,
                        product,
                        category,
                        version_after(&lower_value, marker),
                        0.99,
                        evidence.clone(),
                    );
                }
            }
        }
        if lower_name.contains("cloudflare")
            || lower_name == "cf-ray"
            || lower_name == "cf-cache-status"
        {
            add_technology(
                technologies,
                "Cloudflare",
                "cdn-and-edge",
                None,
                0.99,
                evidence.clone(),
            );
        }
        for (header, product, category) in [
            ("x-vercel-id", "Vercel", "hosting"),
            ("x-nf-request-id", "Netlify", "hosting"),
            ("x-amz-cf-id", "Amazon CloudFront", "cdn-and-edge"),
            ("x-served-by", "Fastly", "cdn-and-edge"),
            ("x-drupal-cache", "Drupal", "cms"),
            ("x-generator", "Generator", "publishing-platform"),
        ] {
            if lower_name == header {
                let (name, version) = if product == "Generator" {
                    generator_parts(value)
                } else {
                    (
                        product.to_owned(),
                        version_after(&lower_value, product.to_ascii_lowercase().as_str()),
                    )
                };
                add_technology(
                    technologies,
                    &name,
                    category,
                    version,
                    0.97,
                    evidence.clone(),
                );
            }
        }
        if lower_name == "x-powered-by" && lower_value.contains("asp.net") {
            add_technology(
                technologies,
                "ASP.NET",
                "backend-framework",
                None,
                0.99,
                evidence.clone(),
            );
        }
        if lower_name == "server" && lower_value.contains("shopify") {
            add_technology(
                technologies,
                "Shopify",
                "e-commerce",
                None,
                0.99,
                evidence.clone(),
            );
        }
        if lower_name == "x-cache" && lower_value.contains("varnish") {
            add_technology(
                technologies,
                "Varnish",
                "cache",
                None,
                0.95,
                evidence.clone(),
            );
        }
        let _ = source;
    }
    for (_name, value) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
    {
        let cookie = value.split(';').next().unwrap_or(value).trim();
        let cookie_name = cookie
            .split('=')
            .next()
            .unwrap_or(cookie)
            .to_ascii_lowercase();
        for (needle, product, category) in [
            ("phpsessid", "PHP", "programming-language"),
            ("laravel_session", "Laravel", "backend-framework"),
            ("connect.sid", "Express", "backend-framework"),
            ("jsessionid", "Java Servlet", "backend-framework"),
            ("asp.net_sessionid", "ASP.NET", "backend-framework"),
            ("csrftoken", "Django", "backend-framework"),
            ("sessionid", "Django", "backend-framework"),
            ("_shopify", "Shopify", "e-commerce"),
            ("ghost", "Ghost", "cms"),
        ] {
            if cookie_name.contains(needle) {
                add_technology(
                    technologies,
                    product,
                    category,
                    None,
                    0.88,
                    format!("Set-Cookie name {cookie_name} from {source}"),
                );
            }
        }
    }
}

fn detect_generator(
    technologies: &mut BTreeMap<String, Technology>,
    generator: &str,
    source: &str,
) {
    let lower = generator.to_ascii_lowercase();
    for (marker, name, category) in [
        ("wordpress", "WordPress", "cms"),
        ("drupal", "Drupal", "cms"),
        ("joomla", "Joomla", "cms"),
        ("ghost", "Ghost", "cms"),
        ("shopify", "Shopify", "e-commerce"),
        ("magento", "Magento", "e-commerce"),
        ("prestashop", "PrestaShop", "e-commerce"),
        ("wix", "Wix", "site-builder"),
        ("squarespace", "Squarespace", "site-builder"),
    ] {
        if lower.contains(marker) {
            add_technology(
                technologies,
                name,
                category,
                version_after(&lower, marker),
                0.99,
                format!("generator meta: {generator} ({source})"),
            );
        }
    }
}

fn generator_parts(value: &str) -> (String, Option<String>) {
    let trimmed = value.trim();
    let name = trimmed.split_whitespace().next().unwrap_or("Generator");
    (
        name.to_owned(),
        version_after(&trimmed.to_ascii_lowercase(), &name.to_ascii_lowercase()),
    )
}

fn detect_url(technologies: &mut BTreeMap<String, Technology>, url: &str) {
    let lower = url.to_ascii_lowercase();
    detect_markers(technologies, &lower, url, "asset URL");
    for (marker, name, category) in [
        ("jquery-", "jQuery", "javascript-library"),
        ("jquery@", "jQuery", "javascript-library"),
        ("bootstrap-", "Bootstrap", "css-framework"),
        ("bootstrap@", "Bootstrap", "css-framework"),
    ] {
        if lower.contains(marker) {
            add_technology(
                technologies,
                name,
                category,
                version_after(&lower, marker),
                0.96,
                format!("versioned asset URL: {url}"),
            );
        }
    }
}

fn detect_body(technologies: &mut BTreeMap<String, Technology>, body: &str, source: &str) {
    let lower = body.to_ascii_lowercase();
    detect_markers(technologies, &lower, source, "response body marker");
    if lower.contains("ng-version=") {
        add_technology(
            technologies,
            "Angular",
            "frontend-framework",
            version_after(&lower, "ng-version="),
            0.99,
            format!("ng-version attribute ({source})"),
        );
    }
    if let Some(meta_start) = lower.find("name=\"generator\"") {
        if let Some(content_start) = lower[meta_start..].find("content=") {
            let rest = &body[meta_start + content_start + 8..];
            let generator = rest
                .trim_start_matches(['\'', '"'])
                .split(['\'', '"', '>'])
                .next()
                .unwrap_or("");
            detect_generator(technologies, generator, source);
        }
    }
}

fn detect_markers(
    technologies: &mut BTreeMap<String, Technology>,
    lower: &str,
    source: &str,
    evidence_kind: &str,
) {
    const SIGNATURES: &[(&str, &str, &str, f32)] = &[
        ("wp-content/", "WordPress", "cms", 0.98),
        ("wp-includes/", "WordPress", "cms", 0.98),
        ("wp-json", "WordPress", "cms", 0.95),
        ("drupal.settings", "Drupal", "cms", 0.98),
        ("/sites/default/", "Drupal", "cms", 0.91),
        ("joomla!", "Joomla", "cms", 0.97),
        ("/media/system/js/", "Joomla", "cms", 0.95),
        ("cdn.shopify.com", "Shopify", "e-commerce", 0.99),
        ("myshopify.com", "Shopify", "e-commerce", 0.99),
        ("shopify.theme", "Shopify", "e-commerce", 0.94),
        ("mage/cookies", "Magento", "e-commerce", 0.95),
        ("mage-cache-storage", "Magento", "e-commerce", 0.96),
        ("prestashop", "PrestaShop", "e-commerce", 0.95),
        ("ghost.org", "Ghost", "cms", 0.90),
        ("ghost-portal", "Ghost", "cms", 0.95),
        ("wixstatic.com", "Wix", "site-builder", 0.99),
        ("wix.com", "Wix", "site-builder", 0.91),
        ("squarespace.com", "Squarespace", "site-builder", 0.99),
        ("static1.squarespace", "Squarespace", "site-builder", 0.99),
        ("webflow.js", "Webflow", "site-builder", 0.97),
        ("webflow.com", "Webflow", "site-builder", 0.94),
        ("data-reactroot", "React", "frontend-framework", 0.96),
        ("react-dom", "React", "frontend-framework", 0.96),
        (
            "__react_devtools_global_hook__",
            "React",
            "frontend-framework",
            0.95,
        ),
        ("vue.runtime", "Vue.js", "frontend-framework", 0.96),
        ("__vue__", "Vue.js", "frontend-framework", 0.91),
        ("vue.global", "Vue.js", "frontend-framework", 0.96),
        ("/_next/", "Next.js", "frontend-framework", 0.99),
        ("__next_data__", "Next.js", "frontend-framework", 0.99),
        ("/_nuxt/", "Nuxt", "frontend-framework", 0.99),
        ("__nuxt__", "Nuxt", "frontend-framework", 0.97),
        ("__sveltekit", "SvelteKit", "frontend-framework", 0.97),
        ("sveltekit", "SvelteKit", "frontend-framework", 0.91),
        ("___gatsby", "Gatsby", "frontend-framework", 0.99),
        ("/page-data/", "Gatsby", "frontend-framework", 0.97),
        ("astro-island", "Astro", "frontend-framework", 0.99),
        ("astro/client", "Astro", "frontend-framework", 0.95),
        ("angular.js", "AngularJS", "frontend-framework", 0.97),
        ("ng-app=", "AngularJS", "frontend-framework", 0.95),
        ("jquery", "jQuery", "javascript-library", 0.89),
        ("bootstrap", "Bootstrap", "css-framework", 0.86),
        ("tailwindcss", "Tailwind CSS", "css-framework", 0.94),
        ("fontawesome", "Font Awesome", "icon-library", 0.94),
        ("cdnjs.cloudflare.com", "cdnjs", "cdn", 0.98),
        ("cdn.jsdelivr.net", "jsDelivr", "cdn", 0.98),
        ("unpkg.com", "unpkg", "cdn", 0.98),
        ("googleapis.com", "Google APIs", "cdn", 0.90),
        (
            "googletagmanager.com/gtm.js",
            "Google Tag Manager",
            "tag-manager",
            0.99,
        ),
        (
            "google-analytics.com",
            "Google Analytics",
            "analytics",
            0.98,
        ),
        (
            "googletagmanager.com/gtag/js",
            "Google tag (gtag.js)",
            "analytics",
            0.98,
        ),
        ("matomo.js", "Matomo", "analytics", 0.99),
        ("_paq.push", "Matomo", "analytics", 0.92),
        ("hotjar.com", "Hotjar", "analytics", 0.98),
        ("segment.com/analytics.js", "Segment", "analytics", 0.98),
        ("mixpanel.com", "Mixpanel", "analytics", 0.95),
        ("plausible.io/js/", "Plausible", "analytics", 0.99),
        ("clarity.ms/tag/", "Microsoft Clarity", "analytics", 0.99),
        ("js.stripe.com", "Stripe", "payment", 0.99),
        ("auth0.com", "Auth0", "identity", 0.94),
        ("firebaseapp.com", "Firebase", "backend-platform", 0.98),
        ("firebase.google.com", "Firebase", "backend-platform", 0.93),
        ("supabase.co", "Supabase", "backend-platform", 0.97),
        ("laravel", "Laravel", "backend-framework", 0.83),
        ("csrfmiddlewaretoken", "Django", "backend-framework", 0.93),
        (
            "__viewstate",
            "ASP.NET Web Forms",
            "backend-framework",
            0.98,
        ),
        (
            "springframework",
            "Spring Framework",
            "backend-framework",
            0.93,
        ),
        ("rails-ujs", "Ruby on Rails", "backend-framework", 0.94),
        ("rack.version", "Rack", "backend-framework", 0.94),
        ("connect.sid", "Express", "backend-framework", 0.89),
        ("/wp-admin/", "WordPress", "cms", 0.94),
        ("contentful", "Contentful", "headless-cms", 0.91),
        ("prismic.io", "Prismic", "headless-cms", 0.95),
        ("sanity.io", "Sanity", "headless-cms", 0.95),
        ("htmx.org", "htmx", "javascript-library", 0.98),
        ("hx-get=", "htmx", "javascript-library", 0.93),
        ("alpinejs", "Alpine.js", "javascript-library", 0.96),
        ("x-data=", "Alpine.js", "javascript-library", 0.87),
        ("ember-view", "Ember.js", "frontend-framework", 0.98),
        ("ember.js", "Ember.js", "frontend-framework", 0.97),
        ("backbone.js", "Backbone.js", "frontend-framework", 0.97),
        ("preact", "Preact", "frontend-framework", 0.91),
        ("lit-element", "Lit", "frontend-framework", 0.96),
        ("lit-html", "Lit", "frontend-framework", 0.95),
        ("@mui/", "MUI", "ui-framework", 0.98),
        ("material-ui", "MUI", "ui-framework", 0.95),
        ("chakra-ui", "Chakra UI", "ui-framework", 0.96),
        ("ant-design", "Ant Design", "ui-framework", 0.95),
        ("webpackjsonp", "Webpack", "build-tool", 0.97),
        ("webpack://", "Webpack", "build-tool", 0.94),
        ("__vite__", "Vite", "build-tool", 0.98),
        ("@vite/client", "Vite", "build-tool", 0.99),
        ("requirejs", "RequireJS", "javascript-library", 0.97),
        ("require.js", "RequireJS", "javascript-library", 0.96),
        ("lodash", "Lodash", "javascript-library", 0.85),
        ("moment.js", "Moment.js", "javascript-library", 0.95),
        ("axios", "Axios", "javascript-library", 0.84),
        ("chart.js", "Chart.js", "javascript-library", 0.95),
        ("d3.min.js", "D3.js", "javascript-library", 0.96),
        ("three.min.js", "Three.js", "javascript-library", 0.96),
        ("gsap.min.js", "GSAP", "javascript-library", 0.96),
        (
            "google.com/recaptcha",
            "Google reCAPTCHA",
            "security-service",
            0.98,
        ),
        (
            "gstatic.com/recaptcha",
            "Google reCAPTCHA",
            "security-service",
            0.98,
        ),
        ("sentry.io", "Sentry", "monitoring", 0.97),
        ("sentry-cdn.com", "Sentry", "monitoring", 0.98),
        ("datadoghq", "Datadog", "monitoring", 0.96),
        ("newrelic.com", "New Relic", "monitoring", 0.98),
        ("intercom.io", "Intercom", "customer-support", 0.97),
        ("zendesk.com", "Zendesk", "customer-support", 0.97),
        ("hubspot.com", "HubSpot", "marketing", 0.97),
        ("bigcommerce.com", "BigCommerce", "e-commerce", 0.97),
        ("woocommerce", "WooCommerce", "e-commerce", 0.89),
        ("strapi", "Strapi", "headless-cms", 0.91),
        ("directus", "Directus", "headless-cms", 0.92),
        ("typo3", "TYPO3", "cms", 0.94),
        ("umbraco", "Umbraco", "cms", 0.93),
        ("clerk.com", "Clerk", "identity", 0.97),
        ("okta.com", "Okta", "identity", 0.96),
        ("cognito-idp", "Amazon Cognito", "identity", 0.96),
        ("imperva", "Imperva", "security-service", 0.94),
        ("akamai", "Akamai", "cdn-and-edge", 0.91),
        (
            "azurewebsites.net",
            "Microsoft Azure App Service",
            "hosting",
            0.98,
        ),
        ("appspot.com", "Google App Engine", "hosting", 0.97),
        (
            "elasticbeanstalk.com",
            "AWS Elastic Beanstalk",
            "hosting",
            0.98,
        ),
        ("solidjs", "Solid.js", "frontend-framework", 0.93),
        ("data-solid-", "Solid.js", "frontend-framework", 0.89),
        ("qwikloader", "Qwik", "frontend-framework", 0.98),
        ("q:container", "Qwik", "frontend-framework", 0.95),
        ("__remixcontext", "Remix", "frontend-framework", 0.98),
        ("remix-run", "Remix", "frontend-framework", 0.93),
        ("fresh.gen.ts", "Deno Fresh", "frontend-framework", 0.95),
        ("svelte-", "Svelte", "frontend-framework", 0.78),
        ("rollupjs", "Rollup", "build-tool", 0.9),
        ("parcelrequire", "Parcel", "build-tool", 0.96),
        ("esbuild", "esbuild", "build-tool", 0.86),
        ("turbopack", "Turbopack", "build-tool", 0.95),
        ("framer-motion", "Framer Motion", "javascript-library", 0.95),
        ("framerusercontent.com", "Framer", "site-builder", 0.98),
        ("leafletjs", "Leaflet", "javascript-library", 0.93),
        ("unpkg.com/leaflet", "Leaflet", "javascript-library", 0.95),
        ("api.mapbox.com", "Mapbox", "javascript-library", 0.97),
        (
            "maps.googleapis.com",
            "Google Maps",
            "javascript-library",
            0.96,
        ),
        ("onesignal.com", "OneSignal", "marketing", 0.97),
        ("widget.intercom.io", "Intercom", "customer-support", 0.97),
        ("js.driftt.com", "Drift", "customer-support", 0.97),
        ("client.crisp.chat", "Crisp", "customer-support", 0.97),
        ("code.tidio.co", "Tidio", "customer-support", 0.97),
        ("fw-cdn.com", "Freshchat", "customer-support", 0.94),
        ("static.klaviyo.com", "Klaviyo", "marketing", 0.97),
        ("list-manage.com", "Mailchimp", "marketing", 0.93),
        ("activehosted.com", "ActiveCampaign", "marketing", 0.93),
        ("braintreegateway.com", "Braintree", "payment", 0.98),
        ("paypal.com/sdk/js", "PayPal", "payment", 0.98),
        ("checkout.adyen.com", "Adyen", "payment", 0.98),
        ("squareup.com", "Square", "payment", 0.94),
        ("checkout.razorpay.com", "Razorpay", "payment", 0.98),
        ("js.recurly.com", "Recurly", "payment", 0.98),
        ("js.chargebee.com", "Chargebee", "payment", 0.98),
        ("algolia.net", "Algolia", "search-service", 0.96),
        ("algolianet.com", "Algolia", "search-service", 0.95),
        ("typesense.io", "Typesense", "search-service", 0.93),
        ("meilisearch", "Meilisearch", "search-service", 0.92),
        (
            "challenges.cloudflare.com",
            "Cloudflare Turnstile",
            "security-service",
            0.97,
        ),
        ("hcaptcha.com", "hCaptcha", "security-service", 0.97),
        (
            "fingerprintjs",
            "FingerprintJS",
            "security-service",
            0.9,
        ),
        ("cdn.optimizely.com", "Optimizely", "ab-testing", 0.97),
        ("dev.visualwebsiteoptimizer.com", "VWO", "ab-testing", 0.97),
        ("try.abtasty.com", "AB Tasty", "ab-testing", 0.95),
        ("unpkg.com/launchdarkly", "LaunchDarkly", "ab-testing", 0.9),
        ("cdn.split.io", "Split.io", "ab-testing", 0.95),
        ("cdn.amplitude.com", "Amplitude", "analytics", 0.95),
        ("heap-analytics", "Heap", "analytics", 0.93),
        ("edge.fullstory.com", "FullStory", "analytics", 0.97),
        ("cdn.mouseflow.com", "Mouseflow", "analytics", 0.95),
        ("cdn.luckyorange.com", "Lucky Orange", "analytics", 0.95),
        ("script.crazyegg.com", "Crazy Egg", "analytics", 0.95),
        ("cdn.rudderlabs.com", "RudderStack", "analytics", 0.94),
        ("app.posthog.com", "PostHog", "analytics", 0.95),
        ("cdn.bugsnag.com", "Bugsnag", "monitoring", 0.96),
        ("cdn.rollbar.com", "Rollbar", "monitoring", 0.96),
        ("cdn.logrocket.com", "LogRocket", "monitoring", 0.96),
        ("elastic-apm", "Elastic APM", "monitoring", 0.92),
        ("vercel.app", "Vercel", "hosting", 0.9),
        ("vercel-insights.com", "Vercel", "hosting", 0.97),
        ("netlify.app", "Netlify", "hosting", 0.92),
        ("netlify.com/img/", "Netlify", "hosting", 0.85),
        ("onrender.com", "Render", "hosting", 0.95),
        (".fly.dev", "Fly.io", "hosting", 0.93),
        ("railway.app", "Railway", "hosting", 0.9),
        ("herokuapp.com", "Heroku", "hosting", 0.93),
        ("digitaloceanspaces.com", "DigitalOcean", "hosting", 0.9),
        ("builder.io", "Builder.io", "headless-cms", 0.93),
        ("notion.site", "Notion Sites", "site-builder", 0.94),
        ("carrd.co", "Carrd", "site-builder", 0.93),
    ];
    for (marker, name, category, confidence) in SIGNATURES {
        if lower.contains(marker) {
            let version = match *name {
                "WordPress" => version_after(lower, "wordpress"),
                "jQuery" => version_after(lower, "jquery"),
                "Bootstrap" => version_after(lower, "bootstrap"),
                "Vue.js" => version_after(lower, "vue@"),
                "React" => version_after(lower, "react@"),
                _ => None,
            };
            add_technology(
                technologies,
                name,
                category,
                version,
                *confidence,
                format!("{evidence_kind}: {marker} ({source})"),
            );
        }
    }
}

fn version_after(text: &str, marker: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find(&marker.to_ascii_lowercase())? + marker.len();
    let tail = text.get(start..)?.chars().take(96).collect::<String>();
    let first_digit = tail.find(|c: char| c.is_ascii_digit())?;
    let candidate: String = tail[first_digit..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let candidate = candidate.trim_end_matches('.');
    (candidate.contains('.')
        && candidate
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())))
    .then(|| candidate.to_owned())
}

struct Tag {
    name: String,
    attrs: BTreeMap<String, String>,
}

fn parse_tags(html: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut cursor = 0usize;
    let bytes = html.as_bytes();
    while cursor < bytes.len() && tags.len() < 20_000 {
        let Some(relative) = html[cursor..].find('<') else {
            break;
        };
        let start = cursor + relative;
        if html[start..].starts_with("<!--") {
            cursor = html[start + 4..]
                .find("-->")
                .map(|end| start + 4 + end + 3)
                .unwrap_or(html.len());
            continue;
        }
        let Some(end) = find_tag_end(bytes, start + 1) else {
            break;
        };
        if end - start <= 8_192 {
            if let Some(tag) = parse_tag(&html[start + 1..end]) {
                tags.push(tag);
            }
        }
        cursor = end + 1;
    }
    tags
}

fn find_tag_end(bytes: &[u8], mut cursor: usize) -> Option<usize> {
    let mut quote = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn parse_tag(raw: &str) -> Option<Tag> {
    let raw = raw.trim_start();
    if raw.is_empty() || raw.starts_with(['!', '?', '/']) {
        return None;
    }
    let bytes = raw.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() && bytes[cursor] != b'/' {
        cursor += 1;
    }
    if cursor == 0 {
        return None;
    }
    let name = raw[..cursor].to_ascii_lowercase();
    let mut attrs = BTreeMap::new();
    while cursor < bytes.len() {
        while cursor < bytes.len() && (bytes[cursor].is_ascii_whitespace() || bytes[cursor] == b'/')
        {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        let name_start = cursor;
        while cursor < bytes.len()
            && !bytes[cursor].is_ascii_whitespace()
            && bytes[cursor] != b'='
            && bytes[cursor] != b'/'
        {
            cursor += 1;
        }
        if cursor == name_start {
            cursor += 1;
            continue;
        }
        let attr_name = raw[name_start..cursor].to_ascii_lowercase();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let mut value = String::new();
        if cursor < bytes.len() && bytes[cursor] == b'=' {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && (bytes[cursor] == b'\'' || bytes[cursor] == b'"') {
                let quote = bytes[cursor];
                cursor += 1;
                let value_start = cursor;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                value = raw[value_start..cursor].to_owned();
                cursor = cursor.saturating_add(1);
            } else {
                let value_start = cursor;
                while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
                    cursor += 1;
                }
                value = raw[value_start..cursor].trim_end_matches('/').to_owned();
            }
        }
        attrs.entry(attr_name).or_insert(value);
    }
    Some(Tag { name, attrs })
}

fn collect_tag_urls(
    tag: &Tag,
    page: &Url,
    origin: &Url,
    depth: usize,
    max_depth: usize,
    output: &mut Vec<WorkItem>,
    external_hosts: &mut BTreeSet<String>,
    technologies: &mut BTreeMap<String, Technology>,
) {
    let mut candidates: Vec<(&str, ResourceKind)> = Vec::new();
    match tag.name.as_str() {
        "script" => {
            if let Some(src) = tag.attrs.get("src") {
                candidates.push((src, ResourceKind::Script));
            }
        }
        "link" => {
            let rel = tag
                .attrs
                .get("rel")
                .map(String::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if let Some(href) = tag.attrs.get("href") {
                if rel.contains("stylesheet") {
                    candidates.push((href, ResourceKind::Style));
                } else if rel.contains("manifest") {
                    candidates.push((href, ResourceKind::Probe));
                } else if rel.contains("icon") {
                    candidates.push((href, ResourceKind::Probe));
                } else if rel.contains("preload")
                    && tag
                        .attrs
                        .get("as")
                        .is_some_and(|value| value.eq_ignore_ascii_case("script"))
                {
                    candidates.push((href, ResourceKind::Script));
                }
            }
        }
        "a" => {
            if depth < max_depth {
                if let Some(href) = tag.attrs.get("href") {
                    candidates.push((href, ResourceKind::Page));
                }
            }
        }
        "iframe" | "frame" => {
            if depth < max_depth {
                if let Some(src) = tag.attrs.get("src") {
                    candidates.push((src, ResourceKind::Page));
                }
            }
        }
        _ => {}
    }
    if tag.name == "meta" {
        if let Some(content) = tag.attrs.get("content") {
            let name = tag.attrs.get("name").map(String::as_str).unwrap_or("");
            if name.eq_ignore_ascii_case("generator") {
                detect_generator(technologies, content, page.as_str());
            }
        }
    }
    for (raw, kind) in candidates {
        let raw = raw.trim().replace("&amp;", "&");
        if raw.is_empty()
            || raw.starts_with('#')
            || raw.starts_with("data:")
            || raw.starts_with("javascript:")
            || raw.starts_with("mailto:")
            || raw.starts_with("tel:")
        {
            continue;
        }
        let Ok(url) = page.join(&raw) else {
            continue;
        };
        let url = clean_url(url);
        detect_url(technologies, url.as_str());
        if url.origin() != origin.origin() {
            if let Some(host) = url.host_str() {
                external_hosts.insert(host.to_owned());
                detect_external_host(technologies, host, page.as_str());
            }
            continue;
        }
        if kind == ResourceKind::Probe && tag.name != "link" {
            continue;
        }
        let next_depth = if kind == ResourceKind::Page {
            depth + 1
        } else {
            depth
        };
        output.push(WorkItem {
            url,
            depth: next_depth,
            kind,
        });
    }
}

fn detect_external_host(technologies: &mut BTreeMap<String, Technology>, host: &str, source: &str) {
    let lower = host.to_ascii_lowercase();
    for (marker, name, category) in [
        ("cloudflare", "Cloudflare", "cdn-and-edge"),
        ("cloudfront", "Amazon CloudFront", "cdn-and-edge"),
        ("fastly", "Fastly", "cdn-and-edge"),
        ("akamai", "Akamai", "cdn-and-edge"),
        ("jsdelivr", "jsDelivr", "cdn"),
        ("cdnjs", "cdnjs", "cdn"),
        ("unpkg", "unpkg", "cdn"),
        ("googleapis", "Google APIs", "cdn"),
        ("gstatic", "Google static assets", "cdn"),
        ("shopify", "Shopify", "e-commerce"),
        ("wixstatic", "Wix", "site-builder"),
        ("squarespace", "Squarespace", "site-builder"),
    ] {
        if lower.contains(marker) {
            add_technology(
                technologies,
                name,
                category,
                None,
                0.94,
                format!("external resource host {host}, referenced by {source}"),
            );
        }
    }
}

fn enqueue_robots_sitemaps(text: &str, origin: &Url, queue: &mut VecDeque<WorkItem>) {
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if key.trim().eq_ignore_ascii_case("sitemap") {
                if let Ok(url) = Url::parse(value.trim()) {
                    if url.origin() == origin.origin() {
                        queue.push_back(WorkItem {
                            url,
                            depth: 0,
                            kind: ResourceKind::Probe,
                        });
                    }
                }
            }
        }
    }
}

export!(Plugin);
