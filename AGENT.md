# gpt-transcription — заметки/инсайты

Реплей приватного эндпоинта голосового ввода ChatGPT (`/backend-api/transcribe`):
запись микрофона → WebM/Opus → POST со своими куками → `{"text": "..."}`.

## Формат аудио (как читается из байтов)

- `\x1a\x45\xdf\xa3` в начале — **EBML magic** (`0x1A45DFA3`), сигнатура Matroska/WebM.
- `B\x82\x84webm` — DocType = `webm`. Рядом `EncoderWA...Chrome` — кем писалось.
- `A_OPUS` + `OpusHead\x01\x01...` — кодек **Opus**: `\x01` каналов = mono, sample rate **48 kHz**.
- Блоки `\xa3` (**SimpleBlock**) — сжатые Opus-фреймы, собственно звук.
- WebM = подмножество Matroska. Дерево EBML (бинарный XML): `ID + длина + данные`.
  Поэтому `OpusHead`/`webm` читаются как ASCII прямо в потоке, а звук между ними — нет.
- `ffmpeg -f avfoundation -i ":1" -c:a libopus -ac 1 -ar 48000 out.webm` даёт **байт-в-байт**
  тот же формат, что MediaRecorder в Chrome (mono/48k Opus — дефолт Chrome для голоса).

## Содержимое речи из сырых байтов узнать нельзя

Это сжатые Opus-фреймы (психоакустический кодек), не текст. Нужно реально декоднуть:
`ffmpeg webm → PCM`, затем ASR (Whisper). По байтам слова не «прочитать».

## Авторизация: Bearer ≠ приватный ключ

- `authorization: Bearer eyJ...` — токен-**предъявитель**. Кто строку держит — тот и ты для сервера.
  Как ключ от квартиры: замок не проверяет чей, только что подходит.
- JWT внутри подписан **серверным** приватным ключом OpenAI — подделать/выпустить свой нельзя.
  Но и не надо: токен уже выдан, лежит в запросе целиком.
- Свой токен берётся из залогиненного ChatGPT (DevTools → Network → Copy as cURL).
  Чужой не сфабриковать.

## Cloudflare: голый curl не пройдёт (главный камень)

- 403 с `cf-mitigated: challenge` — Cloudflare bot-management снимает **отпечаток TLS-рукопожатия**
  (JA3/JA4) и порядок HTTP/2-фреймов. У Chrome отпечаток один, у системного curl
  (LibreSSL/SecureTransport) — совсем другой.
- `cf_clearance` привязан к отпечатку клиента, что решал челлендж, **и к IP**. Шлёшь его из curl →
  отпечаток не тот → повторный challenge → 403. Заголовки и куки НЕ спасают: палят байты рукопожатия.
- Чинится подделкой отпечатка: **`curl_cffi`** (форк libcurl, копирует ClientHello + HTTP/2-профиль
  Chrome). Те же куки, тот же запрос — разница только в TLS → 200.
- `curl_cffi` 0.15 сменил API: multipart через **`CurlMime`**, а не `files=` (`NotImplementedError`).

## Эксплуатация / протухание

- Куки и токен живут **дни**. Не-200 → обнови `creds.env` из свежего DevTools.
- `cf_clearance` **IP-bound**: сменил сеть/VPN → снова challenge, нужен свежий с того же IP.
- Эндпоинт приватный, OpenAI стабильность не обещает — может смениться путь/заголовки/защита.
  Нужна надёжность → официальный `/v1/audio/transcriptions` (ключ `sk-`, без Cloudflare-стены).
- Креды в `creds.env` (в `.gitignore`), не в коде — иначе утекут в git-историю.

## Окружение

- macOS Python = externally-managed (PEP 668) → ставим в **venv** (`.venv/`), не `--break-system-packages`.
- Микрофоны: `ffmpeg -f avfoundation -list_devices true -i ""`. Дефолт `[1] MacBook Pro Microphone`.

## Push-to-talk (диктовка на Right Option)

- Глобальный hold-хоткей — `pynput` (keyDown/keyUp). Right Option = `Key.alt_r` / vk `61`
  (Left Option = vk `58`). Зажатие шлёт повтор `on_press` — старт пишем по флагу, не каждый раз.
- Остановка ffmpeg — **SIGINT**, не SIGKILL: ffmpeg ловит SIGINT и дописывает EBML-трейлер,
  файл валиден. KILL → обрезанный/нефинализированный webm.
- Вставка с сохранением буфера: `pbpaste` (сохранить) → `pbcopy` текст → **`Cmd+V` через pynput
  Controller** (тот же процесс, что слушает клавиши) → пауза → `pbcopy` назад. `pbpaste`/`pbcopy` —
  **только текст**: картинка/файлы в буфере не сохранятся (ceiling; если надо — AppleScript с типами clipboard).
- **Почему не osascript:** `osascript ... keystroke "v"` падал с `1002 not allowed to send keystrokes` —
  отдельный бинарь не наследует TCC-грант Accessibility ответственного процесса. Шлём `Cmd+V` из самого
  python-процесса (у него уже есть event-tap) → грант нужен только одному приложению (терминалу).
- Права macOS — **две разные** привилегии: **Input Monitoring** (pynput слушает Right Option, запись)
  и **Accessibility/Универсальный доступ** (синтез `Cmd+V`). Запись может идти, а вставка молчать —
  это значит дан только Input Monitoring. `ensure_accessibility()` на старте поднимает системный диалог
  (`AXIsProcessTrustedWithOptions` + prompt) и добавляет ответственное приложение в список Accessibility.
- Плюс **Микрофон** для ffmpeg.
