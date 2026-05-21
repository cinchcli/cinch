#!/usr/bin/env bash
# Verifies version parity across all components in the monorepo.
# Usage: ./scripts/check-version-parity.sh [expected-version]
#   With argument: also asserts current versions equal expected-version.

set -euo pipefail

cd "$(dirname "$0")/.."

extract() { grep -m1 '^version = ' "$1" | cut -d'"' -f2; }
extract_json() { grep -m1 '"version"' "$1" | cut -d'"' -f4; }

CLIENT_CORE_VERSION=$(extract crates/client-core/Cargo.toml)
CLI_VERSION=$(extract crates/cli/Cargo.toml)
DESKTOP_RUST_VERSION=$(extract apps/desktop/src-tauri/Cargo.toml)
DESKTOP_PKG_VERSION=$(extract_json apps/desktop/package.json)
DESKTOP_TAURI_VERSION=$(extract_json apps/desktop/src-tauri/tauri.conf.json)

echo "client-core:   $CLIENT_CORE_VERSION"
echo "cli:           $CLI_VERSION"
echo "desktop rs:    $DESKTOP_RUST_VERSION"
echo "desktop pkg:   $DESKTOP_PKG_VERSION"
echo "desktop tauri: $DESKTOP_TAURI_VERSION"

versions=("$CLIENT_CORE_VERSION" "$CLI_VERSION" "$DESKTOP_RUST_VERSION" "$DESKTOP_PKG_VERSION" "$DESKTOP_TAURI_VERSION")
unique=$(printf '%s\n' "${versions[@]}" | sort -u | wc -l | tr -d ' ')

if [ "$unique" != "1" ]; then
  echo "ERROR: version mismatch across components" >&2
  exit 1
fi

if [ "${1:-}" != "" ] && [ "$1" != "$CLIENT_CORE_VERSION" ]; then
  echo "ERROR: expected $1, found $CLIENT_CORE_VERSION" >&2
  exit 1
fi

echo "OK: all versions = $CLIENT_CORE_VERSION"
