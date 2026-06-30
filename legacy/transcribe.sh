#!/usr/bin/env bash
# Thin wrapper: Chrome TLS via curl_cffi (the system curl gets a Cloudflare 403).
# Usage: ./transcribe.sh [file]   (default whisper.webm)
set -euo pipefail
cd "$(dirname "$0")"
exec .venv/bin/python transcribe.py "$@"
