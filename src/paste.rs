use std::os::raw::c_void;
use std::process::Command;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
        ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

extern "C" {
    // kAXTrustedCheckOptionPrompt — CFStringRef
    static kAXTrustedCheckOptionPrompt: *const c_void;
    // kCFBooleanTrue
    static kCFBooleanTrue: *const c_void;
    // CoreFoundation dict callbacks
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}

pub fn ensure_accessibility() {
    let trusted = unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let opts = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );
        let result = AXIsProcessTrustedWithOptions(opts);
        CFRelease(opts);
        result
    };

    if !trusted {
        eprintln!("⚠️  No Accessibility permission — paste (Cmd+V) won't work (recording still will).");
        eprintln!("    System Settings → Privacy & Security → Accessibility: enable the app");
        eprintln!("    you launch ptt from. Then restart.");
    }
}

/// Save clipboard text, set new text, press Cmd+V, restore.
pub fn paste_text(text: &str) {
    let old = Command::new("pbpaste")
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();

    let _ = Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin.take().unwrap().write_all(text.as_bytes())?;
            c.wait()
        });

    send_cmd_v();

    std::thread::sleep(std::time::Duration::from_millis(200));

    let _ = Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin.take().unwrap().write_all(&old)?;
            c.wait()
        });
}

/// Synthetic Cmd+V via CGEvent — keycode 9 (physical V), layout-independent.
fn send_cmd_v() {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).unwrap();

    let key_down = CGEvent::new_keyboard_event(source.clone(), 9, true).unwrap();
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(CGEventTapLocation::HID);

    let key_up = CGEvent::new_keyboard_event(source, 9, false).unwrap();
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);
    key_up.post(CGEventTapLocation::HID);
}
