#!/usr/bin/env python3
"""Push-to-talk dictation. Hold Right Command to record the mic; release to transcribe
and paste the text into the focused field (Cmd+V), saving and restoring the clipboard.
Ctrl-C in the terminal quits. Mic: env MIC overrides; default = built-in MacBook mic (auto-detected)."""
import os, subprocess, threading, time, pathlib
from pynput import keyboard
from transcribe import transcribe

HERE = pathlib.Path(__file__).parent
OUT = HERE / "_ptt.webm"


def pick_audio_index(listing, want="MacBook"):
    """Find the avfoundation *audio* device index whose name contains `want`.
    Indices restart per section, so only scan lines after 'audio devices:'.
    Returns the index string, or None if not found."""
    import re
    in_audio = False
    for line in listing.splitlines():
        if "audio devices:" in line:
            in_audio = True
            continue
        if "video devices:" in line:
            in_audio = False
            continue
        if not in_audio:
            continue
        m = re.search(r"\[(\d+)\]\s*(.+?)\s*$", line)
        if m and want.lower() in m.group(2).lower():
            return m.group(1)
    return None


def resolve_mic():
    """MIC env overrides; otherwise auto-detect the built-in MacBook mic, fall back to '1'."""
    env = os.environ.get("MIC")
    if env:
        return env
    listing = subprocess.run(
        ["ffmpeg", "-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""],
        capture_output=True, text=True).stderr  # device list goes to stderr
    return pick_audio_index(listing) or "1"


MIC = resolve_mic()

state = {"rec": False, "busy": False}
_kb = keyboard.Controller()
_V = keyboard.KeyCode.from_vk(9)  # kVK_ANSI_V — physical V key, layout-independent

# Audio cues — built-in macOS sounds, played non-blocking so they never delay the hot path.
START_SOUND = "/System/Library/Sounds/Morse.aiff"   # press
STOP_SOUND = "/System/Library/Sounds/Bottle.aiff"   # release

# Persistent warm capture: one ffmpeg holds the mic open and streams raw PCM, so a key press
# just flips a flag (instant) instead of cold-starting avfoundation (~1s) on every press.
RATE = 48000
_buf = bytearray()
_buf_lock = threading.Lock()
_warm = {"proc": None}


def ensure_accessibility():
    """Cmd+V needs the Accessibility permission — separate from Input Monitoring, which
    is what recording uses. Without it the paste silently does nothing. Check it and raise
    the system dialog that adds this app to the Accessibility list."""
    try:
        from ApplicationServices import (AXIsProcessTrustedWithOptions,
                                          kAXTrustedCheckOptionPrompt)
        trusted = AXIsProcessTrustedWithOptions({kAXTrustedCheckOptionPrompt: True})
    except Exception:
        return  # could not check — don't get in the way
    if not trusted:
        print("⚠️  No Accessibility permission — paste (Cmd+V) won't work (recording still will).")
        print("    System Settings → Privacy & Security → Accessibility: enable the app")
        print("    you launch dictate.sh from (the dialog is already open). Then restart the script.")


def is_right_cmd(key):
    # Right Command: pynput Key.cmd_r or keycode vk 54 (Left Command = 55)
    return key == keyboard.Key.cmd_r or getattr(key, "vk", None) == 54


def warm_start():
    """Open the mic once and keep it open, streaming s16le PCM to stdout. A daemon thread
    drains the pipe and only keeps bytes while state['rec'] is set. Keeping avfoundation open
    is what makes recording instant. Cost: the mic stays active (macOS shows the orange
    indicator) the whole time the script runs.
    ponytail: warm capture for instant start. Want a quiet mic between presses instead? Revert
    to a per-press `subprocess.Popen` in on_press — that brings back the ~1s avfoundation cold start."""
    p = subprocess.Popen(
        ["ffmpeg", "-hide_banner", "-loglevel", "error",
         "-f", "avfoundation", "-i", f":{MIC}",
         "-ac", "1", "-ar", str(RATE), "-f", "s16le", "-"],
        stdout=subprocess.PIPE, stdin=subprocess.DEVNULL)
    _warm["proc"] = p

    def reader():
        while True:
            chunk = p.stdout.read(4096)  # ~43ms of audio per chunk at 48k/s16/mono
            if not chunk:
                break  # ffmpeg closed/died
            if state["rec"]:
                with _buf_lock:
                    _buf.extend(chunk)

    threading.Thread(target=reader, daemon=True).start()


def beep(sound):
    # fire-and-forget; the orphan afplay exits on its own. Output muted so it can't spam the terminal.
    subprocess.Popen(["afplay", sound],
                      stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def encode(pcm):
    # Raw PCM -> opus/webm (the format the endpoint expects). Runs after release, off the hot path.
    subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error",
         "-f", "s16le", "-ar", str(RATE), "-ac", "1", "-i", "-",
         "-c:a", "libopus", "-y", str(OUT)],
        input=pcm)


def paste_text(text):
    # pbpaste/pbcopy — text only; images/files on the clipboard won't be preserved (ceiling)
    old = subprocess.run(["pbpaste"], capture_output=True).stdout
    subprocess.run(["pbcopy"], input=text.encode())
    # Send Cmd+V from this same process via pynput — it already holds the event tap.
    # osascript failed with 1002: a separate binary doesn't inherit the Accessibility TCC grant.
    # Press by vk 9 (physical V), not the char "v": on a non-Latin layout (e.g. Russian) pynput
    # can't find a keycode for Latin v and falls back to vk 0 = the A key -> Cmd+A (select all).
    with _kb.pressed(keyboard.Key.cmd):
        _kb.press(_V)
        _kb.release(_V)
    time.sleep(0.2)  # let the paste land before restoring the clipboard
    subprocess.run(["pbcopy"], input=old)


def on_press(key):
    if not is_right_cmd(key) or state["rec"] or state["busy"]:
        return  # key-repeat while held / busy — ignore
    with _buf_lock:
        _buf.clear()
    state["rec"] = True  # device already warm — reader appends from the next chunk, no cold start
    beep(START_SOUND)    # the warm mic may catch a faint tick at the very start (ceiling) — harmless
    print("● recording...")


def on_release(key):
    if not is_right_cmd(key) or not state["rec"]:
        return
    state["rec"] = False
    beep(STOP_SOUND)  # after rec=False, so it's never captured into the recording
    state["busy"] = True
    print("■ stop, transcribing...")
    try:
        with _buf_lock:
            pcm = bytes(_buf)  # snapshot; an in-flight chunk may drop ~43ms of the tail (ceiling)
        if len(pcm) > 4000:  # ~40ms+ of audio; shorter = accidental tap
            encode(pcm)
            text = transcribe(OUT)
            if text.strip():
                paste_text(text)
                print(f"→ {text!r}")
            else:
                print("(empty)")
        else:
            print("(too short)")
    except Exception as e:
        print("error:", e)
    finally:
        state["busy"] = False


if __name__ == "__main__":
    ensure_accessibility()
    warm_start()  # open the mic now so the very first press records instantly
    print(f"Push-to-talk ready. Mic [{MIC}]. Hold Right Command to record. Ctrl-C to quit.")
    try:
        with keyboard.Listener(on_press=on_press, on_release=on_release) as l:
            l.join()
    except KeyboardInterrupt:
        print("\nbye")
    finally:
        if _warm["proc"] and _warm["proc"].poll() is None:
            _warm["proc"].terminate()  # release the mic on exit
