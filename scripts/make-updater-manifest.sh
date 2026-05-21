#!/usr/bin/env bash
# Generates the Tauri updater manifest (latest.json) from signed bundle assets.
#
# Usage: ./scripts/make-updater-manifest.sh <version> <channel> <out-path> <release-notes-file>
#
# Required env vars (paths to signature files; tarball paths are derived):
#   MACOS_AARCH64_SIG          path to .app.tar.gz.sig (minisign signature contents)
#
# The signatures must be the COMPLETE minisign sig file contents (text). Tauri's
# updater verifies the signature line against the embedded pubkey.
#
# The desktop app is macOS-only (Apple Silicon). Intel Mac is intentionally
# not shipped (Cask requires arm64; Formula odie's on Intel). Windows ships
# only the CLI, not the desktop, so windows-x86_64 is omitted from the
# updater manifest — if a Windows desktop bundle is ever added, set
# WINDOWS_X86_64_SIG and extend manifest.platforms below.

set -euo pipefail

VERSION="${1:?version required}"
CHANNEL="${2:?channel required}"
OUT="${3:?output path required}"
NOTES_FILE="${4:?notes file required}"

: "${MACOS_AARCH64_SIG:?MACOS_AARCH64_SIG env var required}"

for f in "$MACOS_AARCH64_SIG" "$NOTES_FILE"; do
  if [ ! -f "$f" ]; then
    echo "missing file: $f" >&2
    exit 1
  fi
done

PUB_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
URL_BASE="https://github.com/cinchcli/cinch/releases/download/${CHANNEL}/${VERSION}"

# Pipe everything into python3 so JSON escaping is handled correctly.
python3 - "$VERSION" "$PUB_DATE" "$URL_BASE" \
  "$MACOS_AARCH64_SIG" \
  "$NOTES_FILE" "$OUT" <<'PY'
import json
import sys
from pathlib import Path

(version, pub_date, url_base,
 sig_macos_arm,
 notes_file, out_path) = sys.argv[1:]

manifest = {
    "version": version,
    "notes": Path(notes_file).read_text(),
    "pub_date": pub_date,
    "platforms": {
        "darwin-aarch64": {
            "signature": Path(sig_macos_arm).read_text().strip(),
            # Tauri 2 emits the updater bundle as plain `<productName>.app.tar.gz`
            # with no version or arch suffix. Each release tag scopes the URL,
            # so the unversioned filename is unambiguous.
            "url": f"{url_base}/Cinch.app.tar.gz",
        },
    },
}

Path(out_path).write_text(json.dumps(manifest, indent=2) + "\n")
PY

# Validate the produced JSON (already valid since python wrote it, but assert anyway)
python3 -m json.tool "$OUT" > /dev/null
echo "Wrote $OUT"
