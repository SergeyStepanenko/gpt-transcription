#!/usr/bin/env bash
# Тонкая обёртка: TLS Chrome через curl_cffi (системный curl Cloudflare режет 403).
# Использование: ./transcribe.sh [файл]   (дефолт whisper.webm)
set -euo pipefail
cd "$(dirname "$0")"
exec .venv/bin/python transcribe.py "$@"
