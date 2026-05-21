#!/usr/bin/env bash
# Creates a new agent worktree for parallel work on the cinch monorepo.
# Usage: ./scripts/new-agent-worktree.sh <agent> <task>
# Example: ./scripts/new-agent-worktree.sh claude refactor-auth
# Creates: ../<agent>-<task>/ on branch agent/<agent>/<task> from main

set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <agent> <task>" >&2
  exit 1
fi

AGENT="$1"
TASK="$2"
WORKTREE_DIR="../${AGENT}-${TASK}"
BRANCH="agent/${AGENT}/${TASK}"

cd "$(dirname "$0")/.."

git worktree add -b "$BRANCH" "$WORKTREE_DIR" main
echo "Created worktree at $WORKTREE_DIR on branch $BRANCH"
