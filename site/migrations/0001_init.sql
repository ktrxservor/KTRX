-- KTRX catalog schema

CREATE TABLE categories (
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  sigil TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE tools (
  id TEXT PRIMARY KEY,              -- slug, e.g. "hash-audit"
  myth_name TEXT UNIQUE NOT NULL,   -- e.g. "K~ANUBIS"
  display_name TEXT NOT NULL,       -- e.g. "Hash Audit"
  description TEXT NOT NULL,
  category_id TEXT NOT NULL REFERENCES categories(id),
  author TEXT NOT NULL DEFAULT 'KTRX',
  tags TEXT NOT NULL DEFAULT '[]',          -- JSON array
  permissions TEXT NOT NULL DEFAULT '[]',   -- JSON array, latest version's permissions (denormalized for listing)
  latest_version TEXT NOT NULL,
  icon_key TEXT,                    -- R2 key for icon, e.g. "icons/ANUBIS.png"
  readme_md TEXT,
  deprecated INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE tool_versions (
  tool_id TEXT NOT NULL REFERENCES tools(id),
  version TEXT NOT NULL,
  r2_key TEXT NOT NULL,             -- e.g. "tools/hash-audit/1.1.0/K~ANUBIS.tool"
  sha256 TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  signature_ed25519 TEXT,           -- base64 signature of the archive bytes
  permissions TEXT NOT NULL DEFAULT '[]',
  changelog TEXT,
  min_engine_version TEXT,
  yanked INTEGER NOT NULL DEFAULT 0,
  published_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (tool_id, version)
);

CREATE TABLE download_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  tool_id TEXT NOT NULL,
  version TEXT NOT NULL,
  at TEXT NOT NULL DEFAULT (datetime('now')),
  country TEXT,
  user_agent TEXT
);

CREATE TABLE api_keys (
  id TEXT PRIMARY KEY,              -- random id (safe to log)
  key_hash TEXT NOT NULL,           -- sha256 of the secret key
  label TEXT NOT NULL,
  scopes TEXT NOT NULL DEFAULT '[]', -- JSON array: "upload" | "delete" | "admin"
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  revoked_at TEXT
);

CREATE TABLE audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  at TEXT NOT NULL DEFAULT (datetime('now')),
  actor_key_id TEXT,
  action TEXT NOT NULL,             -- "create" | "upload_version" | "delete_version" | "delete_tool" | "yank"
  tool_id TEXT,
  version TEXT,
  ip TEXT,
  detail TEXT
);

CREATE INDEX idx_tool_versions_tool ON tool_versions(tool_id);
CREATE INDEX idx_download_events_tool ON download_events(tool_id);
CREATE INDEX idx_audit_log_tool ON audit_log(tool_id);

INSERT INTO categories (id, label, sigil, sort_order) VALUES
  ('investigation', 'Réseau', '⌖', 1),
  ('analysis', 'Analyse', '⟐', 2),
  ('encoding', 'Utilitaires', '⌁', 3),
  ('generation', 'Génération', '✶', 4);
