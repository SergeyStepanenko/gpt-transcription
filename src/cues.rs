use std::os::raw::c_void;

type SystemSoundID = u32;

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioServicesCreateSystemSoundID(url: *const c_void, sound_id: *mut SystemSoundID) -> i32;
    fn AudioServicesPlaySystemSound(sound_id: SystemSoundID);
    fn AudioServicesDisposeSystemSoundID(sound_id: SystemSoundID) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: *const c_void,
        buffer: *const u8,
        buf_len: isize,
        is_directory: bool,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

pub struct Cue {
    sound_id: SystemSoundID,
}

impl Cue {
    pub fn load(path: &str) -> Self {
        let sound_id = unsafe {
            let url = CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                path.as_ptr(),
                path.len() as isize,
                false,
            );
            assert!(!url.is_null(), "failed to create CFURL for {path}");
            let mut sid: SystemSoundID = 0;
            let status = AudioServicesCreateSystemSoundID(url, &mut sid);
            CFRelease(url);
            assert!(status == 0, "AudioServicesCreateSystemSoundID failed: {status}");
            sid
        };
        Cue { sound_id }
    }

    pub fn play(&self) {
        unsafe { AudioServicesPlaySystemSound(self.sound_id); }
    }
}

impl Drop for Cue {
    fn drop(&mut self) {
        unsafe { AudioServicesDisposeSystemSoundID(self.sound_id); }
    }
}

pub struct Cues {
    pub start: Cue,
    pub stop: Cue,
}

impl Cues {
    pub fn load() -> Self {
        Cues {
            start: Cue::load("/System/Library/Sounds/Morse.aiff"),
            stop: Cue::load("/System/Library/Sounds/Bottle.aiff"),
        }
    }

    /// Prime CoreAudio output so the first real cue plays without latency.
    pub fn prime(&self) {
        // AudioServices is already low-latency after CreateSystemSoundID;
        // a single silent play primes the audio device.
        unsafe {
            AudioServicesPlaySystemSound(self.start.sound_id);
            AudioServicesPlaySystemSound(self.stop.sound_id);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
