#!/usr/bin/env python3
"""Register, inspect, or remove Tide's macOS browser companion native messaging host."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import stat
import sys
import tempfile

HOST_NAME = "org.tidemusic.session"
HOST_DESCRIPTION = "Tide YouTube Music session receiver"
FIREFOX_EXTENSION_ID = "tide-session@tide-music"
POLICY_FILENAME = "tide-session-host-policy.json"
MAX_ALLOWED_ORIGINS = 16
MAX_POLICY_BYTES = 2048
DEFAULT_HOST = Path("/Applications/Tide.app/Contents/MacOS/tide-session-host")

BROWSER_MANIFEST_DIRS = {
    "chrome": Path("Library/Application Support/Google/Chrome/NativeMessagingHosts"),
    "edge": Path("Library/Application Support/Microsoft Edge/NativeMessagingHosts"),
    "brave": Path("Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
    "arc": Path("Library/Application Support/Arc/User Data/NativeMessagingHosts"),
    "firefox": Path("Library/Application Support/Mozilla/NativeMessagingHosts"),
}
CHROMIUM_BROWSERS = ("chrome", "edge", "brave", "arc")


class SetupError(ValueError):
    """Raised when registration inputs or target files fail safety checks."""


def normalize_chromium_extension_id(raw: str) -> str:
    value = raw.strip().lower()
    if value.startswith("chrome-extension://"):
        value = value[len("chrome-extension://") :]
        if value.endswith("/"):
            value = value[:-1]
    if not re.fullmatch(r"[a-p]{32}", value):
        raise SetupError(
            "Chromium extension ID must be 32 lowercase letters between 'a' and 'p'."
        )
    return value


def chromium_origin(extension_id: str) -> str:
    return f"chrome-extension://{normalize_chromium_extension_id(extension_id)}/"


def _reject_symlink(path: Path) -> None:
    if os.path.lexists(path):
        info = path.lstat()
        if stat.S_ISLNK(info.st_mode):
            raise SetupError(f"Refusing to touch symlink: {path}")


def _check_ancestors_no_symlink(target: Path, stop_at: Path) -> None:
    stop = stop_at.resolve()
    current = target
    while True:
        _reject_symlink(current)
        if current == stop or current.parent == current:
            break
        current = current.parent


def _validate_host_executable(path: Path) -> Path:
    if not path.is_absolute():
        raise SetupError("Host executable path must be absolute.")
    _reject_symlink(path)
    if not path.is_file():
        raise SetupError(f"Host executable does not exist: {path}")
    if path.name != "tide-session-host":
        raise SetupError("Host executable must be named tide-session-host.")
    if not os.access(path, os.X_OK):
        raise SetupError(f"Host executable is not executable: {path}")
    return path.resolve(strict=True)


def _policy_path(home: Path) -> Path:
    return home / "Library/Application Support/TideMusic" / POLICY_FILENAME


def _manifest_path(home: Path, browser: str) -> Path:
    return home / BROWSER_MANIFEST_DIRS[browser] / f"{HOST_NAME}.json"


def _atomic_write_json(path: Path, payload: dict, mode: int) -> None:
    _reject_symlink(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = (json.dumps(payload, indent=2) + "\n").encode("utf-8")
    fd, tmp_name = tempfile.mkstemp(prefix=".tide-nmh-", dir=path.parent)
    tmp_path = Path(tmp_name)
    try:
        os.fchmod(fd, mode)
        with os.fdopen(fd, "wb") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(tmp_path, path)
    except Exception:
        try:
            tmp_path.unlink(missing_ok=True)
        except OSError:
            pass
        raise


def _read_existing_origins(policy_file: Path) -> list[str]:
    _reject_symlink(policy_file)
    if not policy_file.exists():
        return []
    info = policy_file.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_size > MAX_POLICY_BYTES:
        raise SetupError(f"Existing host policy is invalid: {policy_file}")
    try:
        data = json.loads(policy_file.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise SetupError(f"Existing host policy could not be parsed: {policy_file}") from exc
    if not isinstance(data, dict) or set(data) != {"allowed_origins"}:
        raise SetupError(f"Existing host policy has unexpected fields: {policy_file}")
    origins = data.get("allowed_origins")
    if not isinstance(origins, list) or len(origins) > MAX_ALLOWED_ORIGINS:
        raise SetupError(f"Existing host policy origins are invalid: {policy_file}")
    normalized: list[str] = []
    for entry in origins:
        if not isinstance(entry, str) or not entry.startswith("chrome-extension://") or not entry.endswith("/"):
            raise SetupError(f"Existing host policy contains an invalid origin: {policy_file}")
        ext_id = normalize_chromium_extension_id(entry)
        origin = f"chrome-extension://{ext_id}/"
        if origin not in normalized:
            normalized.append(origin)
    return normalized


def _ensure_tide_owned_manifest(path: Path) -> None:
    _reject_symlink(path)
    if not path.exists():
        return
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise SetupError(f"Refusing to modify unreadable manifest: {path}") from exc
    if not isinstance(data, dict) or data.get("name") != HOST_NAME:
        raise SetupError(f"Refusing to modify manifest not owned by {HOST_NAME}: {path}")


def register(
    home: Path,
    browsers: list[str],
    host_executable: Path,
    chromium_extension_id: str | None = None,
) -> dict:
    host = _validate_host_executable(host_executable)
    needs_chromium = any(b in CHROMIUM_BROWSERS for b in browsers)
    policy_file = _policy_path(home)
    origins: list[str] = []

    if needs_chromium:
        if not chromium_extension_id:
            raise SetupError(
                "Registering Chrome, Edge, Brave, or Arc requires --chromium-extension-id."
            )
        new_origin = chromium_origin(chromium_extension_id)
        _check_ancestors_no_symlink(policy_file, home)
        origins = _read_existing_origins(policy_file)
        if new_origin not in origins:
            if len(origins) >= MAX_ALLOWED_ORIGINS:
                raise SetupError(
                    f"Host policy already contains the maximum of {MAX_ALLOWED_ORIGINS} extension IDs."
                )
            origins.append(new_origin)
        encoded_policy = json.dumps({"allowed_origins": origins}, indent=2) + "\n"
        if len(encoded_policy.encode("utf-8")) > MAX_POLICY_BYTES:
            raise SetupError("Host policy exceeds maximum allowed size.")
        _atomic_write_json(policy_file, {"allowed_origins": origins}, 0o600)

    written: dict[str, str] = {}
    for browser in browsers:
        manifest_file = _manifest_path(home, browser)
        _check_ancestors_no_symlink(manifest_file, home)
        _ensure_tide_owned_manifest(manifest_file)
        if browser in CHROMIUM_BROWSERS:
            manifest = {
                "name": HOST_NAME,
                "description": HOST_DESCRIPTION,
                "path": str(host),
                "type": "stdio",
                "allowed_origins": origins,
            }
        else:
            manifest = {
                "name": HOST_NAME,
                "description": HOST_DESCRIPTION,
                "path": str(host),
                "type": "stdio",
                "allowed_extensions": [FIREFOX_EXTENSION_ID],
            }
        _atomic_write_json(manifest_file, manifest, 0o644)
        written[browser] = str(manifest_file)
    return {
        "host": str(host),
        "policy": str(policy_file) if needs_chromium else None,
        "manifests": written,
    }


def unregister(home: Path, browsers: list[str]) -> dict:
    removed: dict[str, bool] = {}
    for browser in browsers:
        manifest_file = _manifest_path(home, browser)
        _check_ancestors_no_symlink(manifest_file, home)
        if not manifest_file.exists():
            removed[browser] = False
            continue
        _ensure_tide_owned_manifest(manifest_file)
        manifest_file.unlink()
        removed[browser] = True
    return {"removed": removed}


def status(home: Path, browsers: list[str]) -> dict:
    policy_file = _policy_path(home)
    origins: list[str] = []
    if policy_file.exists() and not policy_file.is_symlink():
        try:
            origins = _read_existing_origins(policy_file)
        except SetupError:
            origins = []
    registered: dict[str, dict] = {}
    for browser in browsers:
        manifest_file = _manifest_path(home, browser)
        entry = {"manifest": str(manifest_file), "registered": False, "path": None}
        if manifest_file.exists() and not manifest_file.is_symlink():
            try:
                data = json.loads(manifest_file.read_text(encoding="utf-8"))
                if isinstance(data, dict) and data.get("name") == HOST_NAME:
                    entry["registered"] = True
                    entry["path"] = data.get("path")
            except (OSError, json.JSONDecodeError):
                pass
        registered[browser] = entry
    return {
        "policy": str(policy_file),
        "allowed_origins": origins,
        "browsers": registered,
    }


def _select_browsers(choice: str, has_chromium_id: bool) -> list[str]:
    if choice == "all":
        return list(BROWSER_MANIFEST_DIRS) if has_chromium_id else ["firefox"]
    return [choice]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mode",
        choices=("register", "unregister", "status"),
        required=True,
    )
    parser.add_argument(
        "--browser",
        choices=(*BROWSER_MANIFEST_DIRS, "all"),
        default="all",
    )
    parser.add_argument(
        "--host-executable",
        type=Path,
        default=DEFAULT_HOST,
        help=f"Absolute path to tide-session-host (default: {DEFAULT_HOST})",
    )
    parser.add_argument(
        "--chromium-extension-id",
        help="Installed 32-letter Chrome/Edge/Brave/Arc extension ID",
    )
    parser.add_argument(
        "--home",
        type=Path,
        default=Path.home(),
        help="Override home directory (for testing)",
    )
    args = parser.parse_args(argv)
    home = args.home.expanduser()
    browsers = _select_browsers(args.browser, bool(args.chromium_extension_id))
    try:
        if args.mode == "register":
            result = register(home, browsers, args.host_executable, args.chromium_extension_id)
        elif args.mode == "unregister":
            browsers_unreg = list(BROWSER_MANIFEST_DIRS) if args.browser == "all" else [args.browser]
            result = unregister(home, browsers_unreg)
        else:
            browsers_stat = list(BROWSER_MANIFEST_DIRS) if args.browser == "all" else [args.browser]
            result = status(home, browsers_stat)
    except SetupError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
