#!/usr/bin/env bash
# Generates the Tauri updater manifest (latest.json) from signed bundle assets.
#
# Usage: ./scripts/make-updater-manifest.sh <version> <channel> <out-path> <release-notes-file>
#
# Required env vars (paths to signature files; tarball/installer paths are derived):
#   MACOS_AARCH64_SIG          path to .app.tar.gz.sig (minisign signature contents)
#   MACOS_X86_64_SIG           path to .app.tar.gz.sig (Intel)
#   WINDOWS_X86_64_SIG         path to .msi.sig (or .exe.sig depending on bundle type)
#
# The signatures must be the COMPLETE minisign sig file contents (text). Tauri's
# updater verifies the signature line against the embedded pubkey.

set -euo pipefail

VERSION="${1:?version required}"
CHANNEL="${2:?channel required}"
OUT="${3:?output path required}"
NOTES_FILE="${4:?notes file required}"

: "${MACOS_AARCH64_SIG:?MACOS_AARCH64_SIG env var required}"
: "${MACOS_X86_64_SIG:?MACOS_X86_64_SIG env var required}"
: "${WINDOWS_X86_64_SIG:?WINDOWS_X86_64_SIG env var required}"

for f in "$MACOS_AARCH64_SIG" "$MACOS_X86_64_SIG" "$WINDOWS_X86_64_SIG" "$NOTES_FILE"; do
  if [ ! -f "$f" ]; then
    echo "missing file: $f" >&2
    exit 1
  fi
done

PUB_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
URL_BASE="https://github.com/cinchcli/cinch/releases/download/${CHANNEL}/${VERSION}"

# Pipe everything into python3 so JSON escaping is handled correctly.
python3 - "$VERSION" "$PUB_DATE" "$URL_BASE" \
  "$MACOS_AARCH64_SIG" "$MACOS_X86_64_SIG" "$WINDOWS_X86_64_SIG" \
  "$NOTES_FILE" "$OUT" <<'PY'
import json
import sys
from pathlib import Path

(version, pub_date, url_base,
 sig_macos_arm, sig_macos_x86, sig_win,
 notes_file, out_path) = sys.argv[1:]

manifest = {
    "version": version,
    "notes": Path(notes_file).read_text(),
    "pub_date": pub_date,
    "platforms": {
        "darwin-aarch64": {
            "signature": Path(sig_macos_arm).read_text().strip(),
            "url": f"{url_base}/Cinch_{version}_aarch64.app.tar.gz",
        },
        "darwin-x86_64": {
            "signature": Path(sig_macos_x86).read_text().strip(),
            "url": f"{url_base}/Cinch_{version}_x64.app.tar.gz",
        },
        "windows-x86_64": {
            "signature": Path(sig_win).read_text().strip(),
            "url": f"{url_base}/Cinch_{version}_x64-setup.exe",
        },
    },
}

Path(out_path).write_text(json.dumps(manifest, indent=2) + "\n")
PY

# Validate the produced JSON (already valid since python wrote it, but assert anyway)
python3 -m json.tool "$OUT" > /dev/null
echo "Wrote $OUT"
