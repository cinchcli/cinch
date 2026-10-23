#!/usr/bin/env python3
"""Verify the updater signature and the public key embedded in the macOS bundle."""

import argparse
import base64
import json
import pathlib
import subprocess
import tarfile
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("config", type=pathlib.Path)
parser.add_argument("archive", type=pathlib.Path)
args = parser.parse_args()
key = json.loads(args.config.read_text())["plugins"]["updater"]["pubkey"]

with tarfile.open(args.archive, "r:gz") as archive:
    binary = archive.extractfile("Cinch.app/Contents/MacOS/Cinch")
    if binary is None or key.encode() not in binary.read():
        raise SystemExit("Updater public key not found in the packaged application")

with tempfile.TemporaryDirectory(prefix="cinch-updater-") as directory:
    public_key = pathlib.Path(directory) / "updater.pub"
    signature = pathlib.Path(directory) / "updater.sig"
    public_key.write_bytes(base64.b64decode(key, validate=True))
    encoded_signature = pathlib.Path(str(args.archive) + ".sig").read_text().strip()
    signature.write_bytes(base64.b64decode(encoded_signature, validate=True))
    subprocess.run(
        ["minisign", "-V", "-p", str(public_key), "-x", str(signature), "-m", str(args.archive)],
        check=True,
    )
