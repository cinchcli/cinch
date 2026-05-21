#!/usr/bin/env bash
# Generates a new minisign keypair for the Tauri updater and updates
# apps/desktop/src-tauri/tauri.conf.json with the new public key.
#
# Prints the encrypted private key to stdout. The user MUST save it
# to their password manager — it cannot be recovered.
#
# Usage: ./scripts/generate-tauri-keypair.sh
# Requires: minisign installed (`brew install minisign`)

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v minisign >/dev/null; then
  echo "ERROR: minisign not installed. Run: brew install minisign" >&2
  exit 1
fi

TMP=$(mktemp -d)
trap "rm -rf $TMP" EXIT

PRIV="$TMP/tauri.key"
PUB="$TMP/tauri.pub"

echo "==> Generating new minisign keypair"
echo "    You'll be prompted for a password. Use something strong."
echo "    Save BOTH the password AND the private key in your password manager."
echo ""

minisign -G -p "$PUB" -s "$PRIV"

PUB_B64=$(base64 < "$PUB" | tr -d '\n')

echo ""
echo "==> Updating apps/desktop/src-tauri/tauri.conf.json"

# Use python for robust JSON edit
python3 <<EOF
import json, pathlib
p = pathlib.Path("apps/desktop/src-tauri/tauri.conf.json")
data = json.loads(p.read_text())
data.setdefault("plugins", {}).setdefault("updater", {})["pubkey"] = "${PUB_B64}"
p.write_text(json.dumps(data, indent=2) + "\n")
print("Updated pubkey in", p)
EOF

echo ""
echo "================================================================"
echo "  TAURI_SIGNING_PRIVATE_KEY (save this — it will not be shown again):"
echo "================================================================"
cat "$PRIV"
echo "================================================================"
echo ""
echo "Public key (now in tauri.conf.json):"
echo "$PUB_B64"
echo ""
echo "Next steps:"
echo "  1. Save the private key content above to .secrets.env (see verify-secrets.sh)"
echo "  2. Save the password you just typed to .secrets.env as TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
echo "  3. Commit the updated tauri.conf.json (the pubkey change is safe to publish)"
