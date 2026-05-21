#!/usr/bin/env bash
# Removes an agent worktree. Optionally deletes the branch.
# Usage: ./scripts/clean-agent-worktree.sh <agent-task> [--delete-branch]
# Example: ./scripts/clean-agent-worktree.sh claude-refactor-auth --delete-branch

set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <agent-task> [--delete-branch]" >&2
  exit 1
fi

NAME="$1"
DELETE_BRANCH="${2:-}"
WORKTREE_DIR="../${NAME}"

cd "$(dirname "$0")/.."

git worktree remove "$WORKTREE_DIR" --force

if [ "$DELETE_BRANCH" = "--delete-branch" ]; then
  AGENT_BRANCH="agent/${NAME%%-*}/${NAME#*-}"
  git branch -D "$AGENT_BRANCH" || true
fi

echo "Removed worktree $WORKTREE_DIR"
