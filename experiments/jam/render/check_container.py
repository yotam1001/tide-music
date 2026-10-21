"""Disposable CI probe; never prints room capabilities or request bodies."""
import json
import subprocess
import time
import urllib.error
import urllib.request


def request(path, *, method="GET", headers=None, data=None):
    values = {"Host": "relay-test.onrender.com", "X-Tide-Jam-Protocol": "1"}
    values.update(headers or {})
    req = urllib.request.Request(
        "http://127.0.0.1:18765" + path, data=data, headers=values, method=method
    )
    try:
        with urllib.request.urlopen(req, timeout=3) as response:
            return response.status, response.read(32769)
    except urllib.error.HTTPError as error:
        return error.code, error.read(32769)


def main():
    for _ in range(60):
        try:
            if request("/healthz") == (200, b"tide-jam-ready-v1"):
                break
        except (OSError, urllib.error.URLError):
            pass
        time.sleep(0.5)
    else:
        raise AssertionError("Container did not become ready")
    status, _ = request("/v1/rooms", method="POST", data=b"{}", headers={
        "Content-Type": "application/json", "Origin": "https://untrusted.example"
    })
    assert status == 403, "Browser origins must remain rejected"
    for n in range(2):
        status, body = request("/v1/rooms", method="POST", data=b"{}", headers={
            "Content-Type": "application/json",
            "X-Tide-Jam-Peer": f"198.51.100.{n + 1}",
            "X-Forwarded-For": f"198.51.100.{n + 1}",
        })
        assert status == 200, "Room creation failed through container proxy"
        assert set(json.loads(body)) == {"room", "host", "invite"}
    status, _ = request("/v1/rooms", method="POST", data=b"{}", headers={
        "Content-Type": "application/json",
        "X-Tide-Jam-Peer": "203.0.113.99", "X-Forwarded-For": "203.0.113.99",
    })
    assert status == 429, "Spoofed headers must not bypass the shared trial quota"
    print("Container health, room creation, origin rejection and quota spoof protection passed")
    subprocess.run(["docker", "stats", "--no-stream", "--format", "{{.MemUsage}}", "tide-render-test"], check=True)
    # Kill only the relay inside this disposable container. PID 1 must stop the
    # proxy as well, so a dead relay cannot keep returning healthy responses.
    command = '''for f in /proc/[0-9]*/comm; do
read -r name < "$f" || continue
if [[ "$name" == tide-jam-relay ]]; then
pid=${f#/proc/}; kill -TERM "${pid%/comm}"; exit 0
fi
done
exit 1'''
    subprocess.run(["docker", "exec", "tide-render-test", "bash", "-c", command], check=True)
    for _ in range(30):
        running = subprocess.check_output(["docker", "inspect", "--format", "{{.State.Running}}", "tide-render-test"], text=True).strip()
        if running == "false":
            print("Relay failure correctly stopped the proxy container")
            return
        time.sleep(0.2)
    raise AssertionError("Proxy survived a failed relay")


if __name__ == "__main__":
    main()
