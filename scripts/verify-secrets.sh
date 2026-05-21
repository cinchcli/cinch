#!/usr/bin/env bash
# Verifies all 10 GitHub Actions secrets are correctly set up.
#
# Usage:
#   1. Copy scripts/.secrets.env.template to .secrets.env (gitignored)
#   2. Fill in real values
#   3. ./scripts/verify-secrets.sh
#
# Each secret runs a real verification check (sign+verify, import cert, API call).
# Reports pass/fail per secret with details.

set -uo pipefail   # no -e so we continue past individual failures

cd "$(dirname "$0")/.."

ENV_FILE=".secrets.env"
if [ ! -f "$ENV_FILE" ]; then
  echo "ERROR: $ENV_FILE not found"
  echo "       Run: cp scripts/.secrets.env.template .secrets.env"
  echo "       Then fill in real values."
  exit 1
fi

# shellcheck disable=SC1090
set -a; source "$ENV_FILE"; set +a

pass=0
fail=0
declare -a failed

check() {
  local name="$1"
  shift
  echo -n "[$name] "
  if "$@" > /tmp/verify-$name.log 2>&1; then
    echo "OK"
    pass=$((pass + 1))
  else
    echo "FAIL"
    failed+=("$name")
    sed 's/^/    /' /tmp/verify-$name.log
    fail=$((fail + 1))
  fi
}

require_var() {
  local name="$1"
  if [ -z "${!name:-}" ]; then
    echo "[$name] FAIL (not set in .secrets.env)"
    failed+=("$name")
    fail=$((fail + 1))
    return 1
  fi
  return 0
}

# --- RELAY_SYNC_TOKEN ---
check_relay_token() {
  require_var RELAY_SYNC_TOKEN || return 1
  curl -fsS -H "Authorization: token $RELAY_SYNC_TOKEN" \
    https://api.github.com/repos/cinchcli/relay | grep -q '"name": "relay"'
}
check RELAY_SYNC_TOKEN check_relay_token

# --- HOMEBREW_TAP_TOKEN ---
check_homebrew_token() {
  require_var HOMEBREW_TAP_TOKEN || return 1
  curl -fsS -H "Authorization: token $HOMEBREW_TAP_TOKEN" \
    https://api.github.com/repos/cinchcli/homebrew-tap | grep -q '"name": "homebrew-tap"'
}
check HOMEBREW_TAP_TOKEN check_homebrew_token

# --- TAURI_SIGNING_* ---
check_tauri_signing() {
  require_var TAURI_SIGNING_PRIVATE_KEY || return 1
  require_var TAURI_SIGNING_PRIVATE_KEY_PASSWORD || return 1
  command -v minisign >/dev/null || { echo "minisign not installed (brew install minisign)"; return 1; }

  local tmp
  tmp=$(mktemp -d)
  trap "rm -rf $tmp" RETURN

  printf '%s' "$TAURI_SIGNING_PRIVATE_KEY" > "$tmp/key"
  echo "test message $(date)" > "$tmp/msg"

  # extract pubkey from tauri.conf.json
  python3 -c "
import json, base64, pathlib, sys
data = json.loads(pathlib.Path('apps/desktop/src-tauri/tauri.conf.json').read_text())
pub_b64 = data['plugins']['updater']['pubkey']
sys.stdout.buffer.write(base64.b64decode(pub_b64))
" > "$tmp/pub"

  # Sign
  echo "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" | minisign -S -s "$tmp/key" -m "$tmp/msg" 2>&1 || return 1

  # Verify against tauri.conf.json pubkey
  minisign -V -p "$tmp/pub" -m "$tmp/msg" 2>&1
}
check TAURI_SIGNING check_tauri_signing

# --- APPLE_CERTIFICATE + IDENTITY ---
check_apple_cert() {
  require_var APPLE_CERTIFICATE || return 1
  require_var APPLE_CERTIFICATE_PASSWORD || return 1
  require_var APPLE_SIGNING_IDENTITY || return 1

  local tmp
  tmp=$(mktemp -d)
  trap "rm -rf $tmp; security delete-keychain verify-secrets.keychain 2>/dev/null || true" RETURN

  echo "$APPLE_CERTIFICATE" | base64 -d > "$tmp/cert.p12"

  security create-keychain -p "" verify-secrets.keychain 2>&1 || return 1
  security set-keychain-settings verify-secrets.keychain
  security import "$tmp/cert.p12" -k verify-secrets.keychain -P "$APPLE_CERTIFICATE_PASSWORD" -A 2>&1 || return 1

  # find identity
  security find-identity -v -p codesigning verify-secrets.keychain | grep -F "$APPLE_SIGNING_IDENTITY"
}
check APPLE_CERTIFICATE check_apple_cert

# --- APPLE notarytool credentials ---
check_apple_notarytool() {
  require_var APPLE_ID || return 1
  require_var APPLE_PASSWORD || return 1
  require_var APPLE_TEAM_ID || return 1

  xcrun notarytool history \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    2>&1 | head -10
}
check APPLE_NOTARYTOOL check_apple_notarytool

# --- summary ---
echo ""
echo "================================================================"
echo "  Summary: $pass passed, $fail failed"
if [ $fail -gt 0 ]; then
  echo "  Failed:"
  for n in "${failed[@]}"; do echo "    - $n"; done
fi
echo "================================================================"

if [ $fail -gt 0 ]; then
  exit 1
fi

echo ""
echo "All secrets verified! Now upload to GitHub:"
echo ""
echo "  gh secret set RELAY_SYNC_TOKEN -R cinchcli/cinch --body \"\$RELAY_SYNC_TOKEN\""
echo "  gh secret set HOMEBREW_TAP_TOKEN -R cinchcli/cinch --body \"\$HOMEBREW_TAP_TOKEN\""
echo "  gh secret set TAURI_SIGNING_PRIVATE_KEY -R cinchcli/cinch --body \"\$TAURI_SIGNING_PRIVATE_KEY\""
echo "  gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R cinchcli/cinch --body \"\$TAURI_SIGNING_PRIVATE_KEY_PASSWORD\""
echo "  gh secret set APPLE_CERTIFICATE -R cinchcli/cinch --body \"\$APPLE_CERTIFICATE\""
echo "  gh secret set APPLE_CERTIFICATE_PASSWORD -R cinchcli/cinch --body \"\$APPLE_CERTIFICATE_PASSWORD\""
echo "  gh secret set APPLE_SIGNING_IDENTITY -R cinchcli/cinch --body \"\$APPLE_SIGNING_IDENTITY\""
echo "  gh secret set APPLE_ID -R cinchcli/cinch --body \"\$APPLE_ID\""
echo "  gh secret set APPLE_PASSWORD -R cinchcli/cinch --body \"\$APPLE_PASSWORD\""
echo "  gh secret set APPLE_TEAM_ID -R cinchcli/cinch --body \"\$APPLE_TEAM_ID\""
echo ""
echo "Or run: ./scripts/upload-secrets.sh"
