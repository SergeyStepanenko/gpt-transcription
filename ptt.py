#!/usr/bin/env python3
"""Push-to-talk диктовка. Держи Right Option — пишется микрофон; отпустил — транскрипция,
текст вставляется в активное поле (Cmd+V) с сохранением и восстановлением буфера обмена.
Ctrl-C в терминале — выход. Микрофон: env MIC (дефолт 1)."""
import os, signal, subprocess, time, pathlib
from pynput import keyboard
from transcribe import transcribe

HERE = pathlib.Path(__file__).parent
OUT = HERE / "_ptt.webm"
MIC = os.environ.get("MIC", "1")

state = {"rec": False, "busy": False, "proc": None}
_kb = keyboard.Controller()


def ensure_accessibility():
    """Cmd+V требует право Accessibility (Универсальный доступ) — отдельное от Input Monitoring,
    на котором работает запись. Без него вставка тихо не сработает. Проверяем и поднимаем
    системный диалог добавления приложения в список."""
    try:
        from ApplicationServices import (AXIsProcessTrustedWithOptions,
                                          kAXTrustedCheckOptionPrompt)
        trusted = AXIsProcessTrustedWithOptions({kAXTrustedCheckOptionPrompt: True})
    except Exception:
        return  # не смогли проверить — не мешаем работе
    if not trusted:
        print("⚠️  Нет права Accessibility — вставка (Cmd+V) не сработает (запись будет идти).")
        print("    System Settings → Privacy & Security → Accessibility: включи приложение,")
        print("    из которого запускаешь dictate.sh (диалог уже открыт). Потом перезапусти скрипт.")


def is_right_option(key):
    # Right Option: pynput Key.alt_r или keycode vk 61 (Left Option = 58)
    return key == keyboard.Key.alt_r or getattr(key, "vk", None) == 61


def start_record():
    return subprocess.Popen(
        ["ffmpeg", "-hide_banner", "-loglevel", "error",
         "-f", "avfoundation", "-i", f":{MIC}",
         "-c:a", "libopus", "-ac", "1", "-ar", "48000", "-y", str(OUT)],
        stdin=subprocess.DEVNULL)


def stop_record(proc):
    # SIGINT, не kill: ffmpeg дописывает EBML-трейлер, файл валиден
    proc.send_signal(signal.SIGINT)
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()


def paste_text(text):
    # pbpaste/pbcopy — только текст; картинка/файлы в буфере не сохранятся (ceiling)
    old = subprocess.run(["pbpaste"], capture_output=True).stdout
    subprocess.run(["pbcopy"], input=text.encode())
    # Cmd+V шлём из этого же процесса через pynput — у него уже есть event-tap.
    # osascript падал с 1002: отдельный бинарь не наследует TCC-грант Accessibility.
    with _kb.pressed(keyboard.Key.cmd):
        _kb.press("v")
        _kb.release("v")
    time.sleep(0.2)  # дать вставке проскочить до восстановления
    subprocess.run(["pbcopy"], input=old)


def on_press(key):
    if not is_right_option(key) or state["rec"] or state["busy"]:
        return  # повтор зажатой клавиши / занято — игнор
    state["rec"] = True
    state["proc"] = start_record()
    print("● запись...")


def on_release(key):
    if not is_right_option(key) or not state["rec"]:
        return
    state["rec"] = False
    state["busy"] = True
    print("■ стоп, транскрибирую...")
    try:
        stop_record(state["proc"])
        if OUT.exists() and OUT.stat().st_size > 2000:
            text = transcribe(OUT)
            if text.strip():
                paste_text(text)
                print(f"→ {text!r}")
            else:
                print("(пусто)")
        else:
            print("(слишком коротко)")
    except Exception as e:
        print("ошибка:", e)
    finally:
        state["busy"] = False


if __name__ == "__main__":
    ensure_accessibility()
    print(f"Push-to-talk готов. Микрофон [{MIC}]. Держи Right Option для записи. Ctrl-C — выход.")
    with keyboard.Listener(on_press=on_press, on_release=on_release) as l:
        l.join()
