#!/usr/bin/env python3
"""Materialize the persistent CI Android key without printing secret values."""

import base64
import os
from pathlib import Path


def main() -> None:
    required = ("ANDROID_KEYSTORE_BASE64", "ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_ALIAS")
    present = [bool(os.environ.get(name)) for name in required]
    publish = os.environ.get("PUBLISH", "false").lower() == "true"
    if not all(present):
        if any(present) or publish:
            raise SystemExit("Android release signing requires ANDROID_KEYSTORE_BASE64, ANDROID_KEYSTORE_PASSWORD, and ANDROID_KEY_ALIAS repository secrets")
        print("Build-only run: using a debug certificate; the APK filename will include -debug.")
        return
    path = Path(os.environ["RUNNER_TEMP"]) / "rustsend-release.keystore"
    key = base64.b64decode(os.environ["ANDROID_KEYSTORE_BASE64"], validate=True)
    if not key:
        raise SystemExit("The Android release keystore is empty")
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(key)
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as stream:
        stream.write(f"BERN_ANDROID_KEYSTORE={path}\nANDROID_RELEASE_SIGNED=true\n")
    print("Persistent Android release key prepared.")


if __name__ == "__main__":
    main()
