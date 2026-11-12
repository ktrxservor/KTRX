//! Generic, permission-gated file primitives exposed to sandboxed tools.
//!
//! Reads (`file.read`, `file.list`, `file.stat`) follow the `file-read`
//! permission and may address any path the user approves for the run. Writes
//! (`file.create`, `file.write`, `file.append`, `file.mkdir`) follow the
//! `file-write` permission and are confined to one directory tree chosen by
//! the user when the run starts, so a tool can never write outside it, whatever
//! path it asks for. Formats, reports, and wordlists belong to the tools.

use base64::Engine as _;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

pub(crate) const READ_OPERATIONS: [&str; 3] = ["file.read", "file.list", "file.stat"];
pub(crate) const WRITE_OPERATIONS: [&str; 4] =
    ["file.create", "file.write", "file.append", "file.mkdir"];
/// Closes a handle opened by a read or write operation.
pub(crate) const CLOSE_OPERATION: &str = "file.close";

const MAX_CHUNK: u64 = 64 * 1024;
const MAX_OPEN_READ_PATHS: usize = 8;
const MAX_OPEN_WRITE_PATHS: usize = 8;
const MAX_READ_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_WRITE_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_WRITE_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const DEFAULT_LIST_LIMIT: u64 = 1_000;
const MAX_LIST_LIMIT: u64 = 5_000;
const MAX_PATH_BYTES: usize = 4_096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteMode {
    /// Created by `file.create`: random-access writes and appends are allowed.
    Create,
    /// Opened by `file.append`: every write goes to the end of the file.
    Append,
}

struct WriteHandle {
    file: File,
    mode: WriteMode,
    len: u64,
}

pub(crate) struct FileState {
    read_handles: HashMap<String, File>,
    write_handles: HashMap<PathBuf, WriteHandle>,
    /// Canonical directory every write must stay inside. `None` means the run
    /// holds no write approval.
    write_root: Option<PathBuf>,
    bytes_written: u64,
}

impl FileState {
    pub(crate) fn new(write_root: Option<PathBuf>) -> Result<Self, String> {
        let write_root = match write_root {
            Some(root) => {
                let canonical = std::fs::canonicalize(&root).map_err(|e| {
                    format!("write directory '{}' is not usable: {e}", root.display())
                })?;
                if !canonical.is_dir() {
                    return Err(format!(
                        "write path '{}' is not a directory",
                        root.display()
                    ));
                }
                Some(canonical)
            }
            None => None,
        };
        Ok(Self {
            read_handles: HashMap::new(),
            write_handles: HashMap::new(),
            write_root,
            bytes_written: 0,
        })
    }

    pub(crate) fn request(&mut self, operation: &str, request: &Value) -> Result<String, String> {
        match operation {
            "file.read" => self.read(request),
            "file.list" => self.list(request),
            "file.stat" => self.stat(request),
            "file.create" => self.create(request),
            "file.write" => self.write(request),
            "file.append" => self.append(request),
            "file.mkdir" => self.mkdir(request),
            CLOSE_OPERATION => self.close(request),
            other => Err(format!("unsupported file capability '{other}'")),
        }
    }

    fn read(&mut self, request: &Value) -> Result<String, String> {
        let path = string(request, "path")?;
        let offset = u64_field(request, "offset")?;
        let length = u64_field(request, "length")?;
        if length == 0 || length > MAX_CHUNK {
            return Err("file read chunks must be 1-65536 bytes".into());
        }
        if !self.read_handles.contains_key(path) {
            if self.read_handles.len() >= MAX_OPEN_READ_PATHS {
                return Err("a tool may open at most eight file paths".into());
            }
            let canonical =
                std::fs::canonicalize(path).map_err(|e| format!("cannot open file: {e}"))?;
            let file = File::open(&canonical).map_err(|e| format!("cannot open file: {e}"))?;
            let metadata = file.metadata().map_err(|e| e.to_string())?;
            if !metadata.is_file() || metadata.len() > MAX_READ_FILE_BYTES {
                return Err("file must be regular and no larger than 64 MiB".into());
            }
            self.read_handles.insert(path.to_owned(), file);
        }
        let file = self
            .read_handles
            .get_mut(path)
            .ok_or("file handle unavailable")?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::with_capacity(length as usize);
        file.take(length)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "path": path,
            "offset": offset,
            "bytes_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
        })
        .to_string())
    }

    fn list(&self, request: &Value) -> Result<String, String> {
        let path = string(request, "path")?;
        let limit = request
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_LIST_LIMIT)
            .clamp(1, MAX_LIST_LIMIT) as usize;
        let canonical =
            std::fs::canonicalize(path).map_err(|e| format!("cannot list directory: {e}"))?;
        let mut entries = Vec::new();
        for entry in
            std::fs::read_dir(&canonical).map_err(|e| format!("cannot list directory: {e}"))?
        {
            let entry = entry.map_err(|e| format!("cannot list directory: {e}"))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = entry.metadata().ok();
            let kind = entry.file_type().map(file_kind).unwrap_or("other");
            entries.push((name, kind, metadata));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let total = entries.len();
        let listed: Vec<Value> = entries
            .into_iter()
            .take(limit)
            .map(|(name, kind, metadata)| {
                json!({
                    "name": name,
                    "type": kind,
                    "size": metadata.as_ref().filter(|m| m.is_file()).map(|m| m.len()),
                    "modified": metadata.as_ref().and_then(modified_seconds),
                })
            })
            .collect();
        Ok(json!({
            "path": display_path(&canonical),
            "entries": listed,
            "total": total,
            "truncated": total > limit,
        })
        .to_string())
    }

    fn stat(&self, request: &Value) -> Result<String, String> {
        let path = string(request, "path")?;
        let link = match std::fs::symlink_metadata(path) {
            Ok(link) => link,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(json!({"path": path, "exists": false}).to_string());
            }
            Err(error) => return Err(format!("cannot inspect path: {error}")),
        };
        let target = std::fs::metadata(path).unwrap_or_else(|_| link.clone());
        Ok(json!({
            "path": path,
            "exists": true,
            "type": file_kind(target.file_type()),
            "symlink": link.file_type().is_symlink(),
            "size": target.is_file().then_some(target.len()),
            "modified": modified_seconds(&target),
            "readonly": target.permissions().readonly(),
        })
        .to_string())
    }

    fn create(&mut self, request: &Value) -> Result<String, String> {
        let overwrite = request
            .get("overwrite")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let initial = optional_bytes(request)?;
        let path = self.resolve_write_path(string(request, "path")?)?;
        if self.write_handles.contains_key(&path) && !overwrite {
            return Err(
                "file was already created during this run; pass overwrite to replace it".into(),
            );
        }
        if self.write_handles.len() >= MAX_OPEN_WRITE_PATHS
            && !self.write_handles.contains_key(&path)
        {
            return Err("a tool may keep at most eight files open for writing".into());
        }
        self.write_handles.remove(&path);
        let mut options = OpenOptions::new();
        options.write(true);
        if overwrite {
            options.create(true).truncate(true);
        } else {
            options.create_new(true);
        }
        let mut file = options.open(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                "file already exists; pass overwrite to replace it".to_owned()
            }
            _ => format!("cannot create file: {e}"),
        })?;
        ensure_regular(&file)?;
        let mut written = 0u64;
        if !initial.is_empty() {
            self.reserve(initial.len() as u64)?;
            file.write_all(&initial)
                .map_err(|e| format!("cannot write file: {e}"))?;
            written = initial.len() as u64;
        }
        let shown = display_path(&path);
        self.write_handles.insert(
            path,
            WriteHandle {
                file,
                mode: WriteMode::Create,
                len: written,
            },
        );
        Ok(json!({"path": shown, "created": true, "bytes_written": written}).to_string())
    }

    fn write(&mut self, request: &Value) -> Result<String, String> {
        let offset = u64_field(request, "offset")?;
        let bytes = required_bytes(request)?;
        let path = self.resolve_write_path(string(request, "path")?)?;
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or("write range overflows")?;
        if end > MAX_WRITE_FILE_BYTES {
            return Err("a written file may not exceed 256 MiB".into());
        }
        let handle = self
            .write_handles
            .get(&path)
            .ok_or("create the file with file.create before writing at an offset")?;
        if handle.mode == WriteMode::Append {
            return Err("this file was opened for append; use file.append".into());
        }
        let growth = end.saturating_sub(handle.len);
        self.reserve(growth)?;
        let handle = self
            .write_handles
            .get_mut(&path)
            .ok_or("file handle unavailable")?;
        handle
            .file
            .seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        handle
            .file
            .write_all(&bytes)
            .map_err(|e| format!("cannot write file: {e}"))?;
        handle.len = handle.len.max(end);
        Ok(
            json!({"path": display_path(&path), "offset": offset, "written": bytes.len()})
                .to_string(),
        )
    }

    fn append(&mut self, request: &Value) -> Result<String, String> {
        let bytes = required_bytes(request)?;
        let create = request
            .get("create")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let path = self.resolve_write_path(string(request, "path")?)?;
        if !self.write_handles.contains_key(&path) {
            if self.write_handles.len() >= MAX_OPEN_WRITE_PATHS {
                return Err("a tool may keep at most eight files open for writing".into());
            }
            let mut options = OpenOptions::new();
            options.append(true);
            if create {
                options.create(true);
            }
            let file = options
                .open(&path)
                .map_err(|e| format!("cannot open file for append: {e}"))?;
            ensure_regular(&file)?;
            let len = file.metadata().map_err(|e| e.to_string())?.len();
            self.write_handles.insert(
                path.clone(),
                WriteHandle {
                    file,
                    mode: WriteMode::Append,
                    len,
                },
            );
        }
        let current = self.write_handles.get(&path).map_or(0, |h| h.len);
        if current.saturating_add(bytes.len() as u64) > MAX_WRITE_FILE_BYTES {
            return Err("a written file may not exceed 256 MiB".into());
        }
        self.reserve(bytes.len() as u64)?;
        let handle = self
            .write_handles
            .get_mut(&path)
            .ok_or("file handle unavailable")?;
        if handle.mode == WriteMode::Create {
            handle
                .file
                .seek(SeekFrom::End(0))
                .map_err(|e| e.to_string())?;
        }
        handle
            .file
            .write_all(&bytes)
            .map_err(|e| format!("cannot write file: {e}"))?;
        handle.len = handle.len.saturating_add(bytes.len() as u64);
        Ok(
            json!({"path": display_path(&path), "appended": bytes.len(), "size": handle.len})
                .to_string(),
        )
    }

    fn mkdir(&mut self, request: &Value) -> Result<String, String> {
        let recursive = request
            .get("recursive")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let path = self.resolve_write_path(string(request, "path")?)?;
        if path.exists() {
            return if path.is_dir() {
                Ok(json!({"path": display_path(&path), "created": false}).to_string())
            } else {
                Err("path exists and is not a directory".into())
            };
        }
        let result = if recursive {
            std::fs::create_dir_all(&path)
        } else {
            std::fs::create_dir(&path)
        };
        result.map_err(|e| format!("cannot create directory: {e}"))?;
        Ok(json!({"path": display_path(&path), "created": true}).to_string())
    }

    fn close(&mut self, request: &Value) -> Result<String, String> {
        let raw = string(request, "path")?;
        let mut closed = self.read_handles.remove(raw).is_some();
        if self.write_root.is_some()
            && let Ok(path) = self.resolve_write_path(raw)
            && let Some(handle) = self.write_handles.remove(&path)
        {
            handle
                .file
                .sync_all()
                .map_err(|e| format!("cannot flush file: {e}"))?;
            closed = true;
        }
        Ok(json!({"path": raw, "closed": closed}).to_string())
    }

    fn reserve(&mut self, growth: u64) -> Result<(), String> {
        let total = self.bytes_written.saturating_add(growth);
        if total > MAX_WRITE_TOTAL_BYTES {
            return Err("a run may write at most 1 GiB in total".into());
        }
        self.bytes_written = total;
        Ok(())
    }

    /// Resolve a tool-supplied path to a location inside the write root.
    fn resolve_write_path(&self, raw: &str) -> Result<PathBuf, String> {
        let root = self
            .write_root
            .as_ref()
            .ok_or("file-write permission was not granted for this run")?;
        confine_to_root(root, raw)
    }
}

impl Drop for FileState {
    fn drop(&mut self) {
        for handle in self.write_handles.values() {
            let _ = handle.file.sync_all();
        }
    }
}

/// Lexically normalise `raw` (relative paths start at `root`), then require the
/// result — after resolving symlinks in its deepest existing ancestor — to stay
/// inside the canonical `root`.
pub(crate) fn confine_to_root(root: &Path, raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() || raw.len() > MAX_PATH_BYTES || raw.contains('\0') {
        return Err("path must be 1-4096 characters without NUL bytes".into());
    }
    let requested = Path::new(raw);
    let start = if requested.is_absolute() || requested.has_root() {
        PathBuf::new()
    } else {
        root.to_path_buf()
    };
    let mut normal = start;
    for component in requested.components() {
        match component {
            Component::Prefix(prefix) => normal.push(prefix.as_os_str()),
            Component::RootDir => normal.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normal.pop() {
                    return Err("path escapes the permitted directory".into());
                }
            }
            Component::Normal(part) => {
                check_component(part)?;
                normal.push(part);
            }
        }
    }
    // Resolve symlinks in the deepest ancestor that already exists, then
    // re-attach the components that do not exist yet. `symlink_metadata` is
    // used instead of `exists` so a dangling symlink counts as existing: it is
    // then canonicalised (and refused) instead of being treated as a free name
    // that `create`/`append` would follow to a target outside the root.
    let mut existing = normal.clone();
    let mut remainder: Vec<std::ffi::OsString> = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        match existing.file_name() {
            Some(name) => remainder.push(name.to_owned()),
            None => return Err("path has no existing ancestor".into()),
        }
        if !existing.pop() {
            return Err("path has no existing ancestor".into());
        }
    }
    let mut resolved =
        std::fs::canonicalize(&existing).map_err(|e| format!("cannot resolve path: {e}"))?;
    for part in remainder.into_iter().rev() {
        resolved.push(part);
    }
    if !resolved.starts_with(root) {
        return Err("path is outside the directory approved for writing".into());
    }
    Ok(resolved)
}

fn check_component(part: &std::ffi::OsStr) -> Result<(), String> {
    let text = part.to_string_lossy();
    if text.contains('\0') {
        return Err("path contains a NUL byte".into());
    }
    #[cfg(windows)]
    {
        if text
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        {
            return Err(
                "path contains a character that is not allowed in a Windows file name".into(),
            );
        }
        let stem = text
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end()
            .to_ascii_uppercase();
        let reserved = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
        if reserved {
            return Err("path uses a reserved Windows device name".into());
        }
    }
    Ok(())
}

fn ensure_regular(file: &File) -> Result<(), String> {
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if metadata.is_file() {
        Ok(())
    } else {
        Err("target is not a regular file".into())
    }
}

fn file_kind(kind: std::fs::FileType) -> &'static str {
    if kind.is_symlink() {
        "symlink"
    } else if kind.is_dir() {
        "dir"
    } else if kind.is_file() {
        "file"
    } else {
        "other"
    }
}

fn modified_seconds(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_secs())
}

/// Render a path without the Windows verbatim (`\\?\`) prefix.
pub(crate) fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

fn string<'a>(request: &'a Value, key: &str) -> Result<&'a str, String> {
    request
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("request requires {key}"))
}

fn u64_field(request: &Value, key: &str) -> Result<u64, String> {
    request
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("request requires {key}"))
}

fn decode(encoded: &str) -> Result<Vec<u8>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("invalid Base64 data: {e}"))?;
    if bytes.len() as u64 > MAX_CHUNK {
        return Err("file writes are limited to 65536 bytes per request".into());
    }
    Ok(bytes)
}

fn required_bytes(request: &Value) -> Result<Vec<u8>, String> {
    decode(string(request, "bytes_base64")?)
}

fn optional_bytes(request: &Value) -> Result<Vec<u8>, String> {
    match request.get("bytes_base64").and_then(Value::as_str) {
        Some(encoded) => decode(encoded),
        None => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch() -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ktrx-fs-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn call(state: &mut FileState, op: &str, body: Value) -> Result<Value, String> {
        state
            .request(op, &body)
            .map(|text| serde_json::from_str(&text).unwrap())
    }

    #[test]
    fn creates_writes_appends_and_reads_back() {
        let dir = scratch();
        let mut state = FileState::new(Some(dir.clone())).unwrap();
        call(&mut state, "file.mkdir", json!({"path": "out/nested"})).unwrap();
        call(
            &mut state,
            "file.create",
            json!({"path": "out/nested/report.txt", "bytes_base64": b64(b"hello")}),
        )
        .unwrap();
        call(
            &mut state,
            "file.write",
            json!({"path": "out/nested/report.txt", "offset": 5, "bytes_base64": b64(b" world")}),
        )
        .unwrap();
        call(
            &mut state,
            "file.append",
            json!({"path": "out/nested/report.txt", "bytes_base64": b64(b"!")}),
        )
        .unwrap();
        call(
            &mut state,
            "file.close",
            json!({"path": "out/nested/report.txt"}),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out/nested/report.txt")).unwrap(),
            "hello world!"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refuses_to_overwrite_without_the_overwrite_flag() {
        let dir = scratch();
        std::fs::write(dir.join("keep.txt"), "original").unwrap();
        let mut state = FileState::new(Some(dir.clone())).unwrap();
        let error = call(&mut state, "file.create", json!({"path": "keep.txt"})).unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert_eq!(
            std::fs::read_to_string(dir.join("keep.txt")).unwrap(),
            "original"
        );
        call(
            &mut state,
            "file.create",
            json!({"path": "keep.txt", "overwrite": true, "bytes_base64": b64(b"new")}),
        )
        .unwrap();
        call(&mut state, "file.close", json!({"path": "keep.txt"})).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("keep.txt")).unwrap(),
            "new"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn confines_every_write_to_the_approved_directory() {
        let dir = scratch();
        let outside = scratch();
        let mut state = FileState::new(Some(dir.clone())).unwrap();
        for escape in [
            "../escape.txt".to_owned(),
            "a/../../escape.txt".to_owned(),
            outside.join("escape.txt").to_string_lossy().into_owned(),
        ] {
            let error = call(&mut state, "file.create", json!({"path": escape})).unwrap_err();
            assert!(
                error.contains("outside") || error.contains("escapes"),
                "{escape}: {error}"
            );
        }
        assert!(!outside.join("escape.txt").exists());
        assert!(!dir.parent().unwrap().join("escape.txt").exists());
        // Absolute paths inside the root are fine.
        let inside = dir.join("inside.txt");
        call(
            &mut state,
            "file.create",
            json!({"path": inside.to_string_lossy()}),
        )
        .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn denies_writes_when_no_directory_was_approved() {
        let mut state = FileState::new(None).unwrap();
        let error = call(&mut state, "file.create", json!({"path": "x.txt"})).unwrap_err();
        assert!(error.contains("file-write permission was not granted"));
    }

    #[test]
    fn enforces_size_limits() {
        let dir = scratch();
        let mut state = FileState::new(Some(dir.clone())).unwrap();
        call(&mut state, "file.create", json!({"path": "big.bin"})).unwrap();
        let oversized = b64(&vec![0u8; 70_000]);
        assert!(
            call(
                &mut state,
                "file.write",
                json!({"path": "big.bin", "offset": 0, "bytes_base64": oversized})
            )
            .is_err()
        );
        let far = call(
            &mut state,
            "file.write",
            json!({"path": "big.bin", "offset": MAX_WRITE_FILE_BYTES, "bytes_base64": b64(b"x")}),
        )
        .unwrap_err();
        assert!(far.contains("256 MiB"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn writing_requires_create_first_and_append_mode_rejects_offsets() {
        let dir = scratch();
        let mut state = FileState::new(Some(dir.clone())).unwrap();
        assert!(
            call(
                &mut state,
                "file.write",
                json!({"path": "late.txt", "offset": 0, "bytes_base64": b64(b"x")})
            )
            .is_err()
        );
        call(
            &mut state,
            "file.append",
            json!({"path": "log.txt", "bytes_base64": b64(b"a")}),
        )
        .unwrap();
        let error = call(
            &mut state,
            "file.write",
            json!({"path": "log.txt", "offset": 0, "bytes_base64": b64(b"b")}),
        )
        .unwrap_err();
        assert!(error.contains("append"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lists_and_stats_without_a_write_directory() {
        let dir = scratch();
        std::fs::write(dir.join("b.txt"), "12345").unwrap();
        std::fs::create_dir(dir.join("a_dir")).unwrap();
        let mut state = FileState::new(None).unwrap();
        let listing = call(
            &mut state,
            "file.list",
            json!({"path": dir.to_string_lossy()}),
        )
        .unwrap();
        let names: Vec<_> = listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["name"].as_str().unwrap().to_owned(),
                    e["type"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("a_dir".into(), "dir".into()),
                ("b.txt".into(), "file".into())
            ]
        );
        let stat = call(
            &mut state,
            "file.stat",
            json!({"path": dir.join("b.txt").to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(stat["exists"], true);
        assert_eq!(stat["size"], 5);
        assert_eq!(stat["type"], "file");
        let missing = call(
            &mut state,
            "file.stat",
            json!({"path": dir.join("nope").to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(missing["exists"], false);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn list_reports_truncation() {
        let dir = scratch();
        for i in 0..5 {
            std::fs::write(dir.join(format!("f{i}")), "x").unwrap();
        }
        let mut state = FileState::new(None).unwrap();
        let listing = call(
            &mut state,
            "file.list",
            json!({"path": dir.to_string_lossy(), "limit": 2}),
        )
        .unwrap();
        assert_eq!(listing["entries"].as_array().unwrap().len(), 2);
        assert_eq!(listing["total"], 5);
        assert_eq!(listing["truncated"], true);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_device_names_and_stream_syntax() {
        let dir = scratch();
        let mut state = FileState::new(Some(dir.clone())).unwrap();
        for name in ["NUL", "con.txt", "COM1", "file.txt:stream", "a<b"] {
            assert!(
                call(&mut state, "file.create", json!({"path": name})).is_err(),
                "{name}"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_a_symlink_out_of_the_write_root() {
        let dir = scratch();
        let outside = scratch();
        std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();
        let mut state = FileState::new(Some(dir.clone())).unwrap();
        let error = call(
            &mut state,
            "file.create",
            json!({"path": "link/stolen.txt"}),
        )
        .unwrap_err();
        assert!(error.contains("outside"), "{error}");
        assert!(!outside.join("stolen.txt").exists());
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_a_dangling_symlink_out_of_the_write_root() {
        let dir = scratch();
        let outside = scratch();
        let target = outside.join("planted.txt");
        std::os::unix::fs::symlink(&target, dir.join("dangling")).unwrap();
        let mut state = FileState::new(Some(dir.clone())).unwrap();
        for (op, body) in [
            (
                "file.create",
                json!({"path": "dangling", "overwrite": true, "bytes_base64": b64(b"x")}),
            ),
            (
                "file.append",
                json!({"path": "dangling", "bytes_base64": b64(b"x")}),
            ),
        ] {
            assert!(call(&mut state, op, body).is_err(), "{op}");
        }
        assert!(!target.exists());
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
}
