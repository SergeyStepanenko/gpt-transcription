#!/usr/bin/env bash
# Record the mic into whisper.webm (WebM/Opus, mono, 48k) — same format as ChatGPT voice input.
# Usage: ./record.sh [seconds] [file]
#   ./record.sh            -> 5 sec into whisper.webm
#   ./record.sh 8          -> 8 sec
#   ./record.sh 8 out.webm -> 8 sec into out.webm
# Mic: env MIC (index from `ffmpeg -f avfoundation -list_devices true -i ""`).
# Default = built-in MacBook mic, auto-detected by name (reuses ptt.py's resolver).
set -euo pipefail
cd "$(dirname "$0")"

DUR="${1:-5}"
OUT="${2:-whisper.webm}"
MIC="${MIC:-$(.venv/bin/python -c 'import ptt; print(ptt.MIC)')}"

echo "Recording ${DUR}s from mic [$MIC] -> $OUT ..."
ffmpeg -hide_banner -loglevel error \
  -f avfoundation -i ":${MIC}" -t "$DUR" \
  -c:a libopus -ac 1 -ar 48000 \
  -y "$OUT"

echo "Done: $OUT ($(wc -c <"$OUT" | tr -d ' ') bytes)"
