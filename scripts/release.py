#!/usr/bin/env python3
"""Prepare, push, validate, tag, publish, and document one fbfast release."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]
REMOTE = "origin"
BRANCH = "main"
POLL_SECONDS = 10


class ReleaseError(RuntimeError):
    pass


def run(*args: str, capture: bool = False, check: bool = True) -> str:
    result = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        capture_output=capture,
        check=False,
    )
    if check and result.returncode:
        detail = (result.stderr if capture else "").strip()
        raise ReleaseError(f"Command failed ({result.returncode}): {' '.join(args)}"
                           + (f"\n{detail}" if detail else ""))
    return result.stdout.strip() if capture else ""


def output(*args: str) -> str:
    return run(*args, capture=True)


def version() -> str:
    return str(tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]["version"])


def release_title(notes: Path, fallback_version: str) -> str:
    for line in notes.read_text().splitlines():
        if line.startswith("# "):
            title = line[2:].strip()
            return re.sub(r"^fbfast\s+", "", title, flags=re.IGNORECASE)
    return fallback_version


def require_tools() -> None:
    missing = [tool for tool in ("git", "gh") if shutil.which(tool) is None]
    if missing:
        raise ReleaseError(f"Required command not found: {', '.join(missing)}")
    run("gh", "auth", "status")


def confirm(prompt: str, assume_yes: bool) -> None:
    if assume_yes:
        return
    if input(f"{prompt} [y/N] ").strip().lower() not in {"y", "yes", "s", "sim"}:
        raise ReleaseError("Release cancelled")


def verify_checkout(notes: Path, tag: str) -> list[str]:
    branch = output("git", "branch", "--show-current")
    if branch != BRANCH:
        raise ReleaseError(f"Release must run from {BRANCH}; current branch is {branch or 'detached'}")
    if not notes.is_file():
        raise ReleaseError(f"Missing release notes: {notes.relative_to(ROOT)}")
    run(sys.executable, "scripts/check_release.py")
    changes = output("git", "status", "--short").splitlines()
    remote_commit = remote_tag_commit(tag)
    if remote_commit:
        if remote_commit == output("git", "rev-parse", "HEAD"):
            if changes:
                raise ReleaseError(f"{tag} already points to HEAD, but the checkout has new changes")
            return []
        raise ReleaseError(f"Remote tag {tag} already exists; published tags are immutable")
    return changes


def local_tag(tag: str) -> bool:
    return subprocess.run(
        ["git", "show-ref", "--verify", "--quiet", f"refs/tags/{tag}"], cwd=ROOT
    ).returncode == 0


def remote_tag_commit(tag: str) -> str | None:
    raw = output(
        "git", "ls-remote", "--tags", REMOTE,
        f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}",
    )
    refs = dict(line.split(maxsplit=1)[::-1] for line in raw.splitlines())
    return refs.get(f"refs/tags/{tag}^{{}}") or refs.get(f"refs/tags/{tag}")


def workflow_run(workflow: str, commit: str, branch: str) -> dict[str, object] | None:
    raw = output(
        "gh", "run", "list", "--workflow", workflow, "--commit", commit,
        "--event", "push", "--limit", "20",
        "--json", "databaseId,status,conclusion,headBranch,headSha,createdAt",
    )
    candidates = [item for item in json.loads(raw)
                  if item["headSha"] == commit and item["headBranch"] == branch]
    return max(candidates, key=lambda item: str(item["createdAt"])) if candidates else None


def wait_for_workflow(workflow: str, commit: str, branch: str) -> None:
    print(f"Waiting for {workflow} on {branch} ({commit[:12]}) ...")
    deadline = time.monotonic() + 15 * 60
    found = None
    while time.monotonic() < deadline:
        found = workflow_run(workflow, commit, branch)
        if found is not None:
            break
        time.sleep(POLL_SECONDS)
    if found is None:
        raise ReleaseError(f"No {workflow} run appeared within 15 minutes")
    if found["status"] != "completed":
        run("gh", "run", "watch", str(found["databaseId"]), "--exit-status", check=False)
        found = workflow_run(workflow, commit, branch)
    if found is None or found["conclusion"] != "success":
        run("gh", "run", "view", str(found["databaseId"]), "--web", check=False)
        raise ReleaseError(
            f"{workflow} did not complete successfully; re-run the failed workflow on GitHub "
            "and execute this command again"
        )
    print(f"{workflow}: success")


def commit_and_push(changes: list[str], release_version: str, assume_yes: bool) -> str:
    if changes:
        print("Changes included in the release commit:")
        print("\n".join(changes))
        confirm("Commit all listed changes and continue?", assume_yes)
        run("git", "add", "-A")
        message_file = ROOT / ".tools" / f"release-{release_version}-commit.txt"
        if message_file.is_file():
            run("git", "commit", "-F", str(message_file))
        else:
            run("git", "commit", "-m", f"release: prepare {release_version}")
    commit = output("git", "rev-parse", "HEAD")
    run("git", "fetch", REMOTE, BRANCH)
    remote = output("git", "rev-parse", f"{REMOTE}/{BRANCH}")
    if remote != commit:
        base = output("git", "merge-base", commit, remote)
        if base != remote:
            raise ReleaseError(f"{BRANCH} is not a fast-forward of {REMOTE}/{BRANCH}; rebase first")
        run("git", "push", REMOTE, BRANCH)
    else:
        print(f"{REMOTE}/{BRANCH} already points to {commit[:12]}")
    return commit


def create_tag(tag: str, title: str, commit: str) -> None:
    remote_commit = remote_tag_commit(tag)
    if remote_commit:
        if remote_commit != commit:
            raise ReleaseError(f"Remote tag {tag} points to another commit")
        print(f"Remote tag {tag} already points to {commit[:12]}")
        return
    if local_tag(tag):
        if output("git", "rev-list", "-n", "1", tag) != commit:
            raise ReleaseError(f"Local tag {tag} points to another commit")
    else:
        run("git", "tag", "-a", tag, "-m", f"Release {title}", commit)
    run("git", "push", REMOTE, f"refs/tags/{tag}")


def create_github_release(tag: str, title: str, notes: Path) -> None:
    exists = subprocess.run(
        ["gh", "release", "view", tag], cwd=ROOT, stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    ).returncode == 0
    if exists:
        print(f"GitHub Release {tag} already exists")
        return
    run("gh", "release", "create", tag, "--verify-tag", "--title", title,
        "--notes-file", str(notes))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--yes", action="store_true", help="skip the release-commit prompt")
    parser.add_argument("--dry-run", action="store_true", help="validate and show the release only")
    args = parser.parse_args()
    release_version = version()
    tag = f"v{release_version}"
    notes = ROOT / "docs" / "releases" / f"{release_version}.md"
    title = release_title(notes, release_version)
    try:
        require_tools()
        changes = verify_checkout(notes, tag)
        print(f"Release: {tag} — {title}")
        if changes:
            print("Pending changes:\n" + "\n".join(changes))
        if args.dry_run:
            print("Dry run complete; no commit, push, tag, publication, or release was created")
            return
        commit = commit_and_push(changes, release_version, args.yes)
        wait_for_workflow("build-wheels.yml", commit, BRANCH)
        create_tag(tag, title, commit)
        wait_for_workflow("release.yml", commit, tag)
        create_github_release(tag, title, notes)
        print(f"Release {tag} published and documented successfully")
    except (OSError, ReleaseError, subprocess.SubprocessError, json.JSONDecodeError) as exc:
        parser.exit(1, f"release: {exc}\n")


if __name__ == "__main__":
    main()
