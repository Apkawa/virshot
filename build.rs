//! Build metadata stamp (task 2026-11-10_about-dialog): two embeds.
//!
//! 1. `shadow-rs` generates `OUT_DIR/shadow.rs` (package version, git fields,
//!    build timestamps) which `shadow_rs::shadow!(build)` includes at runtime.
//!    Git fields come from the `git` CLI subprocess — the `git2` feature stays
//!    off, so no libgit2 C build on any target; missing git data leaves the
//!    constants empty.
//! 2. `cargo:rustc-env=VIRSHOT_BUILD_TIME=<ISO-8601 with +03:00>`: shadow-rs'
//!    own timestamps follow the *build host* timezone (CI runners are UTC), so
//!    the Moscow build date is computed here with std only and embedded for
//!    `about::version_line`. Failures are values: an unreadable clock emits an
//!    empty value and the runtime falls back to `build::BUILD_TIME_3339`.

fn main() {
    // `RealTime` keeps the timestamp fresh on every build: the default `Lazy`
    // pattern emits no rerun directives in debug builds, which would freeze
    // the stamped time. A generation failure ends the build with a message —
    // shipping a binary without the `build` module would only surface later
    // as a missing-file compile error.
    if let Err(err) = shadow_rs::ShadowBuilder::builder()
        .build_pattern(shadow_rs::BuildPattern::RealTime)
        .build()
    {
        eprintln!("shadow-rs metadata generation failed: {err}");
        std::process::exit(1);
    }

    println!("cargo:rustc-env=VIRSHOT_BUILD_TIME={}", moscow_build_time());

    // Re-stamp when this script changes; shadow-rs emits its own rerun
    // directives for the timestamp sources.
    println!("cargo:rerun-if-changed=build.rs");
}

/// ISO-8601 `YYYY-MM-DDTHH:MM:SS+03:00` for the current moment, std only.
/// Europe/Moscow is a fixed +03:00 offset (no DST since 2014), so adding 3 h
/// to UTC seconds is exact wall time — no timezone database needed. Returns an
/// empty string when the clock cannot be read (system time before the Unix
/// epoch); the runtime fallback (`build::BUILD_TIME_3339`) then applies.
fn moscow_build_time() -> String {
    let Ok(elapsed) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return String::new();
    };
    let moscow_seconds = elapsed.as_secs() + 3 * 3600;
    let days = moscow_seconds / 86_400;
    let seconds_of_day = (moscow_seconds % 86_400) as u32;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}+03:00",
        seconds_of_day / 3600,
        (seconds_of_day % 3600) / 60,
        seconds_of_day % 60,
    )
}

/// Days since the Unix epoch (1970-01-01) → civil (year, month, day) in the
/// proleptic Gregorian calendar. Era-free calendar-year walk (Jan 1
/// boundaries): exact for every reachable timestamp and trivially auditable —
/// a build script runs once, so the loop cost is irrelevant.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let mut year = 1970i64;
    let mut day_of_year = days;
    while day_of_year < 0 {
        year -= 1;
        day_of_year += days_in_year(year);
    }
    while day_of_year >= days_in_year(year) {
        day_of_year -= days_in_year(year);
        year += 1;
    }
    const MONTHS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = is_leap(year);
    let mut month = 0usize;
    let mut day = day_of_year;
    for (index, len) in MONTHS.iter().enumerate() {
        let len = len + (leap && index == 1) as i64; // February: +1 on leap years
        if day < len {
            month = index;
            break;
        }
        day -= len;
    }
    (year, (month + 1) as u32, (day + 1) as u32)
}

/// Gregorian leap rule: every 4th year except centuries, unless /400.
fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_year(year: i64) -> i64 {
    if is_leap(year) { 366 } else { 365 }
}
