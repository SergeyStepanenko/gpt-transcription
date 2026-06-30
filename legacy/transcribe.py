#!/usr/bin/env python3
"""Send audio to ChatGPT /backend-api/transcribe with a Chrome TLS fingerprint (Cloudflare bypass).
Credentials come from creds.env. CLI: python transcribe.py [file]  (default whisper.webm).
As a module: from transcribe import transcribe; transcribe(Path('x.webm')) -> str."""
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
            raise SystemExit(f"{k} is not set in creds.env")
    return env


def transcribe(audio: pathlib.Path) -> str:
    """POST the audio, return the recognized text. Raises on non-200."""
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
            impersonate="chrome",  # Chrome TLS/HTTP2 fingerprint — otherwise Cloudflare 403
            timeout=30,  # curl_cffi default is None: without a timeout ptt hangs in busy forever
        )
    finally:
        mp.close()  # free the libcurl mime handle — transcribe is called on every dictation
    if r.status_code != 200:
        raise RuntimeError(f"HTTP {r.status_code}: {r.text[:300]}")
    return r.json().get("text", "")


if __name__ == "__main__":
    name = sys.argv[1] if len(sys.argv) > 1 else "whisper.webm"
    audio = HERE / name
    if not audio.exists():
        sys.exit(f"No such file: {audio} (run ./record.sh first)")
    print(transcribe(audio))
