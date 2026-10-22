#!/usr/bin/env python3
"""Resume an incomplete draft release without modifying a published release."""

import hashlib
import json
import os
from pathlib import Path
import subprocess

from release import ROOT, metadata


def gh(*args: str) -> None:
    subprocess.run(["gh", *args], cwd=ROOT, check=True)


def release_info(repository: str, tag: str) -> dict | None:
    result = subprocess.run(
        ["gh", "api", f"repos/{repository}/releases/tags/{tag}"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        if "(HTTP 404)" in result.stderr:
            return None
        raise RuntimeError(f"Unable to inspect the existing release: {result.stderr.strip()}")
    return json.loads(result.stdout)


def require_draft(info: dict | None, tag: str) -> dict:
    if info is None:
        raise RuntimeError(f"The {tag} draft disappeared; refusing to continue")
    if not info["draft"]:
        raise RuntimeError(f"{tag} is already published. Its notes and assets have not been changed; refusing to overwrite it.")
    return info


def verify_assets(info: dict, files: dict[str, Path]) -> None:
    remote = {asset["name"]: asset for asset in info["assets"]}
    if remote.keys() != files.keys():
        raise RuntimeError("The draft does not contain exactly the complete local release asset set")
    for name, path in files.items():
        asset = remote[name]
        if asset["size"] != path.stat().st_size:
            raise RuntimeError(f"Uploaded asset size mismatch: {name}")
        if digest := asset.get("digest"):
            with path.open("rb") as stream:
                expected = "sha256:" + hashlib.file_digest(stream, "sha256").hexdigest()
            if digest != expected:
                raise RuntimeError(f"Uploaded asset SHA-256 mismatch: {name}")


def main() -> None:
    version, _ = metadata()
    tag = f"v{version}"
    repository = os.environ["GITHUB_REPOSITORY"]
    files = {path.name: path for path in (ROOT / "dist").iterdir() if path.is_file()}
    if "SHA256SUMS.txt" not in files:
        raise RuntimeError("Run release.py checksums before publishing")
    info = release_info(repository, tag)
    if info is not None:
        draft = require_draft(info, tag)
        extra = {asset["name"] for asset in draft["assets"]} - files.keys()
        if extra:
            raise RuntimeError(f"Existing draft has unexpected assets; review them before retrying: {sorted(extra)}")
        print(f"Resuming draft {tag}.")
        gh("release", "edit", tag, "--title", f"RustSend {version}", "--notes-file", f"docs/releases/{version}.md")
    else:
        gh("release", "create", tag, "--draft", "--verify-tag", "--title", f"RustSend {version}", "--notes-file", f"docs/releases/{version}.md")
    require_draft(release_info(repository, tag), tag)
    gh("release", "upload", tag, *(str(files[name]) for name in sorted(files)), "--clobber")
    draft = require_draft(release_info(repository, tag), tag)
    verify_assets(draft, files)
    gh("release", "edit", tag, "--draft=false", "--latest")
    print(draft["html_url"])


if __name__ == "__main__":
    main()
