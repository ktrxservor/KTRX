//! Client for the KTRX tool registry: `ktrx install`, `update`, `list`,
//! `uninstall` and `search`. Downloads are verified against the SHA-256 the
//! registry publishes and validated as real `.tool` archives before they are
//! moved into the install directory.

use anyhow::{Context, Result, bail, ensure};
use ktrx_core::{MAX_ARCHIVE_BYTES, ToolPackage};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const DEFAULT_REGISTRY: &str = "https://ktrx-site.tiagoantunes2009.workers.dev";

pub struct Registry {
    base: String,
    agent: ureq::Agent,
}

struct Remote {
    id: String,
    myth_name: String,
    latest: String,
}

impl Registry {
    pub fn new(base: Option<String>) -> Result<Self> {
        let base = base
            .or_else(|| std::env::var("KTRX_REGISTRY").ok())
            .unwrap_or_else(|| DEFAULT_REGISTRY.to_owned());
        let base = base.trim_end_matches('/').to_owned();
        ensure!(
            base.starts_with("https://")
                || base.starts_with("http://localhost")
                || base.starts_with("http://127.0.0.1"),
            "the registry must use https:// (got {base})"
        );
        #[allow(unused_mut)] // only mutated on Linux
        let mut tls = native_tls::TlsConnector::builder();
        // A Linux machine without a CA bundle (minimal container, bare install) still trusts
        // Mozilla's public roots, so `ktrx install` works everywhere.
        #[cfg(target_os = "linux")]
        for root in webpki_root_certs::TLS_SERVER_ROOT_CERTS {
            if let Ok(certificate) = native_tls::Certificate::from_der(root.as_ref()) {
                tls.add_root_certificate(certificate);
            }
        }
        let tls = tls.build().context("initialising TLS")?;
        let agent = ureq::AgentBuilder::new()
            .tls_connector(Arc::new(tls))
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .user_agent(concat!("ktrx/", env!("CARGO_PKG_VERSION")))
            // Corporate networks: honour HTTPS_PROXY / HTTP_PROXY / ALL_PROXY (and NO_PROXY).
            .try_proxy_from_env(true)
            .build();
        Ok(Self { base, agent })
    }

    fn get_json(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        let response = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| describe_http_error(&url, e))?;
        response
            .into_json()
            .with_context(|| format!("reading the registry answer from {url}"))
    }

    fn catalog(&self) -> Result<Vec<Remote>> {
        let value = self.get_json("/api/tools")?;
        let tools = value["tools"]
            .as_array()
            .context("the registry returned an unexpected catalog")?;
        Ok(tools
            .iter()
            .filter_map(|t| {
                Some(Remote {
                    id: t["id"].as_str()?.to_owned(),
                    myth_name: t["myth_name"].as_str()?.to_owned(),
                    latest: t["latest_version"].as_str()?.to_owned(),
                })
            })
            .collect())
    }

    /// Match `anubis`, `K~ANUBIS` or the technical id `hash-audit`, ignoring case.
    fn resolve(&self, query: &str) -> Result<Remote> {
        let wanted = normalize(query);
        let mut found = None;
        for tool in self.catalog()? {
            if normalize(&tool.myth_name) == wanted || tool.id.eq_ignore_ascii_case(&wanted) {
                found = Some(tool);
                break;
            }
        }
        found.with_context(|| format!("no tool named {query:?} in the registry (try `ktrx search`)"))
    }

    pub fn search(&self, query: Option<&str>) -> Result<()> {
        let value = self.get_json("/api/tools")?;
        let needle = query.map(str::to_lowercase);
        let mut shown = 0;
        for t in value["tools"].as_array().into_iter().flatten() {
            let line = format!(
                "{:14} {:8} {}",
                t["myth_name"].as_str().unwrap_or("?"),
                t["latest_version"].as_str().unwrap_or("?"),
                t["description"].as_str().unwrap_or("")
            );
            let hay = format!("{} {}", line, t["id"].as_str().unwrap_or("")).to_lowercase();
            if needle.as_ref().is_none_or(|n| hay.contains(n)) {
                println!("{line}");
                shown += 1;
            }
        }
        if shown == 0 {
            println!("No tool matches.");
        }
        Ok(())
    }

    /// Install `spec` (`name` or `name@version`) into `dir`.
    pub fn install(&self, spec: &str, dir: &Path, force: bool) -> Result<PathBuf> {
        let (query, requested) = match spec.split_once('@') {
            Some((q, v)) => (q, Some(v)),
            None => (spec, None),
        };
        let remote = self.resolve(query)?;
        let version = requested.unwrap_or(&remote.latest);

        let detail = self.get_json(&format!("/api/tools/{}", remote.id))?;
        let entry = detail["versions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|v| v["version"].as_str() == Some(version))
            .with_context(|| format!("{} has no version {version}", remote.myth_name))?;
        ensure!(
            entry["yanked"].as_i64().unwrap_or(0) == 0,
            "{} {version} was withdrawn by its publisher",
            remote.myth_name
        );
        let expected = entry["sha256"]
            .as_str()
            .context("the registry did not publish a checksum for this version")?
            .to_ascii_lowercase();

        let target = dir.join(format!("{}.tool", remote.myth_name));
        if target.exists() && !force {
            if let Ok(installed) = ToolPackage::open(&target) {
                if installed.manifest.version == version {
                    println!("{} {version} is already installed ({}).", remote.myth_name, target.display());
                    return Ok(target);
                }
            }
        }

        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let part = dir.join(format!(".{}.part", remote.myth_name));
        println!("Downloading {} {version}…", remote.myth_name);
        let digest = self
            .download(&format!("/dl/{}?version={version}", remote.id), &part)
            .inspect_err(|_| {
                let _ = fs::remove_file(&part);
            })?;
        if digest != expected {
            let _ = fs::remove_file(&part);
            bail!("checksum mismatch for {} {version}: expected {expected}, got {digest}; nothing was installed", remote.myth_name);
        }
        // A real archive, a valid manifest and a compatible engine, or nothing.
        let package = ToolPackage::open(&part).inspect_err(|_| {
            let _ = fs::remove_file(&part);
        })?;
        ensure!(
            package.manifest.id == remote.id,
            "the downloaded archive is {:?}, not {:?}",
            package.manifest.id,
            remote.id
        );
        fs::rename(&part, &target).with_context(|| format!("writing {}", target.display()))?;
        println!("Installed {} {} → {}", package.manifest.name, package.manifest.version, target.display());
        println!("sha256 {digest} (verified)");
        let perms: Vec<&str> = package.manifest.permissions.iter().map(|p| p.as_str()).collect();
        println!(
            "Permissions: {}",
            if perms.is_empty() { "none".to_owned() } else { perms.join(", ") }
        );
        println!("Run it with: ktrx run {} --help", remote.myth_name.trim_start_matches("K~").to_lowercase());
        Ok(target)
    }

    fn download(&self, path: &str, dest: &Path) -> Result<String> {
        let url = format!("{}{}", self.base, path);
        let response = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| describe_http_error(&url, e))?;
        let mut reader = response.into_reader().take(MAX_ARCHIVE_BYTES + 1);
        let mut file = File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).context("downloading")?;
            if n == 0 {
                break;
            }
            total += n as u64;
            ensure!(total <= MAX_ARCHIVE_BYTES, "the download exceeds the 64 MiB archive limit");
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])?;
        }
        file.flush()?;
        Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Update one installed tool, or all of them.
    pub fn update(&self, name: Option<&str>, dir: &Path) -> Result<()> {
        let installed = installed_tools(dir);
        let targets: Vec<_> = match name {
            Some(n) => {
                let wanted = normalize(n);
                installed
                    .into_iter()
                    .filter(|(_, p)| normalize(&p.manifest.name) == wanted || p.manifest.id == wanted)
                    .collect()
            }
            None => installed,
        };
        if targets.is_empty() {
            println!("Nothing to update: no matching tool is installed in {}.", dir.display());
            return Ok(());
        }
        let catalog = self.catalog()?;
        for (_, package) in targets {
            let Some(remote) = catalog.iter().find(|r| r.id == package.manifest.id) else {
                println!("{}: no longer in the registry, kept as is.", package.manifest.name);
                continue;
            };
            if remote.latest == package.manifest.version {
                println!("{} {} is up to date.", package.manifest.name, package.manifest.version);
            } else {
                println!("{}: {} → {}", package.manifest.name, package.manifest.version, remote.latest);
                self.install(&remote.myth_name, dir, true)?;
            }
        }
        Ok(())
    }
}

pub fn list(dir: &Path) -> Result<()> {
    let tools = installed_tools(dir);
    if tools.is_empty() {
        println!("No tool installed in {}. Try `ktrx search`, then `ktrx install <name>`.", dir.display());
        return Ok(());
    }
    for (_, p) in tools {
        println!("{:14} {:8} {}", p.manifest.name, p.manifest.version, p.manifest.description);
    }
    Ok(())
}

pub fn uninstall(name: &str, dir: &Path) -> Result<()> {
    let wanted = normalize(name);
    for (path, package) in installed_tools(dir) {
        if normalize(&package.manifest.name) == wanted || package.manifest.id == wanted {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            println!("Removed {} {}.", package.manifest.name, package.manifest.version);
            return Ok(());
        }
    }
    bail!("{name:?} is not installed in {}", dir.display())
}

/// The install directory: `--dir`, else `$KTRX_HOME/tools`, else `~/.ktrx/tools`.
pub fn install_dir(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = explicit {
        return Ok(dir);
    }
    if let Some(home) = std::env::var_os("KTRX_HOME") {
        return Ok(PathBuf::from(home).join("tools"));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("cannot find your home directory; pass --dir or set KTRX_HOME")?;
    Ok(PathBuf::from(home).join(".ktrx").join("tools"))
}

/// Let `ktrx run anubis` find `K~ANUBIS.tool` in the install directory when no
/// such file exists in the current one.
pub fn locate(tool: PathBuf) -> PathBuf {
    if tool.exists() || tool.components().count() != 1 {
        return tool;
    }
    let Ok(dir) = install_dir(None) else { return tool };
    let wanted = normalize(&tool.to_string_lossy());
    installed_tools(&dir)
        .into_iter()
        .find(|(_, p)| normalize(&p.manifest.name) == wanted || p.manifest.id == wanted)
        .map(|(path, _)| path)
        .unwrap_or(tool)
}

fn installed_tools(dir: &Path) -> Vec<(PathBuf, ToolPackage)> {
    let mut tools: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "tool"))
        .filter_map(|p| ToolPackage::open(&p).ok().map(|pkg| (p, pkg)))
        .collect();
    tools.sort_by(|a, b| a.1.manifest.name.cmp(&b.1.manifest.name));
    tools
}

fn normalize(name: &str) -> String {
    name.trim()
        .trim_start_matches("K~")
        .trim_start_matches("k~")
        .trim_end_matches(".tool")
        .to_ascii_lowercase()
}

fn describe_http_error(url: &str, error: ureq::Error) -> anyhow::Error {
    match error {
        ureq::Error::Status(code, _) => anyhow::anyhow!("the registry answered HTTP {code} for {url}"),
        other => anyhow::anyhow!("cannot reach the registry ({url}): {other}"),
    }
}
