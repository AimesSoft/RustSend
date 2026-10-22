#!/usr/bin/env python3
"""Validate release metadata, collect Bern products, and hash the final assets."""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def manifest(path: Path) -> dict:
    with path.open("rb") as stream:
        return tomllib.load(stream)


def metadata() -> tuple[str, str]:
    version = manifest(ROOT / "Bern.toml")["app"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Bern.toml must use a three-part release version, e.g. 1.0.0")
    for package in ("engine", "protocol", "ui"):
        actual = manifest(ROOT / "bern" / package / "Cargo.toml")["package"]["version"]
        if actual != version:
            raise ValueError(f"bern/{package}/Cargo.toml is {actual}, expected {version}")
    notes = ROOT / "docs" / "releases" / f"{version}.md"
    if not notes.is_file() or not notes.read_text(encoding="utf-8").strip():
        raise ValueError(f"Missing release notes: {notes.relative_to(ROOT)}")
    return version, manifest(ROOT / "rust-toolchain.toml")["toolchain"]["channel"]


def validate() -> None:
    version, rust = metadata()
    tag = f"v{version}"
    if os.environ.get("GITHUB_REF_TYPE") == "tag":
        if os.environ.get("GITHUB_REF_NAME") != tag:
            raise ValueError(f"Tag must be {tag}, matching Bern.toml and the three app crates")
    publish = os.environ.get("PUBLISH", "false").lower() == "true"
    if publish:
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        tagged = subprocess.check_output(
            ["git", "rev-parse", f"refs/tags/{tag}^{{commit}}"], cwd=ROOT, text=True
        ).strip()
        if head != tagged:
            raise ValueError(f"Publishing requires the checked-out commit to be tagged {tag}")
    outputs = {"version": version, "tag": tag, "rust": rust, "publish": str(publish).lower()}
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a", encoding="utf-8") as stream:
            stream.writelines(f"{key}={value}\n" for key, value in outputs.items())
    for key, value in outputs.items():
        print(f"{key}={value}")


def copy_product(source: Path, destination: Path) -> None:
    if not source.is_file():
        raise FileNotFoundError(f"Expected Bern product was not built: {source}")
    shutil.copy2(source, destination)
    print(f"{destination.name}: {destination.stat().st_size:,} bytes")


def collect(args: argparse.Namespace) -> None:
    version, _ = metadata()
    output = ROOT / "dist"
    output.mkdir(exist_ok=True)
    name = f"RustSend-{version}-{args.platform}-{args.arch}"
    built = ROOT / "target" / "bern"
    if args.platform == "macOS":
        app = built / "macos" / "RustSend.app"
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
        subprocess.run(
            ["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(output / f"{name}.zip")],
            check=True,
        )
    elif args.platform == "Linux":
        copy_product(built / "dist" / f"RustSend-{version}-linux-{args.arch}.tar.gz", output / f"{name}.tar.gz")
        deb_arch = {"x64": "amd64", "arm64": "arm64"}[args.arch]
        copy_product(built / "dist" / f"rust-send_{version}_{deb_arch}.deb", output / f"{name}.deb")
    elif args.platform == "Windows":
        for suffix in (".zip", "-setup.exe"):
            copy_product(built / "dist" / f"RustSend-{version}-windows-{args.arch}{suffix}", output / f"{name}{suffix}")
    elif args.platform == "Android":
        if os.environ.get("ANDROID_RELEASE_SIGNED") != "true":
            if os.environ.get("PUBLISH", "false").lower() == "true":
                raise ValueError("Refusing to publish an Android APK without the persistent release key")
            name += "-debug"
        copy_product(built / "android" / "RustSend.apk", output / f"{name}.apk")
        # Matching dex metadata enables ahead-of-time compilation on Android 12+.
        copy_product(built / "android" / "RustSend.dm", output / f"{name}.dm")
    elif args.platform == "iOS":
        app = built / "ios" / "RustSend.app"
        if not (app / "RustSend").is_file():
            raise FileNotFoundError(f"Missing iOS device executable: {app}")
        with tempfile.TemporaryDirectory(prefix="rustsend-ipa-") as temporary:
            payload = Path(temporary) / "Payload"
            payload.mkdir()
            shutil.copytree(app, payload / "RustSend.app", symlinks=True)
            subprocess.run(
                ["ditto", "-c", "-k", "--keepParent", str(payload), str(output / f"{name}-unsigned.ipa")],
                check=True,
            )
    else:
        raise ValueError(f"Unknown platform: {args.platform}")


def checksums() -> None:
    version, _ = metadata()
    output = ROOT / "dist"
    expected = {
        f"RustSend-{version}-macOS-arm64.zip",
        f"RustSend-{version}-Linux-x64.tar.gz",
        f"RustSend-{version}-Linux-x64.deb",
        f"RustSend-{version}-Windows-x64.zip",
        f"RustSend-{version}-Windows-x64-setup.exe",
        f"RustSend-{version}-Android-arm64.apk",
        f"RustSend-{version}-Android-arm64.dm",
        f"RustSend-{version}-iOS-arm64-unsigned.ipa",
    }
    actual = {file.name for file in output.iterdir() if file.is_file()}
    if actual != expected:
        raise ValueError(f"Incomplete or unexpected release assets. Missing: {expected - actual}; extra: {actual - expected}")
    with (output / "SHA256SUMS.txt").open("w", encoding="utf-8", newline="\n") as stream:
        for name in sorted(expected):
            path = output / name
            if not path.stat().st_size:
                raise ValueError(f"Empty release asset: {name}")
            with path.open("rb") as asset:
                digest = hashlib.file_digest(asset, "sha256").hexdigest()
            stream.write(f"{digest}  {name}\n")
    print((output / "SHA256SUMS.txt").read_text(encoding="utf-8"))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("validate")
    package = commands.add_parser("collect")
    package.add_argument("--platform", choices=("macOS", "Linux", "Windows", "Android", "iOS"), required=True)
    package.add_argument("--arch", choices=("arm64", "x64"), required=True)
    commands.add_parser("checksums")
    args = parser.parse_args()
    if args.command == "validate":
        validate()
    elif args.command == "collect":
        collect(args)
    else:
        checksums()


if __name__ == "__main__":
    main()
