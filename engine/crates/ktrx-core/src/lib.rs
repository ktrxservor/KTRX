use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};
use thiserror::Error;

pub const API_VERSION: &str = "1";
/// Version of this engine build, checked against a manifest's `min_engine_version`.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
pub const MAX_MODULE_BYTES: u64 = 48 * 1024 * 1024;
/// Longest program name accepted in a manifest's `programs` list.
pub const MAX_PROGRAM_NAME: usize = 64;
/// Most programs a manifest may list.
pub const MAX_PROGRAMS: usize = 32;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub api_version: String,
    /// Optional per-invocation limit: absent or 0 means no deadline; 30–180 sets an explicit limit.
    #[serde(default)]
    pub execution_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Programs a tool with the `process` permission may start, as bare
    /// program names looked up on `PATH` (for example `["git", "nmap"]`).
    /// Empty means the tool may start any program, which the approval prompt
    /// states explicitly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<String>,
    /// Oldest engine release able to run this tool, as `MAJOR.MINOR.PATCH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_engine_version: Option<String>,
    #[serde(default)]
    pub arguments: Vec<Argument>,
    #[serde(default)]
    pub examples: Vec<String>,
}

impl Manifest {
    /// Whether the manifest declares `permission`.
    pub fn declares(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    /// DNS, TCP, TLS, and UDP communication with a destination chosen at run time.
    Network,
    /// Read files and inspect directories chosen at run time.
    FileRead,
    /// Create, write, and append to files, and create directories, inside a
    /// directory chosen by the user at run time.
    FileWrite,
    /// Privileged network primitives: ICMP echo, ARP resolution, and raw TCP
    /// probes. These may need elevated rights on the host operating system.
    RawNet,
    /// Start programs on the host, outside the sandbox, with the rights of the
    /// user running KTRX. Limited to the manifest's `programs` when it lists
    /// any, and further to the runner's `--allow-program` choices.
    Process,
}

impl Permission {
    /// Every permission, in the order the runner asks for approval.
    pub const ALL: [Permission; 5] = [
        Self::Network,
        Self::FileRead,
        Self::FileWrite,
        Self::RawNet,
        Self::Process,
    ];

    /// The identifier used in manifests.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::FileRead => "file-read",
            Self::FileWrite => "file-write",
            Self::RawNet => "raw-net",
            Self::Process => "process",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Argument {
    pub name: String,
    pub description: String,
    #[serde(default = "default_type")]
    pub value_type: String,
    #[serde(default)]
    pub required: bool,
    pub default: Option<toml::Value>,
    #[serde(default)]
    pub positional: bool,
}
fn default_type() -> String {
    "string".into()
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("cannot read .tool archive: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid .tool archive: {0}")]
    Archive(String),
    #[error("invalid manifest: {0}")]
    Manifest(String),
    #[error("unsupported KTRX API version {0}")]
    ApiVersion(String),
    #[error("this tool requires KTRX {required} or newer; this engine is {current}")]
    EngineVersion { required: String, current: String },
    #[error("invalid arguments: {0}")]
    Argument(String),
}

pub struct ToolPackage {
    pub manifest: Manifest,
    pub wasm: Vec<u8>,
}

impl ToolPackage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ToolError> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        if size > MAX_ARCHIVE_BYTES {
            return Err(ToolError::Archive("archive exceeds 64 MiB limit".into()));
        }
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| ToolError::Archive(e.to_string()))?;
        let mut manifest_bytes = Vec::new();
        let mf = archive
            .by_name("manifest.toml")
            .map_err(|_| ToolError::Archive("missing manifest.toml".into()))?;
        if mf.size() > MAX_MANIFEST_BYTES {
            return Err(ToolError::Archive("manifest exceeds 256 KiB limit".into()));
        }
        mf.take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut manifest_bytes)?;
        if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(ToolError::Archive("manifest exceeds 256 KiB limit".into()));
        }
        let manifest_text =
            std::str::from_utf8(&manifest_bytes).map_err(|e| ToolError::Manifest(e.to_string()))?;
        let manifest: Manifest =
            toml::from_str(manifest_text).map_err(|e| ToolError::Manifest(e.to_string()))?;
        validate_manifest(&manifest)?;
        let entry = archive
            .by_name("module.wasm")
            .map_err(|_| ToolError::Archive("missing module.wasm".into()))?;
        if entry.size() > MAX_MODULE_BYTES {
            return Err(ToolError::Archive("module exceeds 48 MiB limit".into()));
        }
        let mut wasm = Vec::with_capacity(entry.size() as usize);
        entry.take(MAX_MODULE_BYTES + 1).read_to_end(&mut wasm)?;
        if wasm.len() as u64 > MAX_MODULE_BYTES {
            return Err(ToolError::Archive("module exceeds 48 MiB limit".into()));
        }
        if !wasm.starts_with(b"\0asm") {
            return Err(ToolError::Archive(
                "module.wasm is not a WebAssembly binary".into(),
            ));
        }
        Ok(Self { manifest, wasm })
    }
}

pub fn validate_manifest(m: &Manifest) -> Result<(), ToolError> {
    let id_ok = !m.id.is_empty()
        && m.id.len() <= 80
        && m.id.as_bytes()[0].is_ascii_lowercase()
        && m.id.as_bytes()[m.id.len() - 1].is_ascii_alphanumeric()
        && m.id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !id_ok {
        return Err(ToolError::Manifest(
            "id must be 1-80 lowercase letters, digits, or hyphens".into(),
        ));
    }
    for (label, value) in [
        ("name", &m.name),
        ("description", &m.description),
        ("version", &m.version),
        ("author", &m.author),
    ] {
        if value.trim().is_empty() {
            return Err(ToolError::Manifest(format!("{label} must not be empty")));
        }
    }
    if m.api_version != API_VERSION {
        return Err(ToolError::ApiVersion(m.api_version.clone()));
    }
    if let Some(seconds) = m.execution_timeout_seconds
        && seconds != 0
        && !(30..=180).contains(&seconds)
    {
        return Err(ToolError::Manifest(
            "execution_timeout_seconds must be 0 (unlimited) or between 30 and 180".into(),
        ));
    }
    let mut declared = std::collections::HashSet::new();
    for permission in &m.permissions {
        if !declared.insert(permission.as_str()) {
            return Err(ToolError::Manifest(format!(
                "permission {} is declared more than once",
                permission.as_str()
            )));
        }
    }
    validate_programs(m)?;
    if let Some(required) = &m.min_engine_version {
        let wanted = parse_version(required).ok_or_else(|| {
            ToolError::Manifest("min_engine_version must be MAJOR.MINOR.PATCH".into())
        })?;
        let current = parse_version(ENGINE_VERSION).unwrap_or_default();
        if wanted > current {
            return Err(ToolError::EngineVersion {
                required: required.clone(),
                current: ENGINE_VERSION.into(),
            });
        }
    }
    let mut seen = std::collections::HashSet::new();
    for arg in &m.arguments {
        if arg.name.is_empty()
            || !arg
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !seen.insert(arg.name.clone())
        {
            return Err(ToolError::Manifest(format!(
                "invalid or duplicate argument {}",
                arg.name
            )));
        }
        if !["string", "integer", "boolean", "path"].contains(&arg.value_type.as_str()) {
            return Err(ToolError::Manifest(format!(
                "unsupported argument type {}",
                arg.value_type
            )));
        }
        if let Some(default) = &arg.default {
            let type_matches = match arg.value_type.as_str() {
                "integer" => default.is_integer(),
                "boolean" => default.is_bool(),
                "string" | "path" => default.is_str(),
                _ => false,
            };
            if !type_matches {
                return Err(ToolError::Manifest(format!(
                    "default value for {} does not match type {}",
                    arg.name, arg.value_type
                )));
            }
        }
    }
    Ok(())
}

/// Whether `name` is a bare program name: no directory part and only
/// characters that are valid in file names on every supported system.
/// Looking the name up on `PATH` is the runtime's job.
pub fn is_valid_program_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_PROGRAM_NAME
        && !name.starts_with(['.', '-'])
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
}

fn validate_programs(m: &Manifest) -> Result<(), ToolError> {
    if m.programs.is_empty() {
        return Ok(());
    }
    if !m.declares(Permission::Process) {
        return Err(ToolError::Manifest(
            "programs may only be listed by a tool that declares the process permission".into(),
        ));
    }
    if m.programs.len() > MAX_PROGRAMS {
        return Err(ToolError::Manifest(format!(
            "programs may list at most {MAX_PROGRAMS} entries"
        )));
    }
    let mut seen = std::collections::HashSet::new();
    for program in &m.programs {
        if !is_valid_program_name(program) {
            return Err(ToolError::Manifest(format!(
                "program {program:?} must be a bare name of at most {MAX_PROGRAM_NAME} letters, digits, '.', '_', '-' or '+'"
            )));
        }
        if !seen.insert(program.to_ascii_lowercase()) {
            return Err(ToolError::Manifest(format!(
                "program {program} is listed more than once"
            )));
        }
    }
    Ok(())
}

/// Parse `MAJOR.MINOR.PATCH`; a pre-release or build suffix is ignored.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

pub fn validate_arguments(m: &Manifest, args: &[String]) -> Result<serde_json::Value, ToolError> {
    let mut out = serde_json::Map::new();
    let positional: Vec<_> = m.arguments.iter().filter(|a| a.positional).collect();
    let mut pos = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(option) = args[i].strip_prefix("--") {
            // Accept both `--name value` and `--name=value`.
            let (key, inline) = match option.split_once('=') {
                Some((key, value)) => (key, Some(value.to_string())),
                None => (option, None),
            };
            let arg = m
                .arguments
                .iter()
                .find(|a| a.name == key)
                .ok_or_else(|| ToolError::Argument(format!("unknown option --{key}")))?;
            let value = match inline {
                Some(value) => value,
                None if arg.value_type == "boolean" => "true".to_string(),
                None => {
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| ToolError::Argument(format!("--{key} requires a value")))?
                }
            };
            out.insert(key.into(), convert(arg, &value)?);
        } else {
            pos.push(args[i].clone());
        }
        i += 1;
    }
    for (idx, arg) in positional.iter().enumerate() {
        if let Some(v) = pos.get(idx) {
            out.insert(arg.name.clone(), convert(arg, v)?);
        }
    }
    if pos.len() > positional.len() {
        return Err(ToolError::Argument("too many positional arguments".into()));
    }
    for arg in &m.arguments {
        if !out.contains_key(&arg.name) {
            if let Some(v) = &arg.default {
                out.insert(arg.name.clone(), toml_to_json(v.clone()));
            } else if arg.required {
                return Err(ToolError::Argument(format!(
                    "missing required argument {}",
                    arg.name
                )));
            }
        }
    }
    Ok(serde_json::Value::Object(out))
}
fn convert(arg: &Argument, value: &str) -> Result<serde_json::Value, ToolError> {
    match arg.value_type.as_str() {
        "integer" => value
            .parse::<i64>()
            .map(serde_json::Value::from)
            .map_err(|_| ToolError::Argument(format!("{} must be an integer", arg.name))),
        "boolean" => value
            .parse::<bool>()
            .map(serde_json::Value::from)
            .map_err(|_| ToolError::Argument(format!("{} must be true or false", arg.name))),
        _ => Ok(serde_json::Value::String(value.to_string())),
    }
}
fn toml_to_json(v: toml::Value) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_ids() {
        let m = Manifest {
            id: "Bad ID".into(),
            name: "N".into(),
            description: "D".into(),
            version: "1".into(),
            author: "A".into(),
            api_version: "1".into(),
            execution_timeout_seconds: None,
            permissions: vec![],
            arguments: vec![],
            examples: vec![],
            ..Manifest::default()
        };
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn parses_every_permission_and_rejects_duplicates() {
        let manifest: Manifest = toml::from_str(
            "id='demo'
name='Demo'
description='D'
version='1.0.0'
author='A'
api_version='1'
permissions=['network','file-read','file-write','raw-net','process']
",
        )
        .unwrap();
        let names: Vec<_> = manifest
            .permissions
            .iter()
            .map(Permission::as_str)
            .collect();
        assert_eq!(
            names,
            ["network", "file-read", "file-write", "raw-net", "process"]
        );
        assert_eq!(manifest.permissions, Permission::ALL);
        assert!(validate_manifest(&manifest).is_ok());

        let mut duplicated = manifest.clone();
        duplicated.permissions.push(Permission::Network);
        assert!(validate_manifest(&duplicated).is_err());

        assert!(
            toml::from_str::<Manifest>(
                "id='demo'
name='Demo'
description='D'
version='1.0.0'
author='A'
api_version='1'
permissions=['root']
"
            )
            .is_err()
        );
    }

    #[test]
    fn validates_the_program_list() {
        let mut manifest = basic_manifest();
        manifest.programs = vec!["git".into()];
        let error = validate_manifest(&manifest).unwrap_err().to_string();
        assert!(error.contains("process permission"), "{error}");

        manifest.permissions = vec![Permission::Process];
        assert!(validate_manifest(&manifest).is_ok());
        manifest.programs = vec!["nmap".into(), "g++".into(), "python3.12".into()];
        assert!(validate_manifest(&manifest).is_ok());

        for bad in [
            "",
            "../git",
            "bin/git",
            "C:\\git.exe",
            ".hidden",
            "-rf",
            "git status",
            "gît",
        ] {
            manifest.programs = vec![bad.into()];
            assert!(validate_manifest(&manifest).is_err(), "{bad:?}");
        }
        manifest.programs = vec!["Git".into(), "git".into()];
        assert!(validate_manifest(&manifest).is_err());
        manifest.programs = (0..=MAX_PROGRAMS).map(|i| format!("p{i}")).collect();
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn checks_the_minimum_engine_version() {
        assert_eq!(parse_version("0.4.0"), Some((0, 4, 0)));
        assert_eq!(parse_version("1.2.3-beta.1+build"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("one.two.three"), None);

        let mut manifest = basic_manifest();
        manifest.min_engine_version = Some(ENGINE_VERSION.into());
        assert!(validate_manifest(&manifest).is_ok());
        manifest.min_engine_version = Some("0.1.0".into());
        assert!(validate_manifest(&manifest).is_ok());
        manifest.min_engine_version = Some("999.0.0".into());
        assert!(matches!(
            validate_manifest(&manifest),
            Err(ToolError::EngineVersion { .. })
        ));
        manifest.min_engine_version = Some("latest".into());
        assert!(matches!(
            validate_manifest(&manifest),
            Err(ToolError::Manifest(_))
        ));
    }

    #[test]
    fn new_manifest_fields_are_optional_and_round_trip() {
        let manifest: Manifest = toml::from_str(
            "id='runner'
name='Runner'
description='D'
version='1.0.0'
author='A'
api_version='1'
permissions=['process']
programs=['git']
min_engine_version='0.4.0'
",
        )
        .unwrap();
        assert_eq!(manifest.programs, ["git"]);
        assert!(validate_manifest(&manifest).is_ok());
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(json["programs"], serde_json::json!(["git"]));
        assert_eq!(json["min_engine_version"], "0.4.0");
        // Older manifests serialise exactly as before.
        let plain = serde_json::to_value(basic_manifest()).unwrap();
        assert!(plain.get("programs").is_none());
        assert!(plain.get("min_engine_version").is_none());
    }

    #[test]
    fn validates_extended_execution_limits() {
        let mut manifest = basic_manifest();
        manifest.execution_timeout_seconds = Some(0);
        assert!(validate_manifest(&manifest).is_ok());
        manifest.execution_timeout_seconds = Some(180);
        assert!(validate_manifest(&manifest).is_ok());
        manifest.execution_timeout_seconds = Some(29);
        assert!(validate_manifest(&manifest).is_err());
        manifest.execution_timeout_seconds = Some(181);
        assert!(validate_manifest(&manifest).is_err());
    }

    fn basic_manifest() -> Manifest {
        Manifest {
            id: "demo".into(),
            name: "Demo".into(),
            description: "A demo".into(),
            version: "1.0.0".into(),
            author: "Tests".into(),
            api_version: "1".into(),
            execution_timeout_seconds: None,
            permissions: vec![],
            arguments: vec![
                Argument {
                    name: "count".into(),
                    description: "A count".into(),
                    value_type: "integer".into(),
                    required: true,
                    default: None,
                    positional: true,
                },
                Argument {
                    name: "verbose".into(),
                    description: "Verbose".into(),
                    value_type: "boolean".into(),
                    required: false,
                    default: None,
                    positional: false,
                },
            ],
            examples: vec![],
            ..Manifest::default()
        }
    }

    #[test]
    fn validates_and_converts_arguments() {
        let values =
            validate_arguments(&basic_manifest(), &["42".into(), "--verbose".into()]).unwrap();
        assert_eq!(values["count"], 42);
        assert_eq!(values["verbose"], true);
    }

    #[test]
    fn accepts_inline_option_values() {
        let values = validate_arguments(
            &basic_manifest(),
            &["--count=7".into(), "--verbose=false".into()],
        )
        .unwrap();
        assert_eq!(values["count"], 7);
        assert_eq!(values["verbose"], false);
        let error = validate_arguments(&basic_manifest(), &["--nope=1".into()]).unwrap_err();
        assert!(matches!(error, ToolError::Argument(_)), "{error}");
    }

    #[test]
    fn rejects_missing_and_mistyped_arguments() {
        let m = basic_manifest();
        assert!(validate_arguments(&m, &[]).is_err());
        assert!(validate_arguments(&m, &["many".into()]).is_err());
    }

    #[test]
    fn opens_a_valid_tool_archive() {
        use std::{io::Write, path::PathBuf};
        let path: PathBuf = std::env::temp_dir().join(format!(
            "ktrx-core-package-{}-{}.tool",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file = File::create(&path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        archive.start_file("manifest.toml", options).unwrap();
        archive.write_all(b"id='demo'\nname='Demo'\ndescription='Test'\nversion='1.0.0'\nauthor='Test'\napi_version='1'\n").unwrap();
        archive.start_file("module.wasm", options).unwrap();
        archive.write_all(b"\0asm\x01\0\0\0").unwrap();
        archive.finish().unwrap();
        let package = ToolPackage::open(&path).unwrap();
        assert_eq!(package.manifest.id, "demo");
        assert_eq!(package.wasm, b"\0asm\x01\0\0\0");
        std::fs::remove_file(path).unwrap();
    }
}
