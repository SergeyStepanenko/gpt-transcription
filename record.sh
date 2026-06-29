#!/usr/bin/env bash
# Записать микрофон в whisper.webm (WebM/Opus, mono, 48k) — формат как у голосового ввода ChatGPT.
# Использование: ./record.sh [секунды] [файл]
#   ./record.sh            -> 5 сек в whisper.webm
#   ./record.sh 8          -> 8 сек
#   ./record.sh 8 out.webm -> 8 сек в out.webm
# Микрофон: env MIC (номер из `ffmpeg -f avfoundation -list_devices true -i ""`). Дефолт 1 = MacBook Pro Mic.
set -euo pipefail
cd "$(dirname "$0")"

DUR="${1:-5}"
OUT="${2:-whisper.webm}"
MIC="${MIC:-1}"

echo "Пишу ${DUR}с с микрофона [$MIC] -> $OUT ..."
ffmpeg -hide_banner -loglevel error \
  -f avfoundation -i ":${MIC}" -t "$DUR" \
  -c:a libopus -ac 1 -ar 48000 \
  -y "$OUT"

echo "Готово: $OUT ($(wc -c <"$OUT" | tr -d ' ') байт)"
