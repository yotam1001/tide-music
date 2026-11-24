"""Attach rebuilt Mac disk images or Linux AppImages to a draft release, never to a published one.

A published release's SHA256SUMS.txt records the digests people already
downloaded. Rebuilt DMGs are not byte-identical, so replacing them on a public
release would silently invalidate those checksums. A rerun against a published
release therefore only verifies that the expected assets are present.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys


class AttachError(Exception):
    pass


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def plan(release, files):
    """Return the files to upload with --clobber, or [] to leave the release alone.

    `release` is `gh release view --json isDraft,assets` output and `files`
    maps each local asset name to its SHA-256 digest.
    """
    if release["isDraft"]:
        return sorted(files)
    published = {asset["name"]: asset for asset in release["assets"]}
    missing = sorted(set(files) - published.keys())
    if missing:
        raise AttachError("Published release lacks " + ", ".join(missing)
                          + "; assets cannot be added after publishing, since SHA256SUMS.txt would not cover them.")
    for name in sorted(files):
        digest = (published[name].get("digest") or "").removeprefix("sha256:")
        if digest != files[name]:
            print(f"::notice::{name} was rebuilt with a different digest; the published asset is kept.")
    return []


def gh(*args, capture=False):
    result = subprocess.run(["gh", *args], check=True, text=True,
                            stdout=subprocess.PIPE if capture else None)
    return result.stdout if capture else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("dist", type=Path)
    args = parser.parse_args()
    if not args.tag.startswith("v"):
        print("Expected v-prefixed release tag", file=sys.stderr)
        return 1
    files = sorted([*args.dist.glob("*.dmg"), *args.dist.glob("*.dmg.sha256"),
                    *args.dist.glob("*.AppImage"), *args.dist.glob("*.AppImage.sha256")])
    if not any(path.suffix in (".dmg", ".AppImage") for path in files):
        print("No disk images or AppImages to attach", file=sys.stderr)
        return 1
    try:
        view = gh("release", "view", args.tag, "--json", "isDraft,assets", capture=True)
    except subprocess.CalledProcessError:
        gh("release", "create", args.tag, "--verify-tag", "--draft", "--title", args.tag)
        view = gh("release", "view", args.tag, "--json", "isDraft,assets", capture=True)
    try:
        uploads = plan(json.loads(view), {path.name: sha256(path) for path in files})
    except AttachError as error:
        print(error, file=sys.stderr)
        return 1
    if not uploads:
        print(f"{args.tag} is published; verified its assets and left them unchanged.")
        return 0
    gh("release", "upload", args.tag, *(str(args.dist / name) for name in uploads), "--clobber")
    return 0


if __name__ == "__main__":
    sys.exit(main())
