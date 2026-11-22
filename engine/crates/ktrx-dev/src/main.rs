//! ktrx-dev — the KTRX team tool. Creates, checks and publishes tools to the registry, publishes
//! engine releases. Not distributed to users: the public `ktrx`
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
    /// Enregistre le mot de passe de l'équipe sur cette machine (saisie masquée, ou lu sur l'entrée standard, ou `--password`).
    Login {
        /// Le mot de passe (sinon il est demandé, ou lu sur l'entrée standard).
        #[arg(long)]
        password: Option<String>,
        /// Registre à utiliser (par défaut : le registre officiel).
        #[arg(long)]
        registry: Option<String>,
    },
    /// Supprime le mot de passe enregistré sur cette machine.
    Logout,
    /// Vérifie que le mot de passe enregistré est accepté par le registre.
    Whoami,
    /// Vérifie que la machine est prête : Rust, cible wasm32-wasip2, connexion (le mot de passe n'est requis que pour publier).
    Doctor {
        /// Installe ce qui peut l'être seul (cible wasm32-wasip2, rustfmt).
        #[arg(long)]
        fix: bool,
    },
    /// Affiche le guide développeur (le même que celui du dépôt, embarqué dans ce programme).
    Guide {
        /// Numéro de section à afficher (ex. 3) ; sans argument, tout le guide. `--list` liste les sections.
        section: Option<String>,
        #[arg(long)]
        list: bool,
    },
    /// Crée AGENTS.md et CLAUDE.md dans le dossier courant : les consignes d'un dossier de travail pour une IA.
    Init,
    /// Récupère les sources d'un outil publié (sur n'importe quelle machine) pour le modifier.
    Get {
        /// Nom (neith), K~NEITH ou id.
        tool: String,
        /// Dossier parent (par défaut : le dossier courant).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
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
    /// Supprime un outil du registre, toutes versions comprises.
    Delete {
        tool: String,
        /// Confirme sans question.
        #[arg(long)]
        yes: bool,
    },
    /// Publie sur le site les binaires d'une release GitHub du moteur.
    Engine {
        #[command(subcommand)]
        action: EngineCmd,
    },
    /// Dernières actions faites sur le registre.
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
        Cmd::Login { registry, password } => login(registry.unwrap_or_else(|| config.registry()), password),
        Cmd::Logout => {
            let path = api::config_path()?;
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            println!("Mot de passe supprimé de cette machine.");
            Ok(())
        }
        Cmd::Whoami => {
            api.admin_json("GET", "/whoami", None)?;
            println!("Mot de passe accepté par {}", api.base);
            Ok(())
        }
        Cmd::Doctor { fix } => doctor(&api, &config, fix),
        Cmd::Guide { section, list } => guide(section.as_deref(), list),
        Cmd::Init => init(),
        Cmd::Get { tool, dir } => get(&api, &tool, dir),
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
        Cmd::Audit { limit } => {
            let out = api.admin_json("GET", &format!("/audit?limit={limit}"), None)?;
            for e in out["entries"].as_array().into_iter().flatten() {
                println!(
                    "{}  {:<14} {:<16} {:<22} {}",
                    e["at"].as_str().unwrap_or(""),
                    e["ip"].as_str().unwrap_or("?"),
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

fn login(registry: String, password: Option<String>) -> Result<()> {
    let password = match password {
        Some(p) => p,
        None if io::stdin().is_terminal() => rpassword::prompt_password("Mot de passe KTRX : ")?,
        None => {
            let mut line = String::new();
            io::stdin().lock().read_line(&mut line)?;
            line
        }
    };
    let password = password.trim().to_owned();
    if password.is_empty() {
        bail!("mot de passe vide");
    }
    let api = Api::new(&Config { registry: Some(registry.clone()), password: Some(password.clone()) })?;
    api.admin_json("GET", "/whoami", None)?;
    let path = Config::save(&registry, &password)?;
    println!("Connecté. Mot de passe enregistré dans {}", path.display());
    Ok(())
}

fn doctor(api: &Api, config: &Config, fix: bool) -> Result<()> {
    if fix {
        for args in [["target", "add", "wasm32-wasip2"], ["component", "add", "rustfmt"]] {
            let ok = std::process::Command::new("rustup").args(args).status().is_ok_and(|s| s.success());
            if !ok {
                println!("  (rustup indisponible : `rustup {}` n'a pas pu être lancé)", args.join(" "));
            }
        }
        println!();
    }
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
    line(wasm, "cible wasm32-wasip2", "rustup target add wasm32-wasip2   (ou : ktrx-dev doctor --fix)");
    let rustfmt = std::process::Command::new("cargo").args(["fmt", "--version"]).output().is_ok_and(|o| o.status.success());
    line(rustfmt, "rustfmt", "rustup component add rustfmt");
    let online = api.get_public("/api/engine").is_ok();
    line(online, &format!("registre joignable ({})", api.base), "vérifier la connexion Internet (HTTPS_PROXY derrière un proxy)");
    // Only publishing needs the password: report it without counting it as a problem.
    let logged = config.password.is_some() && api.admin_json("GET", "/whoami", None).is_ok();
    println!("  {} mot de passe enregistré{}", if logged { "✓" } else { "·" }, if logged { "" } else { " (non : requis seulement pour publier, `ktrx-dev login`)" });
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
    let source = tool::source_zip(&checked.dir)?;
    api.admin_bytes("PUT", &format!("/tools/{}/source?version={}", manifest.id, manifest.version), "application/zip", &source)?;
    println!("  ✓ sources ({} Ko) : récupérables sur toute machine avec `ktrx-dev get {name}`", source.len() / 1024);
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

/// Release archives and the registry platform each one is stored under (ktrx-dev as `dev-<platform>`).
const ENGINE_FILES: [(&str, &str); 8] = [
    ("ktrx-windows.zip", "windows"),
    ("ktrx-linux.tar.gz", "linux"),
    ("ktrx-linux-arm64.tar.gz", "linux-arm64"),
    ("ktrx-macos.tar.gz", "macos"),
    ("ktrx-dev-windows.zip", "dev-windows"),
    ("ktrx-dev-linux.tar.gz", "dev-linux"),
    ("ktrx-dev-linux-arm64.tar.gz", "dev-linux-arm64"),
    ("ktrx-dev-macos.tar.gz", "dev-macos"),
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
            Ok(_) => println!("  ✓ {platform:<16} {file} ({:.1} Mo)", bytes.len() as f64 / 1_048_576.0),
            Err(e) if e.to_string().contains("déjà publié") => println!("  • {platform:<16} déjà publié"),
            Err(e) => return Err(e),
        }
    }
    println!("\nLe site propose maintenant KTRX {version} : {}", api.site_url("/telecharger"));
    Ok(())
}

/// The developer guide, embedded at build time so it always matches this ktrx-dev.
const GUIDE: &str = include_str!("../../../../DEVELOPING.md");

fn guide(section: Option<&str>, list: bool) -> Result<()> {
    // Titles of the guide, ignoring lines inside code blocks (the README template has its own).
    let mut headings: Vec<(usize, &str)> = Vec::new();
    let mut fence: Option<usize> = None;
    for (index, line) in GUIDE.lines().enumerate() {
        let ticks = line.chars().take_while(|c| *c == '`').count();
        if ticks >= 3 {
            match fence {
                None => fence = Some(ticks),
                Some(open) if ticks >= open => fence = None,
                _ => {}
            }
        } else if fence.is_none() && line.starts_with("## ") {
            headings.push((index, line.trim_start_matches("## ")));
        }
    }
    if list {
        for (_, title) in &headings {
            println!("{title}");
        }
        return Ok(());
    }
    let Some(section) = section else {
        print!("{GUIDE}");
        return Ok(());
    };
    let wanted = section.trim_end_matches('.');
    let lines: Vec<&str> = GUIDE.lines().collect();
    let found = headings.iter().position(|(_, t)| t.split('.').next() == Some(wanted));
    let Some(position) = found else {
        bail!("section « {section} » introuvable : `ktrx-dev guide --list` donne la liste");
    };
    let start = headings[position].0;
    let end = headings.get(position + 1).map(|(i, _)| *i).unwrap_or(lines.len());
    println!("{}", lines[start..end].join("\n"));
    Ok(())
}

const AGENT_NOTES: &str = "# Dossier de travail KTRX

Ce dossier sert à créer des outils pour KTRX avec `ktrx-dev` (installé ; le mot de passe de l'équipe, demandé à l'utilisateur, n'est nécessaire qu'à la publication : `ktrx-dev login`).

- Lis le guide complet : `ktrx-dev guide` (ou une section : `ktrx-dev guide 3`, liste : `ktrx-dev guide --list`).
- Créer un outil : `ktrx-dev new NOM Nom-Lisible --category <investigation|analysis|encoding|generation>`, remplir les « À COMPLÉTER » et « À REMPLACER », puis `ktrx-dev check NOM` jusqu'à « Tout est bon », puis `ktrx-dev publish NOM`.
- Modifier un outil existant : `ktrx-dev get NOM` récupère ses sources, puis incrémenter `version` (manifest.toml ET Cargo.toml), `check`, `publish`.
- Une version publiée ne change jamais. Ne mets jamais le mot de passe dans un fichier. Ne contourne aucun contrôle de `check`.
- Un outil = un projet Rust indépendant (cible wasm32-wasip2), sans aucun accès implicite : tout accès réseau, fichier ou programme passe par le moteur (guide, section 9).
";

fn init() -> Result<()> {
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let path = std::path::Path::new(name);
        if path.exists() {
            println!("  • {name} existe déjà : conservé");
        } else {
            std::fs::write(path, AGENT_NOTES)?;
            println!("  ✓ {name} créé");
        }
    }
    println!("\nLe dossier est prêt. Une IA lancée ici sait par où commencer : `ktrx-dev guide`.");
    Ok(())
}

fn get(api: &Api, spec: &str, dir: Option<PathBuf>) -> Result<()> {
    let key = tool::short_name(spec);
    let bytes = api.admin_get_bytes(&format!("/tools/{key}/source"))?;
    let parent = match dir {
        Some(d) => d,
        None => std::env::current_dir()?,
    };
    let folder = tool::extract_source(&bytes, &parent)?;
    println!("Sources de {spec} récupérées dans {}", folder.display());
    println!("Pour publier une modification : incrémenter `version` (manifest.toml ET Cargo.toml), puis `ktrx-dev publish {key}`.");
    Ok(())
}
