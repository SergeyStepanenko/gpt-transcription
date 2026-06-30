mod audio;
mod config;
mod cues;
mod hotkey;
mod menu;
mod paste;
mod transcribe;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    // creds.env: look next to the binary first, then CWD
    let creds_dir = if exe_dir.join("creds.env").exists() {
        exe_dir.clone()
    } else {
        PathBuf::from(".")
    };
    let creds = config::load_creds(&creds_dir);

    paste::ensure_accessibility();

    let devices = audio::list_audio_devices();
    let mic = menu::choose_mic(&devices);
    let warm = menu::choose_warm();

    let out_path = std::env::current_dir().unwrap().join("_ptt.webm");

    let cue = cues::Cues::load();
    cue.prime();

    let rec = Arc::new(AtomicBool::new(false));
    let busy = Arc::new(AtomicBool::new(false));

    let capture = if warm {
        Some(audio::WarmCapture::start(&mic))
    } else {
        None
    };

    println!("Push-to-talk ready. Mic [{mic}], mode: {}. Hold Right Command to record. Ctrl-C to quit.",
             if warm { "warm" } else { "cold" });

    ctrlc::set_handler(move || {
        println!("\nbye");
        hotkey::stop_run_loop();
    }).ok();

    struct Handler {
        rec: Arc<AtomicBool>,
        busy: Arc<AtomicBool>,
        mic: String,
        warm: bool,
        out_path: PathBuf,
        creds: config::Creds,
        cue_start: cues::Cue,
        cue_stop: cues::Cue,
        capture_buf: Option<Arc<std::sync::Mutex<Vec<u8>>>>,
        capture_rec: Option<Arc<AtomicBool>>,
        // Cold mode: the recording thread collects PCM here
        cold_buf: Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl hotkey::HotkeyHandler for Handler {
        fn on_press(&self) {
            if self.rec.load(Ordering::Relaxed) || self.busy.load(Ordering::Relaxed) {
                return;
            }

            if self.warm {
                if let Some(ref buf) = self.capture_buf {
                    buf.lock().unwrap().clear();
                }
                if let Some(ref r) = self.capture_rec {
                    r.store(true, Ordering::Relaxed);
                }
            } else {
                // Cold: spawn ffmpeg in a thread
                self.cold_buf.lock().unwrap().clear();
                let mic = self.mic.clone();
                let rec = Arc::clone(&self.rec);
                let cold_buf = Arc::clone(&self.cold_buf);
                // rec will be set to true below — the thread reads it
                std::thread::spawn(move || {
                    if let Some(pcm) = audio::cold_record(&mic, rec) {
                        *cold_buf.lock().unwrap() = pcm;
                    }
                });
            }

            self.rec.store(true, Ordering::Relaxed);
            self.cue_start.play();
            println!("● recording...");
        }

        fn on_release(&self) {
            if !self.rec.load(Ordering::Relaxed) {
                return;
            }
            self.rec.store(false, Ordering::Relaxed);

            if let Some(ref r) = self.capture_rec {
                r.store(false, Ordering::Relaxed);
            }

            self.cue_stop.play();
            self.busy.store(true, Ordering::Relaxed);
            println!("■ stop, transcribing...");

            // Give cold-mode thread a moment to finish collecting + kill ffmpeg
            if !self.warm {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }

            let pcm = if self.warm {
                self.capture_buf.as_ref().and_then(|buf| {
                    let b = buf.lock().unwrap();
                    if b.len() > 4000 { Some(b.clone()) } else { None }
                })
            } else {
                let b = self.cold_buf.lock().unwrap();
                if b.len() > 4000 { Some(b.clone()) } else { None }
            };

            if let Some(pcm) = pcm {
                audio::encode(&pcm, &self.out_path);
                match transcribe::transcribe(&self.out_path, &self.creds) {
                    Ok(text) if !text.trim().is_empty() => {
                        paste::paste_text(&text);
                        println!("→ {text:?}");
                    }
                    Ok(_) => println!("(empty)"),
                    Err(e) => eprintln!("error: {e}"),
                }
            } else {
                println!("(too short)");
            }

            self.busy.store(false, Ordering::Relaxed);
        }
    }

    let handler = Handler {
        rec: Arc::clone(&rec),
        busy: Arc::clone(&busy),
        mic: mic.clone(),
        warm,
        out_path,
        creds,
        cue_start: cues::Cue::load("/System/Library/Sounds/Morse.aiff"),
        cue_stop: cues::Cue::load("/System/Library/Sounds/Bottle.aiff"),
        capture_buf: capture.as_ref().map(|c| Arc::clone(&c.buf)),
        capture_rec: capture.as_ref().map(|c| Arc::clone(&c.rec)),
        cold_buf: Arc::new(std::sync::Mutex::new(Vec::new())),
    };

    // This blocks on CFRunLoop
    hotkey::install_and_run(Box::new(handler));
}
