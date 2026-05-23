#!/usr/bin/env bash
# E2E image roundtrip smoke test for cinch.
#
# Pushes an image clip from the local machine, pulls it back on this
# machine and on a remote machine, and verifies sha256 byte-equality
# end to end. Validates the relay's image transport (push -> wire
# -> store -> pull, plus E2EE round-trip) without exercising the
# desktop GUI.
#
# Usage:
#   ./scripts/e2e-image-roundtrip.sh [image_path]
#
# If no path is given, generates a 1 KB random payload at $TMPDIR
# and pushes it with --type image (the bytes do not have to decode
# as a real image; we only care about byte equality on roundtrip).
#
# Env:
#   CINCH_E2E_SSH_HOST   remote pull target (default: oci_atlas_1)
#
# Exit codes:
#   0  both local and remote pulls byte-match the source
#   1  push failed, pull failed, or checksum mismatch
#   2  environment problem (cinch missing, ssh unreachable, etc.)

set -euo pipefail

SSH_HOST="${CINCH_E2E_SSH_HOST:-oci_atlas_1}"
SOURCE="${1:-}"

red()    { printf '\033[31m%s\033[0m\n' "$*"; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
note()   { printf '%s\n' "$*" >&2; }
fail()   { red "FAIL: $*" >&2; exit 1; }
envfail(){ red "$*" >&2; exit 2; }

sha256_of() {
  shasum -a 256 "$1" | awk '{print $1}'
}

# `cinch auth status` always exits 0 — even on expired/revoked tokens. Parse
# the first line: it's "Authenticated" on healthy state, otherwise an error
# message like "Credentials expired or revoked …".
auth_first_line() {
  cinch auth status 2>&1 | sed -n '1p'
}
remote_auth_first_line() {
  ssh -o BatchMode=yes -o ConnectTimeout=5 "$SSH_HOST" 'cinch auth status' 2>&1 | sed -n '1p'
}

command -v cinch >/dev/null || envfail "cinch not found in PATH"
LOCAL_AUTH="$(auth_first_line)"
[ "$LOCAL_AUTH" = "Authenticated" ] \
  || envfail "local cinch auth: $LOCAL_AUTH (run: cinch auth login)"

ssh -o BatchMode=yes -o ConnectTimeout=5 "$SSH_HOST" 'command -v cinch' >/dev/null 2>&1 \
  || envfail "cannot reach $SSH_HOST or cinch not installed there"
REMOTE_AUTH="$(remote_auth_first_line)"
[ "$REMOTE_AUTH" = "Authenticated" ] \
  || envfail "$SSH_HOST cinch auth: $REMOTE_AUTH (run: ssh $SSH_HOST 'cinch auth login')"

CLEANUP=()
cleanup() {
  for f in "${CLEANUP[@]+"${CLEANUP[@]}"}"; do
    [ -n "$f" ] && rm -f "$f"
  done
}
trap cleanup EXIT

if [ -z "$SOURCE" ]; then
  SOURCE="$(mktemp -t cinch-e2e-src.XXXXXX)"
  CLEANUP+=("$SOURCE")
  dd if=/dev/urandom of="$SOURCE" bs=1024 count=1 2>/dev/null
  note "generated 1 KB random source at $SOURCE"
fi

[ -r "$SOURCE" ] || fail "source not readable: $SOURCE"

SRC_SHA="$(sha256_of "$SOURCE")"
SRC_SIZE="$(wc -c < "$SOURCE" | tr -d ' ')"
note "source: $SOURCE ($SRC_SIZE bytes, sha256=$SRC_SHA)"

LABEL="e2e-roundtrip-$(date +%s)"
note "pushing as label=$LABEL ..."
if ! cinch push --type image --label "$LABEL" --silent < "$SOURCE"; then
  fail "cinch push failed"
fi

# Grace period: relay commit + fan-out to remote subscribers.
sleep 1

LOCAL_PULL="$(mktemp -t cinch-e2e-local.XXXXXX)"
CLEANUP+=("$LOCAL_PULL")
if ! cinch pull --raw > "$LOCAL_PULL"; then
  fail "cinch pull failed on local"
fi
LOCAL_SHA="$(sha256_of "$LOCAL_PULL")"
[ "$LOCAL_SHA" = "$SRC_SHA" ] || fail "local pull mismatch: src=$SRC_SHA got=$LOCAL_SHA"
note "local pull OK"

REMOTE_PULL="$(mktemp -t cinch-e2e-remote.XXXXXX)"
CLEANUP+=("$REMOTE_PULL")
if ! ssh "$SSH_HOST" 'cinch pull --raw' > "$REMOTE_PULL"; then
  fail "cinch pull failed on $SSH_HOST"
fi
REMOTE_SHA="$(sha256_of "$REMOTE_PULL")"
[ "$REMOTE_SHA" = "$SRC_SHA" ] || fail "remote pull mismatch ($SSH_HOST): src=$SRC_SHA got=$REMOTE_SHA"
note "remote pull OK ($SSH_HOST)"

green "PASS: sha256 $SRC_SHA matches on local and $SSH_HOST"
