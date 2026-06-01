#!/usr/bin/env bash
# Generates GitButler-style categorized release notes from conventional commits.
#
# Usage: ./scripts/generate-release-notes.sh <channel> <version> > release-notes.md
#
# Reads `git log <prev-tag>..HEAD` and routes each commit into a section by
# its conventional-commit scope:
#   - feat|fix|refactor (desktop)            -> ## Desktop
#   - feat|fix|refactor (cli)                -> ## CLI
#   - feat|fix|refactor (client-core)        -> ## Client library
#   - ci:* / feat|fix|chore (ci)             -> ## CI / Release
#   - docs:*                                 -> ## Documentation
#   - feat|fix (no scope, or unmatched)      -> ## Changes & Fixes
#   - chore:* / release:* / merge commits    -> skipped
#
# Falls back to a single flat list if zero commits land in known categories.

set -euo pipefail

CHANNEL="${1:?channel required}"
VERSION="${2:?version required}"

REPO_URL="https://github.com/cinchcli/cinch"

PREV_TAG=$(git tag --list "${CHANNEL}/*" | sort -V | tail -n1 || true)

if [ -z "$PREV_TAG" ]; then
  RANGE="HEAD"
  COMPARE_FROM=""
else
  RANGE="${PREV_TAG}..HEAD"
  COMPARE_FROM="$PREV_TAG"
fi

# Portable accumulators (macOS ships bash 3.2 — no mapfile, no declare -A).
SEC_DESKTOP=""
SEC_CLI=""
SEC_CORE=""
SEC_CI=""
SEC_DOCS=""
SEC_OTHER=""

append() {
  # $1: variable name; $2: line to append (each line ends with \n)
  local var="$1" line="$2"
  printf -v "$var" '%s- %s\n' "${!var}" "$line"
}

while IFS= read -r subject; do
  case "$subject" in
    release:*|chore\(release\):*)
      ;;  # skip release-bot commits
    feat\(desktop\):*|fix\(desktop\):*|refactor\(desktop\):*)
      append SEC_DESKTOP "$subject" ;;
    feat\(cli\):*|fix\(cli\):*|refactor\(cli\):*)
      append SEC_CLI "$subject" ;;
    feat\(client-core\):*|fix\(client-core\):*|refactor\(client-core\):*)
      append SEC_CORE "$subject" ;;
    ci:*|feat\(ci\):*|fix\(ci\):*|chore\(ci\):*)
      append SEC_CI "$subject" ;;
    docs:*|docs\(*\):*)
      append SEC_DOCS "$subject" ;;
    chore:*)
      ;;  # skip generic chores
    feat:*|fix:*|refactor:*|perf:*|feat\(*\):*|fix\(*\):*|refactor\(*\):*|perf\(*\):*)
      append SEC_OTHER "$subject" ;;
    *)
      append SEC_OTHER "$subject" ;;
  esac
done < <(git log --no-merges --pretty=format:"%s" "$RANGE")

emit_section() {
  local title="$1" body="$2"
  if [ -z "$body" ]; then
    return
  fi
  printf '## %s\n%s\n' "$title" "$body"
}

# If everything fell into nothing (no conventional commits at all), bail to
# the original flat list so we never produce empty notes.
if [ -z "$SEC_DESKTOP$SEC_CLI$SEC_CORE$SEC_CI$SEC_DOCS$SEC_OTHER" ]; then
  echo "## Changes"
  git log --no-merges --pretty=format:"- %s" "$RANGE"
  echo
else
  emit_section "Changes & Fixes" "$SEC_OTHER"
  emit_section "Desktop"          "$SEC_DESKTOP"
  emit_section "CLI"              "$SEC_CLI"
  emit_section "Client library"   "$SEC_CORE"
  emit_section "CI / Release"     "$SEC_CI"
  emit_section "Documentation"    "$SEC_DOCS"
fi

if [ -n "$COMPARE_FROM" ]; then
  echo "**Full Changelog**: ${REPO_URL}/compare/${COMPARE_FROM}...${CHANNEL}/${VERSION}"
  echo
fi

cat <<EOF
## Downloads

| Platform | Install |
|---|---|
| macOS (Apple Silicon) desktop + CLI | \`brew install --cask cinchcli/tap/cinch\` |
| macOS CLI only                      | \`brew install cinchcli/tap/cinch\` |
| Linux CLI only                      | \`curl -fsSL https://cinchcli.com/install.sh | sh\` |
| Windows CLI                         | Download \`cinch-cli-x86_64-pc-windows-msvc.zip\` below |
| Direct binaries                     | See assets below |
EOF
