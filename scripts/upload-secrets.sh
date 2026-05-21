#!/usr/bin/env bash
# Uploads all 10 secrets from .secrets.env to cinchcli/cinch GitHub repo.
# Idempotent — re-uploading overwrites existing secrets.
# Usage: ./scripts/upload-secrets.sh

set -euo pipefail

cd "$(dirname "$0")/.."

ENV_FILE=".secrets.env"
[ -f "$ENV_FILE" ] || { echo "ERROR: $ENV_FILE not found"; exit 1; }

# shellcheck disable=SC1090
set -a; source "$ENV_FILE"; set +a

SECRETS=(
  RELAY_SYNC_TOKEN
  HOMEBREW_TAP_TOKEN
  TAURI_SIGNING_PRIVATE_KEY
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  APPLE_CERTIFICATE
  APPLE_CERTIFICATE_PASSWORD
  APPLE_SIGNING_IDENTITY
  APPLE_ID
  APPLE_PASSWORD
  APPLE_TEAM_ID
)

for name in "${SECRETS[@]}"; do
  if [ -z "${!name:-}" ]; then
    echo "[$name] SKIP (not set)"
    continue
  fi
  printf '%s' "${!name}" | gh secret set "$name" -R cinchcli/cinch
  echo "[$name] uploaded"
done

echo ""
echo "Current secrets on cinchcli/cinch:"
gh secret list -R cinchcli/cinch
