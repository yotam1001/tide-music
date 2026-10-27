"""Generate Tide's three WinGet manifests from an exact installer artifact.

This does not publish anything or imply that InstallerUrl is already live.
Run winget validate on the output and verify the public asset hash before submission.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

IDENTIFIER = "TideMusic.Tide"
SCHEMA = "1.12.0"
REPOSITORY = "https://github.com/yotam1001/tide-music"


def scalar(value):
    return json.dumps(value, ensure_ascii=False)


def generate(installer, version, installer_url, destination):
    if not re.fullmatch(r"[0-9][A-Za-z0-9.+-]{0,63}", version):
        raise ValueError("Invalid package version")
    if not re.fullmatch(re.escape(REPOSITORY) + r"/releases/download/v" + re.escape(version) + r"/[A-Za-z0-9._-]+\.exe", installer_url):
        raise ValueError("Installer URL must point to this version's Tide GitHub release")
    installer = Path(installer)
    if installer.suffix.lower() != ".exe" or installer.read_bytes()[:2] != b"MZ":
        raise ValueError("Expected a Windows EXE installer")
    with installer.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest().upper()
    if digest == "BD0A2504709772174EFF2EB91E86789EB4DF625CA749E86D1DD7FB2AD0A72775":
        raise ValueError("The original v0.4.5 installer does not support silent installation")
    destination = Path(destination)
    if destination.exists():
        raise FileExistsError("Refusing to overwrite an existing manifest directory")
    common = f"PackageIdentifier: {IDENTIFIER}\nPackageVersion: {scalar(version)}\n"
    contents = {
        f"{IDENTIFIER}.yaml": common + f"DefaultLocale: en-US\nManifestType: version\nManifestVersion: {SCHEMA}\n",
        f"{IDENTIFIER}.installer.yaml": common + f"""InstallerType: exe
Scope: user
InstallModes:
- interactive
- silent
InstallerSwitches:
  Silent: --silent
  SilentWithProgress: --silent
UpgradeBehavior: install
AppsAndFeaturesEntries:
- DisplayName: Tide
  Publisher: Tide contributors
  DisplayVersion: {scalar(version)}
  ProductCode: TideMusic
Installers:
- Architecture: x64
  InstallerUrl: {installer_url}
  InstallerSha256: {digest}
ManifestType: installer
ManifestVersion: {SCHEMA}
""",
        f"{IDENTIFIER}.locale.en-US.yaml": common + f"""PackageLocale: en-US
Publisher: Tide contributors
PublisherUrl: {REPOSITORY}
PublisherSupportUrl: {REPOSITORY}/issues
PackageName: Tide
PackageUrl: https://tide-music-links.netlify.app/
License: MIT
LicenseUrl: {REPOSITORY}/blob/v{version}/LICENSE
ShortDescription: Native Windows music player built for low RAM use.
Description: |-
  Tide is a free, open-source Windows music player with a native Rust interface
  and native audio playback. Listen to YouTube Music, manage playlists and the
  queue, use a mini player, or play local audio files without keeping a browser
  window open. YouTube Music integration is unofficial. Account connection
  requires a manually loaded browser companion extension.
Moniker: tide-music
Tags:
- audio
- music
- music-player
- rust
- youtube-music
ReleaseNotesUrl: {REPOSITORY}/releases/tag/v{version}
ManifestType: defaultLocale
ManifestVersion: {SCHEMA}
""",
    }
    destination.mkdir(parents=True)
    for name, text in contents.items():
        kind = "installer" if name.endswith(".installer.yaml") else "defaultLocale" if ".locale." in name else "version"
        header = f"# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}.{SCHEMA}.schema.json\n"
        (destination / name).write_text(header + text, encoding="utf-8", newline="\n")
    return digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--installer", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--installer-url", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    digest = generate(args.installer, args.version, args.installer_url, args.output)
    print(f"Generated {IDENTIFIER} {args.version}; SHA256 {digest}")
    print("Not submitted. Verify the published asset hash and run winget validate before submission.")


if __name__ == "__main__":
    main()
