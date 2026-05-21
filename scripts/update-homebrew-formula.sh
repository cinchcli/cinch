#!/usr/bin/env bash
# Updates Formula/cinch.rb in cinchcli/homebrew-tap with the new version and CLI tarball SHAs.
#
# Usage: ./scripts/update-homebrew-formula.sh <version> <aarch64-tar.gz> <x86_64-tar.gz> <linux-arm64-tar.gz> <linux-x86_64-tar.gz>
#
# The Formula currently carries 3 platform-specific SHAs:
#   - macOS arm64 (only)
#   - Linux arm64
#   - Linux x86_64
# (macOS x86_64 is intentionally NOT supported by the Formula — see the `odie` clause.)
#
# Required env: HOMEBREW_TAP_TOKEN

set -euo pipefail

VERSION="${1:?version required}"
DARWIN_AARCH64="${2:?darwin-arm64 tarball required}"
DARWIN_X86_64="${3:?darwin-x86_64 tarball required (unused; pass empty for API stability)}"
LINUX_ARM64="${4:?linux-arm64 tarball required}"
LINUX_X86_64="${5:?linux-x86_64 tarball required}"

: "${HOMEBREW_TAP_TOKEN:?HOMEBREW_TAP_TOKEN env var required}"

# darwin_x86_64 is required by argument count but unused (Formula odie's on Intel macs)

shasum_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    sha256sum "$1" | cut -d' ' -f1
  fi
}

for f in "$DARWIN_AARCH64" "$LINUX_ARM64" "$LINUX_X86_64"; do
  if [ ! -f "$f" ]; then
    echo "tarball not found: $f" >&2
    exit 1
  fi
done

SHA_DARWIN_ARM64=$(shasum_file "$DARWIN_AARCH64")
SHA_LINUX_ARM64=$(shasum_file "$LINUX_ARM64")
SHA_LINUX_X86_64=$(shasum_file "$LINUX_X86_64")

echo "Formula update: ${VERSION}"
echo "  darwin-arm64:  ${SHA_DARWIN_ARM64}"
echo "  linux-arm64:   ${SHA_LINUX_ARM64}"
echo "  linux-x86_64:  ${SHA_LINUX_X86_64}"

WORK=$(mktemp -d)
trap "rm -rf $WORK" EXIT
cd "$WORK"

git clone --depth 1 \
  "https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/cinchcli/homebrew-tap.git" .

# Use Python to rewrite the formula — sed isn't reliable for the multi-block conditional structure
python3 - "$VERSION" "$SHA_DARWIN_ARM64" "$SHA_LINUX_ARM64" "$SHA_LINUX_X86_64" <<'PY'
import re
import sys
from pathlib import Path

version, sha_darwin_arm, sha_linux_arm, sha_linux_x86 = sys.argv[1:]

p = Path("Formula/cinch.rb")
src = p.read_text()

# 1. Bump version
src = re.sub(r'(\n\s*version\s+)"[^"]+"', f'\\1"{version}"', src, count=1)

# 2. Replace URLs and SHAs by walking the on_macos / on_linux blocks structurally.
# The existing Formula uses paths like:
#   https://github.com/cinchcli/cinch/releases/download/v<version>/cinch_<OS>_<arch>.tar.gz
# We migrate to the new monorepo's release path:
#   https://github.com/cinchcli/cinch/releases/download/release/<version>/cinch-cli-<triple>.tar.gz
url_base = f"https://github.com/cinchcli/cinch/releases/download/release/{version}"

replacements = [
    # darwin arm64
    (
        r'url\s+"[^"]*cinch[^"]*Darwin[^"]*arm64[^"]*"\s*\n\s*sha256\s+"[^"]+"',
        f'url "{url_base}/cinch-cli-aarch64-apple-darwin.tar.gz"\n      sha256 "{sha_darwin_arm}"',
    ),
    # linux arm64
    (
        r'url\s+"[^"]*cinch[^"]*Linux[^"]*arm64[^"]*"\s*\n\s*sha256\s+"[^"]+"',
        f'url "{url_base}/cinch-cli-aarch64-unknown-linux-gnu.tar.gz"\n      sha256 "{sha_linux_arm}"',
    ),
    # linux x86_64
    (
        r'url\s+"[^"]*cinch[^"]*Linux[^"]*x86_64[^"]*"\s*\n\s*sha256\s+"[^"]+"',
        f'url "{url_base}/cinch-cli-x86_64-unknown-linux-gnu.tar.gz"\n      sha256 "{sha_linux_x86}"',
    ),
]

for pat, repl in replacements:
    new_src, n = re.subn(pat, repl, src, count=1)
    if n != 1:
        print(f"WARNING: pattern matched {n} times: {pat}", file=sys.stderr)
    src = new_src

p.write_text(src)
PY

# Verify the formula still parses
ruby -c Formula/cinch.rb

git config user.name 'cinch-release[bot]'
git config user.email 'cinch-release@users.noreply.github.com'
git add Formula/cinch.rb
git commit -m "cinch CLI ${VERSION}"
git push
