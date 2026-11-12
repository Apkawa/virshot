//! Release archive INSTALL (task 2026-11-11_auto-update, Stage 5): download
//! the platform archive into a temp dir, verify sha512 + size against the
//! `release.yaml` values, unpack the new binary there, hand it to the
//! self-replace seam, and garbage-collect update leftovers on every startup.
//!
//! One responsibility: the install pipeline. Metadata/decision logic stays in
//! `update`; dialog reactions in `gui` (Stage 6); startup wiring + restart in
//! `main` (Stage 7). This module RETURNS success after the replace call and
//! does not restart itself — the Stage 7 caller releases the `InstanceGuard`
//! (so the restarted process can take the lock) and spawns
//! `Command::new(std::env::current_exe())`.
//!
//! Temp-dir policy (approved spec): every artifact lives in
//! `<OS temp dir>/virshot-update-<commit_sha>/` — the archive and the unpacked
//! binary — and the whole dir is removed when `install` finishes, success or
//! failure. `commit_sha` comes from the REMOTE manifest and becomes a path
//! component, so [`install_dir`] keeps only `[0-9a-f]`; a hostile manifest
//! cannot inject separators or `..` traversal.
//!
//! Failure modes are typed (`InstallError`); nothing here panics outside
//! tests. The unit tests never hit the network and never replace a running
//! binary: `install` routes through `install_with` with injectable download
//! and replace seams.
//!
//! Leftover garbage (`cleanup_leftovers`): self-replace 1.5.1 can leave
//! `<exe dir>/.<stem>.<rand>.__relocated__.exe`,
//! `<exe dir>/.<stem>.<rand>.__temp__.exe` (Windows),
//! `<temp dir or exe dir>/.<stem>.<32 fastrand chars>.__selfdelete__.exe`,
//! and unix `<exe dir>/.<stem>.__temp__<rand>` copies; this module also drops
//! stale `<temp dir>/virshot-update-*` dirs. `NotFound` is ignored (nothing to
//! clean is success); any other I/O failure is logged and NEVER fails startup.

use std::io;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use super::UpdateInfo;

/// Same budget as the manifest fetch: an asset is ~6 MB over a slow link.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// Streaming buffer for hashing, verification, and archive copying.
const CHUNK_SIZE: usize = 64 * 1024;

/// ustar blocks are fixed 512 bytes; the tar reader relies on that.
const TAR_BLOCK_SIZE: usize = 512;

/// Release binaries are named `virshot` / `virshot.exe`; `package-release.sh`
/// archives the bare binary only, so the entry name is a plain filename.
const BINARY_PREFIX: &str = "virshot";

/// Standard base64 alphabet (RFC 4648), index = 6-bit value.
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Install failures, typed per module (`thiserror` enum). `Clone` so one
/// outcome can be logged and surfaced as a toast from the same value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InstallError {
    /// The request or the streamed body failed: DNS/TLS/timeout/transport
    /// error from ureq, or an interrupted body read. Carries the crate's
    /// message.
    #[error("failed to download release archive: {0}")]
    Download(String),

    /// The server answered a non-2xx status. Data, not a crash: a nightly
    /// asset replaced mid-flight can answer 404 on the old asset URL.
    #[error("archive download returned HTTP status {0}")]
    HttpStatus(u16),

    /// The archive digest does not match the `release.yaml` value: corrupt
    /// download, truncated write, or a manifest describing a different asset.
    /// Both values are base64 of the raw 64-byte digest (88 chars).
    #[error("archive sha512 mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    /// The archive byte count does not match `release.yaml`. More bytes than
    /// `size` abort the stream mid-download (hard upper bound); fewer bytes
    /// mean a truncated download or a stale manifest.
    #[error("archive size mismatch: expected {expected} bytes, got {actual} bytes")]
    SizeMismatch { expected: u64, actual: u64 },

    /// The archive could not be opened or unpacked: unsupported extension,
    /// malformed zip/tar structure, an unsafe entry name (`..` or a path
    /// separator), or no `virshot` binary entry.
    #[error("failed to unpack release archive: {0}")]
    Unpack(String),

    /// The `sha512` value from the manifest is not decodable base64: wrong
    /// length, an alphabet-invalid byte, or misplaced padding.
    #[error("invalid base64 digest: {0}")]
    Base64(String),

    /// A file-system step failed: creating the temp dir, writing the archive,
    /// creating the extracted binary, or the self-replace call itself.
    #[error("file system error during update install: {0}")]
    Io(String),
}

/// 6-bit value of a standard base64 character, or `None` if outside the
/// alphabet (`=` padding included — callers decide whether padding is legal).
fn base64_char_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes standard base64 (RFC 4648 alphabet, `=` padding). Length must be a
/// multiple of 4; padding is only allowed in the final group at positions 2/3.
/// Failure: [`InstallError::Base64`] on invalid length, invalid byte, or
/// misplaced/excessive padding — never a panic.
pub(crate) fn base64_decode(input: &str) -> Result<Vec<u8>, InstallError> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(InstallError::Base64(format!(
            "length {} is not a multiple of 4",
            bytes.len()
        )));
    }
    let groups = bytes.len() / 4;
    let mut out = Vec::with_capacity(groups * 3);
    for (index, group) in bytes.chunks(4).enumerate() {
        // Padding lives only in the final group; a run of 1/2 `=` is legal,
        // 3 (`a===`) or 4 (`====`) are not. In earlier groups any `=` hits the
        // alphabet check below as an invalid byte.
        let padding = if index + 1 == groups {
            group.iter().rev().take_while(|&byte| *byte == b'=').count()
        } else {
            0
        };
        if padding > 2 {
            return Err(InstallError::Base64(format!(
                "{input}: {padding} padding bytes in the final group"
            )));
        }
        let data_len = 4 - padding;
        let mut values = [0u8; 4];
        for (position, byte) in group[..data_len].iter().enumerate() {
            values[position] = base64_char_value(*byte).ok_or_else(|| {
                InstallError::Base64(format!(
                    "{input}: invalid base64 byte {byte:?} at offset {}",
                    index * 4 + position
                ))
            })?;
        }
        match data_len {
            4 => {
                out.push(values[0] << 2 | values[1] >> 4);
                out.push((values[1] & 0b1111) << 4 | values[2] >> 2);
                out.push((values[2] & 0b11) << 6 | values[3]);
            }
            3 => {
                out.push(values[0] << 2 | values[1] >> 4);
                out.push((values[1] & 0b1111) << 4 | values[2] >> 2);
            }
            2 => out.push(values[0] << 2 | values[1] >> 4),
            // Unreachable: `padding > 2` rejected anything below 2 data bytes.
            other => {
                return Err(InstallError::Base64(format!(
                    "{input}: {other} data bytes before padding"
                )));
            }
        }
    }
    Ok(out)
}

/// Standard base64 encoding WITH padding — used to render the ACTUAL digest
/// in [`InstallError::ChecksumMismatch`] in the same wire format the manifest
/// uses (`expected` and `actual` compare naturally).
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for group in data.chunks(3) {
        let packed = match group.len() {
            3 => u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]),
            2 => u32::from(group[0]) << 16 | u32::from(group[1]) << 8,
            1 => u32::from(group[0]) << 16,
            // `chunks(3)` never yields an empty slice.
            _ => 0,
        };
        out.push(BASE64_ALPHABET[(packed >> 18) as usize] as char);
        out.push(BASE64_ALPHABET[((packed >> 12) & 0b11_1111) as usize] as char);
        match group.len() {
            3 => {
                out.push(BASE64_ALPHABET[((packed >> 6) & 0b11_1111) as usize] as char);
                out.push(BASE64_ALPHABET[(packed & 0b11_1111) as usize] as char);
            }
            2 => {
                out.push(BASE64_ALPHABET[((packed >> 6) & 0b11_1111) as usize] as char);
                out.push('=');
            }
            _ => {
                out.push('=');
                out.push('=');
            }
        }
    }
    out
}

/// sha512 of everything `reader` yields, as the raw 64-byte digest. Chunked
/// read (64 KiB) so a multi-megabyte archive never lands in memory; failure:
/// [`InstallError::Io`] on a read error.
///
/// Test-facing helper only (`cfg(test)`): the production verification path in
/// [`verify_archive`] streams with a manifest SIZE bound inside its own loop,
/// so it cannot reuse an unbounded digest reader — the unit tests pin this
/// chunked streaming behavior against the `openssl` digest of the fixture.
#[cfg(test)]
pub(crate) fn sha512_of_reader(reader: impl io::Read) -> Result<Vec<u8>, InstallError> {
    use sha2::Digest;

    let mut reader = reader;
    let mut hasher = sha2::Sha512::new();
    let mut buffer = [0u8; CHUNK_SIZE];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| io_err("sha512 stream read", err))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().as_slice().to_vec())
}

/// Verifies the file at `path` against the manifest: the byte count must
/// equal `expected_size` (checked during the streaming read, so extra bytes
/// abort early), then the sha512 digest must equal `expected_sha512_base64`.
/// Failures: [`InstallError::Base64`] (undecodable digest), `Io` (unreadable
/// file), `SizeMismatch`, `ChecksumMismatch`.
pub(crate) fn verify_archive(
    path: &Path,
    expected_sha512_base64: &str,
    expected_size: u64,
) -> Result<(), InstallError> {
    use sha2::Digest;

    // Decode the expected digest FIRST: undecodable manifest data is an input
    // error and must be reported without touching the filesystem.
    let expected = base64_decode(expected_sha512_base64)?;
    let mut file = std::fs::File::open(path)
        .map_err(|err| io_err(&format!("open {}", path.display()), err))?;
    let mut hasher = sha2::Sha512::new();
    let mut buffer = [0u8; CHUNK_SIZE];
    let mut total = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| io_err(&format!("read {}", path.display()), err))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > expected_size {
            // Hard upper bound: stop reading a file bigger than the manifest.
            return Err(InstallError::SizeMismatch {
                expected: expected_size,
                actual: total,
            });
        }
        hasher.update(&buffer[..read]);
    }
    if total != expected_size {
        return Err(InstallError::SizeMismatch {
            expected: expected_size,
            actual: total,
        });
    }
    let actual = hasher.finalize().as_slice().to_vec();
    // Raw-digest comparison; the rendered `actual` uses the manifest's own
    // wire format so `expected` and `actual` read alike in the error. Plain
    // `!=`, not constant-time: this digest is from an HTTPS-fetched manifest,
    // not a secret or attacker-chosen value.
    if actual != expected {
        return Err(InstallError::ChecksumMismatch {
            expected: expected_sha512_base64.to_owned(),
            actual: base64_encode(&actual),
        });
    }
    Ok(())
}

/// The archive file name inside the install dir, derived from the manifest
/// URL: the unpack dispatch keys off the EXTENSION, so the downloaded file
/// must keep it. Failure: [`InstallError::Unpack`] for any URL that is not a
/// `.zip` / `.tar.gz` asset.
fn archive_file_name(url: &str) -> Result<String, InstallError> {
    if url.ends_with(".zip") {
        Ok("archive.zip".to_owned())
    } else if url.ends_with(".tar.gz") {
        Ok("archive.tar.gz".to_owned())
    } else {
        Err(InstallError::Unpack(format!(
            "unsupported release archive extension in {url}"
        )))
    }
}

/// Archive entry names must be plain file names inside the install dir:
/// shipped archives hold the bare binary (`zip -j` / `tar -C dir virshot`),
/// so separators and parent-dir references are never legitimate.
fn is_unsafe_entry_name(name: &str) -> bool {
    name.contains('/') || name.contains('\\') || name.contains("..")
}

/// Unpacks the release archive into `out_dir` and returns the extracted
/// binary path. Dispatch by extension: `.zip` → zip reader (Windows asset),
/// `.tar.gz` → gzip + hand-written ustar reader (Linux asset). Failures:
/// `Io` (unreadable archive), `Unpack` (unsupported extension, empty archive,
/// unsafe entry name, missing `virshot*` entry).
pub(crate) fn unpack_archive(archive_path: &Path, out_dir: &Path) -> Result<PathBuf, InstallError> {
    let file_name = archive_path.file_name().ok_or_else(|| {
        InstallError::Unpack(format!("{} has no file name", archive_path.display()))
    })?;
    let name = file_name.to_string_lossy();
    if name.ends_with(".zip") {
        unpack_zip(archive_path, out_dir)
    } else if name.ends_with(".tar.gz") {
        unpack_tar_gz(archive_path, out_dir)
    } else {
        Err(InstallError::Unpack(format!(
            "unsupported archive type {name}"
        )))
    }
}

/// Windows asset: exactly one deflate entry named `virshot.exe` (package
/// release ships bare binaries). Failures: `Io`, `Unpack` (bad zip, empty
/// archive, unsafe names, no binary entry).
fn unpack_zip(archive_path: &Path, out_dir: &Path) -> Result<PathBuf, InstallError> {
    let file = std::fs::File::open(archive_path)
        .map_err(|err| io_err(&format!("open {}", archive_path.display()), err))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| InstallError::Unpack(format!("{}: {err}", archive_path.display())))?;
    if archive.is_empty() {
        return Err(InstallError::Unpack(
            "zip archive holds no entries".to_owned(),
        ));
    }
    let mut binary: Option<String> = None;
    for entry in archive.file_names() {
        let name = entry.map_err(|err| InstallError::Unpack(format!("zip entry name: {err}")))?;
        if is_unsafe_entry_name(&name) {
            return Err(InstallError::Unpack(format!(
                "unsafe zip entry name {name}"
            )));
        }
        if binary.is_none() && name.starts_with(BINARY_PREFIX) {
            // Owned because the `file_names()` borrow ends before `by_name`
            // needs `&mut archive`.
            binary = Some(name.into_owned());
        }
    }
    let name = binary
        .ok_or_else(|| InstallError::Unpack("zip archive holds no virshot binary".to_owned()))?;
    let mut entry = archive
        .by_name(&name)
        .map_err(|err| InstallError::Unpack(format!("zip entry {name}: {err}")))?;
    let dest = out_dir.join(&name);
    let mut out_file = std::fs::File::create(&dest)
        .map_err(|err| io_err(&format!("create {}", dest.display()), err))?;
    io::copy(&mut entry, &mut out_file)
        .map_err(|err| io_err(&format!("extract zip entry {name}"), err))?;
    Ok(dest)
}

/// Linux asset: one regular entry named `virshot` (GNU tar may include
/// directory entries, which are skipped). Gzip decompression is flate2; the
/// ustar header parsing is hand-written (no tar crate). Failures: `Io`,
/// `Unpack` (truncated/garbled archive, unsafe names, unsupported entry type,
/// no binary entry).
fn unpack_tar_gz(archive_path: &Path, out_dir: &Path) -> Result<PathBuf, InstallError> {
    let file = std::fs::File::open(archive_path)
        .map_err(|err| io_err(&format!("open {}", archive_path.display()), err))?;
    let mut reader = flate2::read::GzDecoder::new(file);
    let mut binary: Option<PathBuf> = None;
    let mut block = [0u8; TAR_BLOCK_SIZE];
    loop {
        if !read_tar_block(&mut reader, &mut block)? {
            break; // EOF right at a block boundary: archive ended.
        }
        if block.iter().all(|byte| *byte == 0) {
            break; // GNU's end-of-archive zero block.
        }
        let Some(entry) = parse_tar_header(&block)? else {
            break; // Header with an empty name: archive ended.
        };
        // Entry data is padded to the next 512-byte block boundary.
        let padded_len = entry.size.div_ceil(TAR_BLOCK_SIZE as u64) * TAR_BLOCK_SIZE as u64;
        match entry.kind {
            b'2' => {
                // Directory entry: skip its (empty) data and padding.
                discard(&mut reader, entry.size)?;
                discard(&mut reader, padded_len - entry.size)?;
                continue;
            }
            b'0' => {}
            other => {
                // Symlinks, GNU long-name blocks, etc. never appear in a
                // bare-binary archive; treat them as a malformed asset.
                return Err(InstallError::Unpack(format!(
                    "unsupported tar entry type {:?} for {}",
                    other as char, entry.name
                )));
            }
        }
        if is_unsafe_entry_name(&entry.name) {
            return Err(InstallError::Unpack(format!(
                "unsafe tar entry name {}",
                entry.name
            )));
        }
        if binary.is_some() {
            return Err(InstallError::Unpack(
                "tar archive holds more than one release binary".to_owned(),
            ));
        }
        if !entry.name.starts_with(BINARY_PREFIX) {
            discard(&mut reader, entry.size)?;
            discard(&mut reader, padded_len - entry.size)?;
            continue;
        }
        let dest = out_dir.join(&entry.name);
        let mut out_file = std::fs::File::create(&dest)
            .map_err(|err| io_err(&format!("create {}", dest.display()), err))?;
        copy_exact(&mut reader, entry.size, &mut out_file)?;
        discard(&mut reader, padded_len - entry.size)?;
        binary = Some(dest);
    }
    binary.ok_or_else(|| InstallError::Unpack("tar archive holds no virshot binary".to_owned()))
}

/// One parsed ustar header block: entry type, NUL-trimmed (prefix-joined)
/// name, and octal size. Owned because the entry outlives the header buffer.
struct TarEntry {
    kind: u8,
    name: String,
    size: u64,
}

/// Parses one 512-byte ustar header block. `Ok(None)` marks the end of the
/// archive (an all-zero block); failures: [`InstallError::Unpack`] on a
/// non-UTF-8 name or a malformed octal size field.
fn parse_tar_header(block: &[u8; TAR_BLOCK_SIZE]) -> Result<Option<TarEntry>, InstallError> {
    let name = tar_field(&block[..100])?;
    if name.is_empty() {
        return Ok(None);
    }
    let size = parse_octal(&block[124..136])?;
    let kind = block[156];
    // POSIX ustar: a non-empty name prefix at 345..500 (with the `ustar`
    // magic at 257..263) is joined to the name with `/`. Our validation
    // rejects the joined name, so prefixes cannot smuggle a path.
    if tar_field(&block[257..263])? == "ustar" {
        let prefix = tar_field(&block[345..500])?;
        if !prefix.is_empty() {
            return Ok(Some(TarEntry {
                kind,
                name: format!("{prefix}/{name}"),
                size,
            }));
        }
    }
    Ok(Some(TarEntry { kind, name, size }))
}

/// A NUL-padded ustar text field: value up to the first NUL. Failure:
/// [`InstallError::Unpack`] if the bytes are not UTF-8 (base-256 encodings
/// never appear in archives built by `package-release.sh`).
fn tar_field(field: &[u8]) -> Result<String, InstallError> {
    let value = field.split(|byte| *byte == 0).next().unwrap_or(&[]);
    std::str::from_utf8(value)
        .map_err(|_| InstallError::Unpack("tar header field is not UTF-8".to_owned()))
        .map(str::to_owned)
}

/// The ustar size field (offset 124..136): octal digits, NUL/space-padded to
/// 11 bytes. GNU tar also has a base-256 form, which cannot be UTF-8 decoded
/// and therefore fails here — exactly what we want for a shipped archive.
fn parse_octal(field: &[u8]) -> Result<u64, InstallError> {
    let text = std::str::from_utf8(field)
        .map_err(|_| InstallError::Unpack("tar size field is not UTF-8".to_owned()))?;
    let trimmed = text.trim_matches(|char| char == '\0' || char == ' ');
    u64::from_str_radix(trimmed, 8)
        .map_err(|_| InstallError::Unpack(format!("malformed tar size field {text:?}")))
}

/// Reads exactly one 512-byte block. `Ok(false)` on clean EOF (zero bytes
/// read); failure: `Io` or `Unpack` on a partial block (truncated archive).
fn read_tar_block<R: io::Read>(
    reader: &mut R,
    block: &mut [u8; TAR_BLOCK_SIZE],
) -> Result<bool, InstallError> {
    let mut filled = 0usize;
    while filled < block.len() {
        let read = reader
            .read(&mut block[filled..])
            .map_err(|err| io_err("tar stream read", err))?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    if filled == 0 {
        Ok(false)
    } else if filled == block.len() {
        Ok(true)
    } else {
        Err(InstallError::Unpack(format!(
            "truncated tar header block ({filled} bytes)"
        )))
    }
}

/// Advances past `len` bytes without buffering them. Failure: `Io` or
/// `Unpack` if the stream ends early.
fn discard<R: io::Read>(reader: &mut R, len: u64) -> Result<(), InstallError> {
    let mut buffer = [0u8; CHUNK_SIZE];
    let mut remaining = len;
    while remaining > 0 {
        let want = std::cmp::min(remaining as usize, CHUNK_SIZE);
        let read = reader
            .read(&mut buffer[..want])
            .map_err(|err| io_err("tar entry skip", err))?;
        if read == 0 {
            return Err(InstallError::Unpack("truncated tar entry data".to_owned()));
        }
        remaining -= read as u64;
    }
    Ok(())
}

/// Copies exactly `len` bytes (never more — padding after an entry must stay
/// in the stream for the next header). Failures: `Io`, `Unpack` (EOF early).
fn copy_exact<R: io::Read, W: io::Write>(
    reader: &mut R,
    len: u64,
    writer: &mut W,
) -> Result<(), InstallError> {
    let mut buffer = [0u8; CHUNK_SIZE];
    let mut remaining = len;
    while remaining > 0 {
        let want = std::cmp::min(remaining as usize, CHUNK_SIZE);
        let read = reader
            .read(&mut buffer[..want])
            .map_err(|err| io_err("tar entry read", err))?;
        if read == 0 {
            return Err(InstallError::Unpack("truncated tar entry data".to_owned()));
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|err| io_err("write extracted binary", err))?;
        remaining -= read as u64;
    }
    Ok(())
}

/// Downloads `url` into `dest` (blocking ureq) and enforces the manifest
/// `size` as a HARD upper bound: more bytes than `size` → `SizeMismatch`
/// mid-stream; fewer → `SizeMismatch` at EOF. Failures: `Download` (transport
/// or body read), `HttpStatus` (non-2xx), `Io` (write). NOT exercised by unit
/// tests — no network in the test suite.
///
/// The digest is NOT compared here: it has no argument to compare against,
/// and hashing wire bytes would not catch a truncated WRITE. [`install`] runs
/// [`verify_archive`] over the file on disk right after this returns — one
/// cheap second pass over a ~6 MB archive.
pub(crate) fn download_archive(
    url: &str,
    dest: &Path,
    expected_size: u64,
) -> Result<(), InstallError> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .user_agent(format!("virshot/{}", crate::build::PKG_VERSION))
            .max_redirects(10)
            // non-2xx is data: read status, never panic
            .http_status_as_error(false)
            .build(),
    );
    let mut response = agent
        .get(url)
        .call()
        .map_err(|err| InstallError::Download(err.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(InstallError::HttpStatus(status.into()));
    }
    let mut file = std::fs::File::create(dest)
        .map_err(|err| io_err(&format!("create {}", dest.display()), err))?;
    // `Body` is not an `io::Read`; the explicit reader is (unlimited — the
    // manifest size is the bound enforced in the loop below).
    let mut body = response.body_mut().with_config().reader();
    let mut buffer = [0u8; CHUNK_SIZE];
    let mut total = 0u64;
    loop {
        let read = body
            .read(&mut buffer)
            .map_err(|err| InstallError::Download(format!("archive body read: {err}")))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > expected_size {
            return Err(InstallError::SizeMismatch {
                expected: expected_size,
                actual: total,
            });
        }
        file.write_all(&buffer[..read])
            .map_err(|err| io_err(&format!("write {}", dest.display()), err))?;
    }
    if total != expected_size {
        return Err(InstallError::SizeMismatch {
            expected: expected_size,
            actual: total,
        });
    }
    Ok(())
}

/// The install temp dir: `<OS temp dir>/virshot-update-<sanitized commit_sha>`.
/// `commit_sha` is remote data that becomes a path component, so every byte
/// outside `[0-9a-f]` is DROPPED (never rejected) — path separators, dots,
/// and control bytes cannot escape the temp dir; an all-junk sha yields the
/// plain `virshot-update-` prefix, which is still inside the temp dir.
pub(crate) fn install_dir(commit_sha: &str) -> PathBuf {
    let sanitized: String = commit_sha
        .chars()
        .filter(|char| char.is_ascii_digit() || ('a'..='f').contains(char))
        .collect();
    std::env::temp_dir().join(format!("virshot-update-{sanitized}"))
}

/// The install orchestration: make the temp dir, download the archive named
/// after the URL extension, verify size + sha512, unpack the binary, and call
/// `replace` with the unpacked path (production passes
/// `self_replace::self_replace`; tests pass a fake so the running binary is
/// untouched). The temp dir is removed best-effort on SUCCESS AND on failure.
/// Failures: `Unpack` (URL without a `.zip`/`.tar.gz` suffix — the archive
/// file name decides the unpack dispatch), `Io`, plus everything from the
/// download/verify/unpack steps. Restart is the caller's job (module header).
pub(crate) fn install(
    update: &UpdateInfo,
    replace: impl Fn(&Path) -> Result<(), io::Error>,
) -> Result<(), InstallError> {
    install_with(
        update,
        &install_dir(&update.commit_sha),
        download_archive,
        replace,
    )
}

/// The orchestration behind [`install`] with injectable seams: `dir` is the
/// install temp dir, `download` stands in for [`download_archive`] (unit
/// tests never hit the network), `replace` for `self_replace::self_replace`.
fn install_with(
    update: &UpdateInfo,
    dir: &Path,
    download: impl Fn(&str, &Path, u64) -> Result<(), InstallError>,
    replace: impl Fn(&Path) -> Result<(), io::Error>,
) -> Result<(), InstallError> {
    if let Err(err) = std::fs::create_dir_all(dir) {
        remove_dir_best_effort(dir);
        return Err(io_err(
            &format!("create install dir {}", dir.display()),
            err,
        ));
    }
    let outcome = (|| -> Result<(), InstallError> {
        let archive_path = dir.join(archive_file_name(&update.archive_url)?);
        download(&update.archive_url, &archive_path, update.size)?;
        verify_archive(&archive_path, &update.sha512, update.size)?;
        let new_binary = unpack_archive(&archive_path, dir)?;
        replace(&new_binary).map_err(|err| {
            io_err(
                &format!("self-replace {} failed", new_binary.display()),
                err,
            )
        })
    })();
    remove_dir_best_effort(dir);
    outcome
}

/// Deletes self-replace leftovers in `exe_dir` and `temp_dir` plus stale
/// `virshot-update-*` directories in `temp_dir`, and returns the paths it
/// removed (sorted, for logging). Never returns an error: a missing directory
/// (`NotFound`) is ignored, other I/O failures are logged, startup continues.
pub(crate) fn cleanup_leftovers(exe_dir: &Path, temp_dir: &Path) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    remove_leftover_files(exe_dir, &mut removed);
    remove_leftover_files(temp_dir, &mut removed);
    remove_stale_install_dirs(temp_dir, &mut removed);
    removed.sort();
    removed
}

/// Names self-replace 1.5.1 leaves behind (verified in its vendored source):
/// Windows temp/relocated copies are `.<stem>.<rand>.<suffix>.exe` in the exe
/// dir, the self-delete helper writes `.<stem>.<32 fastrand chars>
/// .__selfdelete__.exe` into the temp dir (or exe dir), unix keeps
/// `.<stem>.__temp__<rand>` next to the exe. Matching on the trailing marker
/// covers every stem/random shape, including the missing leading dot on unix.
fn is_self_replace_leftover(name: &str) -> bool {
    name.ends_with("__relocated__.exe")
        || name.ends_with("__temp__.exe")
        || name.ends_with("__selfdelete__.exe")
        || (name.starts_with('.') && name.contains("__temp__"))
}

/// Removes files matching [`is_self_replace_leftover`] in `dir`. Directories
/// and unrelated files are left alone; failures are logged, never returned.
fn remove_leftover_files(dir: &Path, removed: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return, // nothing to clean
        Err(err) => {
            tracing::warn!("cannot list {}: {err}", dir.display());
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            tracing::warn!("cannot read dir entry in {}", dir.display());
            continue;
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue; // non-UTF-8 names cannot match our ASCII patterns
        };
        if !is_self_replace_leftover(name) {
            continue;
        }
        if !entry.file_type().is_ok_and(|file_type| file_type.is_file()) {
            continue;
        }
        let path = entry.path();
        match std::fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!("could not remove leftover {}: {err}", path.display()),
        }
    }
}

/// Removes stale `<temp dir>/virshot-update-*` directories left by an
/// interrupted install. Only directories match: the install dir is the only
/// artifact named this way.
fn remove_stale_install_dirs(temp_dir: &Path, removed: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(temp_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return, // nothing to clean
        Err(err) => {
            tracing::warn!("cannot list {}: {err}", temp_dir.display());
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            tracing::warn!("cannot read dir entry in {}", temp_dir.display());
            continue;
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !name.starts_with("virshot-update-") {
            continue;
        }
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        let path = entry.path();
        match std::fs::remove_dir_all(&path) {
            Ok(()) => removed.push(path),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(
                "could not remove stale install dir {}: {err}",
                path.display()
            ),
        }
    }
}

/// `install` always tries to delete its temp dir; a failure is logged and
/// never overrides the real outcome (the install result stands on its own).
fn remove_dir_best_effort(dir: &Path) {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => tracing::warn!("could not remove update temp dir {}: {err}", dir.display()),
    }
}

/// Wraps an I/O failure into [`InstallError::Io`] with a human context.
fn io_err(context: &str, err: io::Error) -> InstallError {
    InstallError::Io(format!("{context}: {err}"))
}

#[cfg(test)]
#[path = "install.tests.rs"]
mod tests;
