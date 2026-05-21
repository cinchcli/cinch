#!/usr/bin/env bash
set -euo pipefail

# Asserts the three desktop manifests carry the same version string, and
# (when called with a tag argument) that the tag agrees with them.
#
# Background: pre-2026-05-19, desktop-v0.1.{8,9} releases shipped DMGs
# named after the stale 0.1.7 Cargo.toml/tauri.conf.json version while
# the workflow looked for `Cinch_0.1.9_aarch64.dmg` — the mismatch 404'd
# the homebrew-tap update step. The Tauri builder names release artifacts
# from `src-tauri/Cargo.toml::version`, so any drift between that file
# and the tag breaks the cask updater silently.
#
# Usage:
#   bash scripts/check-version-parity.sh                 # parity only
#   bash scripts/check-version-parity.sh 0.1.10          # parity + tag match

EXPECTED_TAG_VERSION="${1:-}"

CARGO=$(grep -E '^version *= *"' src-tauri/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
TAURI=$(jq -r '.version' src-tauri/tauri.conf.json)
PACKAGE=$(jq -r '.version' package.json)

echo "Cargo.toml      : $CARGO"
echo "tauri.conf.json : $TAURI"
echo "package.json    : $PACKAGE"
if [[ -n "$EXPECTED_TAG_VERSION" ]]; then
  echo "tag             : $EXPECTED_TAG_VERSION"
fi

fail=0
if [[ "$CARGO" != "$TAURI" ]] || [[ "$CARGO" != "$PACKAGE" ]]; then
  echo "::error::Manifest versions disagree — bump all three to the same value before committing."
  fail=1
fi

if [[ -n "$EXPECTED_TAG_VERSION" ]] && [[ "$CARGO" != "$EXPECTED_TAG_VERSION" ]]; then
  echo "::error::Manifest version ($CARGO) does not match release tag ($EXPECTED_TAG_VERSION)."
  echo "::error::The Tauri build will produce Cinch_${CARGO}_*.dmg but the cask updater expects Cinch_${EXPECTED_TAG_VERSION}_*.dmg."
  fail=1
fi

exit $fail
