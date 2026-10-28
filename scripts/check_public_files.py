"""Check tracked/staged source without printing possible credential values."""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PATTERNS = [
    ("private filesystem path", re.compile(rb"[A-Za-z]:[\\/]+Users[\\/]", re.I)),
    ("GitHub credential", re.compile(rb"(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})")),
    ("API credential", re.compile(rb"sk-[A-Za-z0-9_-]{20,}")),
    ("private key", re.compile(rb"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----")),
    ("authorization value", re.compile(rb"Bearer [A-Za-z0-9._-]{20,}", re.I)),
    ("Google session cookie", re.compile(rb"(?:SAPISID|__Secure-3PAPISID)=[^;\s]{10,}")),
]
FORBIDDEN = {".env", "headers.txt", "cookies.txt", "credentials.json", "auth.json"}


def main():
    args = ["git", "ls-files", "-z"]
    names = subprocess.check_output(args, cwd=ROOT).decode().split("\0")
    failed = []
    for name in filter(None, names):
        path = ROOT / name
        if (
            path.name.lower() in FORBIDDEN
            or path.name.lower().endswith((".jam-invite.json", ".jam-operator-key", ".dpapi", ".pfx", ".p12", ".pkcs8"))
            or name.startswith((".tools/", ".local/", "target/"))
        ):
            failed.append((name, "forbidden local file"))
        if not path.is_file() or path.suffix.lower() in {".png", ".jpg", ".ico"}:
            continue
        # Scan index bytes: the next commit's content, not an unstaged replacement.
        data = subprocess.check_output(["git", "show", ":" + name], cwd=ROOT)
        for label, pattern in PATTERNS:
            if pattern.search(data):
                failed.append((name, label))
    for name, label in failed:
        print(f"FAIL: {name}: {label}")
    if failed:
        return 1
    print(f"Public file check passed ({len(list(filter(None, names)))} tracked files).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
