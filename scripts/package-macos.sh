#!/bin/bash
# Stable macOS distribution; Jams must remain opt-in.
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 scripts/package_macos.py "$@"
