#!/usr/bin/env bash
# Updates Casks/cinch.rb in cinchcli/homebrew-tap with the new version and DMG SHA.
#
# Usage: ./scripts/update-homebrew-cask.sh <version> <dmg-path>
# Requires: HOMEBREW_TAP_TOKEN env var (PAT with contents:write on cinchcli/homebrew-tap).

set -euo pipefail

VERSION="${1:?version required}"
DMG="${2:?dmg path required}"

: "${HOMEBREW_TAP_TOKEN:?HOMEBREW_TAP_TOKEN env var required}"

if [ ! -f "$DMG" ]; then
  echo "DMG not found: $DMG" >&2
  exit 1
fi

# shasum is BSD/macOS; sha256sum is Linux. Try both.
if command -v shasum >/dev/null 2>&1; then
  SHA=$(shasum -a 256 "$DMG" | cut -d' ' -f1)
else
  SHA=$(sha256sum "$DMG" | cut -d' ' -f1)
fi

echo "Cask update: version=${VERSION} sha=${SHA}"

WORK=$(mktemp -d)
trap "rm -rf $WORK" EXIT
cd "$WORK"

git clone --depth 1 \
  "https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/cinchcli/homebrew-tap.git" .

# Portable in-place sed (BSD vs GNU)
inplace_sed() {
  sed -E "$1" "$2" > "$2.tmp" && mv "$2.tmp" "$2"
}

inplace_sed "s/version \"[^\"]+\"/version \"${VERSION}\"/" Casks/cinch.rb
# Match both :no_check (Ruby symbol, no quotes) and "..." (quoted SHA)
inplace_sed "s/sha256 (:no_check|\"[^\"]+\")/sha256 \"${SHA}\"/" Casks/cinch.rb

git config user.name 'cinch-release[bot]'
git config user.email 'cinch-release@users.noreply.github.com'
git add Casks/cinch.rb
git commit -m "cinch ${VERSION}"
git push
