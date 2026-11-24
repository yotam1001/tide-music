#!/bin/bash
# Stable Linux AppImage. Jams are included, as on Windows and macOS.
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 -B scripts/package_linux.py "$@"
