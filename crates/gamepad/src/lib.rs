//! `gamepad` — GameInput gamepad events on Windows, as a standalone library.
//!
//! The mechanics are copied from the PoC (`hidpad-poc/`): a hand-written COM
//! vtable shim over `gameinput.dll` exports, version negotiation v3 -> v2 -> v0,
//! background focus policy set BEFORE any registration, `device = NULL`
//! system-button registration, three async device-kind registrations plus one
//! blocking Connected enumeration, a dispatcher pump on the calling thread, and a
//! button-STATE-diff poll. Only the API boundary differs: events go to an
//! [`Observer`] instead of stdout, and init failures come back as [`Error`]
//! instead of exiting the process.
//!
//! No channels: `run` pumps on the calling thread and delivers events on whatever
//! thread GameInput invokes callbacks from (the pump thread inside `Dispatch`, or
//! GameInput's own threads in the dispatcher-less fallback mode).
//!
//! unsafe policy: `#![deny(unsafe_code)]` at the crate root. The single exception
//! is the `gameinput` module — a raw COM vtable cannot be mirrored without
//! `unsafe`; everything above this boundary is safe code.
#![deny(unsafe_code)]

#[cfg(windows)]
mod gameinput;
mod layout;

use std::fmt;
use std::sync::atomic::AtomicBool;

/// A device as GameInput reported it, in the fields the PoC printed on its
/// `device:` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Position in the crate's device registry — the id used by
    /// `Observer::buttons_changed` and [`button_label`]. Stable for a whole run:
    /// registry entries are never removed (a disconnect nulls the pointer).
    pub index: usize,
    /// `displayName` from the device info block, or `<null>` when absent.
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    /// HID usage code (`GameInputDeviceInfo.usage`).
    pub usage_id: u16,
    /// `GameInputKind` bitmask of what the device supports.
    pub supported_input: i32,
    /// `GameInputSystemButtons` bitmask (v2/v3). The v0 info block has no such
    /// field, so on v0 this carries v0's `capabilities` mask instead.
    pub supported_system_buttons: i32,
}

/// System buttons the registrations deliver. Guide and Share (v2/v3); v0's
/// `RegisterGuideButtonCallback` delivers Guide only — Share starts at v2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemButton {
    Guide,
    Share,
}

/// Event sink for one [`run`]. Callbacks fire on GameInput's threads or on the
/// pump thread inside `Dispatch`, so an observer must be `Send + Sync` and must
/// return quickly: blocking in a callback stalls the pump (and GameInput can
/// terminate the process inside a callback on its own threads).
pub trait Observer: Send + Sync {
    /// Once per device pointer, when the device first enters the registry
    /// (dedup is by pointer, so a device reported by several kind filters is
    /// delivered once).
    fn device_connected(&self, device: &DeviceInfo);
    /// A device's controller-button state mask changed. `bits` is the new mask
    /// (bit `i` = button index `i` pressed), `pressed_indices` lists the
    /// CURRENTLY pressed indices of that new mask — the event is a STATE change,
    /// not a 0→1 diff: a release changes the mask too (and then lists fewer
    /// indices), and a button held across the change is still listed. A
    /// consumer that needs DOWN edges must diff masks itself: the newly pressed
    /// set is `bits & !previous_bits`. Delivered after the registry lock is
    /// released, so [`button_label`] is safe to call from here.
    fn buttons_changed(&self, device_index: usize, bits: u64, pressed_indices: &[u8]);
    /// Guide/Share edge from the system-button callback: a changed bit present in
    /// the current state is `down`, otherwise `up`. Share first, then Guide —
    /// the PoC's output order.
    fn system_button(&self, button: SystemButton, down: bool, device_name: &str);
    /// A device's pointer stopped resolving (`E_DEVICE_DISCONNECTED` while
    /// polling its buttons). The registry entry stays, so the `index` remains
    /// stable and never gets reused by another device; only the pointer is
    /// nulled, so no further `buttons_changed` arrives for it. A reconnect
    /// creates a NEW entry with a new index (dedup is by pointer), and delivers
    /// [`device_connected`] again. Default no-op: observers that do not care
    /// stay source-compatible.
    fn device_disconnected(&self, _index: usize, _name: &str) {}
    /// Liveness pulse every 5 s: devices registered, and dispatcher ticks since
    /// start (`0` in fallback mode, which has no dispatcher to tick).
    fn heartbeat(&self, devices: usize, dispatches: u64);
}

/// Why [`run`] could not start. `Display` is written by hand: the crate has zero
/// dependencies, so `thiserror` is not available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// GameInput is Windows-only: `run` on any other platform returns this and
    /// touches nothing (the crate still compiles and tests there).
    Unsupported,
    /// `gameinput.dll` could not be loaded, or exports neither `GameInputCreate`
    /// nor `GameInputInitialize`.
    LibraryUnavailable,
    /// The negotiated `IGameInput` interface lacks a callback slot this library
    /// drives (system-button or device registration). The PoC printed and exited
    /// cleanly in the same situation.
    InterfaceUnavailable,
    /// Object creation failed or handed back a null `IGameInput` pointer; the
    /// string names the failing call.
    InitFailed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported => f.write_str("gamepad: GameInput is Windows-only; unsupported platform"),
            Error::LibraryUnavailable => {
                f.write_str("gamepad: gameinput.dll unavailable (no GameInputCreate/GameInputInitialize export)")
            }
            Error::InterfaceUnavailable => {
                f.write_str("gamepad: GameInput interface lacks a required callback slot")
            }
            Error::InitFailed(what) => write!(f, "gamepad: GameInput init failed: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Start GameInput and pump it until `stop` is set.
///
/// Order is exactly the PoC's: load the DLL, resolve exports, create the
/// `IGameInput` object, negotiate v3/v2/v0, set the background focus policy
/// BEFORE registering, register system buttons (`device = NULL`), register the
/// device callbacks (async Controller/Gamepad/ControllerButton, then blocking
/// Connected), then pump `Dispatch` every 10 ms and poll buttons every 50 ms on
/// this thread.
///
/// Blocks until `stop` becomes true, then returns `Ok(())`. GameInput state
/// lives in process-wide statics, so call this once per process.
pub fn run(observer: &dyn Observer, stop: &AtomicBool) -> Result<(), Error> {
    #[cfg(windows)]
    {
        gameinput::run(observer, stop)
    }
    #[cfg(not(windows))]
    {
        let _ = (observer, stop);
        Err(Error::Unsupported)
    }
}

/// Canonical name of a `GameInputLabel` enum value, transcribed from the
/// vendored header (`vendor/GameInput.h:367-494`); the table index IS the enum
/// value, and the header has one gap (`6`) plus `GameInputLabelNone = 0` and
/// `GameInputLabelUnknown = -1`, all of which map to `None`.
///
/// There is no `GameInputButtonFlags` enum in any GameInput header, so button
/// identity is this label enum — surfaced per button by [`button_label`].
pub fn button_name(label: i32) -> Option<&'static str> {
    if label < 0 {
        return None;
    }
    LABEL_NAMES
        .get(label as usize)
        .copied()
        .filter(|name| !name.is_empty())
}

/// Header-truth table for `GameInputLabel`; every entry is a `GameInputLabel`
/// member with the `GameInputLabel` prefix stripped.
const LABEL_NAMES: &[&str] = &[
    "",                         // 0: GameInputLabelNone means "unlabeled", so no name
    "XboxGuide",                // 1
    "XboxBack",                 // 2
    "XboxStart",                // 3
    "XboxMenu",                 // 4
    "XboxView",                 // 5
    "",                         // 6 (no header value)
    "XboxA",                    // 7
    "XboxB",                    // 8
    "XboxX",                    // 9
    "XboxY",                    // 10
    "XboxDPadUp",               // 11
    "XboxDPadDown",             // 12
    "XboxDPadLeft",             // 13
    "XboxDPadRight",            // 14
    "XboxLeftShoulder",         // 15
    "XboxLeftTrigger",          // 16
    "XboxLeftStickButton",      // 17
    "XboxRightShoulder",        // 18
    "XboxRightTrigger",         // 19
    "XboxRightStickButton",     // 20
    "XboxPaddle1",              // 21
    "XboxPaddle2",              // 22
    "XboxPaddle3",              // 23
    "XboxPaddle4",              // 24
    "LetterA",                  // 25
    "LetterB",                  // 26
    "LetterC",                  // 27
    "LetterD",                  // 28
    "LetterE",                  // 29
    "LetterF",                  // 30
    "LetterG",                  // 31
    "LetterH",                  // 32
    "LetterI",                  // 33
    "LetterJ",                  // 34
    "LetterK",                  // 35
    "LetterL",                  // 36
    "LetterM",                  // 37
    "LetterN",                  // 38
    "LetterO",                  // 39
    "LetterP",                  // 40
    "LetterQ",                  // 41
    "LetterR",                  // 42
    "LetterS",                  // 43
    "LetterT",                  // 44
    "LetterU",                  // 45
    "LetterV",                  // 46
    "LetterW",                  // 47
    "LetterX",                  // 48
    "LetterY",                  // 49
    "LetterZ",                  // 50
    "Number0",                  // 51
    "Number1",                  // 52
    "Number2",                  // 53
    "Number3",                  // 54
    "Number4",                  // 55
    "Number5",                  // 56
    "Number6",                  // 57
    "Number7",                  // 58
    "Number8",                  // 59
    "Number9",                  // 60
    "ArrowUp",                  // 61
    "ArrowUpRight",             // 62
    "ArrowRight",               // 63
    "ArrowDownRight",           // 64
    "ArrowDown",                // 65
    "ArrowDownLLeft",           // 66
    "ArrowLeft",                // 67
    "ArrowUpLeft",              // 68
    "ArrowUpDown",              // 69
    "ArrowLeftRight",           // 70
    "ArrowUpDownLeftRight",     // 71
    "ArrowClockwise",           // 72
    "ArrowCounterClockwise",    // 73
    "ArrowReturn",              // 74
    "IconBranding",             // 75
    "IconHome",                 // 76
    "IconMenu",                 // 77
    "IconCross",                // 78
    "IconCircle",               // 79
    "IconSquare",               // 80
    "IconTriangle",             // 81
    "IconStar",                 // 82
    "IconDPadUp",               // 83
    "IconDPadDown",             // 84
    "IconDPadLeft",             // 85
    "IconDPadRight",            // 86
    "IconDialClockwise",        // 87
    "IconDialCounterClockwise", // 88
    "IconSliderLeftRight",      // 89
    "IconSliderUpDown",         // 90
    "IconWheelUpDown",          // 91
    "IconPlus",                 // 92
    "IconMinus",                // 93
    "IconSuspension",           // 94
    "Home",                     // 95
    "Guide",                    // 96
    "Mode",                     // 97
    "Select",                   // 98
    "Menu",                     // 99
    "View",                     // 100
    "Back",                     // 101
    "Start",                    // 102
    "Options",                  // 103
    "Share",                    // 104
    "Up",                       // 105
    "Down",                     // 106
    "Left",                     // 107
    "Right",                    // 108
    "LB",                       // 109
    "LT",                       // 110
    "LSB",                      // 111
    "L1",                       // 112
    "L2",                       // 113
    "L3",                       // 114
    "RB",                       // 115
    "RT",                       // 116
    "RSB",                      // 117
    "R1",                       // 118
    "R2",                       // 119
    "R3",                       // 120
    "PaddleLeft1",              // 121
    "PaddleLeft2",              // 122
    "PaddleRight1",             // 123
    "PaddleRight2",             // 124
];

/// Label of button `button_index` on the device at `device_index`.
///
/// `IGameInputDevice` has NO `GetButtonLabel` slot — the vendored header lists
/// slots 3-17 (`vendor/GameInput.h:1155-1217`: GetDeviceInfo,
/// GetCurrentReading, GetReadingTimestamp, GetDeviceStatus, ...,
/// SendRawDeviceOutput) and none is a label getter; `GetButtonLabel` is the
/// WinRT `Windows.Gaming.Input` name (`AGENTS.md` and `docs/spec.md` use it for
/// the planned `windows`-crate path). Labels come from the device info block
/// instead: v3 `GameInputDeviceInfo.controllerInfo` (`vendor/GameInput.h:859` ->
/// `GameInputControllerInfo.controllerButtonLabels`, :692-699) and v2
/// `controllerButtonInfo` (`include/v2/GameInput.h:582-586`). v0 exposes no
/// per-button labels, so it returns `None`.
///
/// Returns `None` on non-Windows (no device to ask), for an unknown device
/// index, for a disconnected device, for an out-of-range button index, and for
/// buttons the device reports as unlabeled.
#[cfg(windows)]
pub fn button_label(device_index: usize, button_index: u8) -> Option<String> {
    gameinput::button_label(device_index, button_index)
}

/// Non-Windows stub: there is no GameInput device to ask, so no label.
#[cfg(not(windows))]
pub fn button_label(_device_index: usize, _button_index: u8) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::button_name;

    /// The face buttons carry the header's Xbox names, at the header's codes.
    #[test]
    fn button_name_xbox_face_buttons_match_header() {
        assert_eq!(button_name(7), Some("XboxA"));
        assert_eq!(button_name(8), Some("XboxB"));
        assert_eq!(button_name(9), Some("XboxX"));
        assert_eq!(button_name(10), Some("XboxY"));
        assert_eq!(button_name(1), Some("XboxGuide"));
    }

    /// Shoulder/trigger/DPad codes come straight from `GameInput.h:380-389`.
    #[test]
    fn button_name_shoulders_and_dpads_match_header() {
        assert_eq!(button_name(11), Some("XboxDPadUp"));
        assert_eq!(button_name(12), Some("XboxDPadDown"));
        assert_eq!(button_name(13), Some("XboxDPadLeft"));
        assert_eq!(button_name(14), Some("XboxDPadRight"));
        assert_eq!(button_name(15), Some("XboxLeftShoulder"));
        assert_eq!(button_name(16), Some("XboxLeftTrigger"));
        assert_eq!(button_name(18), Some("XboxRightShoulder"));
        assert_eq!(button_name(19), Some("XboxRightTrigger"));
    }

    /// Generic console labels (`GameInput.h:465-473`) name Guide and Share, the
    /// two buttons the system-button callback filter reports.
    #[test]
    fn button_name_generic_console_labels_match_header() {
        assert_eq!(button_name(96), Some("Guide"));
        assert_eq!(button_name(104), Some("Share"));
        assert_eq!(button_name(102), Some("Start"));
        assert_eq!(button_name(116), Some("RT"));
    }

    /// `Unknown` (-1), `None` (0), the header gap at 6, and anything past the
    /// last member (124) have no name.
    #[test]
    fn button_name_unknown_none_and_out_of_range_have_no_name() {
        assert_eq!(button_name(-1), None);
        assert_eq!(button_name(0), None);
        assert_eq!(button_name(6), None);
        assert_eq!(button_name(124), Some("PaddleRight2"));
        assert_eq!(button_name(125), None);
        assert_eq!(button_name(i32::MAX), None);
    }

    /// On non-Windows there is no GameInput to talk to: `run` reports
    /// `Unsupported` without touching anything, and `button_label` has no device
    /// to ask. Compiles and runs on the Linux test target.
    #[cfg(not(windows))]
    #[test]
    fn non_windows_run_reports_unsupported_and_label_is_none() {
        use super::{DeviceInfo, Error, Observer, SystemButton, button_label, run};
        use std::sync::atomic::AtomicBool;

        struct NoopObserver;
        impl Observer for NoopObserver {
            fn device_connected(&self, _device: &DeviceInfo) {}
            fn buttons_changed(&self, _device_index: usize, _bits: u64, _pressed: &[u8]) {}
            fn system_button(&self, _button: SystemButton, _down: bool, _name: &str) {}
            fn heartbeat(&self, _devices: usize, _dispatches: u64) {}
        }
        let stop = AtomicBool::new(false);
        assert_eq!(run(&NoopObserver, &stop), Err(Error::Unsupported));
        assert_eq!(button_label(0, 0), None);
    }
}
