"""Verify the native guest rejects an isolated Windows named pipe before opening it.

Consumes Cargo's --no-run --message-format=json output for the experimental app.
No browser, account, real invite, audio runtime or network listener is used.
"""
import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
import uuid

PROBE = "jam_session::tests::private_pipe_input_does_not_block_cancellation"


class Overlapped(ctypes.Structure):
    _fields_ = [("Internal", ctypes.c_size_t), ("InternalHigh", ctypes.c_size_t),
                ("Offset", wintypes.DWORD), ("OffsetHigh", wintypes.DWORD),
                ("hEvent", wintypes.HANDLE)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-artifacts", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("The private-pipe probe requires Windows")
    if args.cargo_artifacts.stat().st_size > 8 * 1024 * 1024:
        parser.error("Cargo artifact output is unexpectedly large")
    artifacts = [json.loads(line) for line in args.cargo_artifacts.read_text(
        encoding="utf-8-sig").splitlines() if line.strip()]
    executables = {item["executable"] for item in artifacts
                   if item.get("executable") and item.get("profile", {}).get("test")
                   and item.get("target", {}).get("name") == "tide-music"
                   and item.get("target", {}).get("kind") == ["bin"]}
    if len(executables) != 1:
        parser.error("Expected exactly one compiled native app test executable")
    executable = Path(executables.pop())
    if not executable.is_file() or executable.suffix.lower() != ".exe":
        parser.error("The compiled Windows test executable is missing")
    listed = subprocess.run([str(executable), PROBE, "--ignored", "--exact",
                             "--list", "--format", "terse"],
                            capture_output=True, timeout=5,
                            creationflags=subprocess.CREATE_NO_WINDOW, check=False)
    test_names = [line for line in listed.stdout.decode(errors="replace").splitlines()
                  if line.endswith(": test")]
    if listed.returncode != 0 or test_names != [PROBE + ": test"]:
        raise RuntimeError("The native cancellation probe is absent; zero matching tests cannot pass this gate")

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD,
        wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD,
        wintypes.DWORD, wintypes.LPVOID]
    kernel.CreateNamedPipeW.restype = wintypes.HANDLE
    kernel.CreateEventW.argtypes = [wintypes.LPVOID, wintypes.BOOL, wintypes.BOOL,
                                   wintypes.LPCWSTR]
    kernel.CreateEventW.restype = wintypes.HANDLE
    kernel.ConnectNamedPipe.argtypes = [wintypes.HANDLE, ctypes.POINTER(Overlapped)]
    kernel.ConnectNamedPipe.restype = wintypes.BOOL
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.CancelIoEx.argtypes = [wintypes.HANDLE, ctypes.POINTER(Overlapped)]
    kernel.CancelIoEx.restype = wintypes.BOOL
    kernel.DisconnectNamedPipe.argtypes = [wintypes.HANDLE]
    kernel.DisconnectNamedPipe.restype = wintypes.BOOL
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.restype = wintypes.BOOL

    name = r"\\.\pipe\tide-jam-input-probe-" + uuid.uuid4().hex + ".jam-invite.json"
    # Outbound, overlapped, first instance; remote pipe clients are rejected.
    pipe = kernel.CreateNamedPipeW(name, 2 | 0x40000000 | 0x80000, 8,
                                  1, 512, 512, 0, None)
    if pipe == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    event, process = None, None
    overlapped = Overlapped()
    try:
        event = kernel.CreateEventW(None, True, False, None)
        if not event:
            raise ctypes.WinError(ctypes.get_last_error())
        overlapped.hEvent = event
        if not kernel.ConnectNamedPipe(pipe, ctypes.byref(overlapped)):
            if ctypes.get_last_error() != 997:  # ERROR_IO_PENDING
                raise ctypes.WinError(ctypes.get_last_error())
        environment = os.environ.copy()
        environment["JAM_PRIVATE_PIPE_PROBE"] = name
        process = subprocess.Popen([
            str(executable), PROBE,
            "--ignored", "--exact", "--nocapture"], env=environment,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate(timeout=2)
            raise RuntimeError("Private pipe input blocked native guest cancellation") from None
        connected = kernel.WaitForSingleObject(event, 0) == 0
        if process.returncode != 0 or connected:
            raise RuntimeError("Native guest did not reject the special input before opening it")
        print("PASS: native guest rejects private pipe before opening; cancellation finishes; no account or audio used")
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=2)
        kernel.CancelIoEx(pipe, ctypes.byref(overlapped))
        kernel.DisconnectNamedPipe(pipe)
        kernel.CloseHandle(pipe)
        if event:
            kernel.CloseHandle(event)


if __name__ == "__main__":
    main()
