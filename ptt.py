#!/usr/bin/env python3
"""Push-to-talk dictation. Hold Right Command to record the mic; release to transcribe
and paste the text into the focused field (Cmd+V), saving and restoring the clipboard.
Ctrl-C in the terminal quits. Mic: env MIC overrides; default = built-in MacBook mic (auto-detected)."""
import os, sys, subprocess, threading, time, pathlib
from pynput import keyboard
from AppKit import NSSound
from transcribe import transcribe

HERE = pathlib.Path(__file__).parent
OUT = HERE / "_ptt.webm"


def list_audio_devices():
    """[(index, name)] for every avfoundation *audio* input. Indices restart per section,
    so only scan lines after 'audio devices:'. Device list goes to ffmpeg's stderr."""
    import re
    listing = subprocess.run(
        ["ffmpeg", "-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""],
        capture_output=True, text=True).stderr
    out, in_audio = [], False
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
        if m:
            out.append((m.group(1), m.group(2)))
    return out


def _arrow_select(labels, default_i):
    """Arrow-key picker over `labels`, starting on default_i. ↑/↓ move, Enter confirms,
    Ctrl-C aborts. Raw terminal mode (termios) so single keystrokes register without Enter.
    Returns the chosen index. Caller guarantees stdin is a tty."""
    import termios, tty
    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    sel = default_i

    def render(first):
        if not first:
            sys.stdout.write(f"\033[{len(labels)}A")  # cursor up to redraw in place
        for i, lab in enumerate(labels):
            row = ("\033[7m❯ " + lab + "\033[0m") if i == sel else "  " + lab
            sys.stdout.write("\033[2K" + row + "\r\n")  # \033[2K clears the line
        sys.stdout.flush()

    try:
        tty.setraw(fd)
        render(True)
        while True:
            ch = sys.stdin.read(1)
            if ch == "\x1b" and sys.stdin.read(1) == "[":
                arrow = sys.stdin.read(1)
                sel = (sel - 1) % len(labels) if arrow == "A" else \
                      (sel + 1) % len(labels) if arrow == "B" else sel
            elif ch in ("\r", "\n"):
                return sel
            elif ch == "\x03":
                raise KeyboardInterrupt
            render(False)
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)


def choose_mic(devices, want="MacBook"):
    """Arrow-key pick of the audio input. Default highlight = built-in MacBook mic (else first).
    MIC env overrides and skips the menu; non-tty launch silently takes the default."""
    env = os.environ.get("MIC")
    if env:
        return env
    if not devices:
        return "1"  # nothing parsed — fall back to the usual built-in index
    default_i = next((k for k, (_, n) in enumerate(devices) if want.lower() in n.lower()), 0)
    if not sys.stdin.isatty():
        return devices[default_i][0]
    print("Select mic  (↑/↓ to move, Enter to confirm):")
    labels = [f"[{i}] {n}" for i, n in devices]
    sel = _arrow_select(labels, default_i)
    return devices[sel][0]


MIC = None  # set interactively in __main__ before warm_start()

state = {"rec": False, "busy": False}
_kb = keyboard.Controller()
_V = keyboard.KeyCode.from_vk(9)  # kVK_ANSI_V — physical V key, layout-independent

# Audio cues — preloaded NSSound objects. Decoding happens once at import, so play() is
# near-instant: no per-press `afplay` process spawn (~0.3s) and no cold CoreAudio start.
# prime_sounds() at startup wakes the output device so the *first* press is instant too.
_snd_start = NSSound.alloc().initWithContentsOfFile_byReference_("/System/Library/Sounds/Morse.aiff", True)   # press
_snd_stop = NSSound.alloc().initWithContentsOfFile_byReference_("/System/Library/Sounds/Bottle.aiff", True)   # release

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


def beep(snd):
    # Preloaded NSSound: stop() rewinds if a prior cue is still playing, play() returns immediately.
    snd.stop()
    snd.play()


def prime_sounds():
    # Play both cues silently once so CoreAudio's output device is awake before the first real press
    # (a cold device adds ~0.2-0.3s to the very first play). volume 0 = inaudible; restore after.
    for s in (_snd_start, _snd_stop):
        s.setVolume_(0.0)
        s.play()
    time.sleep(0.3)
    for s in (_snd_start, _snd_stop):
        s.stop()
        s.setVolume_(1.0)


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
    beep(_snd_start)     # the warm mic may catch a faint tick at the very start (ceiling) — harmless
    print("● recording...")


def on_release(key):
    if not is_right_cmd(key) or not state["rec"]:
        return
    state["rec"] = False
    beep(_snd_stop)  # after rec=False, so it's never captured into the recording
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
    MIC = choose_mic(list_audio_devices())  # ask which mic before opening it
    warm_start()     # open the chosen mic now so the very first press records instantly
    prime_sounds()   # wake CoreAudio so the first cue plays instantly too
    print(f"Push-to-talk ready. Mic [{MIC}]. Hold Right Command to record. Ctrl-C to quit.")
    try:
        with keyboard.Listener(on_press=on_press, on_release=on_release) as l:
            l.join()
    except KeyboardInterrupt:
        print("\nbye")
    finally:
        if _warm["proc"] and _warm["proc"].poll() is None:
            _warm["proc"].terminate()  # release the mic on exit
