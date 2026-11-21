CREATE TABLE engine_releases (
  version TEXT NOT NULL,
  platform TEXT NOT NULL,           -- windows | linux | macos
  filename TEXT NOT NULL,
  r2_key TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  published_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (version, platform)
);
