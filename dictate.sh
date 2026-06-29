#!/usr/bin/env bash
# Push-to-talk dictation: hold Right Option -> record, release -> paste the transcription.
# Needs macOS permissions: Microphone + Accessibility + Input Monitoring
# for the terminal you launch it from. Ctrl-C quits.
set -euo pipefail
cd "$(dirname "$0")"
exec .venv/bin/python ptt.py "$@"
