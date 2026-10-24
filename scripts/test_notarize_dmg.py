"""Check notarization failure handling without Apple credentials or macOS tools."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("notarize-dmg.sh")
MOCK = '''#!/usr/bin/env python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
step = " ".join(sys.argv[1:3]) if name == "xcrun" else name
with open(os.environ["CALL_LOG"], "a") as log:
    log.write(step + "\\n")
if step == os.environ.get("FAIL_STEP"):
    sys.exit(1)
if step == "notarytool submit":
    print(json.dumps({"status": os.environ["NOTARY_STATUS"]}))
'''


class NotarizeDmgTests(unittest.TestCase):
    def run_case(self, status="Accepted", fail=""):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ["xcrun", "codesign", "spctl"]:
                tool = root / name
                tool.write_text(MOCK)
                tool.chmod(0o755)
            image = root / "Cinch test.dmg"
            image.write_bytes(b"synthetic disk image")
            log = root / "calls.log"
            env = {**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"],
                   "APPLE_ID": "test@example.invalid", "APPLE_PASSWORD": "test-only",
                   "APPLE_TEAM_ID": "TEST", "CALL_LOG": str(log),
                   "NOTARY_STATUS": status, "FAIL_STEP": fail}
            result = subprocess.run(["bash", str(SCRIPT), str(image)], env=env,
                                    capture_output=True, text=True, timeout=10)
            return result.returncode, log.read_text().splitlines()

    def test_accepted_image_runs_every_gate(self):
        code, calls = self.run_case()
        self.assertEqual(code, 0)
        self.assertEqual(calls, ["notarytool submit", "stapler staple", "stapler validate", "codesign", "spctl"])

    def test_rejected_submission_stops_before_stapling(self):
        code, calls = self.run_case(status="Invalid")
        self.assertNotEqual(code, 0)
        self.assertEqual(calls, ["notarytool submit"])

    def test_transport_failure_stops_before_stapling(self):
        code, calls = self.run_case(fail="notarytool submit")
        self.assertNotEqual(code, 0)
        self.assertEqual(calls, ["notarytool submit"])

    def test_stapling_failure_stops_verification(self):
        code, calls = self.run_case(fail="stapler staple")
        self.assertNotEqual(code, 0)
        self.assertEqual(calls, ["notarytool submit", "stapler staple"])

    def test_gatekeeper_rejection_fails_the_gate(self):
        code, calls = self.run_case(fail="spctl")
        self.assertNotEqual(code, 0)
        self.assertEqual(calls[-1], "spctl")


if __name__ == "__main__":
    unittest.main()
