use crate::config::Creds;
use std::path::Path;

/// POST audio to ChatGPT /backend-api/transcribe with Chrome TLS fingerprint.
pub fn transcribe(audio: &Path, creds: &Creds) -> Result<String, String> {
    validate_token(&creds.token)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime: {e}"))?;

    // Per-attempt timeout + up to ATTEMPTS tries. A timed-out attempt drops its
    // request future, which aborts the in-flight connection — so the slow server
    // response is cancelled (or, if it still completes, simply ignored) and we
    // start a fresh request. Only one request is ever in flight at a time.
    const ATTEMPTS: usize = 3;
    const PER_TRY: std::time::Duration = std::time::Duration::from_secs(10);

    rt.block_on(async {
        let data = std::fs::read(audio).map_err(|e| format!("read {}: {e}", audio.display()))?;

        use rquest_util::Emulation;

        let client = rquest::Client::builder()
            .emulation(Emulation::Chrome136)
            .build()
            .map_err(|e| format!("rquest client: {e}"))?;

        let data = &data;
        let client = &client;
        retry(ATTEMPTS, move |_attempt| async move {
            let part = match rquest::multipart::Part::bytes(data.clone())
                .file_name("whisper.webm")
                .mime_str("audio/webm;codecs=opus")
            {
                Ok(part) => part,
                Err(e) => return Step::FailFast(format!("multipart: {e}")),
            };
            let form = rquest::multipart::Form::new().part("file", part);

            let resp = client
                .post("https://chatgpt.com/backend-api/transcribe")
                .header("authorization", format!("Bearer {}", creds.token))
                .header("chatgpt-account-id", &creds.account_id)
                .header("cookie", &creds.cookies)
                .header("origin", "https://chatgpt.com")
                .header("referer", "https://chatgpt.com/")
                .multipart(form)
                .timeout(PER_TRY)
                .send()
                .await;

            let resp = match resp {
                Ok(resp) => resp,
                Err(e) => return Step::Retry(format!("request failed: {e}")),
            };

            let status = resp.status().as_u16();
            let body = match resp.text().await {
                Ok(body) => body,
                Err(e) => return Step::Retry(format!("read body: {e}")),
            };

            outcome(status, &body)
        })
        .await
    })
}

/// One attempt's outcome: finish with text, give up immediately, or retry.
enum Step {
    Done(String),
    FailFast(String),
    Retry(String),
}

/// Run `step` up to `attempts` times. Return on the first `Done` (text) or
/// `FailFast` (error); on `Retry`, try again. After the last attempt, surface
/// the most recent retryable error.
async fn retry<F, Fut>(attempts: usize, mut step: F) -> Result<String, String>
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = Step>,
{
    let mut last_err = String::new();
    for attempt in 1..=attempts {
        match step(attempt).await {
            Step::Done(text) => return Ok(text),
            Step::FailFast(e) => return Err(e),
            Step::Retry(e) => {
                eprintln!("transcribe: attempt {attempt}/{attempts} failed ({e})");
                last_err = e;
            }
        }
    }
    Err(format!("transcribe failed after {attempts} attempts: {last_err}"))
}

/// Classify an HTTP response into a retry decision.
fn outcome(status: u16, body: &str) -> Step {
    if (200..300).contains(&status) {
        return match serde_json::from_str::<serde_json::Value>(body) {
            Ok(json) => Step::Done(json["text"].as_str().unwrap_or("").to_string()),
            Err(e) => Step::FailFast(format!("json parse: {e}")),
        };
    }
    // Auth/permission failures won't change on retry — give up immediately.
    if matches!(status, 401 | 403) {
        return Step::FailFast(auth_error(status, body));
    }
    // 5xx, 429, other transient server errors — worth retrying.
    Step::Retry(auth_error(status, body))
}

fn validate_token(token: &str) -> Result<(), String> {
    let mut parts = token.split('.');
    let Some(_header) = parts.next() else {
        return malformed_token();
    };
    let Some(payload) = parts.next() else {
        return malformed_token();
    };
    let Some(_signature) = parts.next() else {
        return malformed_token();
    };
    if parts.next().is_some() {
        return malformed_token();
    }

    // ponytail: local JWT parsing only checks shape and exp; server signature validity is still checked by ChatGPT.
    let payload = base64url_decode(payload).map_err(|_| {
        "TOKEN in creds.env is not a readable JWT. Restart ptt, choose \"Replace credentials\", then paste a fresh Chrome DevTools cURL.".to_string()
    })?;
    let json: serde_json::Value = serde_json::from_slice(&payload).map_err(|_| {
        "TOKEN in creds.env has an invalid JWT payload. Restart ptt, choose \"Replace credentials\", then paste a fresh Chrome DevTools cURL.".to_string()
    })?;

    if let Some(exp) = json.get("exp").and_then(|v| v.as_u64()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("system clock error: {e}"))?
            .as_secs();
        if now >= exp {
            return Err(format!(
                "TOKEN in creds.env is expired (exp={exp}, now={now}). Restart ptt, choose \"Replace credentials\", then paste a fresh Chrome DevTools cURL."
            ));
        }
    }

    Ok(())
}

fn malformed_token() -> Result<(), String> {
    Err("TOKEN in creds.env is not a JWT (expected three dot-separated parts). Restart ptt, choose \"Replace credentials\", then paste a fresh Chrome DevTools cURL.".to_string())
}

fn base64url_decode(input: &str) -> Result<Vec<u8>, ()> {
    if input.len() % 4 == 1 {
        return Err(());
    }

    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u8;

    for b in input.bytes() {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'=' => break,
            _ => return Err(()),
        } as u32;

        buf = (buf << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }

    Ok(out)
}

fn auth_error(status: u16, body: &str) -> String {
    let preview: String = body.chars().take(300).collect();
    match status {
        401 => format!(
            "ChatGPT rejected TOKEN (HTTP 401). The token is invalid, expired, or no longer matches the session. Restart ptt, choose \"Replace credentials\", then paste a fresh Chrome DevTools cURL. Response: {preview}"
        ),
        403 => format!(
            "ChatGPT rejected the session (HTTP 403). TOKEN may be valid, but cookies/Cloudflare clearance are stale or bound to a different browser/IP. Restart ptt, choose \"Replace credentials\", then paste a fresh Chrome DevTools cURL from this machine/network. Response: {preview}"
        ),
        _ => format!("HTTP {status}: {preview}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_with_exp(exp: u64) -> String {
        let payload = format!(r#"{{"exp":{exp}}}"#);
        let payload = base64url_encode(payload.as_bytes());
        format!("h.{payload}.s")
    }

    fn base64url_encode(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);
            let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;

            out.push(TABLE[((n >> 18) & 63) as usize] as char);
            out.push(TABLE[((n >> 12) & 63) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[((n >> 6) & 63) as usize] as char);
            }
            if chunk.len() > 2 {
                out.push(TABLE[(n & 63) as usize] as char);
            }
        }
        out
    }

    #[test]
    fn rejects_malformed_token() {
        let err = validate_token("not-a-jwt").unwrap_err();
        assert!(err.contains("not a JWT"));
    }

    #[test]
    fn rejects_expired_token() {
        let err = validate_token(&token_with_exp(1)).unwrap_err();
        assert!(err.contains("expired"));
    }

    #[test]
    fn accepts_unexpired_token() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        validate_token(&token_with_exp(now + 3600)).unwrap();
    }

    #[test]
    fn explains_auth_statuses() {
        assert!(auth_error(401, "nope").contains("TOKEN"));
        assert!(auth_error(403, "nope").contains("cookies/Cloudflare"));
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn outcome_success_extracts_text() {
        match outcome(200, r#"{"text":"hello world"}"#) {
            Step::Done(t) => assert_eq!(t, "hello world"),
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn outcome_success_missing_text_is_empty() {
        match outcome(200, r#"{"other":1}"#) {
            Step::Done(t) => assert_eq!(t, ""),
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn outcome_bad_json_fails_fast() {
        match outcome(200, "not json") {
            Step::FailFast(e) => assert!(e.contains("json parse")),
            _ => panic!("expected FailFast on unparseable success body"),
        }
    }

    #[test]
    fn outcome_auth_statuses_fail_fast() {
        assert!(matches!(outcome(401, "x"), Step::FailFast(_)));
        assert!(matches!(outcome(403, "x"), Step::FailFast(_)));
    }

    #[test]
    fn outcome_server_errors_retry() {
        assert!(matches!(outcome(500, "x"), Step::Retry(_)));
        assert!(matches!(outcome(503, "x"), Step::Retry(_)));
        assert!(matches!(outcome(429, "x"), Step::Retry(_)));
    }

    #[test]
    fn retry_returns_first_success_without_retrying() {
        let calls = std::cell::Cell::new(0);
        let out = block_on(retry(3, |_| {
            calls.set(calls.get() + 1);
            async { Step::Done("ok".into()) }
        }));
        assert_eq!(out.unwrap(), "ok");
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn retry_recovers_after_transient_failures() {
        let calls = std::cell::Cell::new(0);
        let out = block_on(retry(3, |attempt| {
            calls.set(calls.get() + 1);
            async move {
                if attempt < 3 {
                    Step::Retry(format!("timeout on {attempt}"))
                } else {
                    Step::Done("late".into())
                }
            }
        }));
        assert_eq!(out.unwrap(), "late");
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn retry_gives_up_after_all_attempts() {
        let calls = std::cell::Cell::new(0);
        let out = block_on(retry(3, |_| {
            calls.set(calls.get() + 1);
            async { Step::Retry("timeout".into()) }
        }));
        let err = out.unwrap_err();
        assert!(err.contains("after 3 attempts"), "got: {err}");
        assert!(err.contains("timeout"), "got: {err}");
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn retry_fail_fast_does_not_retry() {
        let calls = std::cell::Cell::new(0);
        let out = block_on(retry(3, |_| {
            calls.set(calls.get() + 1);
            async { Step::FailFast("401 nope".into()) }
        }));
        assert_eq!(out.unwrap_err(), "401 nope");
        assert_eq!(calls.get(), 1);
    }
}
