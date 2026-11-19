#!/usr/bin/env python3
"""Regenerate the tools table of the root README from the tools' manifests.

    python3 scripts/readme_tools.py          # rewrite README.md in place
    python3 scripts/readme_tools.py --check  # fail if README.md is out of date (CI)

The table lives between the KTRX:TOOLS:START / KTRX:TOOLS:END markers. It is
text only: the catalogue with emblems and full documentation is the registry site.
"""
import sys
import tomllib
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]
README = ROOT / "README.md"
SITE = "https://ktrx-site.tiagoantunes2009.workers.dev"
START, END = "<!-- KTRX:TOOLS:START -->", "<!-- KTRX:TOOLS:END -->"

PERMISSION_LABELS = {
    "network": "réseau",
    "file-read": "lecture de fichiers",
    "file-write": "écriture de fichiers",
    "raw-net": "réseau brut",
    "process": "programmes",
}


def table() -> str:
    rows = []
    for manifest_path in sorted((ROOT / "tools").glob("*/manifest.toml"), key=lambda p: tomllib.loads(p.read_text("utf-8"))["name"].casefold()):
        manifest = tomllib.loads(manifest_path.read_text("utf-8"))
        folder = quote(manifest_path.parent.relative_to(ROOT).as_posix())
        permissions = ", ".join(PERMISSION_LABELS.get(p, p) for p in manifest.get("permissions", [])) or "aucune"
        name = manifest["name"]
        rows.append(
            f"| [`{name}`]({SITE}/tool?id={manifest['id']}) | {manifest['description']} | {permissions} | [source]({folder}/) |"
        )
    header = "| Outil | Description | Permissions | |\n|---|---|---|---|\n"
    return header + "\n".join(rows)


def main() -> int:
    text = README.read_text("utf-8")
    start, end = text.index(START), text.index(END)
    updated = f"{text[: start + len(START)]}\n\n{table()}\n\n{text[end:]}"
    if "--check" in sys.argv:
        if updated != text:
            print("README.md tools table is out of date: run python3 scripts/readme_tools.py")
            return 1
        return 0
    README.write_text(updated, "utf-8")
    print("README.md tools table updated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
