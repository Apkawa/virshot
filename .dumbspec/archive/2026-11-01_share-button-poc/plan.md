# 2026-11-01_share-button-poc: implementation plan (Share-button HID PoC)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

Note: the PoC crate lives in git-ignored `./.tmp/hidpad-poc/` (user decision), so code stages
have no repo commits; commits record `.dumbspec` state only. Cargo runs inside `.tmp/hidpad-poc`
must keep caches in-repo: `CARGO_HOME=/home/apkawa/code/vibe-shot/.cache/cargo cargo ...`.

---

## Stage 0 — Research + spec + plan
- [x] draft.md captured (raw request)
- [x] remapper input-path research (SDL3 HIDAPI, raw HID reports; XInput hard-caps at 11 buttons)
- [x] repo PoC context research (gilrs poll loop, `on_button_pressed` hook)
- [x] user decisions recorded: hidapi crate, `.tmp/hidpad-poc/` scratch crate, usage-page/usage device filter, cross-build here + live run on user's Windows
- [x] spec.md v1 + review; plan.md written

**Criterion:** spec.md v1 (Created + Changes) and plan.md exist; no blocking open questions except hidapi `win32` feature feasibility (resolved by Stage 2 build).
**Commit:** `docs(poc): share-button HID PoC spec + plan`

## Stage 1 — Decoder core (pure logic, TDD on Linux)
- [x] Scaffold `.tmp/hidpad-poc/`: Cargo.toml (edition 2024, `#![forbid(unsafe_code)]`), deps: `hidapi` only under `[target.'cfg(windows)'.dependencies]` — feature `win32` does not exist on `hidapi`; actual pin: `hidapi 2.6.7, default-features = false, features = ["windows-native"]` (verified via crates.io API + cargo resolver)
- [x] Red tests for `decode.rs`: wire report Share-bit index table (size<44→14, size==44→18, size==46→28, size==60→42), BT 17-byte packet (Share byte 16, Back/Guide byte 15), bit extraction `(buf[idx] & 0x01)` — 9 tests, RED (6 fail), SDL table verified against vendored source, no discrepancy
- [x] Red tests for standard-button bitmask decode: mirror SDL `SDL_hidapi_xboxone.c` `HandleStatePacket` bit layout (read the vendored SDL source as the authority, do not trust memory): D-pad, A/B/X/Y, View/Back, Start/menu, sticks, shoulders, triggers from report bytes 0-1 (+ axes sanity) — SDL masks verified (:647-697: byte0 start/back/A/B/X/Y, byte1 hat+shoulders+sticks, LE bytes 0-1); guide NOT in state packet (mode/virtual-key packets) — 11 tests added, RED (7 fail)
- [x] Verify SDL `HandleData` GIP framing (header, packet walk, command IDs GIP_CMD_INPUT/VIRTUAL_KEY/POWER_ON etc., payload offset inside raw HID report) — reconciled: header = cmd/options/seq + varint payload len (4/6-byte, rounded even), INPUT=0x20 external, VIRTUAL_KEY=0x07 internal→guide, share table keyed by **payload** length (raw offset = hdr_len+idx), init sequence bytes recorded; Bluetooth dispatch by report ID
- [x] Green: implement `decode.rs` pure functions (framing + packet payload decode → button set incl. Share/View/Guide deltas) — 46 tests green; GIP chunker + INPUT/VIRTUAL_KEY dispatch + BT report-ID dispatch; share table keyed by payload length
- [x] Refactor: tight module interfaces, no unwrap outside tests — interfaces pub(crate), clippy -D warnings clean, zero unwrap/expect anywhere in src/
- [x] Stage 1 acceptance: criterion met — Linux cargo test 46/46 green covering offset table + all button bits + framing; decoder pure (no hidapi compiled on Linux)

**Criterion:** `cargo test` (Linux native) green in `.tmp/hidpad-poc`, tests cover offset table + all button bits; decoder has no hidapi dependency.
**Commit:** N/A (git-ignored scratch crate); append verified offset table to research.md.

## Stage 2 — HID reader + PoC main (cfg(windows), cross-built here)
- [x] `reader.rs` (cfg windows): `HidApi::new()`, enumerate `device_list()`, filter Usage Page `0x01` && Usage `0x04|0x05` (vid/pid/path logged), open first match — API verified against vendored source: plain u16 usage fields, owning `open_device`, `set_blocking_mode(false)` (no `set_nonblocking`), write power-on `05 20 seq 01 00` with report-ID-prefixed retry variant
- [x] `main.rs` wiring: poll loop ~10 ms, `device.read(&mut buf)` (64 B), raw hex dump per report (Stage 3 aid), decode + transition prints (`<name> DOWN/UP`, share/guide flips), `--bluetooth` flag, disconnect → re-enumerate, errors logged, no unwrap in loop; Linux prints Windows-only notice
- [x] Cross-compile: `cargo build --target x86_64-pc-windows-gnu` via vendored hidapi `[patch.crates-io]` + offline cache (Runet mirror gitverse used for download, checksum matched lock); `windows-native` feature compiles (build.rs no-op), MinGW linker from repo config — exe built
- [x] Linux still green: `cargo check`/`cargo test` native (post-cleanup 12/12 in ./hidpad-poc: 8 layout + 4 build-stamp; root virshot cargo check exit 0)

- [x] Stage 2 acceptance: criterion met — exe `target/x86_64-pc-windows-gnu/debug/hidpad-poc.exe` (PE32+ console x86-64, 8.5 MB), Linux tests 46/46, zero unsafe in crate src (vendored hidapi's internal unsafe is third-party), zero unwrap anywhere, reader cfg-gated, loop error-tolerant

**Criterion:** windows-gnu binary `.tmp/hidpad-poc/target/x86_64-pc-windows-gnu/debug/hidpad-poc.exe` exists; Linux tests still pass; zero `unsafe` in crate source (`rg -n 'unsafe' .tmp/hidpad-poc/src` → no hits outside third-party).
**Commit:** N/A; spec.md Changes entry if hidapi feature choice changed.

## Stage 3 — Live verification on user's Windows host
- [x] User runs `hidpad-poc.exe --gameinput` with an Xbox Series X|S pad (BT, vid 0x045E pid 0x0B13); pressed buttons incl. Share and View — all arrived live
- [x] (superseded) Raw-HID/BT report route proven dead (frozen snapshot); decode.rs + vendored hidapi removed in cleanup; Share semantics proven via GameInput system-button bitmask (Share=0x2, Guide=0x1) instead
- [x] Caveats documented in hidpad-poc/README.md (Game Recording/Broadcasting steals Share; Game Bar can intercept Guide); live run saw Share+Guide fine

**Criterion:** user-confirmed live output shows Share press detected distinctly (and View + standard buttons work); offset table in research.md matches the actual device.
**Commit:** N/A (code in .tmp); research.md + spec.md updates recorded.

- [x] Dispatch: hidapi C backend switch + `--poll-ir` get_input_report probe/poll (subagent f1ee9c7e) — delivered 2026-11-02, all gates green; live run done: id=0x01 returns frozen 17B snapshot, no live data (see research.md).

- [x] Dispatch: GameInput COM shim `--gameinput` per vendored GameInput.h v3 (subagent 61efb895) — delivered 2026-11-02, gates green, unsafe confined to module.
- [x] Live: `--gameinput` on Windows host — device enumerated (v3, supportedSystemButtons=0x3), Share/Guide DOWN/UP, button mask changes; v3 layout kept (offsets fixed via 32-byte APP_LOCAL_DEVICE_ID, layout.rs tests). LT/RT are analog axes, out of scope.

- [x] Dispatch: GameInput export fix (GameInputCreate) + v0/v2/v3 negotiation shim (subagent b6f60533) — delivered 2026-11-02, gates green.

- [x] Dispatch: callback hardening (entry prints before deref, null guards, flush) (subagent fd58718e) — delivered 2026-11-02, gates green.
- [x] Dispatch: GameInputDeviceInfo offset fix (32-byte APP_LOCAL_DEVICE_ID) + layout.rs offset tests + build stamp (subagent ff3a39ad) — delivered 2026-11-02, gates green, 63 linux tests.
- [x] Dispatch: PoC cleanup — dead raw-HID route + vendored hidapi deleted, debug scaffolding trimmed, single --gameinput mode (subagent 4aba7bcb) — delivered 2026-11-02, gates green, 12 linux tests, exe 6,084,020 B.
