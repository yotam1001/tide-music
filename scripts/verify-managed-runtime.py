"""Opt-in real download/import smoke test in a disposable Windows temp fixture."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--download", action="store_true", required=True,
                        help="allow pinned upstream downloads into a temporary folder")
    parser.parse_args()
    if os.name != "nt":
        parser.exit(1, "Windows x64 is required.\n")
    source = Path(__file__).resolve().parent.parent
    environment = os.environ.copy()
    environment["PSModulePath"] = str(Path(os.environ["SystemRoot"]) /
                                        "System32/WindowsPowerShell/v1.0/Modules")
    with tempfile.TemporaryDirectory(prefix="tide-runtime-smoke-") as temporary:
        fixture = Path(temporary).resolve()
        if fixture.parent != Path(tempfile.gettempdir()).resolve():
            raise RuntimeError("Unexpected temporary fixture location")
        (fixture / "scripts").mkdir()
        (fixture / "helpers").mkdir()
        for name in ("install-common.ps1", "setup-provider.ps1", "setup-runtime.ps1"):
            shutil.copyfile(source / "scripts" / name, fixture / "scripts" / name)
        shutil.copyfile(source / "helpers/requirements.txt", fixture / "helpers/requirements.txt")

        def run(arguments, timeout=240):
            result = subprocess.run(arguments, cwd=fixture, env=environment,
                                    capture_output=True, timeout=timeout, check=False)
            if result.returncode:
                detail = result.stderr.decode(errors="replace").replace(str(fixture), "<temporary fixture>")
                raise RuntimeError("Isolated runtime verification failed: " + detail)
            return result.stdout.decode(errors="replace")

        run(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
             str(fixture / "scripts/setup-provider.ps1"), "-Managed"])
        interpreter = fixture / ".tools/provider-venv/Scripts/python.exe"
        run([str(interpreter), "-I", "-c",
             "import sys, importlib.metadata as m, ytmusicapi, requests, certifi; "
             "assert sys.version_info[:3] == (3,14,0); "
             "expected={'ytmusicapi':'1.12.3','requests':'2.34.2','charset-normalizer':'3.5.2',"
             "'idna':'3.20','urllib3':'2.8.0','certifi':'2026.7.22'}; "
             "assert all(m.version(k)==v for k,v in expected.items()); "
             "assert sys.flags.isolated; assert hasattr(ytmusicapi, 'YTMusic')"])
        print("PASS: pinned embedded CPython and all six helper wheel versions/imports", flush=True)
        run(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
             str(fixture / "scripts/setup-runtime.ps1"), "-Destination", "tools"])
        for name, arguments, version in (
            ("mpv.exe", ["--no-config", "--version"], "0.41.0"),
            ("yt-dlp.exe", ["--ignore-config", "--version"], "2026.08.19"),
            ("deno.exe", ["--version"], "2.9.7"),
        ):
            if version not in run([str(fixture / "tools" / name), *arguments], timeout=30):
                raise RuntimeError("Unexpected pinned playback tool version")
        print("PASS: pinned mpv, yt-dlp and Deno download checksums and executable versions", flush=True)
    print("PASS: disposable runtime removed; no installer, shortcut, registry or account operation", flush=True)


if __name__ == "__main__":
    main()
