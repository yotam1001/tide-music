#!/bin/sh
# Developer setup for Tide's account helper on macOS: a private venv in
# .tools/provider-venv with the pinned helper requirements. Packaged Tide.app
# bundles its own Python and never uses this.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
venv="$root/.tools/provider-venv"
if [ ! -x "$venv/bin/python3" ]; then
    python3 -m venv "$venv" || {
        echo "Python 3.10+ is required (for example: brew install python)." >&2
        exit 1
    }
fi
"$venv/bin/python3" -m pip install --disable-pip-version-check --require-virtualenv \
    -r "$root/helpers/requirements.txt"
echo "Account helper ready: $venv/bin/python3"
