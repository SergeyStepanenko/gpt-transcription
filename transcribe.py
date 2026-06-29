#!/usr/bin/env python3
"""Отправка аудио в ChatGPT /backend-api/transcribe с TLS-отпечатком Chrome (обход Cloudflare).
Креды из creds.env. CLI: python transcribe.py [файл]  (дефолт whisper.webm).
Как модуль: from transcribe import transcribe; transcribe(Path('x.webm')) -> str."""
import sys, pathlib
from curl_cffi import requests, CurlMime

HERE = pathlib.Path(__file__).parent


def load_creds():
    env = {}
    for line in (HERE / "creds.env").read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        env[k.strip()] = v.strip().strip("'\"")
    for k in ("TOKEN", "ACCOUNT_ID", "COOKIES"):
        if not env.get(k):
            raise SystemExit(f"{k} не задан в creds.env")
    return env


def transcribe(audio: pathlib.Path) -> str:
    """POST аудио, вернуть распознанный текст. Бросает на не-200."""
    env = load_creds()
    mp = CurlMime()
    mp.addpart(name="file", filename="whisper.webm",
               content_type="audio/webm;codecs=opus", data=audio.read_bytes())
    try:
        r = requests.post(
            "https://chatgpt.com/backend-api/transcribe",
            headers={
                "authorization": f"Bearer {env['TOKEN']}",
                "chatgpt-account-id": env["ACCOUNT_ID"],
                "cookie": env["COOKIES"],
                "origin": "https://chatgpt.com",
                "referer": "https://chatgpt.com/",
            },
            multipart=mp,
            impersonate="chrome",  # TLS/HTTP2-отпечаток Chrome — иначе Cloudflare 403
            timeout=30,  # дефолт curl_cffi = None: без таймаута ptt застрянет в busy навсегда
        )
    finally:
        mp.close()  # освободить libcurl mime-хендл — transcribe зовётся на каждую диктовку
    if r.status_code != 200:
        raise RuntimeError(f"HTTP {r.status_code}: {r.text[:300]}")
    return r.json().get("text", "")


if __name__ == "__main__":
    name = sys.argv[1] if len(sys.argv) > 1 else "whisper.webm"
    audio = HERE / name
    if not audio.exists():
        sys.exit(f"Нет файла: {audio} (сначала ./record.sh)")
    print(transcribe(audio))
