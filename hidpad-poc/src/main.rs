#![deny(unsafe_code)]

use std::io::Write;

/// GameInput struct layout mirrors + their header-offset tests. Compiled on
/// every platform (the Windows COM shim itself is `gameinput`, windows-only),
/// so `cargo test` on Linux checks the mirrored byte offsets.
mod layout;

#[cfg(windows)]
mod gameinput;

/// The PoC has one mode: the GameInput COM shim (`--gameinput`). Running with
/// no flags does the same thing, so the mode needs no argument dispatch.
fn main() {
    // Build stamp FIRST, flushed immediately: live runs need to know which
    // build produced the output even if GameInput aborts the process inside a
    // callback.
    print_build_stamp();

    #[cfg(windows)]
    {
        if !gameinput::run_gameinput() {
            std::process::exit(1);
        }
    }
    #[cfg(not(windows))]
    {
        println!("hidpad-poc: Windows-only PoC; run cargo test for the layout tests");
    }
}

/// Seconds since the UNIX epoch → `YYYY-MM-DD HH:MM:SS UTC` via civil-from-days
/// arithmetic (Howard Hinnant's algorithm). No calendar crate, no `unsafe`; the
/// epoch is passed in, so the conversion is testable deterministically.
fn utc_clock(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let second_of_day = secs % 86_400;
    // Shift so that the era arithmetic stays non-negative for real epochs.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 1 } / 146_097;
    let day_of_era = z - era * 146_097; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_528 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_pulse = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_pulse + 2) / 5 + 1;
    let month = if month_pulse < 10 {
        month_pulse + 3
    } else {
        month_pulse - 9
    };
    if month <= 2 {
        year += 1;
    }
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        second_of_day / 3_600,
        (second_of_day % 3_600) / 60,
        second_of_day % 60
    )
}

/// The one-line build stamp: compile-time epoch from `build.rs`
/// (`POC_BUILD_EPOCH`), human UTC time, cargo profile (`POC_BUILD_PROFILE`)
/// and short git SHA (`POC_BUILD_SHA`). `secs` is injected so tests are
/// deterministic; a non-numeric epoch falls back to 0 rather than panicking.
fn build_stamp_line(secs: u64) -> String {
    format!(
        "hidpad-poc build {secs} ({} UTC) profile={} sha={}",
        utc_clock(secs),
        env!("POC_BUILD_PROFILE"),
        env!("POC_BUILD_SHA")
    )
}

/// Print one line through the locked stdout handle and flush it, so the stamp
/// survives an abrupt process exit (GameInput can abort inside a callback).
fn print_line(line: &str) {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = writeln!(handle, "{line}");
    let _ = handle.flush();
}

/// Prints the build stamp as the first line of every invocation.
fn print_build_stamp() {
    let secs = env!("POC_BUILD_EPOCH").parse::<u64>().unwrap_or(0);
    print_line(&build_stamp_line(secs));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_clock_epoch_zero_is_new_year_midnight() {
        assert_eq!(utc_clock(0), "1970-01-01 00:00:00");
    }

    #[test]
    fn utc_clock_known_epoch_maps_to_known_utc_instant() {
        // 2026-11-02T10:00:51Z (the sample run's build time in UTC).
        assert_eq!(utc_clock(1_793_613_651), "2026-11-02 10:00:51");
    }

    #[test]
    fn utc_clock_leap_day_boundary_stays_correct() {
        assert_eq!(utc_clock(1_709_251_199), "2024-02-29 23:59:59");
        assert_eq!(utc_clock(1_709_251_200), "2024-03-01 00:00:00");
    }

    #[test]
    fn build_stamp_line_carries_epoch_utc_profile_and_sha() {
        let line = build_stamp_line(0);
        assert!(line.starts_with("hidpad-poc build 0 ("));
        assert!(line.contains("(1970-01-01 00:00:00 UTC)"));
        assert!(line.contains("profile="));
        assert!(line.contains("sha="));
    }
}
