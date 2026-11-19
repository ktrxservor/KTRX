#!/usr/bin/env bash
# Publish the tools of this checkout to the KTRX registry.
#
#   KTRX_REGISTRY_KEY=... scripts/publish-tools.sh [registry-url]
#
# For every tools/<DIR>/K~<NAME>.tool:
#   - uploads the archive as a new version (a version that already exists is skipped,
#     so bump `version` in manifest.toml and rebuild the .tool to release an update);
#   - uploads icon.png (resized for the web) and the README, which replace the current ones.
# The category, tags and license come from the manifest (see KTRX_TOOL_AUTHORING_GUIDE.md).
set -euo pipefail

BASE="${1:-${KTRX_REGISTRY_URL:-https://ktrx-site.tiagoantunes2009.workers.dev}}"
KEY="${KTRX_REGISTRY_KEY:-}"
if [ -z "$KEY" ]; then
  echo "KTRX_REGISTRY_KEY is not set; nothing published."
  exit 0
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
failures=0

resize_icon() { # src dst
  if command -v magick >/dev/null 2>&1; then magick "$1" -resize 320x320 "$2"
  elif command -v convert >/dev/null 2>&1; then convert "$1" -resize 320x320 "$2"
  elif command -v sips >/dev/null 2>&1; then sips -Z 320 "$1" --out "$2" >/dev/null
  else cp "$1" "$2"; fi
}

for dir in "$ROOT"/tools/*/; do
  pkg="$(ls "$dir"K~*.tool 2>/dev/null | head -1 || true)"
  [ -z "$pkg" ] && continue
  name="$(basename "$pkg" .tool)"
  id="$(unzip -p "$pkg" manifest.toml | sed -n 's/^id *= *"\(.*\)"/\1/p' | head -1 | tr -d '\r')"
  version="$(unzip -p "$pkg" manifest.toml | sed -n 's/^version *= *"\(.*\)"/\1/p' | head -1 | tr -d '\r')"

  code="$(curl -sS -o "$TMP/out.json" -w '%{http_code}' -X POST "$BASE/api/tools" \
    -H "Authorization: Bearer $KEY" --data-binary @"$pkg")"
  case "$code" in
    200|201) echo "$name $version: published" ;;
    409)     echo "$name $version: already published" ;;
    *)       echo "$name: FAILED (HTTP $code) $(cat "$TMP/out.json")"; failures=$((failures + 1)); continue ;;
  esac

  if [ -f "${dir}icon.png" ]; then
    resize_icon "${dir}icon.png" "$TMP/icon.png"
    curl -sS -f -o /dev/null -X POST "$BASE/api/tools/$id/icon" -H "Authorization: Bearer $KEY" \
      --data-binary @"$TMP/icon.png" || { echo "$name: icon upload failed"; failures=$((failures + 1)); }
  fi
  if [ -f "${dir}README.md" ]; then
    python3 -c 'import sys,json; print(json.dumps({"readme": open(sys.argv[1], encoding="utf-8").read()}))' \
      "${dir}README.md" > "$TMP/readme.json"
    curl -sS -f -o /dev/null -X PATCH "$BASE/api/tools/$id" -H "Authorization: Bearer $KEY" \
      -H "Content-Type: application/json" --data-binary @"$TMP/readme.json" \
      || { echo "$name: README upload failed"; failures=$((failures + 1)); }
  fi
done

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
