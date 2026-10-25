#!/usr/bin/env python3
"""Release dispatch and main-CI gate regressions; no network or publication."""

import contextlib
import io
import subprocess
import unittest
from unittest.mock import patch

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.sha = "a" * 40
        self.run = {
            "head_sha": self.sha, "head_branch": "main", "event": "push",
            "status": "completed", "conclusion": "success", "html_url": "https://ci.test/1",
        }
        self.runs = [self.run]
        self.paths = []
        self.ci_sha = self.sha
        self.head = {
            "commit": {"author": {"name": "Jinmu Go", "email": "dev@example.com"},
                       "message": "feat: change"},
            "parents": [{"sha": "p" * 40}], "files": [],
        }

    def api(self, path):
        self.paths.append(path)
        if path == "git/ref/heads/main":
            return {"object": {"sha": self.sha}}
        if path == f"commits/{self.sha}":
            return self.head
        self.assertIn(f"head_sha={self.ci_sha}", path)
        return {"workflow_runs": self.runs}

    def release_bot_head(self):
        parent = "p" * 40
        self.head = {
            "commit": {"author": {"name": release.RELEASE_BOT[0], "email": release.RELEASE_BOT[1]},
                       "message": "release: release/0.12.1"},
            "parents": [{"sha": parent}],
            "files": [
                {"filename": "Cargo.lock",
                 "patch": '@@ -1 +1 @@\n-version = "0.12.0"\n+version = "0.12.1"'},
                {"filename": "apps/desktop/package.json",
                 "patch": '@@ -1 +1 @@\n-  "version": "0.12.0",\n+  "version": "0.12.1",'},
            ],
        }
        self.run["head_sha"] = parent
        self.ci_sha = parent

    def invoke(self, args):
        with patch.object(release, "api", side_effect=self.api), \
                patch.object(release.subprocess, "run") as dispatch, \
                contextlib.redirect_stdout(io.StringIO()), \
                contextlib.redirect_stderr(io.StringIO()):
            code = release.main(args)
        return code, dispatch

    def test_dispatch_binds_remote_main_and_defaults_to_patch(self):
        code, dispatch = self.invoke([])
        self.assertEqual(code, 0)
        dispatch.assert_called_once_with([
            "gh", "workflow", "run", "publish.yml", "--repo", release.REPO, "--ref", "main",
            "-f", "channel=release", "-f", "bump=patch",
            "-f", "dry_run=false", "-f", f"expected_sha={self.sha}",
        ], check=True)

    def test_minor_and_major(self):
        for bump in ("minor", "major"):
            with self.subTest(bump=bump):
                code, dispatch = self.invoke([bump])
                self.assertEqual(code, 0)
                self.assertIn(f"bump={bump}", dispatch.call_args.args[0])

    def test_dry_run_and_workflow_check_never_dispatch(self):
        for flag in ("--dry-run", "--check"):
            with self.subTest(flag=flag):
                code, dispatch = self.invoke([flag])
                self.assertEqual(code, 0)
                dispatch.assert_not_called()

    def test_moved_main_is_rejected_before_ci_lookup(self):
        code, dispatch = self.invoke(["--expected-sha", "b" * 40])
        self.assertEqual(code, 1)
        self.assertEqual(self.paths, ["git/ref/heads/main"])
        dispatch.assert_not_called()

    def test_missing_ci_is_rejected(self):
        self.runs = []
        code, dispatch = self.invoke([])
        self.assertEqual(code, 1)
        dispatch.assert_not_called()

    def test_untrusted_or_unsuccessful_ci_is_rejected(self):
        for key, value in (
            ("head_sha", "b" * 40), ("head_branch", "feature"),
            ("event", "pull_request"), ("status", "in_progress"),
            ("conclusion", "failure"), ("conclusion", "cancelled"),
        ):
            with self.subTest(key=key, value=value):
                previous = self.run[key]
                self.run[key] = value
                code, dispatch = self.invoke([])
                self.assertEqual(code, 1)
                dispatch.assert_not_called()
                self.run[key] = previous

    def test_release_bot_head_uses_parent_ci(self):
        self.release_bot_head()
        code, dispatch = self.invoke([])
        self.assertEqual(code, 0)
        self.assertIn(f"expected_sha={self.sha}", dispatch.call_args.args[0])
        self.assertIn(f"head_sha={'p' * 40}", self.paths[-1])

    def test_release_bot_head_with_other_changes_needs_its_own_ci(self):
        for change in (
            {"filename": "scripts/release.py", "patch": "+print('x')"},
            {"filename": "Cargo.lock", "patch": '+name = "evil"'},
        ):
            with self.subTest(change=change["filename"]):
                self.release_bot_head()
                self.head["files"].append(change)
                self.ci_sha = self.sha
                code, dispatch = self.invoke([])
                self.assertEqual(code, 1)
                dispatch.assert_not_called()

    def test_api_error_never_dispatches(self):
        with patch.object(release, "api", side_effect=subprocess.CalledProcessError(1, "gh")), \
                patch.object(release.subprocess, "run") as dispatch, \
                contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(release.main([]), 1)
            dispatch.assert_not_called()

    def test_dispatch_failure_is_not_retried(self):
        with patch.object(release, "api", side_effect=self.api), \
                patch.object(release.subprocess, "run",
                             side_effect=subprocess.CalledProcessError(1, "gh")) as dispatch, \
                contextlib.redirect_stdout(io.StringIO()), \
                contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(release.main([]), 1)
            dispatch.assert_called_once()


if __name__ == "__main__":
    unittest.main()
