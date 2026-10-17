//! Whether someone is at the Mac, from two cheap system counters. Only the time
//! since the last keyboard, mouse, or trackpad event and the screen-lock flag are
//! read: no event contents, no Input Monitoring or Accessibility permission.

use std::ffi::{c_char, c_void};

type CFTypeRef = *const c_void;

const COMBINED_SESSION_STATE: i32 = 0;
const ANY_INPUT_EVENT: u32 = u32::MAX;
const UTF8: u32 = 0x0800_0100;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    fn CGSessionCopyCurrentDictionary() -> CFTypeRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        value: *const c_char,
        encoding: u32,
    ) -> CFTypeRef;
    fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFBooleanGetValue(value: CFTypeRef) -> bool;
    fn CFRelease(value: CFTypeRef);
}

/// True when this user's session is on screen, unlocked, and received input
/// within the last `seconds`. Typing a password on the lock screen counts only
/// once the unlock succeeds.
pub(crate) fn used_within(seconds: f64) -> bool {
    let idle =
        unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT) };
    idle < seconds && session_unlocked_on_console()
}

fn session_unlocked_on_console() -> bool {
    unsafe {
        let session = CGSessionCopyCurrentDictionary();
        if session.is_null() {
            return false;
        }
        let on_console = flag(session, c"kCGSSessionOnConsoleKey").unwrap_or(false);
        let locked = flag(session, c"CGSSessionScreenIsLocked").unwrap_or(false);
        CFRelease(session);
        on_console && !locked
    }
}

unsafe fn flag(dictionary: CFTypeRef, key: &std::ffi::CStr) -> Option<bool> {
    unsafe {
        let key = CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), UTF8);
        if key.is_null() {
            return None;
        }
        let value = CFDictionaryGetValue(dictionary, key);
        CFRelease(key);
        (!value.is_null()).then(|| CFBooleanGetValue(value))
    }
}
