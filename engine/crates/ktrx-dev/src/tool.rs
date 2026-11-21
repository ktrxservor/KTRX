//! Tool folders: create (`new`), verify and package (`check`), publish.

use crate::api::Api;
use anyhow::{Context, Result, anyhow, bail};
use ktrx_core::{Manifest, ToolPackage, validate_arguments, validate_manifest};
use ktrx_runtime::{Approvals, Runtime};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

pub const WIT: &str = include_str!("../../../wit/ktrx.wit");
const DEFAULT_ICON: &[u8] = include_bytes!("../assets/default-icon.png");
const TPL_CARGO: &str = include_str!("templates/Cargo.toml.tmpl");
const TPL_LIB: &str = include_str!("templates/lib.rs.tmpl");
const TPL_MANIFEST: &str = include_str!("templates/manifest.toml.tmpl");
const TPL_README: &str = include_str!("templates/README.md.tmpl");

pub const CATEGORIES: [&str; 4] = ["investigation", "analysis", "encoding", "generation"];
/// Placeholders left by `new`: `check` refuses to publish while one remains.
const TODO_MARKERS: [&str; 2] = ["À COMPLÉTER", "À REMPLACER"];

// ---------------------------------------------------------------------------------------------
// Locating tools

/// The KTRX checkout containing the current directory, if any.
pub fn repo_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("tools").is_dir() && dir.join("engine").join("wit").join("ktrx.wit").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// `anubis`, `K~ANUBIS`, `hash-audit` or a path to a tool folder.
pub fn resolve(spec: &str) -> Result<PathBuf> {
    let path = Path::new(spec);
    if path.join("manifest.toml").is_file() {
        return Ok(path.to_path_buf());
    }
    let wanted = short_name(spec);
    let mut places = vec![std::env::current_dir()?];
    if let Some(root) = repo_root() {
        places.insert(0, root.join("tools"));
    }
    for place in places {
        for entry in fs::read_dir(&place).into_iter().flatten().flatten() {
            let dir = entry.path();
            let Ok(text) = fs::read_to_string(dir.join("manifest.toml")) else { continue };
            let Ok(raw) = toml::from_str::<toml::Value>(&text) else { continue };
            let name = raw.get("name").and_then(|v| v.as_str()).unwrap_or_default();
            let id = raw.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            if short_name(name) == wanted || id == wanted {
                return Ok(dir);
            }
        }
    }
    bail!("aucun outil « {spec} » trouvé (dans tools/ du dépôt ni dans le dossier courant)")
}

pub fn short_name(name: &str) -> String {
    name.trim().trim_start_matches("K~").trim_start_matches("k~").to_ascii_lowercase()
}

// ---------------------------------------------------------------------------------------------
// new

pub struct NewTool<'a> {
    pub myth: &'a str,
    pub readable: &'a str,
    pub category: &'a str,
    pub dir: Option<PathBuf>,
}

pub fn create(api: &Api, opts: NewTool) -> Result<PathBuf> {
    let myth = opts.myth.trim().trim_start_matches("K~").to_ascii_uppercase();
    if myth.is_empty() || !myth.chars().all(|c| c.is_ascii_uppercase()) {
        bail!("le nom mythologique doit contenir uniquement des lettres A-Z (ex. NEITH)");
    }
    let readable = opts.readable.trim().split_whitespace().collect::<Vec<_>>().join("-");
    if readable.is_empty() || !readable.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("le nom lisible ne doit contenir que des lettres, chiffres et tirets (ex. JSON-Inspector)");
    }
    if !CATEGORIES.contains(&opts.category) {
        bail!("catégorie inconnue « {} » : {}", opts.category, CATEGORIES.join(", "));
    }
    let id = readable.to_ascii_lowercase();
    let name = myth.to_ascii_lowercase();

    // Name and id must be free, locally and on the registry.
    if resolve(&name).is_ok() || resolve(&id).is_ok() {
        bail!("un outil K~{myth} ou « {id} » existe déjà localement");
    }
    for key in [&name, &id] {
        if let Some(t) = api.get_public(&format!("/api/tools/{key}"))? {
            bail!("« {key} » est déjà pris sur le registre par {}", t["myth_name"].as_str().unwrap_or("un autre outil"));
        }
    }

    let parent = match opts.dir {
        Some(d) => d,
        None => repo_root().map(|r| r.join("tools")).unwrap_or(std::env::current_dir()?),
    };
    let dir = parent.join(format!("{myth} ( {readable} )"));
    if dir.exists() {
        bail!("{} existe déjà", dir.display());
    }
    let author = git_user().unwrap_or_else(|| "KTRX".to_owned());
    let mut title = myth.to_ascii_lowercase();
    title[..1].make_ascii_uppercase();
    let fill = |t: &str| {
        t.replace("{{ID}}", &id)
            .replace("{{MYTH}}", &myth)
            .replace("{{MYTH_TITLE}}", &title)
            .replace("{{NAME}}", &name)
            .replace("{{READABLE}}", &readable)
            .replace("{{CATEGORY}}", opts.category)
            .replace("{{AUTHOR}}", &author)
    };
    fs::create_dir_all(dir.join("src"))?;
    fs::create_dir_all(dir.join("wit"))?;
    fs::write(dir.join("Cargo.toml"), fill(TPL_CARGO))?;
    fs::write(dir.join("src").join("lib.rs"), fill(TPL_LIB))?;
    fs::write(dir.join("manifest.toml"), fill(TPL_MANIFEST))?;
    fs::write(dir.join("README.md"), fill(TPL_README))?;
    fs::write(dir.join("wit").join("ktrx.wit"), WIT)?;
    fs::write(dir.join("icon.png"), DEFAULT_ICON)?;
    fs::write(dir.join(".gitignore"), "target/\nmodule.wasm\n*.tool\n")?;
    Ok(dir)
}

fn git_user() -> Option<String> {
    let out = Command::new("git").args(["config", "user.name"]).output().ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!name.is_empty()).then_some(name)
}

// ---------------------------------------------------------------------------------------------
// check

#[derive(Default)]
pub struct Report {
    failures: usize,
}

impl Report {
    fn ok(&mut self, what: &str) {
        println!("  ✓ {what}");
    }
    fn fail(&mut self, what: &str, fix: &str) {
        self.failures += 1;
        println!("  ✗ {what}");
        if !fix.is_empty() {
            for line in fix.lines() {
                println!("      → {line}");
            }
        }
    }
    fn note(&mut self, what: &str) {
        println!("  • {what}");
    }
    fn check(&mut self, condition: bool, what: &str, fix: &str) -> bool {
        if condition { self.ok(what) } else { self.fail(what, fix) }
        condition
    }
}

/// Everything `check` produced: the packaged archive and what describes it.
pub struct Checked {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub archive: PathBuf,
    pub readme: String,
}

pub fn check(api: &Api, dir: &Path, online: bool) -> Result<Checked> {
    let mut r = Report::default();
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    println!("Vérification de {}\n", dir.display());

    // ---- manifest
    println!("Manifeste");
    let text = fs::read_to_string(dir.join("manifest.toml")).context("manifest.toml illisible")?;
    let raw: toml::Value = toml::from_str(&text).map_err(|e| anyhow!("manifest.toml invalide : {e}"))?;
    let manifest: Manifest = toml::from_str(&text).map_err(|e| anyhow!("manifest.toml invalide : {e}"))?;
    match validate_manifest(&manifest) {
        Ok(()) => r.ok("champs obligatoires, id, permissions, arguments"),
        Err(e) => r.fail(&format!("règles du moteur : {e}"), ""),
    }
    let myth = manifest.name.trim_start_matches("K~").to_owned();
    let name = myth.to_ascii_lowercase();
    r.check(
        manifest.name.starts_with("K~") && !myth.is_empty() && myth.chars().all(|c| c.is_ascii_uppercase()),
        &format!("nom {}", manifest.name),
        "name = \"K~NOM\" avec NOM en lettres majuscules A-Z",
    );
    let folder = dir.file_name().and_then(|f| f.to_str()).unwrap_or_default();
    r.check(
        folder.starts_with(&format!("{myth} (")),
        &format!("dossier « {folder} »"),
        &format!("le dossier doit s'appeler « {myth} ( Nom-Lisible ) »"),
    );
    let category = raw.get("category").and_then(|v| v.as_str()).unwrap_or("");
    r.check(
        CATEGORIES.contains(&category),
        &format!("catégorie « {category} »"),
        &format!("category = l'une de : {}", CATEGORIES.join(", ")),
    );
    let description_ok = !manifest.description.is_empty() && manifest.description.chars().count() <= 300 && !has_todo(&manifest.description);
    r.check(description_ok, "description (1 à 2 phrases, sans « À COMPLÉTER »)", "description = ce que fait l'outil, 300 caractères maximum");
    let undocumented: Vec<_> = manifest.arguments.iter().filter(|a| a.description.trim().len() < 4).map(|a| a.name.clone()).collect();
    r.check(undocumented.is_empty(), "chaque argument a une description", &format!("à décrire : {}", undocumented.join(", ")));
    check_choices(&raw, &mut r);
    let cargo_version = fs::read_to_string(dir.join("Cargo.toml"))
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .and_then(|v| v.get("package")?.get("version")?.as_str().map(str::to_owned));
    r.check(
        cargo_version.as_deref() == Some(manifest.version.as_str()),
        &format!("version {} identique dans manifest.toml et Cargo.toml", manifest.version),
        "mettre la même version dans les deux fichiers",
    );
    r.check(!manifest.examples.is_empty(), "au moins un exemple", "examples = [\"ktrx run nom …\"]");

    // ---- README & emblem
    println!("\nDocumentation");
    let readme = fs::read_to_string(dir.join("README.md")).unwrap_or_default();
    check_readme(&readme, &manifest.name, &mut r);
    check_icon(&dir.join("icon.png"), &mut r);
    let source = fs::read_to_string(dir.join("src").join("lib.rs")).unwrap_or_default();
    r.check(!has_todo(&source), "code sans « À REMPLACER »", "remplacer la logique d'exemple dans src/lib.rs");

    // ---- registry
    if online {
        println!("\nRegistre");
        match api.get_public(&format!("/api/tools/{}", manifest.id)) {
            Ok(None) => {
                match api.get_public(&format!("/api/tools/{name}")) {
                    Ok(Some(other)) => r.fail(
                        &format!("le nom {} est déjà pris par l'outil « {} »", manifest.name, other["id"].as_str().unwrap_or("?")),
                        "choisir un autre nom mythologique",
                    ),
                    _ => r.ok("nouvel outil : id et nom libres"),
                }
            }
            Ok(Some(existing)) => {
                let theirs = existing["myth_name"].as_str().unwrap_or_default();
                if theirs != manifest.name {
                    r.fail(&format!("l'id « {} » appartient déjà à {theirs}", manifest.id), "choisir un autre id");
                } else {
                    let published: Vec<String> = existing["versions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v["version"].as_str().map(str::to_owned))
                        .collect();
                    let latest = existing["latest_version"].as_str().unwrap_or("0.0.0").to_owned();
                    if published.contains(&manifest.version) {
                        r.note(&format!("version {} déjà publiée : `publish --docs` mettra seulement à jour README et emblème", manifest.version));
                    } else {
                        r.check(
                            newer(&manifest.version, &latest),
                            &format!("version {} > {latest} (en ligne)", manifest.version),
                            "incrémenter version (manifest.toml et Cargo.toml)",
                        );
                    }
                }
            }
            Err(e) => r.fail(&format!("registre injoignable : {e}"), "vérifier la connexion, ou `check --offline`"),
        }
    }

    // ---- build
    println!("\nCompilation");
    let toolchain_ok = check_toolchain(&mut r);
    let archive = dir.join(format!("{}.tool", manifest.name));
    let mut packaged = None;
    if toolchain_ok {
        if run_step(&dir, "cargo", &["fmt", "--check"], "formatage (cargo fmt --check)", "lancer `cargo fmt` dans le dossier de l'outil", &mut r, true)
            && run_step(&dir, "cargo", &["test", "--quiet"], "tests (cargo test)", "corriger les tests ci-dessus", &mut r, false)
            && run_step(&dir, "cargo", &["build", "--release", "--target", "wasm32-wasip2", "--quiet"], "compilation WebAssembly (wasm32-wasip2)", "corriger les erreurs ci-dessus", &mut r, false)
        {
            match package(&dir, &archive) {
                Ok(package) => {
                    r.ok(&format!("archive {} ({} Ko)", archive.file_name().unwrap().to_string_lossy(), fs::metadata(&archive)?.len() / 1024));
                    match Runtime::new().and_then(|rt| rt.validate(&package)) {
                        Ok(()) => {
                            r.ok("composant WebAssembly valide pour le moteur");
                            packaged = Some(package);
                        }
                        Err(e) => r.fail(&format!("composant refusé par le moteur : {e:#}"), "vérifier wit_bindgen::generate!({ path: \"wit\", world: \"ktrx-tool\" })"),
                    }
                }
                Err(e) => r.fail(&format!("paquetage : {e:#}"), ""),
            }
        }
    }

    // ---- examples
    if let Some(package) = packaged {
        println!("\nExemples");
        run_examples(&package, &name, &mut r);
    }

    println!();
    if r.failures > 0 {
        bail!("{} point(s) à corriger avant publication", r.failures);
    }
    println!("Tout est bon : {} {} est prêt à être publié.", manifest.name, manifest.version);
    Ok(Checked { dir: dir.to_path_buf(), manifest, archive, readme })
}

fn has_todo(text: &str) -> bool {
    TODO_MARKERS.iter().any(|m| text.contains(m))
}

pub fn newer(candidate: &str, than: &str) -> bool {
    match (ktrx_core::parse_version(candidate), ktrx_core::parse_version(than)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn check_choices(raw: &toml::Value, r: &mut Report) {
    let mut bad = Vec::new();
    for arg in raw.get("arguments").and_then(|a| a.as_array()).into_iter().flatten() {
        let Some(choices) = arg.get("choices").and_then(|c| c.as_array()) else { continue };
        if let Some(default) = arg.get("default") {
            let default = default.as_str().map(str::to_owned).unwrap_or_else(|| default.to_string());
            if !choices.iter().any(|c| c.as_str() == Some(default.as_str())) {
                bad.push(arg.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_owned());
            }
        }
    }
    if !bad.is_empty() {
        r.fail(&format!("valeur par défaut hors de « choices » : {}", bad.join(", ")), "le default doit être l'une des choices");
    }
}

fn check_readme(readme: &str, myth_name: &str, r: &mut Report) {
    if readme.is_empty() {
        r.fail("README.md présent", "créer README.md (voir DEVELOPING.md, « README d'un outil »)");
        return;
    }
    let mut lines = readme.lines().filter(|l| !l.trim().is_empty());
    let title = lines.next().unwrap_or_default();
    r.check(
        title.starts_with(&format!("# {myth_name} — ")),
        "README : titre « # K~NOM — Nom lisible »",
        &format!("première ligne : # {myth_name} — Nom lisible"),
    );
    r.check(
        lines.next().is_some_and(|l| l.starts_with("> ")),
        "README : ligne mythologique en citation sous le titre",
        "deuxième ligne : > Figure, qui elle est : ce que fait l'outil.",
    );
    let lower = readme.to_ascii_lowercase();
    r.check(
        !lower.contains("<img") && !readme.contains("![") && !lower.contains("<p"),
        "README : sans image ni HTML",
        "retirer les images et balises HTML (le site affiche l'emblème)",
    );
    r.check(readme.matches("\n## ").count() >= 3, "README : sections ## (Présentation, Utilisation, Sortie…)", "suivre le modèle de README");
    r.check(!has_todo(readme), "README : sans « À COMPLÉTER »", "remplir les parties « À COMPLÉTER »");
}

fn check_icon(path: &Path, r: &mut Report) {
    match fs::read(path).ok().and_then(|b| png_size(&b)) {
        None => r.fail("emblème icon.png", "ajouter un PNG carré (1024 × 1024 recommandé)"),
        Some((w, h)) => {
            r.check(w == h && w >= 256, &format!("emblème icon.png ({w} × {h})"), "PNG carré d'au moins 256 px (1024 recommandé)");
        }
    }
}

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    Some((u32::from_be_bytes(bytes[16..20].try_into().ok()?), u32::from_be_bytes(bytes[20..24].try_into().ok()?)))
}

fn check_toolchain(r: &mut Report) -> bool {
    let cargo = Command::new("cargo").arg("--version").output().is_ok_and(|o| o.status.success());
    if !cargo {
        r.fail(
            "Rust (cargo) installé",
            "installer Rust : https://rustup.rs (Windows : winget install Rustlang.Rustup)\npuis : rustup target add wasm32-wasip2",
        );
        return false;
    }
    let targets = Command::new("rustup").args(["target", "list", "--installed"]).output();
    match targets {
        Ok(out) if String::from_utf8_lossy(&out.stdout).contains("wasm32-wasip2") => {
            r.ok("Rust et la cible wasm32-wasip2");
            true
        }
        Ok(_) => {
            r.fail("cible wasm32-wasip2 installée", "rustup target add wasm32-wasip2");
            false
        }
        Err(_) => {
            r.note("rustup absent : la cible wasm32-wasip2 n'a pas pu être vérifiée");
            true
        }
    }
}

/// Runs a command in the tool folder; shows its output only when it fails.
fn run_step(dir: &Path, program: &str, args: &[&str], what: &str, fix: &str, r: &mut Report, optional: bool) -> bool {
    let output = Command::new(program).args(args).current_dir(dir).output();
    match output {
        Ok(out) if out.status.success() => {
            r.ok(what);
            true
        }
        Ok(out) => {
            if optional && String::from_utf8_lossy(&out.stderr).contains("not installed") {
                r.note(&format!("{what} : rustfmt absent, ignoré (rustup component add rustfmt)"));
                return true;
            }
            r.fail(what, fix);
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            for line in text.lines().take(60) {
                println!("        {line}");
            }
            false
        }
        Err(e) => {
            r.fail(&format!("{what} : {e}"), fix);
            false
        }
    }
}

/// Builds `K~NAME.tool` from manifest.toml and the compiled component.
pub fn package(dir: &Path, archive: &Path) -> Result<ToolPackage> {
    let cargo: toml::Value = toml::from_str(&fs::read_to_string(dir.join("Cargo.toml"))?)?;
    let crate_name = cargo
        .get("lib")
        .and_then(|l| l.get("name"))
        .or_else(|| cargo.get("package").and_then(|p| p.get("name")))
        .and_then(|n| n.as_str())
        .context("Cargo.toml sans [package] name")?
        .replace('-', "_");
    let wasm = find_wasm(dir, &crate_name)?;
    pack_files(&dir.join("manifest.toml"), &wasm, archive)?;
    Ok(ToolPackage::open(archive)?)
}

/// target/ may be local or shared (CARGO_TARGET_DIR, or a parent workspace).
fn find_wasm(dir: &Path, crate_name: &str) -> Result<PathBuf> {
    let file = format!("wasm32-wasip2/release/{crate_name}.wasm");
    let mut candidates = vec![dir.join("target").join(&file)];
    if let Some(shared) = std::env::var_os("CARGO_TARGET_DIR") {
        candidates.insert(0, PathBuf::from(shared).join(&file));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow!("{file} introuvable après compilation"))
}

pub fn pack_files(manifest: &Path, wasm: &Path, output: &Path) -> Result<()> {
    let manifest_data = fs::read(manifest).with_context(|| format!("reading {}", manifest.display()))?;
    let parsed: Manifest = toml::from_str(std::str::from_utf8(&manifest_data)?)?;
    validate_manifest(&parsed)?;
    let module = fs::read(wasm).with_context(|| format!("reading {}", wasm.display()))?;
    if !module.starts_with(b"\0asm") {
        bail!("{} n'est pas un module WebAssembly", wasm.display());
    }
    let tmp = output.with_extension("tool.tmp");
    let mut zip = zip::ZipWriter::new(fs::File::create(&tmp)?);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("manifest.toml", options)?;
    zip.write_all(&manifest_data)?;
    zip.start_file("module.wasm", options)?;
    zip.write_all(&module)?;
    zip.finish()?;
    fs::rename(&tmp, output)?;
    Ok(())
}

fn run_examples(package: &ToolPackage, name: &str, r: &mut Report) {
    let runtime = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return r.fail(&format!("moteur : {e}"), ""),
    };
    let needs_access = !package.manifest.permissions.is_empty();
    for example in &package.manifest.examples {
        let args = match example_args(example, name) {
            Ok(a) => a,
            Err(e) => {
                r.fail(&format!("« {example} » : {e}"), &format!("forme attendue : ktrx run {name} …"));
                continue;
            }
        };
        if let Err(e) = validate_arguments(&package.manifest, &args) {
            r.fail(&format!("« {example} » : {e}"), "corriger l'exemple ou les [[arguments]]");
            continue;
        }
        if needs_access {
            r.ok(&format!("« {example} » : arguments valides (non exécuté : l'outil demande des permissions)"));
            continue;
        }
        let input = validate_arguments(&package.manifest, &args).unwrap().to_string();
        match runtime.run(package, &input, Approvals::default()) {
            Ok(out) => match serde_json::from_str::<Value>(&out) {
                Ok(v) if v.get("error").is_some() => r.fail(&format!("« {example} » renvoie une erreur : {}", v["error"]), "un exemple doit réussir"),
                Ok(_) => r.ok(&format!("« {example} » s'exécute et renvoie du JSON")),
                Err(_) => r.fail(&format!("« {example} » ne renvoie pas du JSON"), "run doit renvoyer un texte JSON"),
            },
            Err(e) => r.fail(&format!("« {example} » : {e:#}"), ""),
        }
    }
}

/// `ktrx run [-y] [--write-dir D] [--allow-program P] <name> args…` → args.
fn example_args(example: &str, name: &str) -> Result<Vec<String>> {
    let words = split_words(example)?;
    let mut it = words.into_iter().peekable();
    if it.next().as_deref() != Some("ktrx") || it.next().as_deref() != Some("run") {
        bail!("doit commencer par « ktrx run »");
    }
    while let Some(word) = it.peek() {
        match word.as_str() {
            "-y" | "--assume-yes" => {
                it.next();
            }
            "--write-dir" | "--allow-program" => {
                it.next();
                it.next();
            }
            _ => break,
        }
    }
    let tool = it.next().context("nom d'outil manquant")?;
    if short_name(tool.trim_end_matches(".tool").rsplit(['/', '\\']).next().unwrap_or("")) != name {
        bail!("l'exemple lance « {tool} » au lieu de « {name} »");
    }
    Ok(it.filter(|w| w != "--json" && w != "--events").collect())
}

/// Shell-like word splitting: '…', "…" and backslash escapes.
pub fn split_words(line: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(x) => current.push(x),
                        None => bail!("guillemet ' non fermé"),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => current.push(chars.next().unwrap_or('\\')),
                        Some(x) => current.push(x),
                        None => bail!("guillemet \" non fermé"),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(x) = chars.next() {
                    current.push(x);
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                current.push(c);
            }
        }
    }
    if in_word {
        words.push(current);
    }
    Ok(words)
}

/// The tool's source as a zip (rooted in the tool folder's name): no build output, no archive.
pub fn source_zip(dir: &Path) -> Result<Vec<u8>> {
    let root = dir.file_name().and_then(|n| n.to_str()).context("nom de dossier illisible")?.to_owned();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&folder)?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                    stack.push(path);
                }
                continue;
            }
            if name == "module.wasm" || name.ends_with(".tool") || name.ends_with(".tool.tmp") {
                continue;
            }
            let relative = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
            zip.start_file(format!("{root}/{relative}"), options)?;
            zip.write_all(&fs::read(&path)?)?;
        }
    }
    Ok(zip.finish()?.into_inner())
}

/// Extracts a source zip into `parent`, refusing paths that would leave it. Returns the tool folder.
pub fn extract_source(bytes: &[u8], parent: &Path) -> Result<PathBuf> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("archive de sources illisible")?;
    let mut root: Option<PathBuf> = None;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let Some(relative) = file.enclosed_name() else { bail!("chemin dangereux dans l'archive : {}", file.name()) };
        let first = relative.components().next().context("entrée vide")?.as_os_str().to_owned();
        match &root {
            None => root = Some(PathBuf::from(&first)),
            Some(r) if r.as_os_str() != first => bail!("l'archive contient plusieurs dossiers racine"),
            _ => {}
        }
        if index == 0 && parent.join(&first).exists() {
            bail!("{} existe déjà : choisir un autre dossier avec --dir", parent.join(&first).display());
        }
        let target = parent.join(&relative);
        if file.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            fs::create_dir_all(target.parent().unwrap())?;
            let mut out = fs::File::create(&target)?;
            std::io::copy(&mut file, &mut out)?;
        }
    }
    Ok(parent.join(root.context("archive vide")?))
}

/// The emblem as sent to the site: a 320 px PNG.
pub fn web_icon(path: &Path) -> Result<Vec<u8>> {
    let image = image::open(path).with_context(|| format!("lecture de {}", path.display()))?;
    let small = image.resize(320, 320, image::imageops::FilterType::Lanczos3);
    let mut out = std::io::Cursor::new(Vec::new());
    small.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}
