"""Explicit Grove beta payload, checksums and privacy validation; never package a checkout."""
import argparse
import hashlib
import re
import subprocess
import zipfile
from pathlib import Path
from check_public_files import PATTERNS
import importlib.util

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("preview_check", ROOT / "scripts/check-preview-package.py")
preview_check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preview_check)
BINARY_PATTERNS = preview_check.BINARY_PATTERNS
FILES = {
    "grove-music.exe": None,
    "LICENSE": "LICENSE",
    "THIRD_PARTY_NOTICES.md": "THIRD_PARTY_NOTICES.md",
    "DEPENDENCIES.md": None,
    "START_HERE.md": "docs/grove/BETA.md",
    "Uninstall Grove.cmd": "Uninstall Grove.cmd",
    "docs/BETA.md": "docs/grove/BETA.md",
    "scripts/install-grove.ps1": "scripts/install-grove.ps1",
    "scripts/grove-install-common.ps1": "scripts/grove-install-common.ps1",
}

def check(path):
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        if len(names) != len(set(n.lower() for n in names)) or set(names) != set(FILES) | {"SHA256SUMS.txt"}:
            raise ValueError("Unexpected, missing or duplicate beta payload file")
        if any(e.file_size > 128 * 1024 * 1024 for e in archive.infolist()) or sum(e.file_size for e in archive.infolist()) > 256 * 1024 * 1024:
            raise ValueError("Oversized beta payload")
        manifest = {}
        for line in archive.read("SHA256SUMS.txt").decode("utf-8-sig").splitlines():
            match = re.fullmatch(r"([a-f0-9]{64})  (.+)", line)
            if not match or match[2] in manifest: raise ValueError("Invalid beta checksum manifest")
            manifest[match[2]] = match[1]
        if set(manifest) != set(FILES): raise ValueError("Incomplete beta checksum manifest")
        for name in names:
            data = archive.read(name)
            if name in manifest and hashlib.sha256(data).hexdigest() != manifest[name]: raise ValueError(f"Checksum mismatch: {name}")
            for label, pattern in BINARY_PATTERNS if name.endswith(".exe") else PATTERNS:
                if pattern.search(data): raise ValueError(f"Beta privacy check failed: {name}: {label}")
    print("PASS: Grove beta allowlist, checksums and privacy scan")

def package(exe, notices, destination):
    output = destination.resolve()
    if not output.is_relative_to((ROOT / "dist").resolve()) or output.exists() or Path(str(output) + ".zip").exists():
        raise ValueError("Use a new output directory inside dist")
    for parent in [output, *output.parents]:
        if parent.is_symlink() or (parent.exists() and parent.stat().st_file_attributes & 0x400): raise ValueError("Output must not use links or junctions")
    version = subprocess.run([str(exe.resolve()), "--version"], capture_output=True, check=True, timeout=10).stdout.decode().strip()
    if version != "Grove 0.1.0-beta.1": raise ValueError("Expected the Grove beta executable")
    sources = {name: (exe if name == "grove-music.exe" else notices if name == "DEPENDENCIES.md" else ROOT / source) for name, source in FILES.items()}
    payload = {}
    for name, source in sources.items():
        if source.is_symlink(): raise ValueError("Payload source is a symbolic link")
        payload[name] = source.read_bytes()
    output.mkdir(parents=True)
    lines = []
    for name, data in sorted(payload.items()):
        target = output / name
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("xb") as file: file.write(data)
        lines.append(hashlib.sha256(data).hexdigest() + "  " + name)
    (output / "SHA256SUMS.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    archive = Path(str(output) + ".zip")
    with zipfile.ZipFile(archive, "x", zipfile.ZIP_DEFLATED, compresslevel=9) as result:
        for name in sorted([*FILES, "SHA256SUMS.txt"]): result.write(output / name, name)
    check(archive)
    return archive

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", type=Path)
    parser.add_argument("--executable", type=Path, default=ROOT / "target/release/grove-music.exe")
    parser.add_argument("--notices", type=Path)
    parser.add_argument("--destination", type=Path)
    args = parser.parse_args()
    if args.check: check(args.check)
    elif args.notices and args.destination: print(package(args.executable, args.notices, args.destination))
    else: parser.error("Provide --check or --notices and --destination")
