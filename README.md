# gpt-transcription

Push-to-talk dictation for macOS that turns your voice into text **anywhere** — hold a key,
speak, release, and the transcription is pasted straight into whatever field your cursor is in.

The clever (or cheeky) part: there's no local speech model and no API bill. The actual
speech-to-text is done by **ChatGPT's own voice-input backend** (`/backend-api/transcribe`),
replayed with your own logged-in session. This repo is just the thin local glue around it —
a single Rust binary + `ffmpeg`.

> **Disclaimer.** This calls a *private, undocumented* ChatGPT endpoint using **your own**
> browser session cookies. It is not an official API, not affiliated with or endorsed by OpenAI,
> and may break the moment they change anything. It's a personal-use hack. If you need something
> dependable, use the official [`/v1/audio/transcriptions`](https://platform.openai.com/docs/guides/speech-to-text)
> API instead. Use at your own risk and within OpenAI's terms.

## How it works

```
startup: choose mic → choose warm/cold → preload audio cues
       │
       ├─ warm: ffmpeg holds the mic open, streaming raw PCM ──► reader thread
       └─ cold: mic off between presses (privacy, but ~1s lag)
       │
hold Right Command ─► flag flips (warm) / ffmpeg spawns (cold) ─► recording
       │
   release ─► encode buffered PCM ──► WebM/Opus (mono, 48 kHz)
       │
       ▼
 POST to chatgpt.com/backend-api/transcribe   ← with cookies + Chrome TLS fingerprint
       │
       ▼
   {"text": "..."}  ──►  clipboard swap  ──►  Cmd+V into the focused field  ──►  clipboard restored
```

- **Same audio format as the browser.** `ffmpeg` produces mono/48 kHz Opus in WebM —
  byte-for-byte what Chrome's MediaRecorder sends.
- **Cloudflare bypass via TLS impersonation.** Uses `wreq` (Rust HTTP client with BoringSSL)
  to match Chrome's TLS/HTTP2 fingerprint.
- **Clipboard-preserving paste.** Transcript → clipboard → `Cmd+V` → restore previous content.
- **Layout-independent paste.** `Cmd+V` is sent by physical keycode (`kVK_ANSI_V` = 9),
  works on any keyboard layout (Russian, etc.).

See [`AGENT.md`](AGENT.md) for deeper notes (EBML byte layout, why `cf_clearance` is IP-bound, etc.).

## Requirements

- **macOS** (uses `avfoundation`, `CGEventTap`, `AudioToolbox`, and the Accessibility API).
- **ffmpeg** — `brew install ffmpeg`
- **Rust toolchain** — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **cmake + go** — `brew install cmake go` (needed to build BoringSSL during `cargo build`)
- A **logged-in ChatGPT account** (to copy your session token + cookies).

## Build

```bash
git clone <your-repo-url> gpt-transcription
cd gpt-transcription
cargo build --release
```

The binary is at `./target/release/ptt`.

### Credentials (`creds.env`)

Open ChatGPT in your browser while logged in, then **DevTools → Network → any request to
`chatgpt.com` → Copy → Copy as cURL**, and pull out three values:

| Key          | Where it comes from                                          |
|--------------|--------------------------------------------------------------|
| `TOKEN`      | the `authorization: Bearer <…>` header                       |
| `ACCOUNT_ID` | the `chatgpt-account-id: <…>` header                         |
| `COOKIES`    | the whole `cookie:` string (or everything after `-b '…'`)    |

```bash
cp creds.env.example creds.env   # then edit
pbpaste | ./extract_creds.sh > creds.env   # or generate it from "Copy as cURL"
```

`extract_creds.sh` is a small Bash helper around the `curl_to_creds_env` function. It accepts
a copied cURL command and prints a ready-to-save `creds.env` body:

```bash
TOKEN='...'
ACCOUNT_ID='...'
COOKIES='...'
```

Typical flow:

```bash
# 1. In Chrome DevTools: Network -> request to chatgpt.com -> Copy -> Copy as cURL
# 2. Then write the generated env file from your clipboard:
pbpaste | ./extract_creds.sh > creds.env
```

You can also call the function from another Bash script:

```bash
source ./extract_creds.sh
curl_to_creds_env "$copied_curl" > creds.env
```

The helper reads from stdin when called without arguments, or from its arguments when provided.
It extracts `TOKEN` from `authorization: Bearer ...`, `COOKIES` from `-b` / `--cookie` / `cookie:`,
and `ACCOUNT_ID` from `chatgpt-account-id` when present. If that header is missing, it falls back
to the `chatgpt_account_id` claim inside the JWT token.

These expire within days — if you start getting `401`/`403`, copy fresh ones.
`creds.env` is git-ignored, so your secrets never get committed.

### macOS permissions (one-time)

Grant these to **the terminal app you launch `ptt` from** (Terminal, iTerm, VS Code, …):

- **Microphone** — for `ffmpeg` to record.
- **Input Monitoring** — for the global Right Command hotkey (CGEventTap).
- **Accessibility** — to synthesize `Cmd+V`. On first run a system dialog pops up; enable the app,
  then **restart** (macOS only re-checks the permission at process start).

## Usage

```bash
./target/release/ptt
```

On startup you'll be asked two questions:
1. **Select mic** — arrow keys to choose, Enter to confirm. Override with `MIC=2 ./target/release/ptt`.
2. **Keep mic always on?** — "Yes" (warm, instant start, mic indicator always on) or
   "No" (cold, mic only during recording, ~1s lag on each press).

Then hold **Right Command**, speak, release. The text is transcribed and pasted where your cursor is.
`Ctrl-C` quits.

## Legacy

The original Python implementation is preserved in `legacy/` for reference.

## License

Personal-use project, provided as-is. No warranty.
