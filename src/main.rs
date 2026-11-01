//! virshot POC — gamepad polling stage.
//! Helpers hold pure formatting/path logic; `events` holds pure event formatting;
//! `take_screenshot` (Stage 4) captures the primary monitor via xcap and is
//! driven by `on_button_pressed` on a background thread.

mod helpers {
    use chrono::{DateTime, Local};
    use std::path::PathBuf;

    /// Repo convention: temporary artifacts live in ./.tmp (git-ignored).
    /// Returned as the repo-root-relative path `./.tmp/screenshots`.
    pub fn screenshots_dir() -> PathBuf {
        PathBuf::from(".tmp/screenshots")
    }

    /// Timestamped screenshot filename, e.g. `screenshot_20261101_123456.png`.
    pub fn screenshot_filename(now: DateTime<Local>) -> String {
        now.format("screenshot_%Y%m%d_%H%M%S.png").to_string()
    }
}

mod events {
    /// Formats a button-press line: `<gamepad name>: <button> pressed`.
    /// Pure, so it is unit-testable without a real gamepad.
    pub fn format_press(gamepad_name: &str, button: &gilrs::Button) -> String {
        format!("{gamepad_name}: {button:?} pressed")
    }

    #[cfg(test)]
    mod tests {
        use super::format_press;

        #[test]
        fn format_press_includes_gamepad_name_and_button() {
            let line = format_press("Xbox 360 controller", &gilrs::Button::South);
            assert!(
                line.contains("Xbox 360 controller"),
                "gamepad name missing: {line}"
            );
            assert!(line.contains("South"), "button name missing: {line}");
            assert!(line.ends_with("pressed"), "must end with 'pressed': {line}");
            assert_eq!(line, "Xbox 360 controller: South pressed");
        }

        #[test]
        fn format_press_handles_other_buttons() {
            let line = format_press("DualShock 4", &gilrs::Button::North);
            assert!(line.contains("DualShock 4"), "gamepad name missing: {line}");
            assert!(line.contains("North"), "button name missing: {line}");
            assert_eq!(line, "DualShock 4: North pressed");
        }

        #[test]
        fn format_press_handles_unknown_button() {
            let line = format_press("Generic pad", &gilrs::Button::Unknown);
            assert!(line.contains("Generic pad"), "gamepad name missing: {line}");
            assert!(line.contains("Unknown"), "button name missing: {line}");
        }

        #[test]
        fn format_press_single_space_and_colon_layout() {
            let line = format_press("Pad", &gilrs::Button::Start);
            assert_eq!(line, "Pad: Start pressed");
        }
    }
}

/// Stage 4 hook: screenshot capture. Spawned on a background thread so the
/// polling loop is never blocked. The thread owns only the output directory —
/// no Gilrs references, nothing id-dependent — so it cannot race the loop.
/// POC: no debounce, no filename collision handling (same second overwrites).
fn on_button_pressed(_id: gilrs::GamepadId) {
    let dir = helpers::screenshots_dir();
    std::thread::spawn(move || match take_screenshot(&dir) {
        Ok(path) => println!("Saved: {}", path.display()),
        Err(err) => eprintln!("Screenshot failed: {err}"),
    });
}

/// Capture the primary monitor (index 0 fallback when `is_primary()` errors or
/// is unavailable) and save a timestamped PNG into `dir`. Every failure —
/// empty monitor list, capture error, save error — comes back as `Err(String)`
/// so callers print an error line instead of panicking.
fn take_screenshot(dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    if let Err(err) = std::fs::create_dir_all(dir) {
        return Err(format!("create dir {}: {err}", dir.display()));
    }

    let monitors = xcap::Monitor::all().map_err(|err| format!("list monitors: {err}"))?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| "no monitors found".to_owned())?;

    let image = monitor
        .capture_image()
        .map_err(|err| format!("capture image: {err}"))?;

    let path = dir.join(helpers::screenshot_filename(chrono::Local::now()));
    if let Err(err) = image.save(&path) {
        return Err(format!("save {}: {err}", path.display()));
    }
    Ok(path)
}

/// Gamepad name by id, never panicking: `Gilrs::gamepad()` unwraps internally,
/// so prefer the connected handle and fall back to the id itself.
fn gamepad_name(gilrs: &gilrs::Gilrs, id: gilrs::GamepadId) -> String {
    match gilrs.connected_gamepad(id) {
        Some(pad) => pad.name().to_owned(),
        None => format!("gamepad {:?}", id),
    }
}

fn main() {
    // Gilrs::new() returns Result: on systems where the backend cannot init we
    // report the error instead of unwrap-panicking. Having *no* gamepads is not
    // an error — gilrs stays usable and plug-and-play events still arrive.
    let mut gilrs = match gilrs::Gilrs::new() {
        Ok(gilrs) => gilrs,
        Err(err) => {
            eprintln!("Failed to initialize gamepad input: {err}");
            std::process::exit(1);
        }
    };

    println!("Listening for gamepad input... press any button to take a screenshot.");

    // Names of currently connected gamepads, kept for Disconnected reporting.
    let mut names: std::collections::HashMap<gilrs::GamepadId, String> = gilrs
        .gamepads()
        .map(|(id, pad)| (id, pad.name().to_owned()))
        .collect();

    if names.is_empty() {
        println!("No gamepads connected yet — waiting for plug-and-play...");
    } else {
        println!("Connected gamepads:");
        let mut sorted: Vec<&str> = names.values().map(String::as_str).collect();
        sorted.sort_unstable();
        for name in sorted {
            println!("  {name}");
        }
    }

    loop {
        while let Some(gilrs::Event { id, event, .. }) = gilrs.next_event() {
            match event {
                gilrs::EventType::Connected => {
                    let name = gamepad_name(&gilrs, id);
                    println!("{name} connected");
                    names.insert(id, name);
                }
                gilrs::EventType::Disconnected => {
                    let name = names
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| gamepad_name(&gilrs, id));
                    println!("{name} disconnected");
                    names.remove(&id);
                }
                gilrs::EventType::ButtonPressed(button, _data) => {
                    let name = gamepad_name(&gilrs, id);
                    println!("{}", events::format_press(&name, &button));
                    on_button_pressed(id);
                }
                _ => {}
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::helpers::{screenshot_filename, screenshots_dir};
    use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

    #[test]
    fn screenshot_filename_matches_regex() {
        // Equivalent of regex ^screenshot_\d{8}_\d{6}\.png$
        let name = screenshot_filename(Local::now());
        assert!(name.starts_with("screenshot_"), "prefix missing: {name}");
        assert!(name.ends_with(".png"), "suffix missing: {name}");
        let middle = &name["screenshot_".len()..name.len() - ".png".len()];
        let parts: Vec<&str> = middle.split('_').collect();
        assert_eq!(parts.len(), 2, "expected date_time middle: {name}");
        assert_eq!(parts[0].len(), 8, "date part must be 8 digits: {name}");
        assert_eq!(parts[1].len(), 6, "time part must be 6 digits: {name}");
        assert!(
            parts[0].chars().all(|c| c.is_ascii_digit()),
            "date part not digits: {name}"
        );
        assert!(
            parts[1].chars().all(|c| c.is_ascii_digit()),
            "time part not digits: {name}"
        );
    }

    #[test]
    fn screenshot_filename_is_deterministic_for_fixed_instant() {
        let naive = NaiveDateTime::new(
            NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            NaiveTime::from_hms_opt(12, 34, 56).unwrap(),
        );
        let now = Local.from_local_datetime(&naive).unwrap();
        assert_eq!(screenshot_filename(now), "screenshot_20261101_123456.png");
    }

    #[test]
    fn screenshots_dir_is_tmp_screenshots() {
        assert_eq!(
            screenshots_dir(),
            std::path::PathBuf::from(".tmp/screenshots")
        );
    }
}
