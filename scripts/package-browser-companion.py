#!/usr/bin/env python3
"""Package the optional browser companion source prototype without installing it."""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import tempfile
import zipfile

ICON_FILES = {str(size): f"icon{size}.png" for size in (16, 32, 48, 128)}
COMMON_FILES = ("background.js", "popup.html", "popup.js", "popup.css", *ICON_FILES.values())
FAMILY_FILES = {
    "firefox": ("background-firefox.js", "manifest.firefox.json"),
    "chromium": ("background-chromium.js", "manifest.chromium.json"),
}
MUSIC_PATTERN = "https://music.youtube.com/*"
RESERVED_NAMES = {"CON", "PRN", "AUX", "NUL", *(f"COM{i}" for i in range(1, 10)), *(f"LPT{i}" for i in range(1, 10))}


class PackageError(ValueError):
    """A source or destination failed the package's safety checks."""


def _reject_link(path: Path) -> None:
    if not os.path.lexists(path):
        return
    info = path.lstat()
    if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400):
        raise PackageError(f"Symlinks and reparse points are not allowed: {path.name}")


def _stem(value: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,79}", value):
        raise PackageError("Destination must be a simple folder stem inside workspace dist")
    if value.endswith(".") or value.split(".", 1)[0].upper() in RESERVED_NAMES:
        raise PackageError("Destination must be a valid, non-reserved folder stem")
    return value


def _publish_no_replace(staging: Path, destination: Path) -> None:
    """A kernel no-replace rename closes the final existence-check race."""
    if os.name == "nt":
        # Windows os.rename refuses even an empty existing destination.
        os.rename(staging, destination)
        return
    # Linux's stdlib os.rename may replace an empty directory; use its no-replace variant.
    library = ctypes.CDLL(None, use_errno=True)
    rename_no_replace = getattr(library, "renameat2", None)
    if rename_no_replace is None:
        raise PackageError("This platform lacks an atomic no-overwrite directory rename")
    rename_no_replace.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename_no_replace.restype = ctypes.c_int
    if rename_no_replace(-100, os.fsencode(staging), -100, os.fsencode(destination), 1) != 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), str(destination))


def _validate_manifest(manifest: object, family: str) -> None:
    if not isinstance(manifest, dict):
        raise PackageError("Manifest must be an object")
    expected_permissions = {"activeTab", "webRequest", "nativeMessaging"}
    if family == "firefox":
        expected_permissions.add(MUSIC_PATTERN)
    permissions = manifest.get("permissions")
    if not isinstance(permissions, list) or set(permissions) != expected_permissions or len(permissions) != len(expected_permissions):
        raise PackageError("Manifest permissions differ from the prototype allowlist")
    forbidden = {"content_scripts", "externally_connectable", "web_accessible_resources", "optional_permissions", "optional_host_permissions", "key", "update_url", "chrome_settings_overrides"}
    if forbidden.intersection(manifest):
        raise PackageError("Unexpected manifest capabilities")
    if family == "firefox":
        expected_background = {"scripts": ["background.js", "background-firefox.js"], "persistent": True}
        if manifest.get("manifest_version") != 2 or manifest.get("host_permissions") is not None:
            raise PackageError("Firefox package requires its MV2 manifest")
        if manifest.get("browser_specific_settings", {}).get("gecko", {}).get("id") != "tide-session@tide-music":
            raise PackageError("Firefox extension ID does not match the native host")
        action = manifest.get("browser_action", {})
        csp = manifest.get("content_security_policy")
    else:
        expected_background = {"service_worker": "background-chromium.js"}
        if manifest.get("manifest_version") != 3 or manifest.get("host_permissions") != [MUSIC_PATTERN]:
            raise PackageError("Chromium package requires its narrowly scoped MV3 manifest")
        action = manifest.get("action", {})
        csp = manifest.get("content_security_policy", {}).get("extension_pages")
    if manifest.get("background") != expected_background or action.get("default_popup") != "popup.html":
        raise PackageError("Manifest entry points differ from the package allowlist")
    if manifest.get("icons") != ICON_FILES or action.get("default_icon") != {size: ICON_FILES[size] for size in ("16", "32")}:
        raise PackageError("Manifest icons differ from the local Tide icon allowlist")
    if csp != "default-src 'self'; script-src 'self'; object-src 'none'; connect-src 'none'":
        raise PackageError("Manifest must restrict code and resources to the local prototype")


def _read_payload(source: Path, family: str) -> dict[str, bytes]:
    _reject_link(source)
    if not source.is_dir():
        raise PackageError("Missing browser-companion source directory")
    entry, manifest_name = FAMILY_FILES[family]
    payload = {}
    for filename in (*COMMON_FILES, entry, manifest_name):
        path = source / filename
        _reject_link(path)
        if not path.is_file():
            raise PackageError(f"Missing regular source file: {filename}")
        flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
        with os.fdopen(os.open(path, flags), "rb") as stream:
            payload["manifest.json" if filename == manifest_name else filename] = stream.read()
        if filename in ICON_FILES.values():
            data = payload[filename]
            size = int(next(size for size, name in ICON_FILES.items() if name == filename))
            if len(data) < 33 or not data.startswith(b"\x89PNG\r\n\x1a\n") or data[12:16] != b"IHDR" or int.from_bytes(data[16:20], "big") != size or int.from_bytes(data[20:24], "big") != size:
                raise PackageError("Tide icon must be a PNG with the advertised dimensions")
    try:
        manifest = json.loads(payload["manifest.json"])
        _validate_manifest(manifest, family)
    except (json.JSONDecodeError, UnicodeDecodeError, TypeError, AttributeError) as error:
        raise PackageError("Invalid selected manifest") from error
    return payload


def package_companion(workspace_root: Path, family: str, destination: str | None = None) -> Path:
    """Test-injectable workspace; CLI always uses the workspace containing this script."""
    if family not in FAMILY_FILES:
        raise PackageError("Family must be firefox or chromium")
    stem = _stem(destination if destination is not None else f"tide-browser-{family}")
    root = Path(workspace_root).absolute()
    _reject_link(root)
    if not root.is_dir():
        raise PackageError("Workspace must already exist")
    # Inspect all ancestors too, including Windows junctions, before resolving paths.
    for ancestor in root.parents:
        _reject_link(ancestor)
    root = root.resolve(strict=True)
    dist = root / "dist"
    _reject_link(dist)
    if dist.exists() and not dist.is_dir():
        raise PackageError("Workspace dist must be a directory")
    destination_path = dist / stem
    if os.path.lexists(destination_path):
        raise PackageError("Destination already exists; refusing to overwrite")
    payload = _read_payload(root / "browser-companion", family)
    dist.mkdir(exist_ok=True)
    _reject_link(dist)
    if dist.resolve(strict=True).parent != root:
        raise PackageError("Output must remain inside workspace dist")
    staging = Path(tempfile.mkdtemp(prefix=".tide-browser-stage-", dir=dist))
    staging_identity = staging.stat()
    try:
        extension = staging / "extension"
        extension.mkdir()
        for filename, data in sorted(payload.items()):
            (extension / filename).write_bytes(data)
        archive_name = f"{stem}.zip"
        archive = staging / archive_name
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as output:
            for filename, data in sorted(payload.items()):
                item = zipfile.ZipInfo(filename, date_time=(1980, 1, 1, 0, 0, 0))
                item.compress_type = zipfile.ZIP_DEFLATED
                item.create_system = 3
                item.external_attr = (stat.S_IFREG | 0o644) << 16
                output.writestr(item, data, compresslevel=9)
        checksums = {f"extension/{name}": hashlib.sha256(data).hexdigest() for name, data in payload.items()}
        checksums[archive_name] = hashlib.sha256(archive.read_bytes()).hexdigest()
        (staging / "SHA256SUMS").write_text("".join(f"{digest}  {name}\n" for name, digest in sorted(checksums.items())), encoding="ascii", newline="\n")
        _reject_link(dist)
        if dist.resolve(strict=True).parent != root or os.path.lexists(destination_path):
            raise PackageError("Destination changed or already exists; refusing to publish")
        _publish_no_replace(staging, destination_path)
        return destination_path
    finally:
        # Never delete a computed destination or an unrelated staging tree.
        if os.path.lexists(staging):
            _reject_link(staging)
            identity = staging.stat()
            if staging.parent != dist or not staging.name.startswith(".tide-browser-stage-") or (identity.st_dev, identity.st_ino) != (staging_identity.st_dev, staging_identity.st_ino):
                raise PackageError("Staging directory identity changed; refusing cleanup")
            _reject_link(dist)
            if dist.resolve(strict=True).parent != root:
                raise PackageError("Staging parent changed; refusing cleanup")
            shutil.rmtree(staging)


def main() -> int:
    parser = argparse.ArgumentParser(description="Package the optional Tide browser companion source prototype; does not install or sign it.")
    parser.add_argument("--family", required=True, choices=sorted(FAMILY_FILES))
    parser.add_argument("--destination", metavar="FOLDER_STEM", help="New folder stem under workspace dist (default: tide-browser-FAMILY)")
    args = parser.parse_args()
    try:
        output = package_companion(Path(__file__).resolve().parents[1], args.family, args.destination)
    except (PackageError, OSError) as error:
        parser.exit(1, f"Packaging failed: {error}\n")
    print(f"Source prototype package: {output}")
    print("Loadable folder: extension/; archive and SHA256SUMS are alongside it. Installation and signing remain separate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
