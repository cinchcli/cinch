#!/usr/bin/env bash
# Updates the version in all five locations across the monorepo, then runs the
# parity check to confirm everything agrees.
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

./scripts/check-version-parity.sh "$NEW"
echo "Bumped all 5 manifests to ${NEW}"
