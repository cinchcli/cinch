#!/usr/bin/env bash
# Computes the next version for a release.
#
# Usage: ./scripts/next-version.sh <channel> <bump>
#   channel: release | nightly
#   bump:    patch | minor | major | none  (release channel only; nightly ignores bump)
#
# Outputs: NEW_VERSION on stdout (e.g., "0.3.1" or "0.3.20260521" or "0.3.20260521-2")
# Logic:
#   release: read latest `release/X.Y.Z` tag, apply bump. If no tag exists, start from current
#            workspace version (read from crates/cli/Cargo.toml) so the first release uses
#            the version the monorepo has been carrying through Phases 1-3.
#   nightly: NIGHTLY_VERSION = 0.<RELEASE_MINOR>.<YYYYMMDD>; if same date already published
#            (another nightly that day), append `-N` suffix incrementing the largest existing N.

set -euo pipefail

CHANNEL="${1:?channel required: release|nightly}"
BUMP="${2:-patch}"

# Helper: read current workspace version from crates/cli/Cargo.toml
current_workspace_version() {
  grep -m1 '^version = ' "$(dirname "$0")/../crates/cli/Cargo.toml" | cut -d'"' -f2
}

case "$CHANNEL" in
  release)
    LATEST=$(git tag --list 'release/*' | sort -V | tail -n1 | sed 's|^release/||')
    if [ -z "$LATEST" ]; then
      LATEST=$(current_workspace_version)
    fi
    IFS='.' read -r MAJOR MINOR PATCH <<< "$LATEST"
    case "$BUMP" in
      patch) PATCH=$((PATCH + 1)) ;;
      minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
      major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
      none)  ;;
      *) echo "invalid bump: $BUMP" >&2; exit 1 ;;
    esac
    echo "${MAJOR}.${MINOR}.${PATCH}"
    ;;
  nightly)
    DATE=$(date +%Y%m%d)
    # Use current workspace MAJOR.MINOR as the nightly's MAJOR.MINOR; date is the patch number
    CURRENT=$(current_workspace_version)
    IFS='.' read -r CURR_MAJOR CURR_MINOR _ <<< "$CURRENT"
    BASE="${CURR_MAJOR}.${CURR_MINOR}.${DATE}"
    # Check for existing tags from today
    EXISTING_TODAY=$(git tag --list "nightly/${BASE}*" | sort -V)
    if [ -z "$EXISTING_TODAY" ]; then
      echo "$BASE"
    else
      # Find largest -N suffix among today's tags (where bare BASE counts as N=0)
      LARGEST=0
      while read -r tag; do
        tag_version=${tag#nightly/}
        if [ "$tag_version" = "$BASE" ]; then
          LARGEST=$((LARGEST > 0 ? LARGEST : 0))
        else
          suffix=${tag_version#${BASE}-}
          if [[ "$suffix" =~ ^[0-9]+$ ]] && [ "$suffix" -gt "$LARGEST" ]; then
            LARGEST=$suffix
          fi
        fi
      done <<< "$EXISTING_TODAY"
      NEXT=$((LARGEST + 1))
      echo "${BASE}-${NEXT}"
    fi
    ;;
  *)
    echo "invalid channel: $CHANNEL (must be release or nightly)" >&2
    exit 1
    ;;
esac
