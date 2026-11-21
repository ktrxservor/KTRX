//! ktrx-dev — the KTRX team tool. Creates, checks and publishes tools to the registry, publishes
//! engine releases and manages the team's API keys. Not distributed to users: the public `ktrx`
//! engine only installs and runs tools.

mod api;
mod tool;

use anyhow::{Context, Result, bail};
use api::{Api, Config};
use clap::{Parser, Subcommand};
use ktrx_core::ToolPackage;
use ktrx_runtime::Runtime;
use sha2::{Digest, Sha256};
use std::{
    io::{self, BufRead, IsTerminal, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    name = "ktrx-dev",
    version,
    about = "Outil de l'équipe KTRX : créer, vérifier et publier des outils sur le registre.",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Enregistre ta clé d'API sur cette machine (demandée de façon masquée, ou lue sur l'entrée standard).
    Login {
        /// Registre à utiliser (par défaut : le registre officiel).
        #[arg(long)]
        registry: Option<String>,
    },
    /// Supprime la clé enregistrée sur cette machine.
    Logout,
    /// Affiche la clé utilisée et ses droits.
    Whoami,
    /// Vérifie que la machine est prête : Rust, cible wasm32-wasip2, connexion, clé.
    Doctor,
    /// Crée un nouvel outil complet qui compile déjà (code, manifeste, tests, README, emblème).
    New {
        /// Nom mythologique, ex. NEITH.
        myth: String,
        /// Nom lisible, ex. "JSON-Inspector".
        readable: String,
        /// investigation | analysis | encoding | generation
        #[arg(long, default_value = "analysis")]
        category: String,
        /// Dossier parent (par défaut : tools/ du dépôt, sinon le dossier courant).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Vérifie un outil de A à Z, le compile et produit son archive .tool.
    Check {
        /// Nom (neith), K~NEITH, id ou chemin du dossier.
        tool: String,
        /// Ne contacte pas le registre (version, nom déjà pris).
        #[arg(long)]
        offline: bool,
    },
    /// Vérifie puis publie un outil : archive, README et emblème.
    Publish {
        tool: String,
        /// Note de version affichée sur le site.
        #[arg(long)]
        changelog: Option<String>,
        /// Met seulement à jour le README et l'emblème de la version en ligne.
        #[arg(long)]
        docs: bool,
    },
    /// Retire une version publiée (elle n'est plus installable).
    Yank { tool: String, version: String },
    /// Supprime un outil du registre, toutes versions comprises (clé admin).
    Delete {
        tool: String,
        /// Confirme sans question.
        #[arg(long)]
        yes: bool,
    },
    /// Publie sur le site les binaires d'une release GitHub du moteur (clé admin).
    Engine {
        #[command(subcommand)]
        action: EngineCmd,
    },
    /// Gère les clés de l'équipe (clé admin).
    Keys {
        #[command(subcommand)]
        action: KeysCmd,
    },
    /// Dernières actions faites sur le registre (clé admin).
    Audit {
        #[arg(long, default_value_t = 30)]
        limit: u32,
    },
    /// Empaquette manifest.toml + module.wasm en archive .tool.
    Pack {
        manifest: PathBuf,
        wasm: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Valide une archive .tool (manifeste et composant).
    Validate { archive: PathBuf },
    /// Affiche le manifeste d'une archive en JSON.
    Describe { archive: PathBuf },
    /// Liste les opérations hôte disponibles et la permission que chacune demande.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum EngineCmd {
    /// Télécharge les archives de la release vX.Y.Z sur GitHub, vérifie leurs empreintes et les publie.
    Publish { version: String },
}

#[derive(Subcommand)]
enum KeysCmd {
    /// Liste les clés.
    List,
    /// Crée une clé (affichée une seule fois).
    Create {
        /// Nom de la personne ou de la machine.
        label: String,
        /// Donne tous les droits (supprimer un outil, publier le moteur, gérer les clés).
        #[arg(long)]
        admin: bool,
    },
    /// Révoque une clé.
    Revoke { id: String },
}

fn main() {
    if let Err(err) = run(Cli::parse()) {
        eprintln!("ktrx-dev : {err:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let config = Config::load()?;
    let api = Api::new(&config)?;
    match cli.command {
        Cmd::Login { registry } => login(registry.unwrap_or_else(|| config.registry())),
        Cmd::Logout => {
            let path = api::config_path()?;
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            println!("Clé supprimée de cette machine.");
            Ok(())
        }
        Cmd::Whoami => {
            let me = api.admin_json("GET", "/whoami", None)?;
            println!("{} ({}) sur {}", me["label"].as_str().unwrap_or("?"), scopes(&me["scopes"]), api.base);
            Ok(())
        }
        Cmd::Doctor => doctor(&api, &config),
        Cmd::New { myth, readable, category, dir } => {
            let dir = tool::create(&api, tool::NewTool { myth: &myth, readable: &readable, category: &category, dir })?;
            let name = myth.trim_start_matches("K~").to_ascii_lowercase();
            println!("Outil créé : {}", dir.display());
            println!("\nIl compile déjà. Ensuite :");
            println!("  1. code : src/lib.rs (fonction execute) et ses tests");
            println!("  2. interface : manifest.toml (description, [[arguments]], examples, permissions)");
            println!("  3. documentation : README.md, et icon.png (emblème par défaut fourni)");
            println!("  4. ktrx-dev check {name}     jusqu'à « Tout est bon »");
            println!("  5. ktrx-dev publish {name}");
            Ok(())
        }
        Cmd::Check { tool, offline } => {
            tool::check(&api, &tool::resolve(&tool)?, !offline)?;
            Ok(())
        }
        Cmd::Publish { tool, changelog, docs } => publish(&api, &tool, changelog, docs),
        Cmd::Yank { tool, version } => {
            let id = remote_id(&api, &tool)?;
            let out = api.admin_json("POST", &format!("/tools/{id}/versions/{version}/yank"), None)?;
            match out["latest_version"].as_str() {
                Some(latest) => println!("Version {version} retirée. Version installée désormais : {latest}."),
                None => println!("Version {version} retirée. Plus aucune version disponible : l'outil est masqué du site."),
            }
            Ok(())
        }
        Cmd::Delete { tool, yes } => {
            let id = remote_id(&api, &tool)?;
            if !yes && !confirm(&format!("Supprimer définitivement « {tool} » et toutes ses versions du registre ?"))? {
                bail!("annulé");
            }
            api.admin_json("DELETE", &format!("/tools/{id}"), None)?;
            println!("Outil « {tool} » supprimé du registre.");
            Ok(())
        }
        Cmd::Engine { action: EngineCmd::Publish { version } } => engine_publish(&api, &version),
        Cmd::Keys { action } => keys(&api, action),
        Cmd::Audit { limit } => {
            let out = api.admin_json("GET", &format!("/audit?limit={limit}"), None)?;
            for e in out["entries"].as_array().into_iter().flatten() {
                println!(
                    "{}  {:<14} {:<16} {:<22} {}",
                    e["at"].as_str().unwrap_or(""),
                    e["actor"].as_str().unwrap_or("?"),
                    e["action"].as_str().unwrap_or(""),
                    format!("{} {}", e["tool_id"].as_str().unwrap_or(""), e["version"].as_str().unwrap_or("")).trim(),
                    e["detail"].as_str().unwrap_or("")
                );
            }
            Ok(())
        }
        Cmd::Pack { manifest, wasm, output } => {
            tool::pack_files(&manifest, &wasm, &output)?;
            println!("Archive créée : {}", output.display());
            Ok(())
        }
        Cmd::Validate { archive } => {
            let package = ToolPackage::open(&archive)?;
            Runtime::new()?.validate(&package)?;
            println!("Valide : {} {} ({})", package.manifest.name, package.manifest.version, package.manifest.id);
            Ok(())
        }
        Cmd::Describe { archive } => {
            println!("{}", serde_json::to_string_pretty(&ToolPackage::open(&archive)?.manifest)?);
            Ok(())
        }
        Cmd::Capabilities { json } => {
            let capabilities = ktrx_runtime::capabilities();
            if json {
                println!("{}", serde_json::to_string_pretty(&capabilities)?);
            } else {
                println!("KTRX {} — API hôte {}\n", ktrx_core::ENGINE_VERSION, ktrx_core::API_VERSION);
                for c in capabilities {
                    let perms: Vec<_> = c.permissions.iter().map(|p| p.as_str()).collect();
                    println!("  {:20} {}", c.operation, if perms.is_empty() { "aucune".into() } else { perms.join(" ou ") });
                }
            }
            Ok(())
        }
    }
}

fn scopes(value: &serde_json::Value) -> String {
    value.as_array().into_iter().flatten().filter_map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
}

fn login(registry: String) -> Result<()> {
    let key = if io::stdin().is_terminal() {
        rpassword::prompt_password("Clé d'API KTRX (ktrx_…) : ")?
    } else {
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        line
    };
    let key = key.trim().to_owned();
    if !key.starts_with("ktrx_") {
        bail!("une clé KTRX commence par « ktrx_ »");
    }
    let api = Api::new(&Config { registry: Some(registry.clone()), key: Some(key.clone()) })?;
    let me = api.admin_json("GET", "/whoami", None)?;
    let path = Config::save(&registry, &key)?;
    println!("Connecté : {} ({}). Clé enregistrée dans {}", me["label"].as_str().unwrap_or("?"), scopes(&me["scopes"]), path.display());
    Ok(())
}

fn doctor(api: &Api, config: &Config) -> Result<()> {
    let mut problems = 0;
    let mut line = |ok: bool, what: &str, fix: &str| {
        println!("  {} {what}", if ok { "✓" } else { "✗" });
        if !ok {
            problems += 1;
            println!("      → {fix}");
        }
    };
    let cargo = std::process::Command::new("cargo").arg("--version").output().is_ok_and(|o| o.status.success());
    line(cargo, "Rust (cargo)", "installer Rust : https://rustup.rs   (Windows : winget install Rustlang.Rustup)");
    let wasm = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-wasip2"));
    line(wasm, "cible wasm32-wasip2", "rustup target add wasm32-wasip2");
    let rustfmt = std::process::Command::new("cargo").args(["fmt", "--version"]).output().is_ok_and(|o| o.status.success());
    line(rustfmt, "rustfmt", "rustup component add rustfmt");
    line(tool::repo_root().is_some(), "dépôt KTRX (dossier courant)", "cloner le dépôt : git clone https://github.com/ktrxservor/KTRX && cd KTRX");
    let online = api.get_public("/api/engine").is_ok();
    line(online, &format!("registre joignable ({})", api.base), "vérifier la connexion Internet (HTTPS_PROXY derrière un proxy)");
    let logged = config.key.is_some() && api.admin_json("GET", "/whoami", None).is_ok();
    line(logged, "clé d'API valide", "ktrx-dev login");
    println!();
    if problems == 0 {
        println!("Tout est prêt.");
        Ok(())
    } else {
        bail!("{problems} point(s) à régler")
    }
}

fn publish(api: &Api, spec: &str, changelog: Option<String>, docs_only: bool) -> Result<()> {
    let dir = tool::resolve(spec)?;
    let checked = tool::check(api, &dir, true)?;
    let manifest = &checked.manifest;
    let name = tool::short_name(&manifest.name);
    println!("\nPublication");
    if !docs_only {
        let bytes = std::fs::read(&checked.archive)?;
        let mut path = "/tools".to_owned();
        if let Some(note) = &changelog {
            path.push_str(&format!("?changelog={}", url_encode(note)));
        }
        let out = api.admin_bytes("POST", &path, "application/zip", &bytes);
        match out {
            Ok(v) => println!("  ✓ {} {} publié (sha256 {}…)", manifest.name, manifest.version, &v["sha256"].as_str().unwrap_or("")[..12.min(v["sha256"].as_str().unwrap_or("").len())]),
            Err(e) if e.to_string().contains("déjà publiée") => {
                bail!("{e}\n  → pour mettre à jour seulement le README et l'emblème : ktrx-dev publish {name} --docs")
            }
            Err(e) => return Err(e),
        }
    } else if api.get_public(&format!("/api/tools/{}", manifest.id))?.is_none() {
        bail!("{} n'est pas encore publié : lancer `ktrx-dev publish {name}` sans --docs", manifest.name);
    }
    api.admin_bytes("PUT", &format!("/tools/{}/readme", manifest.id), "text/markdown; charset=utf-8", checked.readme.as_bytes())?;
    println!("  ✓ README");
    let icon = tool::web_icon(&checked.dir.join("icon.png"))?;
    api.admin_bytes("PUT", &format!("/tools/{}/icon", manifest.id), "image/png", &icon)?;
    println!("  ✓ emblème");
    println!("\nEn ligne : {}", api.site_url(&format!("/outils/{name}")));
    println!("Installation : ktrx install {name}");
    Ok(())
}

fn url_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The registry id of a tool, from its name, id or local folder.
fn remote_id(api: &Api, spec: &str) -> Result<String> {
    let key = tool::short_name(spec);
    let found = api.get_public(&format!("/api/tools/{key}"))?;
    let tool = found.with_context(|| format!("« {spec} » n'existe pas sur le registre"))?;
    Ok(tool["id"].as_str().unwrap_or_default().to_owned())
}

fn confirm(question: &str) -> Result<bool> {
    print!("{question} [o/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "o" | "oui" | "y" | "yes"))
}

const ENGINE_FILES: [(&str, &str); 4] = [
    ("ktrx-windows.zip", "windows"),
    ("ktrx-linux.tar.gz", "linux"),
    ("ktrx-linux-arm64.tar.gz", "linux-arm64"),
    ("ktrx-macos.tar.gz", "macos"),
];

fn engine_publish(api: &Api, version: &str) -> Result<()> {
    let version = version.trim_start_matches('v');
    if ktrx_core::parse_version(version).is_none() {
        bail!("version attendue : X.Y.Z");
    }
    let repo = std::env::var("KTRX_GITHUB_REPO").unwrap_or_else(|_| "ktrxservor/KTRX".to_owned());
    let base = format!("https://github.com/{repo}/releases/download/v{version}");
    println!("Release GitHub v{version} ({repo})");
    let sums = String::from_utf8(api.download(&format!("{base}/SHA256SUMS.txt")).context("SHA256SUMS.txt introuvable dans la release")?)?;
    for (file, platform) in ENGINE_FILES {
        let expected = sums
            .lines()
            .find_map(|l| {
                let mut parts = l.split_whitespace();
                let hash = parts.next()?;
                (parts.next()?.trim_start_matches('*') == file).then(|| hash.to_ascii_lowercase())
            })
            .with_context(|| format!("{file} absent de SHA256SUMS.txt"))?;
        let bytes = api.download(&format!("{base}/{file}"))?;
        let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if actual != expected {
            bail!("{file} : empreinte inattendue, publication annulée");
        }
        let path = format!("/engine?version={version}&platform={platform}&filename={file}");
        match api.admin_bytes("POST", &path, "application/octet-stream", &bytes) {
            Ok(_) => println!("  ✓ {platform:<12} {file} ({:.1} Mo)", bytes.len() as f64 / 1_048_576.0),
            Err(e) if e.to_string().contains("déjà publié") => println!("  • {platform:<12} déjà publié"),
            Err(e) => return Err(e),
        }
    }
    println!("\nLe site propose maintenant KTRX {version} : {}", api.site_url("/telecharger"));
    Ok(())
}

fn keys(api: &Api, action: KeysCmd) -> Result<()> {
    match action {
        KeysCmd::List => {
            let out = api.admin_json("GET", "/keys", None)?;
            for k in out["keys"].as_array().into_iter().flatten() {
                let state = if k["revoked_at"].is_string() { "révoquée" } else { "active" };
                println!(
                    "{}  {:<20} {:<8} {:<9} dernière utilisation : {}",
                    k["id"].as_str().unwrap_or(""),
                    k["label"].as_str().unwrap_or(""),
                    scopes(&k["scopes"]),
                    state,
                    k["last_used_at"].as_str().unwrap_or("jamais")
                );
            }
        }
        KeysCmd::Create { label, admin } => {
            let out = api.admin_json(
                "POST",
                "/keys",
                Some(serde_json::json!({ "label": label, "scope": if admin { "admin" } else { "publish" } })),
            )?;
            println!("Clé créée pour {} ({}) — id {}", label, out["scope"].as_str().unwrap_or(""), out["id"].as_str().unwrap_or(""));
            println!("\n  {}\n", out["key"].as_str().unwrap_or(""));
            println!("Elle ne sera plus jamais affichée. À transmettre à la personne, qui lance : ktrx-dev login");
        }
        KeysCmd::Revoke { id } => {
            api.admin_json("DELETE", &format!("/keys/{id}"), None)?;
            println!("Clé {id} révoquée.");
        }
    }
    Ok(())
}
