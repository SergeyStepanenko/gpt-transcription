# gpt-transcription — notes / insights

Replays ChatGPT's private voice-input endpoint (`/backend-api/transcribe`):
record mic → WebM/Opus → POST with your own cookies → `{"text": "..."}`.

## Audio format (as read from the bytes)

- `\x1a\x45\xdf\xa3` at the start — **EBML magic** (`0x1A45DFA3`), the Matroska/WebM signature.
- `B\x82\x84webm` — DocType = `webm`. Nearby `EncoderWA...Chrome` — what wrote it.
- `A_OPUS` + `OpusHead\x01\x01...` — the **Opus** codec: `\x01` channels = mono, sample rate **48 kHz**.
- `\xa3` blocks (**SimpleBlock**) — compressed Opus frames, the actual audio.
- WebM is a subset of Matroska. EBML tree (binary XML): `ID + length + data`.
  That's why `OpusHead`/`webm` read as ASCII right in the stream, but the audio between them does not.
- `ffmpeg -f avfoundation -i ":1" -c:a libopus -ac 1 -ar 48000 out.webm` produces the **byte-for-byte**
  same format as Chrome's MediaRecorder (mono/48k Opus — Chrome's default for voice).

## You can't read the speech out of the raw bytes

These are compressed Opus frames (a psychoacoustic codec), not text. You have to actually decode:
`ffmpeg webm → PCM`, then ASR (Whisper). You can't "read" words out of the bytes.

## Auth: Bearer ≠ a private key

- `authorization: Bearer eyJ...` is a **bearer** token. Whoever holds the string is "you" to the server.
  Like an apartment key: the lock doesn't check whose it is, only that it fits.
- The JWT inside is signed with OpenAI's **server-side** private key — you can't forge or mint your own.
  You don't need to: the token is already issued and sits in the request whole.
- You get your own token from a logged-in ChatGPT (DevTools → Network → Copy as cURL).
  You can't fabricate someone else's.

## Cloudflare: bare curl won't get through (the main gotcha)

- A 403 with `cf-mitigated: challenge` — Cloudflare bot-management fingerprints the **TLS handshake**
  (JA3/JA4) and the HTTP/2 frame order. Chrome has one fingerprint, the system curl
  (LibreSSL/SecureTransport) a completely different one.
- `cf_clearance` is bound to the fingerprint of the client that solved the challenge, **and to the IP**.
  Send it from curl → wrong fingerprint → another challenge → 403. Headers and cookies don't help: it's
  the handshake bytes that give you away.
- Fixed by faking the fingerprint: **`curl_cffi`** (a libcurl fork that copies Chrome's ClientHello +
  HTTP/2 profile). Same cookies, same request — the only difference is the TLS → 200.
- `curl_cffi` 0.15 changed the API: multipart goes through **`CurlMime`**, not `files=` (`NotImplementedError`).

## Operation / expiry

- Cookies and token live for **days**. Non-200 → refresh `creds.env` from a fresh DevTools copy.
- `cf_clearance` is **IP-bound**: change network/VPN → another challenge, need a fresh one from the same IP.
- The endpoint is private, OpenAI promises no stability — the path/headers/protection can change.
  Need reliability → the official `/v1/audio/transcriptions` (an `sk-` key, no Cloudflare wall).
- Credentials live in `creds.env` (in `.gitignore`), not in code — otherwise they leak into git history.

## Environment

- macOS Python is externally-managed (PEP 668) → install into a **venv** (`.venv/`), not `--break-system-packages`.
- Mics: `ffmpeg -f avfoundation -list_devices true -i ""`. Default `[1] MacBook Pro Microphone`.

## Push-to-talk (dictation on Right Option)

- The global hold-hotkey is `pynput` (keyDown/keyUp). Right Option = `Key.alt_r` / vk `61`
  (Left Option = vk `58`). Holding sends repeated `on_press` — start recording on a flag, not every time.
- Stopping ffmpeg uses **SIGINT**, not SIGKILL: ffmpeg catches SIGINT and finalizes the EBML trailer,
  so the file is valid. KILL → a truncated/unfinalized webm.
- Paste with clipboard save/restore: `pbpaste` (save) → `pbcopy` text → **`Cmd+V` via the pynput
  Controller** (same process that listens to keys) → pause → `pbcopy` back. `pbpaste`/`pbcopy` are
  **text only**: images/files on the clipboard won't be preserved (ceiling; if needed — AppleScript with clipboard types).
- **Why not osascript:** `osascript ... keystroke "v"` failed with `1002 not allowed to send keystrokes` —
  a separate binary doesn't inherit the responsible process's Accessibility TCC grant. We send `Cmd+V` from
  the python process itself (it already holds the event tap) → the grant is needed for one app only (the terminal).
- **Why press by keycode, not the char:** `Controller.press("v")` maps the *character* to a keycode using
  the *current* layout. On a non-Latin layout (e.g. Russian ЙЦУКЕН) there's no key for Latin v, so pynput
  falls back to vk 0 = the **A** key → `Cmd+A` (select all) instead of paste. We press vk 9 (`kVK_ANSI_V`,
  the physical V key), which is layout-independent.
- macOS permissions — **two distinct** privileges: **Input Monitoring** (pynput listens for Right Option,
  recording) and **Accessibility** (synthesizing `Cmd+V`). Recording can work while paste stays silent —
  that means only Input Monitoring was granted. `ensure_accessibility()` raises the system dialog on start
  (`AXIsProcessTrustedWithOptions` + prompt) and adds the responsible app to the Accessibility list.
- Plus **Microphone** for ffmpeg.
