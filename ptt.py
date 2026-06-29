#!/usr/bin/env python3
"""Push-to-talk dictation. Hold Right Option to record the mic; release to transcribe
and paste the text into the focused field (Cmd+V), saving and restoring the clipboard.
Ctrl-C in the terminal quits. Mic: env MIC (default 1)."""
import os, signal, subprocess, time, pathlib
from pynput import keyboard
from transcribe import transcribe

HERE = pathlib.Path(__file__).parent
OUT = HERE / "_ptt.webm"
MIC = os.environ.get("MIC", "1")

state = {"rec": False, "busy": False, "proc": None}
_kb = keyboard.Controller()
_V = keyboard.KeyCode.from_vk(9)  # kVK_ANSI_V — physical V key, layout-independent


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


def is_right_option(key):
    # Right Option: pynput Key.alt_r or keycode vk 61 (Left Option = 58)
    return key == keyboard.Key.alt_r or getattr(key, "vk", None) == 61


def start_record():
    return subprocess.Popen(
        ["ffmpeg", "-hide_banner", "-loglevel", "error",
         "-f", "avfoundation", "-i", f":{MIC}",
         "-c:a", "libopus", "-ac", "1", "-ar", "48000", "-y", str(OUT)],
        stdin=subprocess.DEVNULL)


def stop_record(proc):
    # SIGINT, not kill: ffmpeg finalizes the EBML trailer, so the file stays valid
    proc.send_signal(signal.SIGINT)
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()


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
    if not is_right_option(key) or state["rec"] or state["busy"]:
        return  # key-repeat while held / busy — ignore
    state["rec"] = True
    state["proc"] = start_record()
    print("● recording...")


def on_release(key):
    if not is_right_option(key) or not state["rec"]:
        return
    state["rec"] = False
    state["busy"] = True
    print("■ stop, transcribing...")
    try:
        stop_record(state["proc"])
        if OUT.exists() and OUT.stat().st_size > 2000:
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
    print(f"Push-to-talk ready. Mic [{MIC}]. Hold Right Option to record. Ctrl-C to quit.")
    try:
        with keyboard.Listener(on_press=on_press, on_release=on_release) as l:
            l.join()
    except KeyboardInterrupt:
        if state["proc"] and state["proc"].poll() is None:
            state["proc"].kill()  # don't leave a stray ffmpeg if Ctrl-C was hit mid-recording
        print("\nbye")
