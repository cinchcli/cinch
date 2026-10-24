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

    def api(self, path):
        self.paths.append(path)
        if path == "git/ref/heads/main":
            return {"object": {"sha": self.sha}}
        self.assertIn(f"head_sha={self.sha}", path)
        return {"workflow_runs": self.runs}

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
