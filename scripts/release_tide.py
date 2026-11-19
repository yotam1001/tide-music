"""Publish an owner-approved Tide release using the hosted release artifacts."""

import argparse
from datetime import date
from functools import cache
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile

from check_public_files import PATTERNS


ROOT = Path(__file__).resolve().parent.parent
REPO = "yotam1001/tide-music"
GITHUB = f"https://github.com/{REPO}"
VERSION = r"[0-9]+\.[0-9]+\.[0-9]+"
WORKSPACE_VERSION = rf'(?m)(?<=\[workspace\.package\]\nversion = "){VERSION}(?=")'
PREVIEW = "tide-windows-preview.zip"
SETUP = "TideSetup.exe"
MANIFEST = "TideUpdate.json"
SUMS = "SHA256SUMS.txt"
COMPANIONS = ("tide-browser-chromium.zip", "tide-browser-firefox.zip")
MACS = {"arm64": "Tide.dmg", "x86_64": "Tide-Intel.dmg"}
WINDOWS_FILES = (SETUP, PREVIEW, *COMPANIONS)
PR_JOBS = {
    "Scripts, privacy and browser checks", "Pull request compile and formatting",
    "Audit currently published Mac downloads", "macOS Rust checks", "playback-smoke",
    "bundle (arm64)", "jam-lab", "private-proxy-config",
    "render-container", "shared-relay-linux",
}
DEVELOPER_JOB = "macOS developer build with Homebrew runtimes"
STAGES = ("prepare", "merge", "windows", "sign", "draft", "tag", "macos", "publish", "verify")
LOCKS = ("Cargo.lock", "experiments/jam/Cargo.lock")


class ReleaseError(Exception):
    pass


class Pending(ReleaseError):
    pass


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


def command(*args, capture=False, env=None, cwd=None):
    result = subprocess.run(
        [str(arg) for arg in args], cwd=cwd or ROOT, check=True, env=env,
        stdout=subprocess.PIPE if capture else None,
        encoding="utf-8", errors="replace",
    )
    return result.stdout.strip() if capture else None


def git(*args, cwd=None):
    return command("git", *args, capture=True, cwd=cwd)


def gh(*args):
    return command("gh", *args, "--repo", REPO, capture=True)


def api(path):
    return json.loads(command("gh", "api", f"repos/{REPO}/{path}", capture=True))


def python(script, *args, cwd=None):
    command(sys.executable, (cwd or ROOT) / "scripts" / script, *args, cwd=cwd)


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def checksum_lines(digests):
    return "".join(f"{digest}  {name}\n" for name, digest in sorted(digests.items()))


def version_key(tag):
    match = re.fullmatch(rf"v({VERSION})", tag or "")
    return tuple(map(int, match[1].split("."))) if match else None


def newer_public_releases(releases, tag):
    """Published stable releases whose version is above `tag`, or that cannot be ordered."""
    own = version_key(tag)
    require(own is not None, f"Unexpected release tag {tag}.")
    newer = []
    for item in releases:
        if item["isDraft"] or item["isPrerelease"] or item["tagName"] == tag:
            continue
        key = version_key(item["tagName"])
        if key is None or key > own:
            newer.append(item["tagName"])
    return newer


def matching_ci_run(workflow, sha, branch):
    runs = json.loads(gh("run", "list", "--workflow", workflow, "--commit", sha,
                         "--branch", branch, "--event", "push", "--limit", "10", "--json",
                         "databaseId,url,headSha,headBranch,event,status,conclusion"))
    if not runs:
        raise Pending(f"Waiting for {workflow} at {sha}.")
    run = runs[0]
    require(run["headSha"] == sha and run["headBranch"] == branch and run["event"] == "push",
            "Unexpected workflow run identity.")
    if run["status"] != "completed":
        raise Pending(f"{workflow} is {run['status']}. {run['url']}")
    require(run["conclusion"] == "success", f"{workflow} failed. {run['url']}")
    print(run["url"], flush=True)
    return run


def update_source(root, previous, version, release_date, extra_pins):
    pins = ("Cargo.toml", *LOCKS, *extra_pins)
    hits = git("grep", "-l", "-F", "-e", previous, "--", ":!CHANGELOG.md", cwd=root).splitlines()
    require(set(hits) <= set(pins), "Review previous version references and use --pin FILE for active pins: "
            + ", ".join(sorted(set(hits) - set(pins))))
    for name in pins:
        require(git("ls-files", "--", name, cwd=root) == name, f"Pin is not a tracked file: {name}")
    changes = {}
    for name in pins:
        path = root / name
        original = path.read_bytes()
        old, new = previous.encode(), version.encode()
        if name in LOCKS:
            # A registry dependency may coincidentally share Tide's version.
            blocks = original.split(b"[[package]]")
            for index, block in enumerate(blocks):
                if re.search(rb'(?m)^name = "tide-[^"\r\n]+"\r?$', block) and not re.search(rb"(?m)^source = ", block):
                    blocks[index] = block.replace(b'version = "' + old + b'"', b'version = "' + new + b'"')
            updated = b"[[package]]".join(blocks)
        elif name == "Cargo.toml":
            updated = re.sub(rb'(\[workspace\.package\]\r?\nversion = ")' + re.escape(old) + b'(")',
                             lambda match: match[1] + new + match[2], original, count=1)
        else:
            updated = original.replace(old, new)
        require(updated != original, f"No active {previous} pin found in {name}.")
        changes[path] = updated

    changelog = root / "CHANGELOG.md"
    original = changelog.read_bytes()
    newline = b"\r\n" if b"\r\n" in original else b"\n"
    heading = b"## Unreleased" + newline
    require(original.count(heading) == 1, "Expected a single Unreleased heading.")
    unreleased = original.split(heading, 1)[1].split(newline + b"## ", 1)[0]
    require(unreleased.strip(), "Unreleased changelog is empty.")
    changes[changelog] = original.replace(heading, heading + newline
                                          + f"## {version} - {release_date}".encode() + newline, 1)
    return changes


class Release:
    def __init__(self, args):
        self.args = args
        self.version = args.version
        self.tag_name = f"v{self.version}"
        self.branch = f"release/{self.version}"
        self.title = f"Release {self.version}"
        self.work = ROOT / ".local" / "tide-release" / self.version
        self.mac_pairs = {stable: f"Tide-{self.version}-{arch}.dmg" for arch, stable in MACS.items()}
        self.mac_images = tuple(self.mac_pairs) + tuple(self.mac_pairs.values())
        self.mac_files = self.mac_images + tuple(f"{name}.sha256" for name in self.mac_images)

    def check_origin(self):
        origin = git("remote", "get-url", "origin").removesuffix(".git")
        require(origin in (GITHUB, f"git@github.com:{REPO}", f"ssh://git@github.com/{REPO}"),
                "origin must be the Tide Music GitHub repository.")
        require(not git("status", "--porcelain", "--untracked-files=no"), "Tracked changes must be committed or removed first.")

    def pr(self):
        records = json.loads(gh("pr", "list", "--base", "main", "--head", self.branch,
                                "--state", "all", "--json",
                                "number,url,state,title,headRefOid,isCrossRepository,mergeCommit,statusCheckRollup"))
        if not records:
            return None
        require(len(records) == 1 and not records[0]["isCrossRepository"] and records[0]["title"] == self.title,
                f"Unexpected PR for {self.branch}.")
        return records[0]

    def prepare(self):
        if (pr := self.pr()) is not None:
            require(pr["state"] in {"OPEN", "MERGED"}, f"Release PR is closed. {pr['url']}")
            print(pr["url"])
            return
        command("git", "fetch", "origin", "main")
        base = git("rev-parse", "origin/main")
        remote = git("ls-remote", "--heads", "origin", f"refs/heads/{self.branch}")
        local = git("branch", "--list", self.branch)
        if local:
            command("git", "switch", self.branch)
        elif remote:
            command("git", "fetch", "origin", self.branch)
            command("git", "switch", "-c", self.branch, "--track", f"origin/{self.branch}")
        else:
            command("git", "switch", "-c", self.branch, "--no-track", base)

        current = re.search(WORKSPACE_VERSION, (ROOT / "Cargo.toml").read_text("utf-8"))
        require(current is not None, "Workspace version could not be read.")
        if current[0] != self.version:
            require(git("rev-parse", "HEAD") == base, "Release branch must start at origin/main.")
            require(tuple(map(int, self.version.split("."))) > tuple(map(int, current[0].split("."))),
                    "Release version must advance the workspace version.")
            # Prepare off-branch so validation failures and interruptions leave the
            # caller's index and files alone. Git's fast-forward protects local edits.
            with tempfile.TemporaryDirectory(prefix="tide-prepare-") as temp:
                checkout = Path(temp) / "work"
                command("git", "worktree", "add", "--detach", checkout, base)
                try:
                    changes = update_source(checkout, current[0], self.version, self.args.date, self.args.pin)
                    for path, data in changes.items():
                        path.write_bytes(data)
                    command("git", "add", "--", *(path.relative_to(checkout) for path in changes), cwd=checkout)
                    python("check_public_files.py", cwd=checkout)
                    command("git", "diff", "--cached", "--check", cwd=checkout)
                    command("git", "commit", "-m", self.title, cwd=checkout)
                    commit = git("rev-parse", "HEAD", cwd=checkout)
                    require(git("branch", "--show-current") == self.branch and git("rev-parse", "HEAD") == base,
                            "Release branch changed during preparation.")
                    status = git("status", "--porcelain", "--untracked-files=no")
                    require(not status, f"Tracked files changed during preparation:\n{status}")
                    command("git", "merge", "--ff-only", commit)
                finally:
                    command("git", "worktree", "remove", "--force", checkout)
        head = git("rev-parse", "HEAD")
        require(git("show", "-s", "--format=%s", head) == self.title, "Unexpected release branch commit.")
        require(re.search(rf"(?m)^## {re.escape(self.version)} - \d{{4}}-\d{{2}}-\d{{2}}$",
                          (ROOT / "CHANGELOG.md").read_text("utf-8")), "Release changelog heading is missing.")
        require(not remote or remote.split()[0] == head, "Remote release branch differs from this checkout.")
        python("check_public_files.py")
        command("git", "push", "-u", "origin", self.branch)
        print(gh("pr", "create", "--base", "main", "--head", self.branch, "--title", self.title,
                 "--body", f"Bump the workspace and finalize the changelog for Tide {self.version}."))

    def merge(self):
        pr = self.pr()
        require(pr and pr["state"] in ("OPEN", "MERGED"), "Open the release PR first.")
        if pr["state"] == "MERGED":
            self.merge_sha()
            return
        checks = {check["name"]: check for check in pr["statusCheckRollup"] if "name" in check}
        pending = []
        for name in sorted(PR_JOBS | {DEVELOPER_JOB} | set(checks)):
            check = checks.get(name)
            if not check or check["status"] != "COMPLETED":
                pending.append(name)
                continue
            result = check["conclusion"]
            if name == DEVELOPER_JOB and result == "CANCELLED" and self.args.allow_queued_developer_cancel:
                job = check["detailsUrl"].split("/")[-1]
                require(job.isdecimal(), "Cannot identify the cancelled developer job.")
                require(not api(f"actions/jobs/{job}")["runner_id"], "Developer job was assigned a runner before cancellation.")
                print(f"Accepted developer job cancellation without a runner. {check['detailsUrl']}")
                continue
            require(result == "SUCCESS" or (result == "SKIPPED" and name not in PR_JOBS | {DEVELOPER_JOB}),
                    f"{name}: {result}. {check['detailsUrl']}")
        if pending:
            raise Pending(f"Waiting for {', '.join(pending)}. {pr['url']}")
        print(gh("pr", "merge", str(pr["number"]), "--merge", "--match-head-commit", pr["headRefOid"]))
        self.merge_sha()

    @cache
    def merge_sha(self):
        pr = self.pr()
        require(pr and pr["state"] == "MERGED" and pr["mergeCommit"], "Release PR must be merged first.")
        sha = pr["mergeCommit"]["oid"]
        command("git", "fetch", "--quiet", "origin", "main")
        command("git", "merge-base", "--is-ancestor", sha, "origin/main")
        parents = git("show", "-s", "--format=%P", sha).split()
        require(len(parents) == 2 and parents[1] == pr["headRefOid"], "Release PR needs a merge commit on main.")
        match = re.search(WORKSPACE_VERSION, git("show", f"{sha}:Cargo.toml"))
        require(match and match[0] == self.version, "Merged source has the wrong workspace version.")
        return sha

    def windows_dir(self):
        path = self.work / self.merge_sha() / "windows"
        for name in WINDOWS_FILES:
            require((path / name).is_file(), f"Missing {name}. Resume from windows.")
        receipt = json.loads((path / "ci-digests.json").read_text("ascii"))
        for name in WINDOWS_FILES:
            require(sha256(path / name) == receipt[name], f"Local CI artifact changed: {name}.")
        return path

    def windows(self):
        sha = self.merge_sha()
        directory = self.work / sha
        if (directory / "windows").is_dir():
            self.windows_dir()
            return
        run = matching_ci_run("ci.yml", sha, "main")
        details = api(f"actions/runs/{run['databaseId']}/jobs?per_page=100")
        require(any(job["name"] == "Release validation" and job["conclusion"] == "success"
                    for job in details["jobs"]), "Windows Release validation did not pass.")
        require(git("check-ignore", "--", str(self.work)), "Release working directory must be ignored.")
        directory.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=directory) as temp:
            temp = Path(temp)
            downloads = temp / "downloads"
            output = temp / "windows"
            output.mkdir()
            print(gh("run", "download", str(run["databaseId"]), "--dir", downloads,
                     "--name", f"tide-windows-preview-{sha}",
                     "--name", f"tide-browser-companion-prototypes-{sha}"))
            for name in WINDOWS_FILES:
                matches = list(downloads.rglob(name))
                require(len(matches) == 1, f"Expected exactly one CI artifact file named {name}.")
                matches[0].rename(output / name)
            (output / "ci-digests.json").write_text(
                json.dumps({name: sha256(output / name) for name in WINDOWS_FILES}), encoding="ascii"
            )
            output.rename(directory / "windows")

    def packaged_verifier(self, path):
        player = path / "tide-music.exe"
        with zipfile.ZipFile(path / PREVIEW) as archive:
            packaged = archive.read(player.name)
        if not player.is_file() or hashlib.sha256(packaged).hexdigest() != sha256(player):
            player.write_bytes(packaged)
        return player

    def inspect_installer(self, path):
        python("check-preview-package.py", path / PREVIEW, "--with-browser-host", "--with-native-login")
        # Reflection-only loading reads the packaged resource without running Setup.
        inspection = r"""
$ErrorActionPreference = 'Stop'
$setup = $env:TIDE_RELEASE_SETUP
$assembly = [Reflection.Assembly]::ReflectionOnlyLoadFrom($setup)
$payload = $assembly.GetManifestResourceStream('Tide.Payload')
try {
    $digest = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($payload))
    $version = (Get-Item -LiteralPath $setup).VersionInfo.ProductVersion
    [Console]::WriteLine($version + ' ' + $digest.Replace('-', '').ToLowerInvariant())
} finally { $payload.Dispose() }
"""
        actual = command("powershell.exe", "-NoProfile", "-NonInteractive", "-Command", inspection,
                         capture=True, env={**os.environ, "TIDE_RELEASE_SETUP": str(path / SETUP)})
        require(actual == f"{self.version} {sha256(path / PREVIEW)}", "Setup version or embedded preview differs.")

    def verify_windows(self, path, manifest, installer):
        python("sign-update.py", "--installer", installer, "--version", self.version, "--verify", manifest)
        command(self.packaged_verifier(path), "--verify-update-files", manifest, installer)

    def sign(self):
        path = self.windows_dir()
        self.inspect_installer(path)
        manifest = path / MANIFEST
        if not manifest.exists():
            with tempfile.TemporaryDirectory(dir=path) as temp:
                output = Path(temp) / MANIFEST
                python("sign-update.py", "--installer", path / SETUP, "--version", self.version, "--output", output)
                self.verify_windows(path, output, path / SETUP)
                output.rename(manifest)
        else:
            self.verify_windows(path, manifest, path / SETUP)

    def release(self):
        try:
            result = json.loads(gh("release", "view", self.tag_name, "--json",
                                   "assets,targetCommitish,isDraft,isPrerelease,url"))
        except subprocess.CalledProcessError as error:
            raise ReleaseError(f"Cannot find release {self.tag_name}. Resume from draft if it does not exist.") from error
        require(not result["isPrerelease"] and result["targetCommitish"] == self.merge_sha(),
                "GitHub release target must match the stable release merge commit.")
        return result

    def require_newest_public(self):
        """Refuse to promote or accept a release that would move Latest backward."""
        releases = json.loads(gh("release", "list", "--limit", "100", "--json",
                                 "tagName,isDraft,isPrerelease,isLatest"))
        newer = newer_public_releases(releases, self.tag_name)
        require(not newer, f"{self.tag_name} is older than published {', '.join(sorted(newer))}; "
                "publishing it would move GitHub Latest backward.")

    def remote_digests(self, release, names, *, exact=False):
        assets = {asset["name"]: asset for asset in release["assets"]}
        require(set(names) <= assets.keys() and (not exact or set(names) == assets.keys()),
                "GitHub release asset list differs from the expected files.")
        for name in names:
            asset = assets[name]
            require(asset["state"] == "uploaded" and asset["size"] > 0
                    and re.fullmatch(r"sha256:[a-f0-9]{64}", asset["digest"] or ""),
                    f"GitHub lacks a verified digest for {name}.")
        return {name: assets[name]["digest"].removeprefix("sha256:") for name in names}

    def draft(self):
        path = self.windows_dir()
        self.verify_windows(path, path / MANIFEST, path / SETUP)
        sha = self.merge_sha()
        names = (*WINDOWS_FILES, MANIFEST)
        digests = {name: sha256(path / name) for name in names}
        (path / SUMS).write_text(checksum_lines(digests), encoding="ascii", newline="\n")
        releases = json.loads(gh("release", "list", "--limit", "100", "--json", "tagName"))
        if not any(item["tagName"] == self.tag_name for item in releases):
            require(self.args.notes and self.args.notes.is_file(), "--notes FILE is required to create the draft release.")
            notes = self.args.notes.read_bytes()
            require(notes.strip() and not any(pattern.search(notes) for _, pattern in PATTERNS),
                    "Release notes are empty or failed the privacy scan.")
            print(gh("release", "create", self.tag_name, "--draft", "--target", sha,
                     "--title", f"Tide {self.version}", "--notes-file", self.args.notes))
        release = self.release()
        held = {asset["name"] for asset in release["assets"]}
        present = set(names) & held
        require(self.remote_digests(release, present) == {name: digests[name] for name in present},
                "Existing remote Windows assets differ from the signed local files.")
        missing = set((*names, SUMS)) - held
        if missing:
            require(release["isDraft"], "Cannot upload missing files to a published release.")
            print(gh("release", "upload", self.tag_name, *(path / name for name in sorted(missing))))
        release = self.release()
        require(self.remote_digests(release, names) == digests, "Windows upload digests differ.")
        if not set(self.mac_files) & {asset["name"] for asset in release["assets"]}:
            require(self.remote_digests(release, (SUMS,))[SUMS] == sha256(path / SUMS),
                    "Draft checksum file differs.")

    def tag(self):
        sha = self.merge_sha()
        release = self.release()
        self.remote_digests(release, (*WINDOWS_FILES, MANIFEST, SUMS))
        remote = git("ls-remote", "--tags", "origin", f"refs/tags/{self.tag_name}", f"refs/tags/{self.tag_name}^{{}}")
        if remote:
            require(remote.splitlines()[-1].split()[0] == sha, "Existing tag points to a different revision.")
            return
        require(release["isDraft"], "Expected a draft before pushing the release tag.")
        path = self.windows_dir()
        digests = {name: sha256(path / name) for name in (*WINDOWS_FILES, MANIFEST)}
        expected = {**digests, SUMS: hashlib.sha256(checksum_lines(digests).encode("ascii")).hexdigest()}
        require(self.remote_digests(release, expected, exact=True) == expected,
                "Draft assets changed after Windows verification.")
        self.verify_windows(path, path / MANIFEST, path / SETUP)
        python("check_public_files.py")
        command("git", "push", "origin", f"{sha}:refs/tags/{self.tag_name}")

    def macos(self):
        sha = self.merge_sha()
        run = matching_ci_run("macos-release.yml", sha, self.tag_name)
        packaging = git("show", f"{sha}:scripts/package_macos.py")
        require("scripts/check_macos_dmg.py" in packaging,
                "The tagged packaging script does not invoke the DMG signature gate.")
        details = api(f"actions/runs/{run['databaseId']}/jobs?per_page=100")
        for arch in MACS:
            jobs = [job for job in details["jobs"] if job["name"] == f"bundle ({arch})"]
            require(len(jobs) == 1 and jobs[0]["conclusion"] == "success"
                    and any(step["name"] == "Build, sign, verify and package stable Tide"
                            and step["conclusion"] == "success" for step in jobs[0]["steps"]),
                    f"Missing successful packaging gate for {arch}.")
            logs = gh("run", "view", str(run["databaseId"]), "--job", str(jobs[0]["id"]), "--log")
            require(re.search(r"Checked [1-9]\d* Mach-O signatures", logs),
                    f"Missing DMG signature evidence for {arch}.")
            print(f"DMG signature gate passed for {arch}. {jobs[0]['html_url']}")

        path = self.windows_dir()
        release = self.release()
        digests = self.remote_digests(release, (*WINDOWS_FILES, MANIFEST, *self.mac_files))
        for name in (*WINDOWS_FILES, MANIFEST):
            require(digests[name] == sha256(path / name), f"Remote Windows digest differs for {name}.")
        require(all(digests[stable] == digests[versioned] for stable, versioned in self.mac_pairs.items()),
                "Stable and versioned Mac images differ.")
        with tempfile.TemporaryDirectory(dir=path) as temp:
            temp = Path(temp)
            print(gh("run", "download", str(run["databaseId"]), "--dir", temp,
                     *(arg for arch in MACS for arg in ("--name", f"macos-release-{arch}"))))
            for name in self.mac_files:
                matches = list(temp.rglob(name))
                require(len(matches) == 1 and sha256(matches[0]) == digests[name],
                        f"GitHub release asset differs from the macOS CI artifact: {name}.")
            for name in self.mac_images:
                checksum = next(temp.rglob(f"{name}.sha256"))
                require(checksum.read_text("ascii").split() == [digests[name], name], f"DMG checksum mismatch for {name}.")
        summed = path / SUMS
        summed.write_text(checksum_lines(digests), encoding="ascii", newline="\n")
        uploaded = {asset["name"]: asset["digest"] for asset in release["assets"]}
        if uploaded.get(SUMS) != f"sha256:{sha256(summed)}":
            require(release["isDraft"], "Cannot replace checksums on a published release.")
            print(gh("release", "upload", self.tag_name, summed, "--clobber"))
        _, verified = self.verify_release_assets()
        evidence = path / "verified-release-digests.json"
        temporary = evidence.with_suffix(".tmp")
        temporary.write_text(json.dumps(verified), encoding="ascii")
        temporary.replace(evidence)

    def verify_release_assets(self):
        path = self.windows_dir()
        names = (*WINDOWS_FILES, MANIFEST, *self.mac_files, SUMS)
        release = self.release()
        digests = self.remote_digests(release, names, exact=True)
        expected = checksum_lines({name: digests[name] for name in names if name != SUMS})
        require(hashlib.sha256(expected.encode("ascii")).hexdigest() == digests[SUMS],
                "Published checksum file does not match every release asset.")
        for name in (*WINDOWS_FILES, MANIFEST):
            require(sha256(path / name) == digests[name], f"Release Windows assets differ for {name}.")
        print(checksum_lines(digests), end="", flush=True)
        return release, digests

    def publish(self):
        release, digests = self.verify_release_assets()
        path = self.windows_dir()
        evidence = path / "verified-release-digests.json"
        require(evidence.is_file() and json.loads(evidence.read_text("ascii")) == digests,
                "Missing macOS CI digest verification. Resume from macos.")
        remote = git("ls-remote", "--tags", "origin", f"refs/tags/{self.tag_name}", f"refs/tags/{self.tag_name}^{{}}")
        require(remote and remote.splitlines()[-1].split()[0] == self.merge_sha(),
                "Release tag must already point to the release merge commit.")
        if release["isDraft"]:
            self.require_newest_public()
            print(gh("release", "edit", self.tag_name, "--draft=false", "--latest=true"))

    def verify(self):
        release, digests = self.verify_release_assets()
        require(not release["isDraft"], "Release is still a draft.")
        self.require_newest_public()
        with urllib.request.urlopen(f"https://api.github.com/repos/{REPO}/releases/latest", timeout=60) as reply:
            require(json.load(reply)["tag_name"] == self.tag_name, f"GitHub Latest is not {self.tag_name}.")
        with tempfile.TemporaryDirectory(dir=self.work) as temp:
            temp = Path(temp)
            for name in (MANIFEST, SETUP, *MACS.values()):
                request = urllib.request.Request(f"{GITHUB}/releases/latest/download/{name}",
                                                 headers={"User-Agent": "Tide release verification"})
                with urllib.request.urlopen(request, timeout=60) as reply, (temp / name).open("wb") as stream:
                    shutil.copyfileobj(reply, stream)
                require(sha256(temp / name) == digests[name], f"Anonymous Latest download differs for {name}.")
                print(f"Verified anonymous Latest download: {name}")
            self.verify_windows(self.windows_dir(), temp / MANIFEST, temp / SETUP)
        print(f"Verified public release: {release['url']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="Numeric version without the v prefix")
    parser.add_argument("--date", type=date.fromisoformat, default=date.today(), help="Changelog date (YYYY-MM-DD)")
    parser.add_argument("--notes", type=Path, help="Reviewed public Markdown notes for the draft")
    parser.add_argument("--pin", action="append", default=[], help="Additional tracked file with active version pins")
    parser.add_argument("--from-stage", choices=STAGES, default=STAGES[0])
    parser.add_argument("--through-stage", choices=STAGES, default=STAGES[-1])
    parser.add_argument("--dry-run", action="store_true", help="Print the stage plan without changing local or remote state")
    parser.add_argument("--wait", action="store_true", help="Wait for CI; otherwise exit with status 2 when CI is pending")
    parser.add_argument("--allow-queued-developer-cancel", action="store_true",
                        help="Accept cancellation of the optional developer job if it never had a runner")
    args = parser.parse_args()
    if not re.fullmatch(VERSION, args.version):
        parser.error("Use a numeric major.minor.patch version.")
    stages = STAGES[STAGES.index(args.from_stage):STAGES.index(args.through_stage) + 1]
    if not stages:
        parser.error("--through-stage must come after or equal --from-stage.")
    if args.dry_run:
        print(f"Tide {args.version}, changelog date {args.date}: " + " -> ".join(stages))
        print("CI artifacts only. Signing uses the existing DPAPI key in LOCALAPPDATA.")
        return 0

    release = Release(args)
    stage = args.from_stage
    try:
        release.check_origin()
        for stage in stages:
            print(f"[{stage}] Tide {args.version}", flush=True)
            while True:
                try:
                    getattr(release, stage)()
                    break
                except Pending as error:
                    if not args.wait:
                        print(error, file=sys.stderr)
                        print(f"Resume with --from-stage {stage}.", file=sys.stderr)
                        return 2
                    print(error, flush=True)
                    time.sleep(45)
        return 0
    except (ReleaseError, OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"[{stage}] {error}\nResume with --from-stage {stage} after resolving the error.",
              file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
