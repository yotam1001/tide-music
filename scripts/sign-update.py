"""Offline maintainer signing. Private keys never enter the repository or CI.

Requires cryptography for signing; installed Tide verifies using Rust ring.
Windows keys are protected with DPAPI CurrentUser and an owner-only directory.
"""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time

DOMAIN = b"Tide update manifest v1\n"
MAX_INSTALLER = 96 * 1024 * 1024
MAX_VALIDITY = 90 * 24 * 60 * 60
ROOT = Path(__file__).resolve().parent.parent


def public_key():
    value = (ROOT / "crates/app/update-public-key.hex").read_text("ascii").strip()
    if not re.fullmatch(r"[0-9a-f]{64}", value):
        raise ValueError("Invalid pinned public key")
    return bytes.fromhex(value)


def protect(data, decrypt=False):
    if os.name != "nt":
        raise RuntimeError("This maintainer key store requires Windows DPAPI")

    class Blob(ctypes.Structure):
        _fields_ = [("size", wintypes.DWORD), ("data", ctypes.POINTER(ctypes.c_ubyte))]

    buffer = ctypes.create_string_buffer(data)
    source = Blob(len(data), ctypes.cast(buffer, ctypes.POINTER(ctypes.c_ubyte)))
    target = Blob()
    entropy_buffer = ctypes.create_string_buffer(DOMAIN)
    entropy = Blob(len(DOMAIN), ctypes.cast(entropy_buffer, ctypes.POINTER(ctypes.c_ubyte)))
    crypt = ctypes.WinDLL("crypt32", use_last_error=True)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.LocalFree.argtypes = [ctypes.c_void_p]
    kernel.LocalFree.restype = ctypes.c_void_p
    if decrypt:
        call = crypt.CryptUnprotectData
        call.argtypes = [ctypes.POINTER(Blob), ctypes.c_void_p, ctypes.POINTER(Blob), ctypes.c_void_p, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(Blob)]
        call.restype = wintypes.BOOL
        ok = call(ctypes.byref(source), None, ctypes.byref(entropy), None, None, 1, ctypes.byref(target))
    else:
        call = crypt.CryptProtectData
        call.argtypes = [ctypes.POINTER(Blob), wintypes.LPCWSTR, ctypes.POINTER(Blob), ctypes.c_void_p, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(Blob)]
        call.restype = wintypes.BOOL
        ok = call(ctypes.byref(source), "Tide release signing", ctypes.byref(entropy), None, None, 1, ctypes.byref(target))
    try:
        if not ok:
            raise RuntimeError("DPAPI signing-key operation failed")
        return ctypes.string_at(target.data, target.size)
    finally:
        ctypes.memset(buffer, 0, len(data))
        if target.data:
            ctypes.memset(target.data, 0, target.size)
            kernel.LocalFree(target.data)


def key_file():
    if os.name != "nt":
        raise RuntimeError("Signing is performed on the maintainer's Windows PC")
    return Path(os.environ["LOCALAPPDATA"]) / "TideReleaseSigning" / "ed25519.dpapi"


def init_key():
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    destination = key_file()
    pinned = ROOT / "crates/app/update-public-key.hex"
    if destination.exists() or pinned.exists():
        raise RuntimeError("A key already exists; initialization never rotates or overwrites it")
    destination.parent.mkdir(parents=True, exist_ok=True)
    # whoami CSV is parsed without logging the local identity.
    import csv
    sid = next(csv.reader(subprocess.check_output(["whoami", "/user", "/fo", "csv", "/nh"], text=True).splitlines()))[1]
    subprocess.run(["icacls", str(destination.parent), "/inheritance:r", "/grant:r", f"*{sid}:(OI)(CI)F"], check=True, stdout=subprocess.DEVNULL)
    key = Ed25519PrivateKey.generate()
    protected = protect(key.private_bytes_raw())
    with destination.open("xb") as output:
        output.write(protected)
    pinned.write_text(key.public_key().public_bytes_raw().hex() + "\n", encoding="ascii")
    # Check the encrypted store before accepting this new trust root.
    restored = Ed25519PrivateKey.from_private_bytes(protect(destination.read_bytes(), decrypt=True))
    if restored.public_key().public_bytes_raw() != public_key():
        raise RuntimeError("Signing-key roundtrip failed")
    print("Created encrypted local signing key and public verification key")


def make_envelope(installer, version, key, now=None):
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("A stable numeric version is required")
    installer = Path(installer)
    size = installer.stat().st_size
    if not 0 < size <= MAX_INSTALLER or installer.name != "TideSetup.exe":
        raise ValueError("Invalid installer name or size")
    now = int(time.time()) if now is None else now
    with installer.open("rb") as input_file:
        digest = hashlib.file_digest(input_file, "sha256").hexdigest()
    manifest = dict(schema=1, version=version, platform="windows-x86_64", filename="TideSetup.exe",
                    size=size, sha256=digest,
                    issued_at=now, expires_at=now + MAX_VALIDITY)
    payload = json.dumps(manifest, separators=(",", ":"), sort_keys=True)
    return json.dumps(dict(payload=payload, signature=key.sign(DOMAIN + payload.encode("utf-8")).hex()), separators=(",", ":")).encode("utf-8")


def verify_envelope(envelope, installer, version, key=None, now=None):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    if len(envelope) > 16 * 1024:
        raise ValueError("Manifest too large")
    outer = json.loads(envelope)
    if set(outer) != {"payload", "signature"}:
        raise ValueError("Unexpected envelope fields")
    Ed25519PublicKey.from_public_bytes(public_key() if key is None else key).verify(bytes.fromhex(outer["signature"]), DOMAIN + outer["payload"].encode("utf-8"))
    value = json.loads(outer["payload"])
    if set(value) != {"schema", "version", "platform", "filename", "size", "sha256", "issued_at", "expires_at"}:
        raise ValueError("Unexpected manifest fields")
    now = int(time.time()) if now is None else now
    with Path(installer).open("rb") as input_file:
        digest = hashlib.file_digest(input_file, "sha256").hexdigest()
    if (value["schema"] != 1 or value["version"] != version or value["platform"] != "windows-x86_64"
            or value["filename"] != "TideSetup.exe" or value["size"] != Path(installer).stat().st_size
            or not 0 < value["size"] <= MAX_INSTALLER or value["sha256"] != digest
            or value["issued_at"] > now + 300 or value["expires_at"] <= now
            or not 0 < value["expires_at"] - value["issued_at"] <= MAX_VALIDITY):
        raise ValueError("Manifest does not authorize this installer")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--init-key", action="store_true")
    parser.add_argument("--installer", type=Path)
    parser.add_argument("--version")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--verify", type=Path, help="Verify an existing public manifest without opening the private key")
    args = parser.parse_args()
    if args.init_key:
        init_key()
        return
    if not args.installer or not args.version:
        parser.error("--installer and --version are required")
    if args.verify:
        verify_envelope(args.verify.read_bytes(), args.installer, args.version)
        print("PASS independent publisher signature and installer identity")
        return
    if not args.output or args.output.exists():
        parser.error("--output must name a new file")
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    key = Ed25519PrivateKey.from_private_bytes(protect(key_file().read_bytes(), decrypt=True))
    if key.public_key().public_bytes_raw() != public_key():
        raise RuntimeError("Local signing key does not match the pinned public key")
    envelope = make_envelope(args.installer, args.version, key)
    verify_envelope(envelope, args.installer, args.version)
    args.output.write_bytes(envelope)
    print("Signed installer identity; private key remains encrypted locally")


if __name__ == "__main__":
    main()
