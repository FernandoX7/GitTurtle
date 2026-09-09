#!/usr/bin/env python3
"""Create isolated profile/identity/signing repositories for native QA.

Never edits app state, user Git configuration, an existing repository, or keyring.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess


def run(args, cwd=None):
    environment = os.environ.copy()
    for name in list(environment):
        if name.startswith("GIT_") or name in {"SSH_AUTH_SOCK", "SSH_AGENT_PID", "GPG_TTY"}:
            environment.pop(name, None)
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull, GIT_TERMINAL_PROMPT="0")
    return subprocess.check_output(args, cwd=cwd, env=environment, text=True, stderr=subprocess.STDOUT).strip()


def git(repo, *args):
    return run(["git", "-C", str(repo), *args])


def repository(root, folder, name, email, key=None):
    path = root / folder
    path.mkdir()
    git(path, "init", "-b", "main")
    hooks = root / f"{folder}-hooks"
    hooks.mkdir()
    hook = hooks / "pre-commit"
    hook.write_text('#!/bin/sh\nevidence=$(git rev-parse --git-path profile-hook-evidence) || exit 1\nprintf "hook ran\\n" >> "$evidence"\n')
    hook.chmod(0o700)
    settings = {"user.name": name, "user.email": email, "core.hooksPath": str(hooks),
                "commit.gpgsign": "true" if key else "false", "tag.gpgsign": "true" if key else "false",
                "custom.profileFixture": "preserve this unrelated configuration"}
    if key:
        settings.update({"user.signingkey": str(key), "gpg.format": "ssh", "gpg.ssh.allowedSignersFile": str(root / "allowed-signers"), "gpg.ssh.program": "ssh-keygen"})
    for setting, value in settings.items():
        git(path, "config", setting, value)
    (path / "identity.txt").write_text(f"Initial identity: {name} <{email}>\n")
    git(path, "add", "identity.txt")
    git(path, "commit", "-m", "Initial disposable profile fixture")
    (path / "identity.txt").write_text("Ready for a reviewed native profile commit.\n")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = args.output.expanduser().absolute()
    if root.exists() and any(root.iterdir()):
        parser.error("Choose a new or empty output directory; existing content is never replaced")
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    keys = {}
    signers = []
    for profile in ("Personal", "Work"):
        key = root / f"{profile.lower()}-fixture-key"
        run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", f"{profile.lower()}@gitturtle.invalid", "-f", str(key)])
        keys[profile] = key
        public = key.with_suffix(".pub").read_text().strip()
        # Either identity can sign with either reviewed profile key. This file
        # is a fixture verification policy, not a user trust configuration.
        for identity in ("personal", "work"):
            signers.append(f"{identity}@gitturtle.invalid {public}")
    (root / "allowed-signers").write_text("\n".join(signers) + "\n")
    unsigned = repository(root, "unsigned", "Unsigned Fixture", "unsigned@gitturtle.invalid")
    personal = repository(root, "personal", "Personal Fixture", "personal@gitturtle.invalid", keys["Personal"])
    work = repository(root, "work", "Work Fixture", "work@gitturtle.invalid", keys["Work"])
    shared = root / "linked-shared"
    git(unsigned, "worktree", "add", "-b", "linked-shared", str(shared))
    git(work, "config", "extensions.worktreeConfig", "true")
    private = root / "linked-private"
    git(work, "worktree", "add", "-b", "linked-private", str(private))
    manifest = {"unsigned": str(unsigned), "personal_signed": str(personal), "work_signed": str(work),
                "linked_shared": str(shared), "linked_private": str(private),
                "disposable_signing_key_paths": {name: str(key) for name, key in keys.items()},
                "allowed_signers": str(root / "allowed-signers"),
                "app_state_modified": False, "user_keyring_modified": False}
    (root / "fixtures.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
