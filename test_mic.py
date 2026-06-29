#!/usr/bin/env python3
"""Self-check for the avfoundation mic-name resolver. Run: .venv/bin/python test_mic.py"""
import os
os.environ["MIC"] = "99"  # set before import so module-level resolve_mic() skips the ffmpeg call
from ptt import pick_audio_index

# Real avfoundation listing shape: two sections, indices restart per section.
LISTING = """\
[AVFoundation indev @ 0x1] AVFoundation video devices:
[AVFoundation indev @ 0x1] [0] FaceTime HD Camera
[AVFoundation indev @ 0x1] [1] Capture screen 0
[AVFoundation indev @ 0x1] AVFoundation audio devices:
[AVFoundation indev @ 0x1] [0] External USB Microphone
[AVFoundation indev @ 0x1] [1] MacBook Pro Microphone
"""

# Picks the audio-section index, not a same-numbered video device.
assert pick_audio_index(LISTING) == "1", pick_audio_index(LISTING)
# Case-insensitive name match.
assert pick_audio_index(LISTING, want="macbook") == "1"
# Absent device -> None (caller falls back to "1").
assert pick_audio_index(LISTING, want="Yeti") is None
# Empty / no audio section -> None, no crash.
assert pick_audio_index("") is None

print("test_mic OK")
