#!/usr/bin/env python3
"""Dispatch the existing Publish workflow from verified remote main."""

import argparse
import json
import shlex
import subprocess
import sys

REPO = "cinchcli/cinch"


def api(path):
    return json.loads(subprocess.check_output(
        ["gh", "api", f"repos/{REPO}/{path}"], text=True,
    ))


def verified_main(expected_sha=None):
    sha = api("git/ref/heads/main")["object"]["sha"]
    if expected_sha and sha != expected_sha:
        raise RuntimeError("Remote main changed. Check its CI and run release again.")
    runs = api(
        f"actions/workflows/ci.yml/runs?branch=main&event=push&head_sha={sha}&per_page=1"
    )["workflow_runs"]
    if not runs:
        raise RuntimeError(f"No main CI run found for {sha}. Merge a PR and wait for CI.")
    run = runs[0]
    if (run["head_sha"] != sha or run["head_branch"] != "main"
            or run["event"] != "push" or run["status"] != "completed"
            or run["conclusion"] != "success"):
        raise RuntimeError(f"Main CI has not passed: {run['html_url']}")
    return sha, run["html_url"]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bump", nargs="?", default="patch", choices=("patch", "minor", "major"))
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", help="check and print; do not dispatch")
    mode.add_argument("--check", action="store_true", help="only verify main and CI (workflow gate)")
    parser.add_argument("--expected-sha", help="reject a different main SHA")
    args = parser.parse_args(argv)

    try:
        sha, ci_url = verified_main(args.expected_sha)
        print(f"Release source: {REPO} main @ {sha}", flush=True)
        print(f"CI: {ci_url}", flush=True)
        print("Local branches and uncommitted changes are not included.", flush=True)
        if args.check:
            return 0
        command = [
            "gh", "workflow", "run", "publish.yml", "--repo", REPO, "--ref", "main",
            "-f", "channel=release", "-f", f"bump={args.bump}",
            "-f", "dry_run=false", "-f", f"expected_sha={sha}",
        ]
        if args.dry_run:
            print("Dry run: " + shlex.join(command))
            return 0
        subprocess.run(command, check=True)
        print(f"Dispatched. Follow https://github.com/{REPO}/actions/workflows/publish.yml")
        return 0
    except (RuntimeError, subprocess.CalledProcessError, OSError, ValueError, KeyError) as error:
        print(f"Release stopped: {error}", file=sys.stderr)
        print("Inspect GitHub Actions before retrying; do not dispatch twice.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
