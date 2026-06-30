use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub const RATE: u32 = 48000;
const MIN_PCM_BYTES: usize = 4000; // ~40ms at 48k/s16/mono

pub struct AudioDevice {
    pub index: String,
    pub name: String,
}

pub fn list_audio_devices() -> Vec<AudioDevice> {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let stderr = match out {
        Ok(o) => String::from_utf8_lossy(&o.stderr).into_owned(),
        Err(_) => return Vec::new(),
    };

    parse_audio_devices(&stderr)
}

pub fn parse_audio_devices(listing: &str) -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    let mut in_audio = false;

    for line in listing.lines() {
        if line.contains("audio devices:") {
            in_audio = true;
            continue;
        }
        if line.contains("video devices:") {
            in_audio = false;
            continue;
        }
        if !in_audio {
            continue;
        }
        // [AVFoundation ...] [0] Device Name
        if let Some(bracket) = line.rfind('[') {
            let rest = &line[bracket + 1..];
            if let Some(close) = rest.find(']') {
                let idx = &rest[..close];
                if idx.chars().all(|c| c.is_ascii_digit()) {
                    let name = rest[close + 1..].trim().to_string();
                    devices.push(AudioDevice {
                        index: idx.to_string(),
                        name,
                    });
                }
            }
        }
    }
    devices
}

pub struct WarmCapture {
    pub buf: Arc<Mutex<Vec<u8>>>,
    pub rec: Arc<AtomicBool>,
    child: Option<std::process::Child>,
}

impl WarmCapture {
    pub fn start(mic: &str) -> Self {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let rec = Arc::new(AtomicBool::new(false));

        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner", "-loglevel", "error",
                "-f", "avfoundation", "-i", &format!(":{mic}"),
                "-ac", "1", "-ar", &RATE.to_string(),
                "-f", "s16le", "-",
            ])
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to start ffmpeg — is it installed? (brew install ffmpeg)");

        let mut stdout = child.stdout.take().unwrap();
        let buf_c = Arc::clone(&buf);
        let rec_c = Arc::clone(&rec);

        thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if rec_c.load(Ordering::Relaxed) {
                            buf_c.lock().unwrap().extend_from_slice(&chunk[..n]);
                        }
                    }
                }
            }
        });

        WarmCapture {
            buf,
            rec,
            child: Some(child),
        }
    }

}

impl Drop for WarmCapture {
    fn drop(&mut self) {
        if let Some(ref mut c) = self.child {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Cold capture: spawn ffmpeg on press, kill on release, return PCM.
pub fn cold_record(mic: &str, rec_flag: Arc<AtomicBool>) -> Option<Vec<u8>> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner", "-loglevel", "error",
            "-f", "avfoundation", "-i", &format!(":{mic}"),
            "-ac", "1", "-ar", &RATE.to_string(),
            "-f", "s16le", "-",
        ])
        .stdout(Stdio::piped())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start ffmpeg");

    let mut stdout = child.stdout.take().unwrap();
    let mut pcm = Vec::new();
    let mut chunk = [0u8; 4096];

    while rec_flag.load(Ordering::Relaxed) {
        match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => pcm.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if pcm.len() > MIN_PCM_BYTES { Some(pcm) } else { None }
}

/// PCM -> WebM/Opus via ffmpeg (same format as Chrome MediaRecorder).
pub fn encode(pcm: &[u8], out: &std::path::Path) {
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner", "-loglevel", "error",
            "-f", "s16le", "-ar", &RATE.to_string(), "-ac", "1", "-i", "-",
            "-c:a", "libopus", "-y",
        ])
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("ffmpeg encode failed to start");

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(pcm);
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "\
[AVFoundation indev @ 0x1] AVFoundation video devices:
[AVFoundation indev @ 0x1] [0] FaceTime HD Camera
[AVFoundation indev @ 0x1] [1] Capture screen 0
[AVFoundation indev @ 0x1] AVFoundation audio devices:
[AVFoundation indev @ 0x1] [0] External USB Microphone
[AVFoundation indev @ 0x1] [1] MacBook Pro Microphone
";

    #[test]
    fn parse_devices() {
        let devs = parse_audio_devices(LISTING);
        assert_eq!(devs.len(), 2);
        assert_eq!(devs[0].index, "0");
        assert!(devs[0].name.contains("External USB"));
        assert_eq!(devs[1].index, "1");
        assert!(devs[1].name.contains("MacBook"));
    }

    #[test]
    fn parse_empty() {
        assert!(parse_audio_devices("").is_empty());
    }
}
