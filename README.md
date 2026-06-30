# gpt-transcription

```
                                                  ╭── chatgpt.com/backend-api/transcribe
  ╭───╮                                           │   (multipart/form-data, WebM/Opus)
  │o o│ ))) speaks ── mic ── [1a 45 df a3 ...] ───┤
  │ ᴗ │                      (WebM binary)        │
  ╰─┬─╯                                           ╰── {"text":"Hello, world"} ─╮
  ┌─┴─┐                                                                        │
  │ ⌘ │ hold Right Cmd                                                         ▼
  └───┘                                                                ╭──────────────╮
                                                                       │              │
    release ── encode ── POST ── transcribe ── paste (Cmd+V) ─────────>│ Hello, world │
                                                                       ╰──────────────╯
```

Push-to-talk dictation for macOS that turns your voice into text **anywhere** — hold a key,
speak, release, and the transcription is pasted straight into whatever field your cursor is in.

The clever (or cheeky) part: there's no local speech model and no API bill. The actual
speech-to-text is done by **ChatGPT's own voice-input backend** (`/backend-api/transcribe`),
replayed with your own logged-in session. This repo is just the thin local glue around it —
a single Rust binary.

> **Disclaimer.** This calls a *private, undocumented* ChatGPT endpoint using **your own**
> browser session cookies. It is not an official API, not affiliated with or endorsed by OpenAI,
> and may break the moment they change anything. It's a personal-use hack. If you need something
> dependable, use the official [`/v1/audio/transcriptions`](https://platform.openai.com/docs/guides/speech-to-text)
> API instead. Use at your own risk and within OpenAI's terms.

## How it works

```
startup: choose action → choose mic → choose warm/cold → preload audio cues
       │
       ├─ warm: cpal holds the mic open, collecting f32 PCM ──► buffer
       └─ cold: mic off between presses (privacy, but ~1s lag)
       │
hold Right Command ─► flag flips (warm) / cpal stream spawns (cold) ─► recording
       │
   release ─► ffmpeg encodes buffered PCM ──► WebM/Opus (mono, 48 kHz)
       │
       ▼
 POST to chatgpt.com/backend-api/transcribe   ← with cookies + Chrome TLS fingerprint
       │
       ▼
   {"text": "..."}  ──►  clipboard swap  ──►  Cmd+V into the focused field  ──►  clipboard restored
```

- **Native mic capture via `cpal`.** No ffmpeg for recording — the mic is read directly through
  Core Audio. Warm mode holds the stream open; cold mode creates/drops it per press.
- **Same audio format as the browser.** `ffmpeg` encodes the captured f32 PCM into mono/48 kHz
  Opus in WebM — the format Chrome's MediaRecorder sends.
- **Cloudflare bypass via TLS impersonation.** Uses `wreq` (Rust HTTP client with BoringSSL)
  with `Chrome136` emulation to match Chrome's TLS/HTTP2 fingerprint.
- **Clipboard-preserving paste.** Transcript → clipboard → `Cmd+V` → restore previous content.
- **Layout-independent paste.** `Cmd+V` is sent by physical keycode (`kVK_ANSI_V` = 9),
  works on any keyboard layout (Russian, etc.).
- **Token validation.** JWT expiry is checked locally before each request — stale tokens
  fail fast with a clear message instead of a cryptic 401.

See [`AGENT.md`](AGENT.md) for deeper notes (EBML byte layout, why `cf_clearance` is IP-bound, etc.).

## Requirements

- **macOS** (uses Core Audio via `cpal`, `CGEventTap`, `AudioToolbox`, and the Accessibility API).
- **ffmpeg** — `brew install ffmpeg` (used for encoding PCM → WebM/Opus).
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

The binary parses Chrome DevTools cURL directly — no external scripts needed.

On first run (or when choosing **Replace credentials**), paste the cURL and press `Ctrl-D`:

1. Open ChatGPT in your browser while logged in.
2. DevTools → Network → any request whose URL starts with `https://chatgpt.com/backend-api`.
3. Right-click → Copy → Copy as cURL.
4. Paste into the prompt when `ptt` asks for it.

The binary extracts three values automatically:

| Key          | Where it comes from                                          |
|--------------|--------------------------------------------------------------|
| `TOKEN`      | the `authorization: Bearer <…>` header                       |
| `ACCOUNT_ID` | the `chatgpt-account-id: <…>` header (or JWT claim fallback) |
| `COOKIES`    | the whole `cookie:` string (or `-b`/`--cookie` value)        |

They are saved to `creds.env` (next to the binary, or CWD). The file is git-ignored.

You can also create `creds.env` manually with the same three keys.

These expire within days — if you start getting `401`/`403`, restart `ptt` and choose
**Replace credentials from Chrome DevTools cURL**.

### macOS permissions (one-time)

Grant these to **the terminal app you launch `ptt` from** (Terminal, iTerm, VS Code, …):

- **Microphone** — for `cpal` to capture audio.
- **Input Monitoring** — for the global Right Command hotkey (CGEventTap).
- **Accessibility** — to synthesize `Cmd+V`. On first run a system dialog pops up; enable the app,
  then **restart** (macOS only re-checks the permission at process start).

## Usage

```bash
./target/release/ptt
```

On startup you'll see a menu:

1. **Start push-to-talk** (or **Add credentials** if no `creds.env` found).
2. **Replace credentials from Chrome DevTools cURL**.

Then:

1. **Select mic** — arrow keys to choose, Enter to confirm. Override with `MIC=2 ./target/release/ptt`.
2. **Keep mic always on?** — "Yes" (warm, instant start, mic indicator always on) or
   "No" (cold, mic only during recording, ~1s lag on each press).

Hold **Right Command**, speak, release. The text is transcribed and pasted where your cursor is.
`Ctrl-C` quits.

### Environment variables

| Variable          | Effect                                                |
|-------------------|-------------------------------------------------------|
| `MIC=<index>`     | Skip mic selection, use device at given index         |
| `PTT_DEBUG_AUDIO` | Write a `.raw.wav` alongside the encoded WebM for debugging |

## License

Personal-use project, provided as-is. No warranty.
