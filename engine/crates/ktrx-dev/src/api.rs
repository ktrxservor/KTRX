//! Team API client and local credentials.

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};

pub const DEFAULT_REGISTRY: &str = "https://ktrx-site.tiagoantunes2009.workers.dev";

/// Where `ktrx-dev login` stores the key: ~/.config/ktrx/dev.toml (Unix), %APPDATA%\ktrx\dev.toml (Windows).
pub fn config_path() -> Result<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    Ok(base.context("dossier de configuration utilisateur introuvable")?.join("ktrx").join("dev.toml"))
}

#[derive(Default)]
pub struct Config {
    pub registry: Option<String>,
    pub key: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        let mut config = Config::default();
        if let Ok(text) = fs::read_to_string(&path) {
            let value: toml::Value = toml::from_str(&text).with_context(|| format!("lecture de {}", path.display()))?;
            config.registry = value.get("registry").and_then(|v| v.as_str()).map(str::to_owned);
            config.key = value.get("key").and_then(|v| v.as_str()).map(str::to_owned);
        }
        if let Ok(key) = std::env::var("KTRX_API_KEY") {
            config.key = Some(key);
        }
        if let Ok(registry) = std::env::var("KTRX_REGISTRY") {
            config.registry = Some(registry);
        }
        Ok(config)
    }

    pub fn save(registry: &str, key: &str) -> Result<PathBuf> {
        let path = config_path()?;
        fs::create_dir_all(path.parent().unwrap())?;
        let mut table = toml::map::Map::new();
        table.insert("registry".into(), toml::Value::String(registry.into()));
        table.insert("key".into(), toml::Value::String(key.into()));
        let text = format!(
            "# Clé ktrx-dev. Fichier personnel : ne pas le partager.\n{}",
            toml::to_string(&toml::Value::Table(table))?
        );
        fs::write(&path, text).with_context(|| format!("écriture de {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(path)
    }

    pub fn registry(&self) -> String {
        self.registry.clone().unwrap_or_else(|| DEFAULT_REGISTRY.to_owned()).trim_end_matches('/').to_owned()
    }
}

pub struct Api {
    pub base: String,
    key: Option<String>,
    agent: ureq::Agent,
}

pub fn agent() -> Result<ureq::Agent> {
    #[allow(unused_mut)]
    let mut tls = native_tls::TlsConnector::builder();
    // A Linux machine without a CA bundle still trusts Mozilla's public roots.
    #[cfg(target_os = "linux")]
    for root in webpki_root_certs::TLS_SERVER_ROOT_CERTS {
        if let Ok(certificate) = native_tls::Certificate::from_der(root.as_ref()) {
            tls.add_root_certificate(certificate);
        }
    }
    Ok(ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls.build().context("initialisation TLS")?))
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .user_agent(concat!("ktrx-dev/", env!("CARGO_PKG_VERSION")))
        .try_proxy_from_env(true)
        .build())
}

impl Api {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self { base: config.registry(), key: config.key.clone(), agent: agent()? })
    }

    pub fn site_url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn key(&self) -> Result<&str> {
        self.key
            .as_deref()
            .ok_or_else(|| anyhow!("pas de clé : lancer `ktrx-dev login` (ou définir KTRX_API_KEY)"))
    }

    /// Public GET; `None` on 404.
    pub fn get_public(&self, path: &str) -> Result<Option<Value>> {
        match self.agent.get(&self.site_url(path)).call() {
            Ok(r) => Ok(Some(r.into_json()?)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(describe(e, path)),
        }
    }

    fn admin(&self, method: &str, path: &str) -> Result<ureq::Request> {
        Ok(self
            .agent
            .request(method, &self.site_url(&format!("/api/admin{path}")))
            .set("Authorization", &format!("Bearer {}", self.key()?)))
    }

    pub fn admin_json(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let request = self.admin(method, path)?;
        let response = match body {
            Some(b) => request.send_json(b),
            None => request.call(),
        };
        response.map_err(|e| describe(e, path))?.into_json().context("réponse de l'API illisible")
    }

    pub fn admin_bytes(&self, method: &str, path: &str, content_type: &str, bytes: &[u8]) -> Result<Value> {
        self.admin(method, path)?
            .set("Content-Type", content_type)
            .send_bytes(bytes)
            .map_err(|e| describe(e, path))?
            .into_json()
            .context("réponse de l'API illisible")
    }

    pub fn download(&self, url: &str) -> Result<Vec<u8>> {
        let response = self.agent.get(url).call().map_err(|e| describe(e, url))?;
        let mut bytes = Vec::new();
        use std::io::Read;
        response.into_reader().take(200 * 1024 * 1024).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}


/// Turns an HTTP error into the API's own message when there is one.
fn describe(error: ureq::Error, what: &str) -> anyhow::Error {
    match error {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            let message = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or(body);
            match code {
                401 => anyhow!("clé refusée par le registre ({message}) : relancer `ktrx-dev login`"),
                403 => anyhow!("{message}"),
                _ => anyhow!("HTTP {code} sur {what} : {message}"),
            }
        }
        other => anyhow!("registre injoignable ({what}) : {other}"),
    }
}
