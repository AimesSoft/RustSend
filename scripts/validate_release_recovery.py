#!/usr/bin/env python3
"""Allow publication recovery only from this tag's successful original builds."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib


def api(path: str, *, paginate: bool = False):
    command = ["gh", "api"]
    if paginate:
        command.extend(["--paginate", "--slurp"])
    command.append(path)
    return json.loads(subprocess.check_output(command, text=True))


def main() -> None:
    root = Path.cwd()
    repository = os.environ["GITHUB_REPOSITORY"]
    tag = os.environ["RELEASE_TAG"]
    run_id = os.environ["SOURCE_RUN_ID"]
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag) or not re.fullmatch(r"[1-9]\d*", run_id):
        raise ValueError("Expected a vMAJOR.MINOR.PATCH tag and a numeric workflow run ID")
    with (root / "Bern.toml").open("rb") as stream:
        version = tomllib.load(stream)["app"]["version"]
    if tag != f"v{version}":
        raise ValueError("Requested tag does not match the checked-out application version")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    tagged = subprocess.check_output(["git", "rev-parse", f"refs/tags/{tag}^{{commit}}"], text=True).strip()
    if head != tagged:
        raise ValueError("Recovery must check out the exact tagged commit")
    subprocess.run([sys.executable, "scripts/release.py", "validate"], check=True)

    run = api(f"repos/{repository}/actions/runs/{run_id}")
    if run["repository"]["full_name"].lower() != repository.lower():
        raise ValueError("Source workflow run belongs to another repository")
    if run["path"] != ".github/workflows/release.yml" or run["event"] not in ("push", "workflow_dispatch"):
        raise ValueError("Source must be this repository's Release workflow, triggered by push or workflow_dispatch")
    if run["head_sha"] != tagged or run["status"] != "completed":
        raise ValueError("Source run must be completed and built from the exact tagged commit")

    # Re-running only Linux retains successful jobs from the earlier attempt.
    # Select each job's latest result; an older success must never mask a failure.
    pages = api(f"repos/{repository}/actions/runs/{run_id}/jobs?filter=all&per_page=100", paginate=True)
    latest = {}
    for page in pages:
        for job in page["jobs"]:
            previous = latest.get(job["name"])
            order = (job["run_attempt"], job["id"])
            if previous is None or order > (previous["run_attempt"], previous["id"]):
                latest[job["name"]] = job
    required = ("metadata", "quality", "macOS arm64", "Linux x64", "Windows x64", "Android arm64", "iOS arm64")
    for name in required:
        job = latest.get(name)
        if job is None or job["status"] != "completed" or job["conclusion"] != "success":
            raise ValueError(f"The latest result of required job {name!r} must be successful")
        print(f"Verified {name}: attempt {job['run_attempt']}, job {job['id']}")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
        stream.write(f"version={version}\ntag={tag}\nrun_id={run_id}\n")
    print(f"Verified original run {run_id} at {tagged}; application sources and release notes remain from {tag}.")


if __name__ == "__main__":
    main()
