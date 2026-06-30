use crate::config::Creds;
use std::path::Path;

/// POST audio to ChatGPT /backend-api/transcribe with Chrome TLS fingerprint.
pub fn transcribe(audio: &Path, creds: &Creds) -> Result<String, String> {
    validate_token(&creds.token)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime: {e}"))?;

    rt.block_on(async {
        let data = std::fs::read(audio).map_err(|e| format!("read {}: {e}", audio.display()))?;

        let part = rquest::multipart::Part::bytes(data)
            .file_name("whisper.webm")
            .mime_str("audio/webm;codecs=opus")
            .map_err(|e| format!("multipart: {e}"))?;

        let form = rquest::multipart::Form::new().part("file", part);

        use rquest_util::Emulation;

        let client = rquest::Client::builder()
            .emulation(Emulation::Chrome136)
            .build()
            .map_err(|e| format!("rquest client: {e}"))?;

        let resp = client
            .post("https://chatgpt.com/backend-api/transcribe")
            .header("authorization", format!("Bearer {}", creds.token))
            .header("chatgpt-account-id", &creds.account_id)
            .header("cookie", &creds.cookies)
            .header("origin", "https://chatgpt.com")
            .header("referer", "https://chatgpt.com/")
            .multipart(form)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| format!("read body: {e}"))?;

        if !status.is_success() {
            return Err(auth_error(status.as_u16(), &body));
        }

        let json: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| format!("json parse: {e}"))?;

        Ok(json["text"].as_str().unwrap_or("").to_string())
    })
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
}
