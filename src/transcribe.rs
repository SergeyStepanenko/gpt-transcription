use crate::config::Creds;
use std::path::Path;

/// POST audio to ChatGPT /backend-api/transcribe with Chrome TLS fingerprint.
pub fn transcribe(audio: &Path, creds: &Creds) -> Result<String, String> {
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
            let preview = if body.len() > 300 { &body[..300] } else { &body };
            return Err(format!("HTTP {status}: {preview}"));
        }

        let json: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| format!("json parse: {e}"))?;

        Ok(json["text"].as_str().unwrap_or("").to_string())
    })
}
