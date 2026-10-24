#!/usr/bin/env bash
# Notarize and verify the signed disk image before publishing it.
set -euo pipefail

dmg="${1:?Usage: notarize-dmg.sh <signed.dmg>}"
[[ -f "$dmg" && "$dmg" == *.dmg ]] || { echo "Expected an existing DMG" >&2; exit 1; }
: "${APPLE_ID:?APPLE_ID required}"
: "${APPLE_PASSWORD:?APPLE_PASSWORD required}"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID required}"

response=$(mktemp)
trap 'rm -f "$response"' EXIT
if ! xcrun notarytool submit "$dmg" \
  --apple-id "$APPLE_ID" \
  --password "$APPLE_PASSWORD" \
  --team-id "$APPLE_TEAM_ID" \
  --wait --timeout 20m --output-format json > "$response"; then
  cat "$response" >&2
  exit 1
fi
python3 - "$response" <<'PY'
import json, sys
with open(sys.argv[1]) as response:
    result = json.load(response)
print("DMG notarization response: " + json.dumps(result), flush=True)
if result.get("status") != "Accepted":
    raise SystemExit("DMG notarization was not accepted: " + str(result.get("status")))
PY
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
codesign --verify --strict "$dmg"
spctl --assess --type open --context context:primary-signature "$dmg"
