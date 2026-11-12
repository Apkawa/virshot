//! Sibling tests for `update`: `release.yaml` metadata parsing, platform
//! target mapping, file selection, the update decision (date/version rule —
//! stage 11 sub-item 1 of task 2026-11-11_auto-update, plus the hotfix
//! 2026-11-12 guard: an equal `commit_sha` suppresses the date trigger),
//! dev-build detection, the hand-written RFC 3339 parser, check-deadline
//! scheduling, channel URL pinning, and the check orchestration behind an
//! injected fake fetcher. No network is performed anywhere:
//! `fetch_manifest_text` is only exercised by the real app, never here. Build
//! times are always injected values — the embedded shadow-rs constants are
//! never read inside the decision.

use super::*;
use crate::config::UpdateCheck;

/// The REAL `release.yaml` asset shipped with the `nightly` prerelease,
/// fetched from GitHub on 2026-11-11 and byte-identical to what
/// `scripts/package-release.sh` generates: colon syntax (YAML), quoted
/// `releaseDate`, SHORT `commit_sha`, base64 `sha512` (88 chars = base64 of
/// the raw 64-byte digest), both platform entries.
const NIGHTLY_FIXTURE: &str = r#"version: 0.1.0
releaseDate: '2026-11-10T11:29:09.928Z'
commit_sha: f710773
files:
  - url: https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-win-x64.zip
    type: x86_64-pc-windows-msvc
    sha512: 2Gh+zeeFehW+ZXL5A30Q3pjMlbu7bSX8UjFkQgPZ5/fPUaQi+30grx2xiYdvTIS1CrH6afAV6hkKT3AU2a+feg==
    size: 3244195
  - url: https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-linux-x64.tar.gz
    type: x86_64-unknown-linux-gnu
    sha512: XnvJ12rWJOuXACymadNltd5jqvPBhxrE0zJ+6FmDPEGwmDmIpce6DdvZcXGzIr+E+4po2RLpyRGvb4FdHOOg/A==
    size: 5617967
"#;

/// Same shape as [`NIGHTLY_FIXTURE`] but with a release date that cannot be
/// parsed — the decision must fail with `Parse`, never report an update.
const BAD_RELEASE_DATE_FIXTURE: &str = r#"version: 0.1.0
releaseDate: 'not-a-date'
commit_sha: f710773
files:
  - url: https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-win-x64.zip
    type: x86_64-pc-windows-msvc
    sha512: 2Gh+zeeFehW+ZXL5A30Q3pjMlbu7bSX8UjFkQgPZ5/fPUaQi+30grx2xiYdvTIS1CrH6afAV6hkKT3AU2a+feg==
    size: 3244195
  - url: https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-linux-x64.tar.gz
    type: x86_64-unknown-linux-gnu
    sha512: XnvJ12rWJOuXACymadNltd5jqvPBhxrE0zJ+6FmDPEGwmDmIpce6DdvZcXGzIr+E+4po2RLpyRGvb4FdHOOg/A==
    size: 5617967
"#;

/// Local build time NEWER than the fixture `releaseDate` (2026-11-10): the
/// user's real scenario — a nightly dated 2026-11-10 seen by a build dated
/// 2026-11-12 must stay up to date even though the nightly was republished
/// from a different commit than the local build (the sha no longer triggers).
const LOCAL_BUILD_TIME: &str = "2026-11-12T07:16:34Z";

/// Fixture release date truncated to whole seconds, for exact-equality tests:
/// `days(20767)*86400 + 11*3600 + 29*60 + 9` from the Unix epoch.
const FIXTURE_RELEASE_SECONDS: i64 = 1_794_310_149;

fn fixture_manifest() -> ReleaseManifest {
    parse_manifest(NIGHTLY_FIXTURE).expect("the real asset must parse")
}

/// Minimal manifest for the decision tests: `decide_update` never touches
/// `files`, so only `version`, `release_date`, and the sha guard matter there.
fn manifest(version: &str, release_date: &str) -> ReleaseManifest {
    manifest_sha(version, release_date, "f710773")
}

/// Same as [`manifest`] with the manifest commit sha chosen by the test — the
/// hotfix guard compares it against the build's `short_commit`.
fn manifest_sha(version: &str, release_date: &str, commit_sha: &str) -> ReleaseManifest {
    ReleaseManifest {
        version: version.to_owned(),
        release_date: release_date.to_owned(),
        commit_sha: commit_sha.to_owned(),
        files: Vec::new(),
    }
}

fn build(version: &str) -> BuildInfo {
    build_at(version, LOCAL_BUILD_TIME)
}

/// Build with a `short_commit` deliberately DIFFERENT from the fixture sha
/// `f710773`, so the existing date/version tests keep exercising the triggers
/// instead of the sha guard.
fn build_at(version: &str, build_time: &str) -> BuildInfo {
    build_at_sha(version, build_time, "abc1234")
}

fn build_at_sha(version: &str, build_time: &str, short_commit: &str) -> BuildInfo {
    BuildInfo {
        version: version.to_owned(),
        build_time: build_time.to_owned(),
        short_commit: short_commit.to_owned(),
    }
}

fn ok_fetch(text: &'static str) -> impl Fn(&str) -> Result<String, UpdateError> {
    move |_url| Ok(text.to_owned())
}

fn err_fetch(err: UpdateError) -> impl Fn(&str) -> Result<String, UpdateError> {
    move |_url| Err(err.clone())
}

#[test]
fn parse_manifest_real_nightly_fixture_parses_every_field() {
    let manifest = fixture_manifest();

    assert_eq!(manifest.version, "0.1.0");
    assert_eq!(manifest.release_date, "2026-11-10T11:29:09.928Z");
    assert_eq!(manifest.commit_sha, "f710773");
    assert_eq!(
        manifest.files.len(),
        2,
        "both platform entries must survive"
    );

    assert_eq!(
        manifest.files[0].url,
        "https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-win-x64.zip"
    );
    assert_eq!(manifest.files[0].file_type, "x86_64-pc-windows-msvc");
    assert_eq!(
        manifest.files[0].sha512,
        "2Gh+zeeFehW+ZXL5A30Q3pjMlbu7bSX8UjFkQgPZ5/fPUaQi+30grx2xiYdvTIS1CrH6afAV6hkKT3AU2a+feg=="
    );
    assert_eq!(manifest.files[0].size, 3_244_195);

    assert_eq!(
        manifest.files[1].url,
        "https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-linux-x64.tar.gz"
    );
    assert_eq!(manifest.files[1].file_type, "x86_64-unknown-linux-gnu");
    assert_eq!(
        manifest.files[1].sha512,
        "XnvJ12rWJOuXACymadNltd5jqvPBhxrE0zJ+6FmDPEGwmDmIpce6DdvZcXGzIr+E+4po2RLpyRGvb4FdHOOg/A=="
    );
    assert_eq!(manifest.files[1].size, 5_617_967);

    assert_eq!(
        manifest.files[0].sha512.len(),
        88,
        "base64 of a 64-byte digest"
    );
    assert_eq!(
        manifest.files[1].sha512.len(),
        88,
        "base64 of a 64-byte digest"
    );
}

#[test]
fn parse_manifest_malformed_body_returns_parse_error() {
    // A separator-less line is malformed YAML. serde-saphyr reports the
    // scanner error with its own position (`line 2 column 1` + a source
    // snippet), so the assertion pins the offending line as a substring
    // instead of the crate's exact wording.
    let err = parse_manifest("version: 0.1.0\nreleaseDate broken line\n")
        .expect_err("a separator-less line must not parse");
    assert!(
        matches!(&err, UpdateError::Parse(message) if message.contains("line 2")),
        "expected a Parse error naming the offending line, got {err:?}"
    );
}

#[test]
fn parse_manifest_missing_files_key_returns_parse_error() {
    let err = parse_manifest("version: 0.1.0\nreleaseDate: 'x'\ncommit_sha: abc\n")
        .expect_err("metadata without files must not parse");
    assert!(matches!(err, UpdateError::Parse(_)));
}

#[test]
fn parse_manifest_incomplete_file_entry_returns_parse_error() {
    // An entry must carry all four fields; a truncated entry would otherwise
    // ship an install without a digest or an archive URL.
    let err = parse_manifest(
        "version: 0.1.0\nreleaseDate: 'x'\ncommit_sha: abc\nfiles:\n  - url: https://host/a.zip\n    type: x86_64-pc-windows-msvc\n",
    )
    .expect_err("an entry without sha512/size must not parse");
    assert!(matches!(err, UpdateError::Parse(_)));
}

#[test]
fn parse_manifest_non_numeric_size_returns_parse_error() {
    // `size` must deserialize as `u64`: serde-saphyr rejects a non-integer
    // scalar with an invalid-u64 message naming the offending line. Assert
    // both substrings — still a type failure on the right line, without
    // pinning the crate's exact wording.
    let err = parse_manifest(
        "version: 0.1.0\nreleaseDate: 'x'\ncommit_sha: abc\nfiles:\n  - url: https://host/a.zip\n    type: t\n    sha512: AAA=\n    size: 3.2 MB\n",
    )
    .expect_err("size must be a plain integer");
    assert!(
        matches!(&err, UpdateError::Parse(message) if message.contains("line 8") && message.contains("invalid u64")),
        "expected a Parse error naming the size line, got {err:?}"
    );
}

#[test]
fn platform_file_type_host_matches_shipped_artifact_target() {
    // The shipped artifacts are built for the MSVC target on Windows (AGENTS.md
    // release delivery), so a local MinGW `-gnu` dev build must still map to
    // `x86_64-pc-windows-msvc`; Linux x86_64 maps to the shipped gnu target.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    assert_eq!(platform_file_type(), Some("x86_64-pc-windows-msvc"));
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    assert_eq!(platform_file_type(), Some("x86_64-unknown-linux-gnu"));
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
    )))]
    assert_eq!(platform_file_type(), None);
}

#[test]
fn select_file_fixture_returns_entry_for_host_target() {
    let manifest = fixture_manifest();
    let file_type = platform_file_type().expect("host target is supported in CI");

    #[cfg(target_os = "windows")]
    let expected_url = "https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-win-x64.zip";
    #[cfg(target_os = "linux")]
    let expected_url = "https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-linux-x64.tar.gz";

    let file = select_file(&manifest, file_type).expect("the fixture carries both entries");
    assert_eq!(file.url, expected_url);
    assert_eq!(file.file_type, file_type);
}

#[test]
fn select_file_unknown_target_returns_no_file_for_target() {
    // ARM64 is deferred (AGENTS.md): its type never appears in release.yaml.
    let manifest = fixture_manifest();
    let err = select_file(&manifest, "aarch64-pc-windows-msvc")
        .expect_err("a deferred target must not select an entry");
    assert_eq!(
        err,
        UpdateError::NoFileForTarget("aarch64-pc-windows-msvc".to_owned())
    );
}

// --- The update decision: release date OR version, guarded by equal sha ---

#[test]
fn decide_update_matching_version_and_older_date_returns_false() {
    // Same version, release date older than the local build time: up to
    // date. The fixture sha `f710773` does not match the injected local
    // `abc1234`, so the guard stays silent and the date path decides.
    assert!(
        !decide_update(&fixture_manifest(), &build("0.1.0"))
            .expect("a valid manifest must produce a conclusive decision")
    );
}

#[test]
fn decide_update_nightly_date_older_than_build_same_version_returns_false() {
    // The user's real case: nightly dated 2026-11-10 seen by a build dated
    // 2026-11-12 that was compiled from a DIFFERENT commit than the nightly
    // republish (fixture sha `f710773`, injected local `abc1234`): the sha
    // guard is silent on unequal shas, the date is older and the stripped
    // version is equal → up to date.
    assert!(
        !decide_update(&fixture_manifest(), &build("0.1.0-dev"))
            .expect("a valid manifest must produce a conclusive decision"),
        "a dev build dated after the release must stay up to date"
    );
}

#[test]
fn decide_update_release_build_date_older_equal_version_returns_false() {
    // Release-build half of the same rule on a synthetic manifest: release
    // dated before the build, versions equal — neither trigger fires.
    assert!(
        !decide_update(
            &manifest("0.1.0", "2026-11-01T00:00:00Z"),
            &build_at("0.1.0", "2026-11-12T00:00:00Z")
        )
        .expect("a valid manifest must produce a conclusive decision")
    );
}

#[test]
fn decide_update_date_newer_version_equal_returns_true() {
    // The date alone is a trigger, even when the manifest version matches
    // what was published before: the nightly moved forward in place
    // (re-publishes `nightly` without a version bump).
    assert!(
        decide_update(
            &fixture_manifest(),
            &build_at("0.1.0", "2026-11-09T00:00:00Z")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "a release newer than the local build must offer an update"
    );
}

#[test]
fn decide_update_higher_version_older_date_returns_true() {
    // Version alone is a trigger: manifest `0.1.0` over local `0.0.9` even
    // though the release date is older than the local build.
    assert!(
        decide_update(&fixture_manifest(), &build("0.0.9"))
            .expect("a valid manifest must produce a conclusive decision")
    );
}

#[test]
fn decide_update_lower_version_newer_date_returns_true() {
    // Accepted tradeoff (user decision): a newer release date triggers an
    // update even when the manifest version is LOWER than the local version
    // — "newer date wins" includes offering a dev build the older nightly.
    assert!(
        decide_update(
            &fixture_manifest(),
            &build_at("0.2.0", "2026-11-09T00:00:00Z")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "date alone must trigger an update over a lower version"
    );
}

#[test]
fn decide_update_same_date_and_version_returns_false() {
    // Truncating the fixture's `.928` fraction puts the release at the exact
    // same whole second as the build time: equal date, equal version → false.
    assert!(
        !decide_update(
            &fixture_manifest(),
            &build_at("0.1.0", "2026-11-10T11:29:09Z")
        )
        .expect("a valid manifest must produce a conclusive decision")
    );
}

#[test]
fn decide_update_dev_build_date_newer_returns_true() {
    // The dev special case (sha-only comparison) is gone: a `-dev` build is
    // compared on the same terms as a release build (§4.7).
    assert!(
        decide_update(
            &fixture_manifest(),
            &build_at("0.1.0-dev", "2026-11-09T00:00:00Z")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "a dev build older than the release must offer an update"
    );
}

#[test]
fn decide_update_dev_suffix_stripped_before_version_compare_returns_false() {
    // `0.1.0-dev` must compare EQUAL to the manifest's `0.1.0` (the `-dev`
    // suffix never makes the version spuriously unequal): with the release
    // date older, nothing triggers.
    assert!(
        !decide_update(
            &fixture_manifest(),
            &build_at("0.1.0-dev", "2026-11-12T00:00:00Z")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "the -dev suffix must not read as a version difference"
    );
}

#[test]
fn decide_update_dev_build_lower_version_older_date_returns_false() {
    // A dev build whose stripped version is LOWER than the manifest and whose
    // build time is newer stays up to date: neither trigger fires.
    assert!(
        !decide_update(
            &manifest("0.1.0", "2026-11-10T00:00:00Z"),
            &build_at("0.2.0-dev", "2026-11-12T00:00:00Z")
        )
        .expect("a valid manifest must produce a conclusive decision")
    );
}

#[test]
fn decide_update_offset_release_date_same_instant_as_z_returns_false() {
    // `2026-11-10T14:29:09.928+03:00` is the same instant as the fixture's
    // Z form: equal date (truncated), equal version → up to date.
    assert!(
        !decide_update(
            &manifest("0.1.0", "2026-11-10T14:29:09.928+03:00"),
            &build_at("0.1.0", "2026-11-10T11:29:09Z")
        )
        .expect("a valid manifest must produce a conclusive decision")
    );
}

#[test]
fn decide_update_offset_release_date_newer_than_build_returns_true() {
    // The offset must be applied BEFORE comparing: 14:29:09+03:00 is
    // 11:29:09 UTC, one second after a build at 11:29:08 UTC.
    assert!(
        decide_update(
            &manifest("0.1.0", "2026-11-10T14:29:09.928+03:00"),
            &build_at("0.1.0", "2026-11-10T11:29:08Z")
        )
        .expect("a valid manifest must produce a conclusive decision")
    );
}

// --- Hotfix 2026-11-12: an equal commit sha suppresses the date trigger ----

#[test]
fn decide_update_same_commit_newer_date_returns_false() {
    // The hotfix scenario: the nightly was RE-PACKAGED from the SAME commit
    // after the local build, so the manifest `releaseDate` (a packaging
    // timestamp from `package-release.sh`, not a commit timestamp) is newer
    // than the embedded build time. Equal shas mean there is nothing newer
    // to install — the date trigger must not fire.
    assert!(
        !decide_update(
            &fixture_manifest(),
            &build_at_sha("0.1.0-dev", "2026-11-09T00:00:00Z", "f710773")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "a re-published nightly from the same commit must stay up to date"
    );
}

#[test]
fn decide_update_release_sha_longer_than_local_short_sha_returns_false() {
    // Abbreviation lengths differ between `git rev-parse --short` (what CI
    // writes into the manifest) and shadow-rs `SHORT_COMMIT`: the local
    // short sha being a prefix of the manifest sha is a match, date ignored.
    assert!(
        !decide_update(
            &manifest_sha(
                "0.1.0",
                "2026-11-12T00:00:00Z",
                "f71077357be6fc6d3e8228c06de60dc07e56702f"
            ),
            &build_at_sha("0.1.0", "2026-11-09T00:00:00Z", "f710773")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "a manifest full sha must match the local abbreviated sha"
    );
}

#[test]
fn decide_update_release_sha_case_insensitive_match_returns_false() {
    // Hex case is a wire artifact, not a commit difference.
    assert!(
        !decide_update(
            &manifest_sha("0.1.0", "2026-11-12T00:00:00Z", "F710773"),
            &build_at_sha("0.1.0", "2026-11-09T00:00:00Z", "f710773")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "case differences must not split the same commit into two"
    );
}

#[test]
fn decide_update_empty_commit_sha_newer_date_returns_true() {
    // An empty sha on EITHER side is inconclusive (shadow-rs leaves git
    // fields empty without git data): the guard stays silent and the date
    // rule applies exactly as before the hotfix.
    assert!(
        decide_update(
            &manifest_sha("0.1.0", "2026-11-12T00:00:00Z", ""),
            &build_at_sha("0.1.0", "2026-11-09T00:00:00Z", "")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "empty shas on both sides must not read as a match"
    );
    assert!(
        decide_update(
            &manifest_sha("0.1.0", "2026-11-12T00:00:00Z", ""),
            &build_at_sha("0.1.0", "2026-11-09T00:00:00Z", "abc1234")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "an empty manifest sha must not suppress the date trigger"
    );
    assert!(
        decide_update(
            &manifest_sha("0.1.0", "2026-11-12T00:00:00Z", "f710773"),
            &build_at_sha("0.1.0", "2026-11-09T00:00:00Z", "")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "an empty local sha must not suppress the date trigger"
    );
}

#[test]
fn decide_update_same_commit_higher_version_returns_true() {
    // The guard suppresses the DATE only: the version trigger keeps its
    // semantics even when the shas match. (Same commit + higher version is
    // not producible by the release flow; the rule stays honest anyway.)
    assert!(
        decide_update(
            &manifest_sha("0.2.0", "2026-11-12T00:00:00Z", "f710773"),
            &build_at_sha("0.1.0-dev", "2026-11-09T00:00:00Z", "f710773")
        )
        .expect("a valid manifest must produce a conclusive decision"),
        "a higher release version must offer an update despite equal shas"
    );
}

#[test]
fn decide_update_release_date_not_a_date_returns_parse_error() {
    // An unparseable release date never becomes "update available" — even
    // when the version alone would have triggered.
    let err = decide_update(&manifest("0.2.0", "not-a-date"), &build("0.1.0"))
        .expect_err("a malformed release date must fail the decision");
    assert!(
        matches!(&err, UpdateError::Parse(message) if message.contains("not-a-date")),
        "expected a Parse error naming the offending value, got {err:?}"
    );
}

#[test]
fn decide_update_release_date_date_only_returns_parse_error() {
    // A bare date without a time part is not RFC 3339: reject, do not guess.
    assert!(matches!(
        decide_update(&manifest("0.1.0", "2026-11-10"), &build("0.1.0")),
        Err(UpdateError::Parse(_))
    ));
}

#[test]
fn decide_update_release_date_empty_returns_parse_error() {
    assert!(matches!(
        decide_update(&manifest("0.1.0", ""), &build("0.1.0")),
        Err(UpdateError::Parse(_))
    ));
}

#[test]
fn decide_update_missing_build_time_equal_version_returns_false() {
    // shadow-rs can leave build fields empty (no clock data). With no
    // comparable build time the rule falls back to version ONLY: the older
    // release date never infers an update, equal version stays up to date.
    assert!(
        !decide_update(&fixture_manifest(), &build_at("0.1.0", ""))
            .expect("version-only fallback must stay conclusive")
    );
}

#[test]
fn decide_update_missing_build_time_higher_version_returns_true() {
    assert!(
        decide_update(&fixture_manifest(), &build_at("0.0.9", ""))
            .expect("version-only fallback must stay conclusive")
    );
}

#[test]
fn decide_update_unparseable_build_time_newer_date_stays_up_to_date() {
    // Same fallback with a garbage build time: a release dated far in the
    // future cannot announce an update when the build time is unusable.
    assert!(
        !decide_update(
            &manifest("0.1.0", "2030-01-01T00:00:00Z"),
            &build_at("0.1.0", "nonsense")
        )
        .expect("version-only fallback must stay conclusive"),
        "an unusable build time must never infer an update from dates"
    );
}

#[test]
fn decide_update_version_two_digit_minor_higher_returns_true() {
    // Numeric segments, not lexicographic: `0.10.0` > `0.9.0`.
    assert!(
        decide_update(
            &manifest("0.10.0", "2026-11-10T00:00:00Z"),
            &build_at("0.9.0", "2026-11-12T00:00:00Z")
        )
        .expect("a valid manifest must produce a conclusive decision")
    );
}

#[test]
fn decide_update_version_missing_patch_counts_as_zero_returns_false() {
    // `0.1` and `0.1.0` are equal: a missing segment counts as 0.
    assert!(
        !decide_update(
            &manifest("0.1", "2026-11-10T00:00:00Z"),
            &build_at("0.1.0", "2026-11-12T00:00:00Z")
        )
        .expect("a valid manifest must produce a conclusive decision")
    );
}

#[test]
fn decide_update_garbage_release_version_older_date_returns_false() {
    // A non-numeric release segment makes the version comparison
    // inconclusive (never higher, never a panic); the older date does not
    // trigger either.
    assert!(
        !decide_update(
            &manifest("0.x.0", "2026-11-10T00:00:00Z"),
            &build_at("0.1.0", "2026-11-12T00:00:00Z")
        )
        .expect("a valid manifest must produce a conclusive decision")
    );
}

#[test]
fn decide_update_garbage_build_version_older_date_returns_false() {
    // The local side mirrors the rule: a garbage stripped version is
    // inconclusive, so nothing triggers on an older date.
    assert!(
        !decide_update(
            &fixture_manifest(),
            &build_at("0.x.0-dev", LOCAL_BUILD_TIME)
        )
        .expect("a valid manifest must produce a conclusive decision")
    );
}

// --- The hand-written RFC 3339 parser --------------------------------------

#[test]
fn parse_rfc3339_epoch_seconds_unix_epoch_anchor_returns_zero() {
    assert_eq!(
        parse_rfc3339_epoch_seconds("1970-01-01T00:00:00Z").expect("epoch anchor must parse"),
        0
    );
}

#[test]
fn parse_rfc3339_epoch_seconds_pre_epoch_date_returns_negative_seconds() {
    assert_eq!(
        parse_rfc3339_epoch_seconds("1969-12-31T00:00:00Z").expect("pre-epoch date must parse"),
        -86_400
    );
}

#[test]
fn parse_rfc3339_epoch_seconds_leap_year_february_29_adds_one_day() {
    // 2000 is a leap year (divisible by 400): Feb 29 exists, so Feb 28 →
    // Mar 1 spans two days.
    let feb_28 = parse_rfc3339_epoch_seconds("2000-02-28T00:00:00Z").expect("must parse");
    let feb_29 = parse_rfc3339_epoch_seconds("2000-02-29T00:00:00Z").expect("leap day must parse");
    let mar_1 = parse_rfc3339_epoch_seconds("2000-03-01T00:00:00Z").expect("must parse");
    assert_eq!(feb_29 - feb_28, 86_400);
    assert_eq!(mar_1 - feb_28, 172_800);
}

#[test]
fn parse_rfc3339_epoch_seconds_century_year_2100_is_not_leap() {
    // Century rule: 2100 % 400 != 0, so Feb 28 → Mar 1 spans exactly one day.
    assert_eq!(
        parse_rfc3339_epoch_seconds("2100-03-01T00:00:00Z").expect("must parse")
            - parse_rfc3339_epoch_seconds("2100-02-28T00:00:00Z").expect("must parse"),
        86_400
    );
}

#[test]
fn parse_rfc3339_epoch_seconds_fractional_seconds_truncate() {
    // All fractions of `11:29:09` map to the same whole second; `.001` of
    // the next second does not.
    let at_second =
        parse_rfc3339_epoch_seconds("2026-11-10T11:29:09.928Z").expect("fixture date must parse");
    assert_eq!(at_second, FIXTURE_RELEASE_SECONDS);
    assert_eq!(
        parse_rfc3339_epoch_seconds("2026-11-10T11:29:09.999Z").expect("must parse"),
        FIXTURE_RELEASE_SECONDS
    );
    assert_eq!(
        parse_rfc3339_epoch_seconds("2026-11-10T11:29:09Z").expect("must parse"),
        FIXTURE_RELEASE_SECONDS
    );
    assert_eq!(
        parse_rfc3339_epoch_seconds("2026-11-10T11:29:10.001Z").expect("must parse"),
        FIXTURE_RELEASE_SECONDS + 1
    );
}

#[test]
fn parse_rfc3339_epoch_seconds_positive_offset_converts_to_utc_instant() {
    // `14:29:09.928+03:00` is the same instant as the fixture Z form.
    assert_eq!(
        parse_rfc3339_epoch_seconds("2026-11-10T14:29:09.928+03:00").expect("offset must parse"),
        FIXTURE_RELEASE_SECONDS
    );
}

#[test]
fn parse_rfc3339_epoch_seconds_negative_offset_converts_to_utc_instant() {
    assert_eq!(
        parse_rfc3339_epoch_seconds("2026-11-10T08:29:09.928-03:00").expect("offset must parse"),
        FIXTURE_RELEASE_SECONDS
    );
}

#[test]
fn parse_rfc3339_epoch_seconds_missing_offset_is_utc() {
    // Embedded build timestamps are UTC RFC 3339 with no offset suffix
    // (`BUILD_TIME_3339`): a missing zone reads as UTC.
    assert_eq!(
        parse_rfc3339_epoch_seconds("2026-11-10T11:29:09.928").expect("zone-less must parse"),
        FIXTURE_RELEASE_SECONDS
    );
}

#[test]
fn parse_rfc3339_epoch_seconds_empty_text_returns_parse_error() {
    assert!(matches!(
        parse_rfc3339_epoch_seconds(""),
        Err(UpdateError::Parse(_))
    ));
}

#[test]
fn parse_rfc3339_epoch_seconds_not_a_date_returns_parse_error() {
    let err =
        parse_rfc3339_epoch_seconds("not-a-date").expect_err("a non-timestamp must not parse");
    assert!(
        matches!(&err, UpdateError::Parse(message) if message.contains("not-a-date")),
        "expected a Parse error naming the offending value, got {err:?}"
    );
}

#[test]
fn parse_rfc3339_epoch_seconds_date_only_returns_parse_error() {
    assert!(matches!(
        parse_rfc3339_epoch_seconds("2026-11-10"),
        Err(UpdateError::Parse(_))
    ));
}

#[test]
fn parse_rfc3339_epoch_seconds_invalid_month_returns_parse_error() {
    assert!(matches!(
        parse_rfc3339_epoch_seconds("2026-13-01T00:00:00Z"),
        Err(UpdateError::Parse(_))
    ));
}

// --- is_dev ----------------------------------------------------------------

#[test]
fn is_dev_dev_suffix_returns_true() {
    assert!(build("0.1.0-dev").is_dev());
}

#[test]
fn is_dev_release_version_returns_false() {
    assert!(!build("0.1.0").is_dev());
}

// --- check scheduling and channel URLs --------------------------------------

#[test]
fn next_check_deadline_never_interval_returns_none() {
    assert_eq!(next_check_deadline(Instant::now(), None), None);
    assert!(UpdateCheck::Never.interval().is_none());
}

#[test]
fn next_check_deadline_hour_interval_returns_deadline_one_hour_later() {
    let now = Instant::now();
    let deadline = next_check_deadline(now, UpdateCheck::Hour.interval())
        .expect("an hour interval must produce a deadline");
    assert_eq!(deadline.duration_since(now), Duration::from_secs(60 * 60));
}

#[test]
fn next_check_deadline_day_interval_returns_deadline_one_day_later() {
    let now = Instant::now();
    let deadline = next_check_deadline(now, UpdateCheck::Day.interval())
        .expect("a day interval must produce a deadline");
    assert_eq!(
        deadline.duration_since(now),
        Duration::from_secs(24 * 60 * 60)
    );
}

#[test]
fn next_check_deadline_week_interval_returns_deadline_one_week_later() {
    let now = Instant::now();
    let deadline = next_check_deadline(now, UpdateCheck::Week.interval())
        .expect("a week interval must produce a deadline");
    assert_eq!(
        deadline.duration_since(now),
        Duration::from_secs(7 * 24 * 60 * 60)
    );
}

#[test]
fn manifest_url_channels_are_exact_and_distinct() {
    assert_eq!(
        manifest_url(config::UpdateChannel::Stable),
        "https://github.com/Apkawa/virshot/releases/latest/download/release.yaml"
    );
    assert_eq!(
        manifest_url(config::UpdateChannel::Nightly),
        "https://github.com/Apkawa/virshot/releases/download/nightly/release.yaml"
    );
    assert_ne!(
        manifest_url(config::UpdateChannel::Stable),
        manifest_url(config::UpdateChannel::Nightly)
    );
}

// --- run_check behind the injected fetch seam -------------------------------

#[test]
fn run_check_dev_build_date_older_fetches_and_reports_up_to_date() {
    // A `-dev` build still fetches (spec §4.7); with the local build dated
    // after the release nothing triggers, republished-sha differences
    // notwithstanding.
    let calls = std::cell::Cell::new(0usize);
    let fetch = |_url: &str| {
        calls.set(calls.get() + 1);
        Ok(NIGHTLY_FIXTURE.to_owned())
    };
    let outcome = run_check(config::UpdateChannel::Nightly, &build("0.1.0-dev"), fetch);

    assert_eq!(calls.get(), 1, "a dev build must fetch release metadata");
    assert!(
        matches!(outcome, CheckOutcome::UpToDate),
        "a dev build dated after the release is up to date"
    );
}

#[test]
fn run_check_dev_build_date_newer_fetches_and_reports_updated() {
    // The dev build is dated BEFORE the release, so the date trigger fires
    // and the summary still carries the manifest values (sha stays in it).
    let calls = std::cell::Cell::new(0usize);
    let fetch = |_url: &str| {
        calls.set(calls.get() + 1);
        Ok(NIGHTLY_FIXTURE.to_owned())
    };
    let outcome = run_check(
        config::UpdateChannel::Nightly,
        &build_at("0.1.0-dev", "2026-11-09T00:00:00Z"),
        fetch,
    );

    assert_eq!(calls.get(), 1, "a dev build must fetch release metadata");
    match outcome {
        CheckOutcome::Updated(info) => {
            assert_eq!(info.version, "0.1.0");
            assert_eq!(info.commit_sha, "f710773");
        }
        other => panic!("expected Updated, got {other:?}"),
    }
}

#[test]
fn run_check_dev_build_same_commit_newer_date_reports_up_to_date() {
    // The hotfix end-to-end behind the fetch seam: the fixture nightly dated
    // 2026-11-10 seen by a dev build dated 2026-11-09 built from the SAME
    // commit `f710773` — the re-packaging date must not announce an update.
    let outcome = run_check(
        config::UpdateChannel::Nightly,
        &build_at_sha("0.1.0-dev", "2026-11-09T00:00:00Z", "f710773"),
        ok_fetch(NIGHTLY_FIXTURE),
    );

    assert!(
        matches!(outcome, CheckOutcome::UpToDate),
        "a re-published nightly from the same commit must stay up to date"
    );
}

#[test]
fn run_check_dev_build_higher_version_older_date_reports_up_to_date() {
    // A dev build ahead of the manifest (stripped version higher, dated
    // after the nightly): fetch happens, no update offered.
    let calls = std::cell::Cell::new(0usize);
    let fetch = |_url: &str| {
        calls.set(calls.get() + 1);
        Ok(NIGHTLY_FIXTURE.to_owned())
    };
    let outcome = run_check(
        config::UpdateChannel::Nightly,
        &build_at("0.2.0-dev", LOCAL_BUILD_TIME),
        fetch,
    );

    assert_eq!(calls.get(), 1, "a dev build must fetch release metadata");
    assert!(
        matches!(outcome, CheckOutcome::UpToDate),
        "a dev build ahead of the manifest is up to date"
    );
}

#[test]
fn run_check_newer_manifest_returns_updated_info() {
    // The fixture lists windows first, linux second, so the expected entry is
    // picked by target — the same way `run_check` picks it — not by a fixed
    // index, which only held on Linux.
    let fixture = fixture_manifest();
    let expected = select_file(
        &fixture,
        platform_file_type().expect("x86_64 test hosts publish an artifact"),
    )
    .expect("the fixture carries the current platform's entry");

    let outcome = run_check(
        config::UpdateChannel::Nightly,
        &build("0.0.9"),
        ok_fetch(NIGHTLY_FIXTURE),
    );

    match outcome {
        CheckOutcome::Updated(info) => {
            assert_eq!(info.version, "0.1.0");
            assert_eq!(info.release_date, "2026-11-10T11:29:09.928Z");
            // `commit_sha` stays in the dialog summary; it differs from the
            // injected build sha here, so the guard does not suppress the
            // date trigger.
            assert_eq!(info.commit_sha, "f710773");
            assert_eq!(info.archive_url, expected.url);
            assert_eq!(info.sha512, expected.sha512);
            assert_eq!(info.sha512.len(), 88);
            assert_eq!(info.size, expected.size);
        }
        other => panic!("expected Updated, got {other:?}"),
    }
}

#[test]
fn run_check_matching_build_returns_up_to_date() {
    let outcome = run_check(
        config::UpdateChannel::Nightly,
        &build("0.1.0"),
        ok_fetch(NIGHTLY_FIXTURE),
    );
    assert!(matches!(outcome, CheckOutcome::UpToDate));
}

#[test]
fn run_check_fetch_failure_returns_failed() {
    let outcome = run_check(
        config::UpdateChannel::Stable,
        &build("0.1.0"),
        err_fetch(UpdateError::HttpStatus(404)),
    );
    assert!(
        matches!(outcome, CheckOutcome::Failed(UpdateError::HttpStatus(404))),
        "a stable channel before any stable release is a 404-as-data case"
    );
}

#[test]
fn run_check_malformed_body_returns_failed_parse() {
    let outcome = run_check(
        config::UpdateChannel::Nightly,
        &build("0.1.0"),
        ok_fetch("not metadata at all"),
    );
    assert!(matches!(
        outcome,
        CheckOutcome::Failed(UpdateError::Parse(_))
    ));
}

#[test]
fn run_check_malformed_release_date_returns_failed_parse() {
    // The YAML parses fine; only the release date is unusable — the check
    // reports a Parse failure, never an update.
    let outcome = run_check(
        config::UpdateChannel::Nightly,
        &build("0.1.0"),
        ok_fetch(BAD_RELEASE_DATE_FIXTURE),
    );
    assert!(
        matches!(&outcome, CheckOutcome::Failed(UpdateError::Parse(message)) if message.contains("not-a-date")),
        "expected a Parse error naming the release date, got {outcome:?}"
    );
}

#[test]
fn spawn_check_fake_fetch_sends_updated_outcome() {
    // One plain std thread behind the seam: with the fetcher injected there is
    // no network, and the thread always sends exactly one outcome.
    let (sender, receiver) = std::sync::mpsc::channel();
    spawn_check(
        config::UpdateChannel::Nightly,
        build("0.0.9"),
        sender,
        ok_fetch(NIGHTLY_FIXTURE),
    );

    let outcome = receiver
        .recv()
        .expect("the check thread must always send an outcome");
    assert!(matches!(outcome, CheckOutcome::Updated(_)));
    assert!(
        receiver.recv().is_err(),
        "exactly one outcome must be sent per spawn"
    );
}

#[test]
fn spawn_check_fake_fetch_failure_sends_failed_outcome() {
    let (sender, receiver) = std::sync::mpsc::channel();
    spawn_check(
        config::UpdateChannel::Stable,
        build("0.1.0"),
        sender,
        err_fetch(UpdateError::Fetch("transport down".to_owned())),
    );

    let outcome = receiver
        .recv()
        .expect("the check thread must always send an outcome");
    assert!(matches!(
        outcome,
        CheckOutcome::Failed(UpdateError::Fetch(_))
    ));
}
