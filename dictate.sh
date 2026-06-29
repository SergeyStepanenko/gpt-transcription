#!/usr/bin/env bash
# Push-to-talk диктовка: держи Right Option -> запись, отпусти -> вставка транскрипции.
# Нужны права macOS: Микрофон + Универсальный доступ (Accessibility) + Мониторинг ввода
# для терминала, из которого запускаешь. Ctrl-C — выход.
set -euo pipefail
cd "$(dirname "$0")"
exec .venv/bin/python ptt.py "$@"
