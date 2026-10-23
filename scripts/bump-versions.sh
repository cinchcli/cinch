#!/usr/bin/env bash
# Updates the five manifests and workspace package entries in Cargo.lock,
# then checks manifest version parity.
# Usage: ./scripts/bump-versions.sh <new-version>

set -euo pipefail

NEW="${1:?usage: $0 <new-version>}"

cd "$(dirname "$0")/.."

# Portable in-place sed wrapper: BSD/macOS sed and GNU sed disagree on -i syntax.
# We write to a temp file and move, which works on both.
inplace_sed() {
  local script="$1"
  local file="$2"
  sed -E "$script" "$file" > "$file.tmp" && mv "$file.tmp" "$file"
}

inplace_sed "s/^version = \"[^\"]+\"/version = \"${NEW}\"/" crates/client-core/Cargo.toml
inplace_sed "s/^version = \"[^\"]+\"/version = \"${NEW}\"/" crates/cli/Cargo.toml
inplace_sed "s/^version = \"[^\"]+\"/version = \"${NEW}\"/" apps/desktop/src-tauri/Cargo.toml
inplace_sed "s/(\"version\":[[:space:]]*\")[^\"]+/\\1${NEW}/" apps/desktop/package.json
inplace_sed "s/(\"version\":[[:space:]]*\")[^\"]+/\\1${NEW}/" apps/desktop/src-tauri/tauri.conf.json

python3 - "$NEW" <<'PY'
import pathlib, re, sys
path = pathlib.Path("Cargo.lock")
content = path.read_text()
for name in ("cinch-cli", "cinch-desktop", "cinchcli-core"):
    pattern = r'(\[\[package\]\]\nname = "' + re.escape(name) + r'"\nversion = ")[^"]+(")'
    content, count = re.subn(pattern, lambda m: m[1] + sys.argv[1] + m[2], content)
    if count != 1:
        raise SystemExit(f"Expected one Cargo.lock entry for {name}, found {count}")
path.write_text(content)
PY

./scripts/check-version-parity.sh "$NEW"
echo "Bumped all 5 manifests and Cargo.lock to ${NEW}"
