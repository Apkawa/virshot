//! Unit tests for `update_install` (task 2026-11-11_auto-update, Stage 5).
//! Pure logic behind seams: no network, no real self-replace. All scratch
//! files live in `./.tmp/` (repo rule for test artifacts); the shipped code
//! writes to the OS temp dir instead.

use super::*;

use std::cell::Cell;
use std::cell::RefCell;
use std::io::Cursor;
use std::io::Write;
use std::rc::Rc;

/// Real `release.yaml` values from the live nightly release (same fixture as
/// `src/update.tests.rs`), used as manifest inputs.
const WIN_SHA512: &str =
    "2Gh+zeeFehW+ZXL5A30Q3pjMlbu7bSX8UjFkQgPZ5/fPUaQi+30grx2xiYdvTIS1CrH6afAV6hkKT3AU2a+feg==";
const LINUX_SHA512: &str =
    "XnvJ12rWJOuXACymadNltd5jqvPBhxrE0zJ+6FmDPEGwmDmIpce6DdvZcXGzIr+E+4po2RLpyRGvb4FdHOOg/A==";

/// sha512 of the chunked fixture below, computed out-of-band with
/// `openssl dgst -sha512 -binary | openssl base64 -A` on the exact bytes
/// `chunked_fixture()` produces.
const CHUNKED_SHA512: &str =
    "xtFYvKD8gBYyIMOZTkd31g3UzUZFzbBl0fLB6fpueNu1iu4h2nfQJ+oqMP5HMtafN1tOZ6n9uRrt5aOOUQj0HQ==";

/// 25 bytes × 5000 = 125_000 bytes — larger than the 64 KiB chunk buffer, so
/// `sha512_of_reader` must loop more than once.
fn chunked_fixture() -> Vec<u8> {
    b"virshot-update-install\n\x00\xff".repeat(5000)
}

/// Fresh scratch dir per test (tests run in parallel, so names must differ).
fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from("./.tmp").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A REAL zip fixture (deflate, the method `zip -j` uses on Windows assets).
fn zip_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .expect("zip entry start");
        writer.write_all(data).expect("zip entry data");
    }
    writer.finish().expect("zip finish").into_inner()
}

/// One ustar tar header block, shaped like GNU tar output: NUL-padded name,
/// octal size at offset 124, typeflag at 156, `ustar\0` + version magic.
fn tar_header(name: &str, size: u64, typeflag: u8) -> [u8; 512] {
    let mut block = [0u8; 512];
    block[..name.len()].copy_from_slice(name.as_bytes());
    let octal = format!("{size:0o}\0");
    block[124..124 + octal.len()].copy_from_slice(octal.as_bytes());
    block[156] = typeflag;
    block[257..262].copy_from_slice(b"ustar");
    block[263..265].copy_from_slice(b"00");
    block
}

/// A REAL gzip-compressed tar fixture: header + padded data per entry, then
/// the two zero blocks GNU tar writes as the end-of-archive marker.
fn tar_gz_fixture(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
    let mut raw = Vec::new();
    for (name, typeflag, data) in entries {
        raw.extend_from_slice(&tar_header(name, data.len() as u64, *typeflag));
        raw.extend_from_slice(data);
        raw.resize(raw.len().div_ceil(512) * 512, 0);
    }
    raw.extend_from_slice(&[0u8; TAR_BLOCK_SIZE]);
    raw.extend_from_slice(&[0u8; TAR_BLOCK_SIZE]);
    let mut encoder =
        flate2::write::GzEncoder::new(Cursor::new(Vec::new()), flate2::Compression::default());
    encoder.write_all(&raw).expect("gzip write");
    encoder.finish().expect("gzip finish").into_inner()
}

/// Manifest-shaped `UpdateInfo` whose sha512/size describe `archive_bytes`.
fn update_info(commit_sha: &str, archive_url: &str, archive_bytes: &[u8]) -> update::UpdateInfo {
    let digest = sha512_of_reader(Cursor::new(archive_bytes.to_vec())).expect("fixture digest");
    update::UpdateInfo {
        version: "0.1.1".to_owned(),
        release_date: "2026-11-11T12:00:00.000Z".to_owned(),
        commit_sha: commit_sha.to_owned(),
        archive_url: archive_url.to_owned(),
        sha512: base64_encode(&digest),
        size: archive_bytes.len() as u64,
    }
}

/// Fake download seam: writes canned bytes where the archive should land.
fn fake_download(bytes: Vec<u8>) -> impl Fn(&str, &Path, u64) -> Result<(), InstallError> {
    move |_url: &str, dest: &Path, _expected_size: u64| {
        std::fs::write(dest, &bytes).expect("fake download write");
        Ok(())
    }
}

// ---------------------------------------------------------------- base64

#[test]
fn base64_decode_empty_input_returns_empty_vec() {
    assert_eq!(base64_decode("").expect("empty is valid"), Vec::<u8>::new());
}

#[test]
fn base64_decode_padded_hello_returns_hello_bytes() {
    assert_eq!(base64_decode("aGVsbG8=").expect("decode"), b"hello");
}

#[test]
fn base64_decode_manifest_sha512_decodes_to_64_raw_bytes() {
    assert_eq!(base64_decode(WIN_SHA512).expect("win digest").len(), 64);
    assert_eq!(base64_decode(LINUX_SHA512).expect("linux digest").len(), 64);
}

#[test]
fn base64_decode_invalid_char_rejected() {
    assert!(matches!(
        base64_decode("aGVsbG8!"),
        Err(InstallError::Base64(_))
    ));
}

#[test]
fn base64_decode_padding_inside_group_rejected() {
    assert!(matches!(
        base64_decode("aG==bG8="),
        Err(InstallError::Base64(_))
    ));
    assert!(matches!(
        base64_decode("===="),
        Err(InstallError::Base64(_))
    ));
}

#[test]
fn base64_decode_three_padding_chars_rejected() {
    assert!(matches!(
        base64_decode("a==="),
        Err(InstallError::Base64(_))
    ));
}

#[test]
fn base64_decode_length_not_multiple_of_four_rejected() {
    assert!(matches!(base64_decode("aGV"), Err(InstallError::Base64(_))));
}

#[test]
fn base64_encode_round_trips_manifest_digest() {
    let raw = base64_decode(WIN_SHA512).expect("decode");
    assert_eq!(base64_encode(&raw), WIN_SHA512);
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    assert_eq!(base64_encode(b"hi"), "aGk=");
    assert_eq!(base64_encode(b"hex"), "aGV4");
}

// ---------------------------------------------------------------- sha512

#[test]
fn sha512_of_reader_chunked_input_matches_openssl_digest() {
    let digest = sha512_of_reader(Cursor::new(chunked_fixture())).expect("digest");
    assert_eq!(digest.len(), 64);
    assert_eq!(base64_encode(&digest), CHUNKED_SHA512);
}

// ---------------------------------------------------------------- verify

#[test]
fn verify_archive_matching_size_and_digest_passes() {
    let dir = scratch_dir("verify-ok");
    let path = dir.join("archive.bin");
    std::fs::write(&path, chunked_fixture()).expect("write fixture");
    assert!(verify_archive(&path, CHUNKED_SHA512, 125_000).is_ok());
}

#[test]
fn verify_archive_extra_bytes_report_size_mismatch() {
    let dir = scratch_dir("verify-extra");
    let path = dir.join("archive.bin");
    std::fs::write(&path, chunked_fixture()).expect("write fixture");
    assert_eq!(
        verify_archive(&path, CHUNKED_SHA512, 100_000),
        Err(InstallError::SizeMismatch {
            expected: 100_000,
            actual: 125_000
        })
    );
}

#[test]
fn verify_archive_short_file_reports_size_mismatch() {
    let dir = scratch_dir("verify-short");
    let path = dir.join("archive.bin");
    std::fs::write(&path, b"hi").expect("write fixture");
    assert_eq!(
        verify_archive(&path, CHUNKED_SHA512, 10),
        Err(InstallError::SizeMismatch {
            expected: 10,
            actual: 2
        })
    );
}

#[test]
fn verify_archive_wrong_digest_reports_checksum_mismatch() {
    let dir = scratch_dir("verify-wrong-digest");
    let path = dir.join("archive.bin");
    std::fs::write(&path, chunked_fixture()).expect("write fixture");
    assert_eq!(
        verify_archive(&path, WIN_SHA512, 125_000),
        Err(InstallError::ChecksumMismatch {
            expected: WIN_SHA512.to_owned(),
            actual: CHUNKED_SHA512.to_owned(),
        })
    );
}

#[test]
fn verify_archive_invalid_base64_rejects_before_reading() {
    // The digest is decoded first, so the (missing) file never gets opened.
    assert!(matches!(
        verify_archive(Path::new("./.tmp/does-not-exist"), "not-base64!!", 0),
        Err(InstallError::Base64(_))
    ));
}

// ---------------------------------------------------------------- unpack

#[test]
fn unpack_archive_zip_extracts_bare_binary() {
    let dir = scratch_dir("unpack-zip-ok");
    let archive = dir.join("archive.zip");
    let payload: &[u8] = b"fake-pe-binary";
    std::fs::write(&archive, zip_fixture(&[("virshot.exe", payload)])).expect("write zip");
    let extracted = unpack_archive(&archive, &dir).expect("unpack");
    assert_eq!(extracted, dir.join("virshot.exe"));
    assert_eq!(std::fs::read(&extracted).expect("read extracted"), payload);
}

#[test]
fn unpack_archive_zip_rejects_traversal_and_separated_entry_names() {
    let dir = scratch_dir("unpack-zip-slip");
    let slipping = dir.join("archive.zip");
    std::fs::write(&slipping, zip_fixture(&[("../virshot.exe", b"x")])).expect("write zip");
    assert!(matches!(
        unpack_archive(&slipping, &dir),
        Err(InstallError::Unpack(_))
    ));
    let nested = dir.join("nested.zip");
    std::fs::write(&nested, zip_fixture(&[("sub/virshot.exe", b"x")])).expect("write zip");
    assert!(matches!(
        unpack_archive(&nested, &dir),
        Err(InstallError::Unpack(_))
    ));
}

#[test]
fn unpack_archive_zip_rejects_empty_archive() {
    let dir = scratch_dir("unpack-zip-empty");
    let archive = dir.join("archive.zip");
    std::fs::write(&archive, zip_fixture(&[])).expect("write zip");
    assert!(matches!(
        unpack_archive(&archive, &dir),
        Err(InstallError::Unpack(_))
    ));
}

#[test]
fn unpack_archive_zip_rejects_missing_binary_entry() {
    let dir = scratch_dir("unpack-zip-no-binary");
    let archive = dir.join("archive.zip");
    std::fs::write(
        &archive,
        zip_fixture(&[("release.yaml", b"version: 0.1.1")]),
    )
    .expect("write zip");
    assert!(matches!(
        unpack_archive(&archive, &dir),
        Err(InstallError::Unpack(_))
    ));
}

#[test]
fn unpack_archive_tar_gz_extracts_bare_binary_and_skips_directories() {
    let dir = scratch_dir("unpack-tar-ok");
    let archive = dir.join("archive.tar.gz");
    let payload: &[u8] = b"\x7fELF fake elf binary";
    std::fs::write(
        &archive,
        tar_gz_fixture(&[("build/", b'2', &[]), ("virshot", b'0', payload)]),
    )
    .expect("write tar.gz");
    let extracted = unpack_archive(&archive, &dir).expect("unpack");
    assert_eq!(extracted, dir.join("virshot"));
    assert_eq!(std::fs::read(&extracted).expect("read extracted"), payload);
}

#[test]
fn unpack_archive_tar_gz_rejects_traversal_entry() {
    let dir = scratch_dir("unpack-tar-slip");
    let archive = dir.join("archive.tar.gz");
    std::fs::write(&archive, tar_gz_fixture(&[("../virshot", b'0', b"x")])).expect("write tar.gz");
    assert!(matches!(
        unpack_archive(&archive, &dir),
        Err(InstallError::Unpack(_))
    ));
    // Rejection happens before any write: nothing lands outside out_dir.
    // (An unrelated pre-existing path there must not be mistaken for output.)
    assert!(!dir.join("../virshot").is_file());
}

#[test]
fn unpack_archive_tar_gz_rejects_missing_binary_entry() {
    let dir = scratch_dir("unpack-tar-no-binary");
    let archive = dir.join("archive.tar.gz");
    std::fs::write(&archive, tar_gz_fixture(&[("notes.txt", b'0', b"hi")])).expect("write tar.gz");
    assert!(matches!(
        unpack_archive(&archive, &dir),
        Err(InstallError::Unpack(_))
    ));
}

#[test]
fn unpack_archive_unknown_extension_rejected() {
    let dir = scratch_dir("unpack-unknown");
    let archive = dir.join("archive.bin");
    std::fs::write(&archive, b"whatever").expect("write archive");
    assert!(matches!(
        unpack_archive(&archive, &dir),
        Err(InstallError::Unpack(_))
    ));
}

// ---------------------------------------------------------------- tar header

#[test]
fn parse_tar_header_parses_name_size_and_typeflag() {
    let entry = parse_tar_header(&tar_header("virshot", 3000, b'0'))
        .expect("valid header")
        .expect("entry");
    assert_eq!(entry.name, "virshot");
    assert_eq!(entry.size, 3000);
    assert_eq!(entry.kind, b'0');
}

#[test]
fn parse_tar_header_reports_directory_typeflag() {
    let entry = parse_tar_header(&tar_header("build/", 0, b'2'))
        .expect("valid header")
        .expect("entry");
    assert_eq!(entry.kind, b'2');
    assert_eq!(entry.size, 0);
}

#[test]
fn parse_tar_header_rejects_malformed_size_field() {
    let mut block = tar_header("virshot", 0, b'0');
    block[124..128].copy_from_slice(b"zzzz");
    assert!(matches!(
        parse_tar_header(&block),
        Err(InstallError::Unpack(_))
    ));
}

#[test]
fn parse_tar_header_empty_name_is_end_of_archive() {
    assert!(
        parse_tar_header(&[0u8; TAR_BLOCK_SIZE])
            .expect("zero block")
            .is_none()
    );
}

// ---------------------------------------------------------------- install dir

#[test]
fn install_dir_hex_commit_maps_to_temp_dir_subdir() {
    assert_eq!(
        install_dir("f710773"),
        std::env::temp_dir().join("virshot-update-f710773")
    );
}

#[test]
fn install_dir_non_hex_chars_are_dropped_from_path_component() {
    // Remote manifest data becomes a path component: traversal and junk bytes
    // must not escape the temp dir. Only [0-9a-f] survives ('e' from "evil").
    let dir = install_dir("../../evil/../..");
    assert_eq!(dir, std::env::temp_dir().join("virshot-update-e"));
    assert!(dir.starts_with(std::env::temp_dir()));
}

// ---------------------------------------------------------------- leftovers

#[test]
fn cleanup_leftovers_removes_self_replace_files_and_update_dirs() {
    let exe_dir = scratch_dir("gc-exe");
    let temp_dir = scratch_dir("gc-temp");

    // Keep: the live binary and unrelated files.
    std::fs::write(exe_dir.join("virshot.exe"), b"live").expect("write live exe");
    std::fs::write(exe_dir.join("config.toml"), b"[app]").expect("write config");
    // Remove: self-replace leftovers (Windows + unix shapes).
    std::fs::write(exe_dir.join("virshot.__relocated__.exe"), b"old").expect("write relocated");
    std::fs::write(
        exe_dir.join(".virshot.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.__temp__.exe"),
        b"tmp",
    )
    .expect("write windows temp exe");
    std::fs::write(exe_dir.join(".virshot.__temp__abc123"), b"unix tmp").expect("write unix temp");
    std::fs::write(
        temp_dir.join(".virshot.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.__selfdelete__.exe"),
        b"del",
    )
    .expect("write selfdelete");
    // Remove: stale install dir from a previous run; Keep: unrelated dir.
    std::fs::create_dir(temp_dir.join("virshot-update-deadbeef")).expect("create stale dir");
    std::fs::write(temp_dir.join("virshot-update-deadbeef/archive.zip"), b"z")
        .expect("write stale archive");
    std::fs::create_dir(temp_dir.join("keep")).expect("create keep dir");
    std::fs::write(temp_dir.join("notes.txt"), b"n").expect("write notes");

    let removed = cleanup_leftovers(&exe_dir, &temp_dir);
    let mut expected = vec![
        exe_dir.join("virshot.__relocated__.exe"),
        exe_dir.join(".virshot.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.__temp__.exe"),
        exe_dir.join(".virshot.__temp__abc123"),
        temp_dir.join(".virshot.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.__selfdelete__.exe"),
        temp_dir.join("virshot-update-deadbeef"),
    ];
    expected.sort();
    assert_eq!(removed, expected);

    assert!(
        exe_dir.join("virshot.exe").exists(),
        "live binary must survive"
    );
    assert!(exe_dir.join("config.toml").exists(), "config must survive");
    assert!(!exe_dir.join("virshot.__relocated__.exe").exists());
    assert!(temp_dir.join("keep").exists(), "unrelated dir must survive");
    assert!(
        temp_dir.join("notes.txt").exists(),
        "unrelated file must survive"
    );
    assert!(!temp_dir.join("virshot-update-deadbeef").exists());
}

#[test]
fn cleanup_leftovers_missing_dirs_return_empty_and_never_fail() {
    let missing = PathBuf::from("./.tmp/gc-missing-does-not-exist");
    assert_eq!(cleanup_leftovers(&missing, &missing), Vec::<PathBuf>::new());
}

// ---------------------------------------------------------------- install orchestration

#[test]
fn install_with_success_replaces_unpacked_binary_and_removes_temp_dir() {
    let dir = scratch_dir("install-ok");
    let archive_bytes = zip_fixture(&[("virshot.exe", b"fake-pe-binary")]);
    let update = update_info(
        "f710773",
        "https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-win-x64.zip",
        &archive_bytes,
    );
    let replaced = Rc::new(RefCell::new(None::<PathBuf>));
    let recorder = Rc::clone(&replaced);
    let result = install_with(
        &update,
        &dir,
        fake_download(archive_bytes),
        |new_binary: &Path| {
            assert!(
                dir.join("archive.zip").exists(),
                "archive lands under the temp dir"
            );
            assert!(
                new_binary.exists(),
                "unpacked binary exists when replace is called"
            );
            *recorder.borrow_mut() = Some(new_binary.to_path_buf());
            Ok(())
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        replaced.borrow().as_deref(),
        Some(dir.join("virshot.exe").as_path())
    );
    assert!(!dir.exists(), "temp dir removed after success");
}

#[test]
fn install_with_replace_failure_reports_io_error_and_removes_temp_dir() {
    let dir = scratch_dir("install-replace-fails");
    let archive_bytes = zip_fixture(&[("virshot.exe", b"fake-pe-binary")]);
    let update = update_info(
        "f710773",
        "https://host/virshot-win-x64.zip",
        &archive_bytes,
    );
    let result = install_with(&update, &dir, fake_download(archive_bytes), |_path| {
        Err(std::io::Error::other("fake self-replace failure"))
    });
    assert!(matches!(result, Err(InstallError::Io(_))));
    assert!(!dir.exists(), "temp dir removed after failure");
}

#[test]
fn install_with_download_failure_propagates_and_removes_temp_dir() {
    let dir = scratch_dir("install-download-fails");
    let update = update_info("f710773", "https://host/virshot-win-x64.zip", b"irrelevant");
    let result = install_with(
        &update,
        &dir,
        |_url, _dest, _size| Err(InstallError::Download("connection reset".to_owned())),
        |_path| Ok(()),
    );
    assert_eq!(
        result,
        Err(InstallError::Download("connection reset".to_owned()))
    );
    assert!(!dir.exists(), "temp dir removed after failure");
}

#[test]
fn install_with_checksum_mismatch_propagates_and_removes_temp_dir() {
    let dir = scratch_dir("install-checksum-fails");
    let archive_bytes = tar_gz_fixture(&[("virshot", b'0', b"ELF-bytes")]);
    // Valid base64 digest of a DIFFERENT asset → typed ChecksumMismatch.
    let update = update::UpdateInfo {
        sha512: WIN_SHA512.to_owned(),
        ..update_info(
            "f710773",
            "https://host/virshot-linux-x64.tar.gz",
            &archive_bytes,
        )
    };
    let result = install_with(&update, &dir, fake_download(archive_bytes), |_path| Ok(()));
    assert!(matches!(result, Err(InstallError::ChecksumMismatch { .. })));
    assert!(!dir.exists(), "temp dir removed after failure");
}

#[test]
fn install_with_unsupported_archive_url_is_rejected_before_download() {
    let dir = scratch_dir("install-bad-url");
    let update = update_info("f710773", "https://host/asset.bin", b"irrelevant");
    let downloaded = Rc::new(Cell::new(false));
    let flag = Rc::clone(&downloaded);
    let result = install_with(
        &update,
        &dir,
        |_url, _dest, _size| {
            flag.set(true);
            Ok(())
        },
        |_path| Ok(()),
    );
    assert!(matches!(result, Err(InstallError::Unpack(_))));
    assert!(
        !downloaded.get(),
        "no download attempted for an unsupported asset URL"
    );
    assert!(!dir.exists(), "temp dir removed after failure");
}

// ---------------------------------------------------------------- real nightly asset

/// End-to-end check against the real nightly asset downloaded OUTSIDE the
/// test suite (curl into `./.tmp/nightly-linux.tar.gz`); run manually via
/// `./cargo.sh test -- --ignored`. Locks the tar.gz pipeline against the
/// live release.yaml fixture: digest, size, extracted entry.
#[test]
#[ignore]
fn nightly_asset_unpack_matches_release_manifest() {
    let archive = PathBuf::from("./.tmp/nightly-linux.tar.gz");
    assert!(
        archive.exists(),
        "download the fixture first: curl -L -o ./.tmp/nightly-linux.tar.gz \
         https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-linux-x64.tar.gz"
    );
    assert!(verify_archive(&archive, LINUX_SHA512, 5_617_967).is_ok());
    let out = scratch_dir("nightly-unpack");
    let extracted = unpack_archive(&archive, &out).expect("unpack nightly asset");
    assert_eq!(extracted, out.join("virshot"));
    assert_eq!(
        std::fs::metadata(&extracted).expect("metadata").len(),
        15_099_256
    );
}
