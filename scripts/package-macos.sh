#!/bin/bash
set -euo pipefail
umask 077
project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$project_root/scripts/package-macos.py" "$@"
