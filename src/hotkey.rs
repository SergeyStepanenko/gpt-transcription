use core_graphics::event::{
    CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};
use std::os::raw::c_void;

// CGEventTapCallBack signature
type TapCallback = unsafe extern "C" fn(
    proxy: *const c_void,
    event_type: CGEventType,
    event: *mut c_void, // CGEventRef
    user_info: *mut c_void,
) -> *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: CGEventTapLocation,
        place: CGEventTapPlacement,
        options: CGEventTapOptions,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> *const c_void; // CFMachPortRef

    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: *const c_void,
        order: isize,
    ) -> *const c_void;

    fn CGEventGetIntegerValueField(event: *const c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *const c_void) -> u64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRunLoopGetCurrent() -> *const c_void;
    fn CFRunLoopAddSource(rl: *const c_void, source: *const c_void, mode: *const c_void);
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: *const c_void);
    fn CFRelease(cf: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

const KEYCODE_FIELD: u32 = 9; // kCGKeyboardEventKeycode
const RIGHT_CMD_KEYCODE: i64 = 54;
const CG_EVENT_FLAG_COMMAND: u64 = 0x00100000;

pub trait HotkeyHandler: Send + Sync {
    fn on_press(&self);
    fn on_release(&self);
}

static mut HANDLER: Option<*const dyn HotkeyHandler> = None;
static mut RUN_LOOP: *const c_void = std::ptr::null();

unsafe extern "C" fn tap_callback(
    _proxy: *const c_void,
    event_type: CGEventType,
    event: *mut c_void,
    _user_info: *mut c_void,
) -> *mut c_void {
    // flagsChanged = 12
    if event_type as u32 != 12 {
        return event;
    }

    let keycode = CGEventGetIntegerValueField(event as *const _, KEYCODE_FIELD);
    if keycode != RIGHT_CMD_KEYCODE {
        return event;
    }

    let flags = CGEventGetFlags(event as *const _);
    let cmd_down = (flags & CG_EVENT_FLAG_COMMAND) != 0;

    if let Some(handler) = HANDLER {
        if cmd_down {
            (*handler).on_press();
        } else {
            (*handler).on_release();
        }
    }

    event
}

/// Install a CGEventTap for Right Command (keycode 54) and run CFRunLoop.
/// This blocks the calling thread. Call `stop_run_loop()` to break out.
pub fn install_and_run(handler: Box<dyn HotkeyHandler>) {
    unsafe {
        let handler_ptr: *const dyn HotkeyHandler = Box::into_raw(handler);
        HANDLER = Some(handler_ptr);

        // flagsChanged = 1 << 12
        let mask: u64 = 1 << 12;

        let tap = CGEventTapCreate(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            mask,
            tap_callback,
            std::ptr::null_mut(),
        );

        if tap.is_null() {
            panic!("CGEventTapCreate failed — grant Input Monitoring to your terminal app");
        }

        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        let rl = CFRunLoopGetCurrent();
        RUN_LOOP = rl;
        CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
        CFRunLoopRun();

        // cleanup (after stop)
        CFRelease(source);
        CFRelease(tap);
        drop(Box::from_raw(handler_ptr as *mut dyn HotkeyHandler));
        HANDLER = None;
    }
}

pub fn stop_run_loop() {
    unsafe {
        if !RUN_LOOP.is_null() {
            CFRunLoopStop(RUN_LOOP);
        }
    }
}
