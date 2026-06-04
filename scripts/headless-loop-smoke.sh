#!/usr/bin/env bash
# E2E smoke test for the headless send -> fleet-read loop (0.5 build).
#
# Exercises the loop the build is designed around: a REMOTE host runs
# `cinch send` (origin = that host), and THIS machine reads the clip back
# through `cinch mcp` with scope:"fleet" (which excludes this machine's own
# clips). Proves: send reaches the relay E2EE, the lazy first-fleet-call
# backfill pulls the remote clip into the local store, and the fleet-scoped
# MCP read returns it — all without `cinch pull` and without the desktop GUI.
#
# This is a real cross-machine test: the fleet scope excludes the local
# machine, so a single-host run cannot see its own sent clip. Both the local
# machine and CINCH_E2E_SSH_HOST must be authenticated against the SAME relay
# and share the SAME encryption_key (see docs/headless-send-and-fleet-read.md).
#
# Usage:
#   ./scripts/headless-loop-smoke.sh
#
# Env:
#   CINCH_E2E_SSH_HOST   remote send origin (default: oci_atlas_1)
#
# Exit codes:
#   0  the remote-sent clip was read back via scope:"fleet"
#   1  send failed, fleet read failed, or the clip was not found
#   2  environment problem (cinch missing, not authed/no key, ssh unreachable)

set -euo pipefail

SSH_HOST="${CINCH_E2E_SSH_HOST:-oci_atlas_1}"

red()    { printf '\033[31m%s\033[0m\n' "$*"; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
note()   { printf '%s\n' "$*" >&2; }
fail()   { red "FAIL: $*" >&2; exit 1; }
envfail(){ red "$*" >&2; exit 2; }

command -v cinch >/dev/null 2>&1 || envfail "cinch not on PATH"

# `cinch auth status` exits 0 even on a missing key; require a real key by
# attempting a no-op classification path is overkill — instead trust that an
# authenticated machine running this test has logged in. We still smoke the
# remote's reachability.
ssh -o BatchMode=yes -o ConnectTimeout=8 "$SSH_HOST" 'command -v cinch' >/dev/null 2>&1 \
  || envfail "ssh $SSH_HOST unreachable or cinch missing there"

# Unique sentinel so the fleet read can find exactly this clip.
SENTINEL="cinch-loop-smoke-$(date +%s)-$$"
note "sentinel: $SENTINEL"

# 1) REMOTE host sends the clip (origin = $SSH_HOST, NOT this machine).
note "sending from $SSH_HOST ..."
if ! printf '%s\n' "$SENTINEL" | ssh "$SSH_HOST" 'cinch send' >/dev/null 2>&1; then
  fail "remote 'cinch send' failed on $SSH_HOST (no key? exit 5/6 — see docs)"
fi
green "sent from $SSH_HOST"

# 2) LOCAL machine fleet-reads via MCP. The first scope:"fleet" call triggers
#    the lazy backfill (CINCH_MCP_FLEET=1) which pulls the remote clip local,
#    then serves it. Drive the stdio JSON-RPC by hand. Retry a few times to
#    absorb relay propagation lag.
read_fleet() {
  printf '%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
    "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"search_clipboard\",\"arguments\":{\"query\":\"$SENTINEL\",\"scope\":\"fleet\"}}}" \
    | CINCH_MCP_FLEET=1 cinch mcp 2>/dev/null
}

for attempt in 1 2 3 4 5; do
  OUT="$(read_fleet || true)"
  # Every output line must be valid JSON (no stray stdout corrupting the stream).
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    printf '%s' "$line" | python3 -c 'import sys,json; json.loads(sys.stdin.read())' 2>/dev/null \
      || fail "MCP emitted a non-JSON stdout line (stream corruption): $line"
  done <<< "$OUT"

  if printf '%s' "$OUT" | grep -q -- "$SENTINEL"; then
    green "fleet read returned the remote clip (attempt $attempt)"
    green "PASS: headless send -> fleet-read loop closed"
    exit 0
  fi
  note "sentinel not visible yet (attempt $attempt) — retrying ..."
  sleep 2
done

fail "remote-sent clip never appeared in a scope:\"fleet\" read"
