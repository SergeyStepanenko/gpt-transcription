use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MIN_SAMPLES: usize = 1920; // ~40ms at 48kHz mono

#[derive(Clone)]
pub struct AudioData {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioData {
    pub fn is_long_enough(&self) -> bool {
        self.samples.len() > MIN_SAMPLES
    }
}

pub struct AudioDevice {
    pub index: String,
    pub name: String,
}

pub fn list_audio_devices() -> Vec<AudioDevice> {
    input_devices()
        .into_iter()
        .enumerate()
        .map(|(i, d)| AudioDevice {
            index: i.to_string(),
            name: d.name().unwrap_or_else(|_| "Unknown input".to_string()),
        })
        .collect()
}

fn input_devices() -> Vec<cpal::Device> {
    cpal::default_host()
        .input_devices()
        .map(|devices| devices.collect())
        .unwrap_or_default()
}

fn input_device(index: &str) -> cpal::Device {
    let devices = input_devices();
    let idx = index.parse::<usize>().ok();
    if let Some(device) = idx.and_then(|i| devices.get(i).cloned()) {
        return device;
    }
    if let Some(device) = cpal::default_host().default_input_device() {
        return device;
    }
    devices
        .into_iter()
        .next()
        .expect("no input audio devices found")
}

fn input_config(device: &cpal::Device) -> cpal::SupportedStreamConfig {
    // Same idea as Handy: prefer native f32 when available, but keep the device's own sample rate.
    if let Ok(configs) = device.supported_input_configs() {
        if let Some(cfg) = configs
            .filter(|c| c.sample_format() == SampleFormat::F32)
            .max_by_key(|c| {
                let channels_score = if c.channels() == 1 { 2 } else { 1 };
                (channels_score, c.max_sample_rate().0)
            })
        {
            return cfg.with_max_sample_rate();
        }
    }

    device
        .default_input_config()
        .expect("failed to read default input config")
}

pub struct WarmCapture {
    pub buf: Arc<Mutex<AudioData>>,
    pub rec: Arc<AtomicBool>,
    _stream: cpal::Stream,
}

impl WarmCapture {
    pub fn start(mic: &str) -> Self {
        let device = input_device(mic);
        let config = input_config(&device);
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let buf = Arc::new(Mutex::new(AudioData {
            samples: Vec::new(),
            sample_rate,
        }));
        let rec = Arc::new(AtomicBool::new(false));
        let stream = build_stream(
            &device,
            &config,
            channels,
            Arc::clone(&rec),
            Arc::clone(&buf),
        );
        stream.play().expect("failed to start input stream");

        WarmCapture {
            buf,
            rec,
            _stream: stream,
        }
    }
}

/// Cold capture: open the input stream on press, stop on release, return mono f32 samples.
pub fn cold_record(mic: &str, rec_flag: Arc<AtomicBool>) -> Option<AudioData> {
    let device = input_device(mic);
    let config = input_config(&device);
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let buf = Arc::new(Mutex::new(AudioData {
        samples: Vec::new(),
        sample_rate,
    }));
    let stream = build_stream(
        &device,
        &config,
        channels,
        Arc::clone(&rec_flag),
        Arc::clone(&buf),
    );
    stream.play().expect("failed to start input stream");

    while rec_flag.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    drop(stream);
    let audio = buf.lock().unwrap().clone();
    audio.is_long_enough().then_some(audio)
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    channels: usize,
    rec: Arc<AtomicBool>,
    buf: Arc<Mutex<AudioData>>,
) -> cpal::Stream {
    let err_fn = |err| eprintln!("audio input stream error: {err}");
    let stream_config: cpal::StreamConfig = config.clone().into();

    match config.sample_format() {
        SampleFormat::F32 => {
            build_stream_typed::<f32>(device, &stream_config, channels, rec, buf, err_fn)
        }
        SampleFormat::I16 => {
            build_stream_typed::<i16>(device, &stream_config, channels, rec, buf, err_fn)
        }
        SampleFormat::U16 => {
            build_stream_typed::<u16>(device, &stream_config, channels, rec, buf, err_fn)
        }
        SampleFormat::I8 => {
            build_stream_typed::<i8>(device, &stream_config, channels, rec, buf, err_fn)
        }
        SampleFormat::I32 => {
            build_stream_typed::<i32>(device, &stream_config, channels, rec, buf, err_fn)
        }
        SampleFormat::U32 => {
            build_stream_typed::<u32>(device, &stream_config, channels, rec, buf, err_fn)
        }
        other => panic!("unsupported input sample format: {other:?}"),
    }
}

fn build_stream_typed<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    rec: Arc<AtomicBool>,
    buf: Arc<Mutex<AudioData>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> cpal::Stream
where
    T: Sample + SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if !rec.load(Ordering::Relaxed) {
                    return;
                }

                let mut out = buf.lock().unwrap();
                append_mono_f32(&mut out.samples, data, channels);
            },
            err_fn,
            None,
        )
        .expect("failed to build input stream")
}

fn append_mono_f32<T>(out: &mut Vec<f32>, data: &[T], channels: usize)
where
    T: Sample,
    f32: FromSample<T>,
{
    if channels <= 1 {
        out.extend(data.iter().copied().map(f32::from_sample));
        return;
    }

    for frame in data.chunks_exact(channels) {
        let sum: f32 = frame.iter().copied().map(f32::from_sample).sum();
        out.push(sum / channels as f32);
    }
}

/// f32 mono PCM -> WebM/Opus via ffmpeg.
pub fn encode(audio: &AudioData, out: &std::path::Path) {
    write_debug_wav_if_requested(audio, out);

    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "f32le",
            "-ar",
            &audio.sample_rate.to_string(),
            "-ac",
            "1",
            "-i",
            "-",
            "-c:a",
            "libopus",
            "-ar",
            "48000",
            "-y",
        ])
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("ffmpeg encode failed to start");

    if let Some(mut stdin) = child.stdin.take() {
        for sample in &audio.samples {
            let _ = stdin.write_all(&sample.to_le_bytes());
        }
    }
    let _ = child.wait();
}

fn write_debug_wav_if_requested(audio: &AudioData, out: &std::path::Path) {
    if std::env::var("PTT_DEBUG_AUDIO").is_err() {
        return;
    }

    let mut path = out.to_path_buf();
    path.set_extension("raw.wav");
    if let Err(e) = write_wav_f32_mono(&path, audio) {
        eprintln!("debug audio write failed: {e}");
    } else {
        eprintln!("debug audio written: {}", path.display());
    }
}

fn write_wav_f32_mono(path: &std::path::Path, audio: &AudioData) -> std::io::Result<()> {
    let data_len = (audio.samples.len() * 4) as u32;
    let byte_rate = audio.sample_rate * 4;
    let mut f = std::fs::File::create(path)?;

    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?; // PCM fmt chunk size
    f.write_all(&3u16.to_le_bytes())?; // IEEE float
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&audio.sample_rate.to_le_bytes())?;
    f.write_all(&byte_rate.to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?; // block align
    f.write_all(&32u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for sample in &audio.samples {
        f.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_mono_f32_keeps_mono() {
        let mut out = Vec::new();
        append_mono_f32(&mut out, &[0.25f32, -0.5], 1);
        assert_eq!(out, vec![0.25, -0.5]);
    }

    #[test]
    fn append_mono_f32_downmixes_channels() {
        let mut out = Vec::new();
        append_mono_f32(&mut out, &[1.0f32, -1.0, 0.5, 0.25], 2);
        assert_eq!(out, vec![0.0, 0.375]);
    }

    #[test]
    fn write_wav_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.wav");
        let audio = AudioData {
            samples: vec![0.0, 0.5],
            sample_rate: 48_000,
        };
        write_wav_f32_mono(&path, &audio).unwrap();
        let data = std::fs::read(path).unwrap();
        assert_eq!(&data[..4], b"RIFF");
        assert_eq!(&data[8..12], b"WAVE");
        assert_eq!(&data[36..40], b"data");
        assert_eq!(&data[40..44], &8u32.to_le_bytes());
    }
}
