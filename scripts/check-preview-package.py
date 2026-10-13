"""Check preview ZIP payloads, checksums and credential/private-path patterns."""
import argparse
import hashlib
import re
import zipfile
from check_public_files import PATTERNS


PAYLOAD = {
    "tide-music.exe", "LICENSE", "THIRD_PARTY_NOTICES.md", "DEPENDENCIES.md",
    "START_HERE.md", "docs/ACCOUNT_SETUP.md", "scripts/setup-runtime.ps1",
    "scripts/setup-provider.ps1", "helpers/youtube_music.py", "helpers/requirements.txt",
}
BROWSER_HOST_PAYLOAD = {
    "tide-session-host.exe", "scripts/setup-browser-companion.ps1",
    "docs/BROWSER_COMPANION.md",
}

# Executables concatenate static strings and contain arbitrary binary bytes.
# Avoid interpreting the SVG property "mask-..." as an API key or consuming
# non-text bytes following a compiled cookie-name literal as a cookie value.
BINARY_PATTERNS = [
    (label, re.compile(rb"(?<![A-Za-z0-9_-])sk-[A-Za-z0-9_-]{20,}")
     if label == "API credential" else
     re.compile(rb"(?:SAPISID|__Secure-3PAPISID)=[\x21-\x3a\x3c-\x7e]{10,}")
     if label == "Google session cookie" else pattern)
    for label, pattern in PATTERNS
]


def check(path, with_browser_host=False):
    expected_payload = PAYLOAD | (BROWSER_HOST_PAYLOAD if with_browser_host else set())
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)) or set(names) != expected_payload | {"SHA256SUMS.txt"}:
            raise ValueError("Archive contains unexpected, missing or duplicate files")
        manifest = {}
        for line in archive.read("SHA256SUMS.txt").decode("utf-8-sig").splitlines():
            match = re.fullmatch(r"([a-f0-9]{64})  (.+)", line)
            if not match or match[2] in manifest:
                raise ValueError("Unsupported or duplicate checksum entry")
            manifest[match[2]] = match[1]
        if set(manifest) != expected_payload:
            raise ValueError("Checksum manifest does not cover the exact payload")
        for name in names:
            data = archive.read(name)
            if name in manifest and hashlib.sha256(data).hexdigest() != manifest[name]:
                raise ValueError(f"Checksum mismatch: {name}")
            patterns = BINARY_PATTERNS if name.endswith(".exe") else PATTERNS
            for label, pattern in patterns:
                if pattern.search(data):
                    raise ValueError(f"Package privacy check failed: {name}: {label}")
    print("PASS: preview ZIP allowlist, payload checksums and privacy scan")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive")
    parser.add_argument("--with-browser-host", action="store_true")
    args = parser.parse_args()
    try:
        check(args.archive, args.with_browser_host)
    except (ValueError, zipfile.BadZipFile, OSError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
