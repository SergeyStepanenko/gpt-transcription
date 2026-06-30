use dialoguer::{Select, theme::ColorfulTheme};

use crate::audio::AudioDevice;

pub fn choose_mic(devices: &[AudioDevice]) -> String {
    if let Ok(env) = std::env::var("MIC") {
        if !env.is_empty() {
            return env;
        }
    }

    if devices.is_empty() {
        return "1".to_string();
    }

    let default_i = devices
        .iter()
        .position(|d| d.name.to_lowercase().contains("macbook"))
        .unwrap_or(0);

    if !atty::is(atty::Stream::Stdin) {
        return devices[default_i].index.clone();
    }

    let labels: Vec<String> = devices
        .iter()
        .map(|d| format!("[{}] {}", d.index, d.name))
        .collect();

    let sel = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select mic  (↑/↓, Enter)")
        .items(&labels)
        .default(default_i)
        .interact()
        .expect("mic selection failed");

    devices[sel].index.clone()
}

pub fn choose_warm() -> bool {
    if !atty::is(atty::Stream::Stdin) {
        return true;
    }

    let items = &[
        "Yes — instant recording (mic stays active, indicator stays on)",
        "No  — mic only while recording (more private, ~1s startup delay)",
    ];

    let sel = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Keep mic always on?")
        .items(items)
        .default(0)
        .interact()
        .expect("warm/cold selection failed");

    sel == 0
}
