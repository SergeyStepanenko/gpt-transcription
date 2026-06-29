# gpt-transcription

Push-to-talk dictation for macOS that turns your voice into text **anywhere** — hold a key,
speak, release, and the transcription is pasted straight into whatever field your cursor is in.

The clever (or cheeky) part: there's no local speech model and no API bill. The actual
speech-to-text is done by **ChatGPT's own voice-input backend** (`/backend-api/transcribe`),
replayed with your own logged-in session. This repo is just the thin local glue around it —
built fast, and deliberately small, because the heavy lifting (the ASR) is ChatGPT's compute,
not ours.

> ⚠️ **Disclaimer.** This calls a *private, undocumented* ChatGPT endpoint using **your own**
> browser session cookies. It is not an official API, not affiliated with or endorsed by OpenAI,
> and may break the moment they change anything. It's a personal-use hack. If you need something
> dependable, use the official [`/v1/audio/transcriptions`](https://platform.openai.com/docs/guides/speech-to-text)
> API instead. Use at your own risk and within OpenAI's terms.

## How it works

```
hold Right Option ──► ffmpeg records mic ──► WebM/Opus (mono, 48 kHz)
       │                                            │
   release ────────────────────────────────────────┘
       │
       ▼
 POST to chatgpt.com/backend-api/transcribe   ← with your cookies + a Chrome TLS fingerprint
       │
       ▼
   {"text": "..."}  ──►  clipboard swap  ──►  Cmd+V into the focused field  ──►  clipboard restored
```

A few things that make it actually work:

- **Same audio format as the browser.** `ffmpeg` is told to produce mono/48 kHz Opus in WebM —
  byte-for-byte what Chrome's MediaRecorder sends, so the endpoint accepts it.
- **Cloudflare bypass via TLS impersonation.** A plain `curl` gets a `403` because Cloudflare
  fingerprints the TLS handshake (JA3/JA4). We use [`curl_cffi`](https://github.com/lexiforest/curl_cffi),
  a libcurl fork that copies Chrome's ClientHello + HTTP/2 profile, so the request looks like Chrome.
- **Clipboard-preserving paste.** The transcript is put on the clipboard, pasted with `Cmd+V`, then
  whatever you had on the clipboard before is restored.
- **Layout-independent paste.** `Cmd+V` is sent by the physical key's keycode (`kVK_ANSI_V`), so it
  works even when a non-Latin keyboard layout is active.

See [`AGENT.md`](AGENT.md) for the deeper notes (EBML byte layout, why `cf_clearance` is IP-bound, etc.).

## Requirements

- **macOS** (uses `avfoundation`, `pbcopy`/`pbpaste`, and the Accessibility API).
- **ffmpeg** — `brew install ffmpeg`
- **Python 3.10+**
- A **logged-in ChatGPT account** (to copy your session token + cookies).

## Setup

```bash
git clone <your-repo-url> gpt-transcription
cd gpt-transcription

python3 -m venv .venv
.venv/bin/pip install -r requirements.txt

cp creds.env.example creds.env   # then edit creds.env (see below)
chmod +x dictate.sh record.sh transcribe.sh
```

### Credentials (`creds.env`)

Open ChatGPT in your browser while logged in, then **DevTools → Network → any request to
`chatgpt.com` → Copy → Copy as cURL**, and pull out three values:

| Key          | Where it comes from                                          |
|--------------|--------------------------------------------------------------|
| `TOKEN`      | the `authorization: Bearer <…>` header                       |
| `ACCOUNT_ID` | the `chatgpt-account-id: <…>` header                         |
| `COOKIES`    | the whole `cookie:` string (or everything after `-b '…'`)    |

These expire within days — if you start getting `401`/`403`, copy fresh ones.
`creds.env` is git-ignored, so your secrets never get committed.

### macOS permissions (one-time)

Grant these to **the terminal app you launch the script from** (Terminal, iTerm, VS Code, …):

- **Microphone** — for `ffmpeg` to record.
- **Input Monitoring** — for the global Right Option hotkey.
- **Accessibility** — to synthesize `Cmd+V`. On first run a system dialog pops up; enable the app,
  then **restart the script** (macOS only re-checks the permission at process start).

## Usage

**Push-to-talk dictation** (the main use):

```bash
./dictate.sh
```

Hold **Right Option**, speak, release. The text is transcribed and pasted where your cursor is.
`Ctrl-C` to quit. Pick a different mic with `MIC=2 ./dictate.sh`
(list devices: `ffmpeg -f avfoundation -list_devices true -i ""`).

**One-shot helpers:**

```bash
./record.sh 5            # record 5 seconds into whisper.webm
./transcribe.sh          # transcribe whisper.webm, print the text
./transcribe.sh foo.webm # transcribe a specific file
```

## Resources it uses

- **ChatGPT private transcribe endpoint** — does the actual speech-to-text (your account, your quota).
- **ffmpeg** — mic capture + Opus/WebM encoding.
- **curl_cffi** — HTTP with a Chrome TLS fingerprint (Cloudflare bypass).
- **pynput** — global hotkey listener + the `Cmd+V` keystroke.
- **pyobjc (ApplicationServices)** — checks/prompts the macOS Accessibility permission.
- **macOS clipboard** (`pbcopy`/`pbpaste`) — for the paste-and-restore trick.

## License

Personal-use project, provided as-is. No warranty.
