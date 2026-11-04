//! Console tester for gamepad buttons: prints every `Observer` event to
//! stdout in the PoC's (`hidpad-poc/`) log format.
//!
//! stdout carries observer events only — the library's own diagnostics
//! (interface version, registrations, dispatch mode, failures) go to stderr
//! via its internal `note!`, so this example must not duplicate them.
//!
//! PoC line formats reproduced here:
//!   device: name=<name> vid=0x045E pid=0x0B13 usage=0x0005 supportedInput=0x1040006 supportedSystemButtons=0x3
//!   button bits=0x0001 idx=[0]
//!   [13438205497550318] <name> guide DOWN|UP   (and `share`, before `guide`)
//!   device gone: index=0 name=<name>   (extra line: the PoC dropped devices
//!   silently; the library reports the disconnect, so this example prints it)
//!   alive: devices=1, dispatch=937
//!
//! No CLI args: one mode, like the PoC. Run on Windows with
//! `cargo run --example buttons`; Ctrl+C kills the process.
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use gamepad::{DeviceInfo, Observer, SystemButton, button_label};

/// One line to locked stdout, flushed immediately — the PoC's `print_flush`:
/// GameInput can abort the process inside a callback, so buffered output would
/// be lost.
fn print_line(line: &str) {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = writeln!(handle, "{line}");
    let _ = handle.flush();
}

/// Milliseconds since the UNIX epoch. The PoC bracketed GameInput's own u64
/// callback timestamp; the `Observer` API carries no timestamp, so this example
/// stamps its own clock instead — same `[..]` brackets, different clock.
fn stamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

/// Prints every event as it arrives, on whatever thread GameInput delivers it.
struct ConsoleObserver;

impl Observer for ConsoleObserver {
    fn device_connected(&self, device: &DeviceInfo) {
        print_line(&format!(
            "device: name={} vid=0x{:04X} pid=0x{:04X} usage=0x{:04X} \
             supportedInput=0x{:X} supportedSystemButtons=0x{:X}",
            device.name,
            device.vendor_id,
            device.product_id,
            device.usage_id,
            device.supported_input as u32,
            device.supported_system_buttons as u32,
        ));
    }

    fn buttons_changed(&self, device_index: usize, bits: u64, pressed_indices: &[u8]) {
        let pressed = pressed_indices
            .iter()
            .map(|index| index.to_string())
            .collect::<Vec<String>>()
            .join(",");
        print_line(&format!("button bits=0x{bits:04X} idx=[{pressed}]"));
        // Extra lines the PoC does not print: the label of each pressed button.
        // `None` (unlabeled, v0, disconnected) prints nothing.
        for index in pressed_indices {
            if let Some(label) = button_label(device_index, *index) {
                print_line(&format!("button {index} = {label}"));
            }
        }
    }

    fn system_button(&self, button: SystemButton, down: bool, device_name: &str) {
        let kind = match button {
            SystemButton::Share => "share",
            SystemButton::Guide => "guide",
        };
        let edge = if down { "DOWN" } else { "UP" };
        print_line(&format!("[{}] {device_name} {kind} {edge}", stamp_ms()));
    }

    fn device_disconnected(&self, index: usize, name: &str) {
        // The registry entry stays (index stable); only the pointer is nulled,
        // so this device never reports again until it reconnects as a new entry.
        print_line(&format!("device gone: index={index} name={name}"));
    }

    fn heartbeat(&self, devices: usize, dispatches: u64) {
        // `dispatches` is 0 only in dispatcher-less fallback mode, where the
        // PoC prints the device count alone.
        if dispatches == 0 {
            print_line(&format!("alive: devices={devices}"));
        } else {
            print_line(&format!("alive: devices={devices}, dispatch={dispatches}"));
        }
    }
}

fn main() {
    let observer = ConsoleObserver;
    // PoC parity: nothing ever sets `stop` — `run` pumps forever and the process
    // is terminated with Ctrl+C.
    let stop = AtomicBool::new(false);

    #[cfg(not(windows))]
    {
        // GameInput is Windows-only: `run` returns `Err(Unsupported)` without
        // touching anything. Calling it keeps the observer compiled on the
        // Linux test target, where `button_label` is a stub returning `None`.
        let _ = gamepad::run(&observer, &stop);
        print_line("gameinput: not supported on this platform");
    }

    #[cfg(windows)]
    {
        if let Err(error) = gamepad::run(&observer, &stop) {
            print_line(&error.to_string());
            std::process::exit(1);
        }
    }
}
