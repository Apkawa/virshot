# Research: 2026-11-01_share-button-poc

Superficial context collection (dependencies, files, fixed constraints, existing solutions). Findings appended as discovered.

## User-provided findings (2026-11-01)

- **Candidate crate: `hidapi`** — safe Rust wrapper over HID; proposed by the user as the way to read Xbox button data. Likely matches the mechanism the remapper uses (pending subagent confirmation).
- **`windows` crate ruled out by constraint:** using it would require `unsafe` blocks in project code; project forbids `unsafe` (`#![forbid(unsafe_code)]` in `AGENTS.md`). `hidapi` API is safe Rust, so it fits the constraint.
- **Xbox HID input report facts (from user's sample code):**
  - Xbox pad HID: vendor ID `0x045e`; product ID varies by model — `0x0b12` (Series X|S), `0x02ea` (Xbox One), etc. So matching must accept a product-ID range/set, not a single PID.
  - Report length reveals firmware version; the Share button byte index differs: index **18** for firmware 5.0/5.1, index **22** for firmware 5.5+ (size < 48 → 18, else 22).
  - Share state = bit 0 of that byte: `(buf[idx] & 0x01) != 0`.
  - Read shape: `HidApi::new()` → `api.open(vendor, product)` → `device.set_nonblocking(true)` → poll `device.read(&mut buf)` with a short sleep when no data.
- Implication for PoC: HID report contains **more than** the 14 standard XInput buttons (View/Select, Share, and others live in extended report bytes), so the PoC can decode button bitmask from the HID report directly.

## Remapper research findings (subagent 62f5cb49, 2026-11-01) — .tmp/xbox-controller-button-remapper

- Uses vendored **SDL3 3.1.6** (git submodule libsdl-org/SDL, unmodified stock; `git status --porcelain` empty in SDL/). Links SDL3.lib/SDL3.dll (`Xbox Controller button remapper.vcxproj`:111,115).
- SDL Windows driver priority: **HIDAPI first** (SDL/src/joystick/SDL_joystick.c:51-66, default true), then RAWINPUT, then XInput/DInput; HIDAPI claiming Xbox makes XInput/RawInput skip it (SDL_joystick.c:680-701).
- **Share comes from raw HID input reports** (SDL/src/joystick/hidapi/SDL_hidapi_xboxone.c):
  - `SDL_GAMEPAD_BUTTON_XBOX_SHARE_BUTTON = 11` (:52-55); gated by `ControllerHasShareButton()` = Xbox Series X VID/PID (:173-176; `usb_ids.h:146` PID 0x0b12).
  - Wired offsets depend on report size (:703-725): size<44 → `data[14]&0x01`; size==44 → `data[18]`; size==46 → `data[28]`; size==60 → `data[42]`.
  - Bluetooth 17-byte packet (:888 comment, :912): Share = `data[16]&0x01` (firmware 5.5.2641: BACK+GUIDE in byte 15, SHARE byte 17).
- **Back/View** is present in the HID report (wired: byte 0 bit 0x08, :664-665; Bluetooth: byte 15 bit 0x04, :917-921). Remapper maps only MISC1+GUIDE (main.cpp:464-465); View unmapped there but decodable from the same report.
- **Guide/Xbox button**: XInput bit `0x0400` (SDL/src/core/windows/SDL_xinput.h:133-134; SDL_xinputjoystick.c:341-344). Via HID: GIP `GIP_CMD_VIRTUAL_KEY` packet (:1427 → :853-858); Bluetooth guide packet (:1057-1063). Windows Game Bar intercepts Guide unless disabled.
- **XInput hard cap**: `joystick->nbuttons = 11` (SDL_xinputjoystick.c:277-280) → Share invisible via XInput; consistent with project's `gilrs xinput` gap.
- Poll loop shape: Win32 message loop + `while (SDL_PollEvent(...))` full drain, `Sleep(50)` ≈ 20 Hz; HID reads non-blocking (`SDL_hid_read_timeout(..., 0)`, :1585). Keyboard injection via `keybd_event` (main.cpp:826-836) — not needed for our PoC.
- **HID init needed for full-state reports**: SDL sends power-on output packet `0x05 0x20 0x00 0x01` (:180-183) + identify/prepare sequence (:44-45). HID open on Windows is shared, not exclusive.
- OS interception caveats: Windows "Game Recording and Broadcasting" steals Share; "Open Game Bar using Xbox button" steals Guide (README.md:12-22).
- SDL's HID transport **is** hidapi (SDL/src/hidapi/windows/hid.c: `HidD_GetHidGuid`, `CreateFileW`, `HidP_GetCaps`, overlapped `ReadFile`) → Rust `hidapi` crate mirrors exactly this path; the `windows` crate route would reimplement SetupAPI enumeration + overlapped reads → requires `unsafe` → forbidden.
- **Offset discrepancy (open question):** user's snippet (size<48 → idx 18, else 22) vs SDL's table (32/40/44/46/60 → 14/18/28/42; BT → 16). Empirical verification on the actual controller/firmware required.

## Decision + platform constraint (user, 2026-11-01)

- **Decision: PoC via `hidapi` crate** (user confirmed: "делаем poc через hidapi"). `windows` crate variant dropped (needs `unsafe` blocks → violates `#![forbid(unsafe_code)]`).
- Confirmed platform constraint: hidden buttons are not an Xbox-controller quirk — **XInput spec itself** (Xbox 360-era fixed button set) has no Share / back paddles / extra buttons; any controller presenting as XInput cannot report them (vendor statements: 8BitDo FAQ — Share doesn't work on Windows 10, behaves like original Xbox pads; GameSir FAQ — XInput over Bluetooth unsupported due to Microsoft restrictions). DirectInput-mode pads can expose more buttons but game compatibility suffers.
- Implication: raw HID report decoding via `hidapi` is the reliable route to all physical buttons; mirrors what stock SDL3 HIDAPI driver does (see remapper findings above).

## Repo PoC context (subagent eb441d54, 2026-11-01)

- `gilrs = { version = "0.11.2", default-features = false, features = ["xinput"] }` (Cargo.toml:7); deliberate backend pin (AGENTS.md). Only gilrs, xcap, chrono actually used in src; eframe/egui/clap/tray-icon/serde/toml/tracing/thiserror declared but unreferenced.
- Single .rs file: `src/main.rs` (219 lines). PoC loop: `loop { while let Some(gilrs::Event{..}) = gilrs.next_event() }` (:147-148) + `sleep(10ms)` (:172); handles Connected (:150), Disconnected (:155), ButtonPressed (:163) — **any** button from **any** gamepad triggers `on_button_pressed(id)` (:72) → spawned thread `take_screenshot()` → PNG to `.tmp/screenshots/...`. No channel/abstraction today.
- Extension point: `on_button_pressed` (:72) is the natural common action entry (ignores id); `events::format_press` (:25) is gilrs-typed (gilrs-specific formatting only); `take_screenshot` is input-agnostic.
- Tests: unit tests only inside src/main.rs (`events::tests`, filename tests); no `tests/` dir, no E2E yet; single binary `virshot`.
- Build: `./cargo.sh` wrapper (CARGO_HOME=./.cache/cargo); pre-commit gate via .rusty-hook.toml; windows-gnu MinGW linker configured in .cargo/config.toml; target installed; build success unverified.
- docs/spec.md facts: MVP §: «Прямое чтение сигналов геймпада (без эмуляции клавиатуры)» (:19); §4.1 input thread via gilrs (:64-65); per-action bind fields store one gamepad button (:68-74); config draft reserves `pad = "Share"` (:191,:196); §6 threading: Input Thread gilrs → command channel (:163); debounce 250 ms (:77, unimplemented).

## Device identification via HID usage (user info, 2026-11-01)

- Prefer **Usage Page / Usage** over VID/PID lists: `DeviceInfo::usage_page()` / `usage()` from `api.device_list()` in `hidapi`; game controllers = Usage Page `0x01` (Generic Desktop), Usage `0x04` (Joystick) or `0x05` (Game Pad). Reliable and standardized; VID/PID stay as secondary exact-model filter (VID `0x045e` = Microsoft).
- Caveat: `usage()/usage_page()` may be unavailable/zero on the Linux libusb backend; on Windows backend they are populated. PoC filtering runs on Windows only (module is cfg(windows)), so fine.
- Report descriptor parsing deemed unnecessary for this PoC (usage pair suffices).

## hidapi crate facts (subagent 3bc2dbed, 2026-11-01)

- `hidapi` crate latest stable = **2.6.7**; feature list (crates.io API + cargo resolver error): default, linux-native*, linux-shared/static-{hidraw,libusb}, macos-shared-device, **`windows-native`** — there is NO `win32` and NO `native` feature (those belong to the separate crate `hidapi-rs`). PoC uses `windows-native` (pulls windows-sys 0.61.2 Win32_* APIs; pure Rust, no libhidapi native lib needed).
- Scaffold verified: `cargo check`/`cargo test` green on Linux with `[target.'cfg(windows)'.dependencies] hidapi = { default-features = false, features = ["windows-native"] }` (Linux validates cfg(windows) features but does not compile them). Windows stages must build `--target x86_64-pc-windows-gnu` with this feature set.

## GIP framing concern (subagent 6df43b34, 2026-11-01)

- SDL state-button masks are **raw-report bytes 0-1 little-endian** (`HandleStatePacket` :647, dispatched from `GIP_CMD_INPUT` :1462): byte0 — start=0x04, back/view=0x08, A=0x10, B=0x20, X=0x40, Y=0x80 (bits 0-1 = no state button); byte1 — D-pad hat 0x01/0x02/0x04/0x08 (:674-686), shoulders 0x10/0x20 (:688-695; Razer Atrox swaps them), stick clicks 0x40/0x80 (:696-697).
- **Guide is NOT in the state packet**: wired via `HandleModePacket` :857 (`data[0]&0x01`), Bluetooth via `HandleGuidePacket` :1062 / `data[15]&0x10` :903. PoC must parse those packet types separately to see Guide.
- **Open concern**: `HandleStatePacket` receives *packet payload*, and the share offsets (14/18/28/42) are relative to that payload; a raw hidapi `read()` returns the whole GIP chunk report (header + command + payload). Need SDL `HandleData` framing verified (offset of state payload inside raw report, command IDs, packet walk) before GREEN — tracked as a Stage 1 step.

## SDL GIP framing recipe (subagent ee13a3f0, 2026-11-01) — authoritative port notes

- Wired entry: `UpdateDevice` (:1572-1634), 64-byte buffer, `SDL_hid_read_timeout(...,0)` loop (:1585); chunker `HIDAPI_GIP_ProcessData` (:1550-1570): while size>3 → decode header (:1300-1316), clamp payload (:1557-1560), process packet at `data+hdr_len` with `size = packet_length` (:1562), advance.
- Header: `[0]=command, [1]=options, [2]=sequence`, then **7-bit varint** payload length (:1234-1248); if `options & 0x80` (CHUNK) another varint chunk_offset; header len = 3+varint bytes **rounded up to even** → 4 bytes for payload<128, 6 bytes for ≥128.
- Commands (numeric, :1170-1187): ACK 0x01, ANNOUNCE 0x02, STATUS 0x03, IDENTIFY 0x04, POWER 0x05, AUTH 0x06, **VIRTUAL_KEY 0x07**, AUDIO 0x08, RUMBLE 0x09, LED 0x0A, HID_REPORT 0x0B, UNMAPPED_STATE 0x0C, SERIAL 0x1E, **INPUT 0x20**. Options: ACK 0x10, INTERNAL 0x20, CHUNK_START 0x40, CHUNK 0x80 (:1190-1193).
- Dispatch (:1374-1478): skip if `options & 0x0F != 0`; internal (`0x20`): **0x07→HandleModePacket → guide = payload[0]&0x01** (:1427,:857); external: **0x20→HandleStatePacket** (:1462). Share table applies to **payload length**, not raw report length (comment "32/40/44/46/60" = payload sizes): payload<44→p[14], 44→p[18], 46→p[28], 60→p[42]; raw offset = hdr_len(4) + idx.
- State payload layout (:662-697 + axes/triggers): bytes 0-1 button mask (LE); triggers u16 LE at p[2..4]/p[4..6] scaled ×64−32768; sticks i16 LE at p[6..14] (ly/ry inverted). Existing decode tests stay valid if semantics renamed payload_len (same numbers 32/40/44/46/60 are valid payload lengths).
- Wired init (SDL_hid_write verbatim; seq byte[2] increments 1..255): power-on `05 20 seq 01 00`; LED `0A 20 seq 03 00 01 14`; security `06 20 seq 02 01 00`; rumble `09 00 seq 09 00 0F 00 00 00 00 FF 00 EB`; then after ANNOUNCE reply send identify `04 20 seq 00`. Windows: SDL suppresses acks (:1346-1351).
- hidapi Windows read **strips leading 0x00 report ID** (hid.c:1385-1393); writes padded to OutputReportByteLength (:1273-1283). So wired decode expects byte0 == GIP command.
- Bluetooth framing (:1589-1620): dispatch raw `data[0]` report ID: 0x01 state (size>=16; buttons bytes 14/15/16 raw-incl-ID, hat@13, axes u16 LE−0x8000 at 1..8, LT@9 RT@11; share byte 16 bit 0x01 when has_share, else folds into BACK; paddles for sizes 55/39/20), 0x02 guide (`data[1]&0x01`; suppresses byte-15 guide bit 0x10 when guide packets seen), 0x04 battery.

## Stage 1 green result (subagent d542bb8a, 2026-11-01)

- decode.rs implemented: GIP chunker (varint payload len; CHUNK packets skipped w/ chunk_offset consumed — PoC limitation), dispatch INPUT 0x20 → mask payload[0..1] LE + share by **payload** length table (17→16 first; 20..44→14; 44→18; 46→28; 60→42 — bare <44 narrowed to ≥20 so degenerate lengths stay None), VIRTUAL_KEY 0x07 internal → guide = payload[0]&0x01; BT by report ID (0x01 state ≥16 incl. HandleButtons16 layout @:866-878, hat@13→dpad bits, share byte16; 0x02 guide byte1&0x01; 0x04 ignored). Header length = raw decoded length (SDL even-pad is encode-only; pad byte is a consumed continuation bit).
- Gates Linux: cargo test 46 passed / 0 failed; cargo check zero warnings; fmt --check clean; clippy -D warnings clean; no unsafe/unwrap outside tests.
- Sandbox network: `cargo fetch --target x86_64-pc-windows-gnu` hangs (crates.io unreachable from bash; 25s timeout hit) and hidapi 2.6.7 is NOT in .cache/cargo. Windows cross-build gate cannot run here without network — fallback: build/run on the user's Windows host (cargo build nthere or pre-fetch there). Stage 2 must attempt fetch first (longer timeout), else mark build verification as user-side.

## Stage 1 ACCEPTED (verification subagent 9cf1cc71, 2026-11-01)

All 8 acceptance items PASS: cargo test 46/46; check --all-targets clean; fmt clean; clippy -D warnings clean; rg 'unsafe' → only the forbid attribute in main.rs; zero unwrap/expect anywhere in src/; no hidapi use in decode.rs (only doc refs); main.rs = forbid + `mod decode;` + empty main. Coverage confirmed: offsets 14/16/18/28/42 + unknown→None, bit0 extraction, LE mask, 14 button names, wired framing (INPUT/VIRTUAL_KEY/multi/chunk-skip/clamp/trailing), BT (17/16-byte state, guide, battery, unknown ID).

## Stage 2 network path resolved (2026-11-01)

- Sandbox whitelist = Runet-only; harness proxy DNS down (web_fetch broken for all hosts). User-provided mirror works from bash: `https://crates-mirror.gitverse.ru/crates/<name>/<ver>/download` mirrors static.crates.io paths (probe 200; api/index paths 404).
- hidapi 2.6.7 .crate downloaded via curl, sha256 `818c0e1d27887…` matches Cargo.lock checksum; unpacked to `.tmp/hidpad-poc/vendor/hidapi/` (verbatim: Cargo.toml, src/, build.rs, LICENSE). `windows-native` feature = windows-sys feature subset (:92-100), all deps cached. `[patch.crates-io] hidapi = { path = "vendor/hidapi" }` to be added in Stage 2.
- Stage 2 subagent 14a230f2 halted cleanly at vendoring (no code written); baseline Linux 46/46 green preserved. MinGW present (`x86_64-w64-mingw32-gcc`), repo `.cargo/config.toml` linker applies via upward discovery.

## Stage 2 implementation result (subagent 14a230f2 resumed, 2026-11-01)

- Vendored hidapi 2.6.7 used via `[patch.crates-io] path = vendor/hidapi`; offline resolution OK (all transitive deps cached; build.rs no-op under windows-native — no C toolchain).
- API corrections vs earlier assumption (verified in vendor/hidapi/src): `DeviceInfo::usage_page()/usage()` are PLAIN u16 (not Option); `info.open_device(&api)` returns owning `HidDevice`; no `set_nonblocking(true)` — use `set_blocking_mode(false)` (windows_native read = read_timeout(0), Ok(0) on empty); write/read return HidResult<usize>.
- reader.rs (cfg windows): enumerate + filter usage_page 0x01 / usage 0x04|0x05, log candidates, open first, set_blocking_mode(false), init write `[05 20 seq 01 00]` seq=1, retry `[00 05 20 seq 01 00]` on Err (report-ID-prefixed variant), log which succeeded; zero unwrap.
- main.rs: `--bluetooth` flag (default Wired); re-enumerate loop 1 s on failure; 10 ms read loop; raw hex dump `report[N]: …` (max 32 bytes) for Stage 3 reconciliation; PadFrame transitions printed as `<name> DOWN/UP` + share/guide flips; Linux prints notice, exit 0.
- Gates: windows-gnu check/build green → `.tmp/hidpad-poc/target/x86_64-pc-windows-gnu/debug/hidpad-poc.exe` (PE32+ console x86-64, ~8.5 MB debug, MinGW via repo .cargo/config.toml); Linux check/test 46/46/clippy/fmt green; clippy clean on both targets; rg unsafe → only forbid attribute.

## Stage 2 ACCEPTED (verification subagent 840f1449, 2026-11-01)

All 8 items PASS: exe PE32+ console x86-64 8,531,053 B; Linux 46/46; windows-gnu offline check 0; unsafe only forbid-attr in src/ (vendored hidapi 101 unsafe = third-party); zero unwrap/expect anywhere; reader cfg(windows)-gated, Linux build excludes hidapi; loop error-tolerant (re-enumerate 1s, raw+ID-prefixed init fallback); fmt clean. Stage 3 next: user live run.

## Stage 3 live findings (user run 2026-11-01 22:24)

- Device found by filter: `vid=045e pid=0b13 usage=0x05 usage_page=0x01`, path `HID#...&VID_045e&PID_0b13&REV_0509&...IG_00` — Xbox Wireless Controller via **Bluetooth LE** (PnP: BTHLE\DEV_..., Class Bluetooth, service BthLEEnum). No wired device present. pid 0x0b13 = Xbox Series X|S BT; SDL BT pids include 0x0a42 (Xbox One BT). Device stays enumerated when pad is off (BLE enumeration persists).
- Init write fails BOTH variants: os error **1784 = ERROR_INVALID_BUFFER_SIZE** — Windows hid_write requires buffer length == device OutputReportByteLength; BT HID device has different/no matching output report. SDL sends GIP init packets only on wired devices; BT controllers report without init. Fix: init failure must be a warning, not fatal; BT transport must skip init.
- Consequence: open aborted before any read → no hex dump yet. Fix plan: transport auto-detect from vid/pid (BT pids → Bluetooth), init write only for Wired, continue on init error (warn).

## BT transport fix (subagent 058b1989, 2026-11-01 22:40)

- SDL BT pids pinned: `SDL_joystick.c:2991-3005` — vid 0x045e, BT pids 0x0b0c/0x0b21/0x02e0/0x02fd/0x0b20/0x0b05/0x0b22/0x0b13 (usb_ids.h lines); wired counterparts 0x0b12/0x02ea/0x0b00/0x02e3/0x0b0a. `0x0a42` NOT present in this SDL revision → excluded (test locks); `--bluetooth` covers such devices. `SDL_hidapi_xbox.c` does not exist in SDL 3.1.6 (xbox360.c/xbox360w.c/xboxone.c). Init gating: SDL_hidapi_xboxone.c:354 + BT read :1589-1620 — no GIP init on BT.
- decode.rs `is_bt_controller(vid,pid)` (TDD, 5 new tests; Linux 51/51 green). reader.rs `open_and_init(api, forced)`: BT → no init write; Wired → init failure now warns and continues (os 1784 tolerated); transport auto-detect `DeviceInfo::bus_type()` first (vendor device_info.rs:61-80 DEVPKEY_Device_CompatibleIds → BusType::Bluetooth), pid-table fallback, then Wired. main.rs: `--bluetooth`/`--wired` overrides (`forced`, `override` reserved in edition 2024), per-device transport line.
- Exe rebuilt: 8,555,687 B, mtime 22:39:51; all gates green both targets incl. windows-gnu test compile.

## Read-path forensics + --probe mode (subagent 792ba67a, 2026-11-01 23:05)

- Vendored hidapi windows_native read = one overlapped ReadFile per report sized caps.InputReportByteLength (mod.rs:162-226, types.rs:105-118); NO HidP_GetData/HidD_GetInputReport (SDL's C hid.c rejects GetInputReport, hid.c:1519). Ok(0)=WaitTimedOut means "no report completed on this handle" — silence is real, not swallowed. os error 995 at power-off proves a pending non-zero read ⇒ InputReportByteLength>0 on opened path.
- Silence hypotheses: (a) reports delivered on a DIFFERENT HID interface/collection of the same BLE device (windows_native interface_number() always -1, device_info.rs:76 — discriminator is IG_xx token in path); (b) InputReportByteLength==0 (unlikely given 995).
- SDL backend priority (SDL_joystick.c:51-66): HIDAPI driver highest on Windows desktop; HIDAPI_DriverXboxOne_IsSupportedDevice accepts 0x0b13 Bluetooth (:352-360); BT read = SDL_hid_read_timeout 64B dispatching report ID 0x01 state/0x02 guide (:1585-1600). No BLE handling in XINPUT/DINPUT/RAWINPUT (src/joystick/windows/ has none). So SDL gets BLE buttons via HIDAPI on some enumerated path — probe must find which interface carries bytes.
- User PnP data (BTHLE device): driver bthleenum.inf 10.0.19041.3636 generic, children BTHLEDevice\{00001800-...} HID interfaces, Category Input.Gaming.Gamepad — no report data in properties (expected).
- New `--probe` mode (src/probe.rs, cfg windows): opens ALL HID devices, descriptor-derived input-report inventory per handle (id/length — tests hypothesis b per interface), round-robin 10 ms reads, report hex lines per source handle, 2 s heartbeat reports/bytes per handle, 5 s re-scan for new interfaces, error-tolerant, no init writes. Exe rebuilt: 8,894,928 B, mtime 23:04:08. Gates green (Linux 51/51, clippy both targets).

## Stage 3 probe result — BLE raw-HID is a dead end on Windows (2026-11-01 23:17)

- `--probe` live run: 24 HID interfaces opened; ZERO reports/bytes anywhere while pressing all buttons. Xbox handle (pid 0b13, IG_00) stayed alive with pending zero-result reads; descriptor 111B "NO input reports declared". Other devices: some read errors os 5 (occupied) / os 1784 (zero-length reads).
- Verdict: Windows BthLEEnum HOGP facade for BLE Xbox declares NO HID input reports; real GATT reports are consumed by Microsoft Input Pipeline (XInput layer for it). Raw HID cannot see BLE pad state on Windows — hidapi bug absent, hypothesis (a) killed (no other interface carries bytes).
- SDL GameInput backend `SDL_windowsgameinput.c` (626 lines) covers mouse/keyboard only — no gamepad buttons; SDL Windows gamepad data paths: HIDAPI (real reports — wired/USB-adapter/classic BT 0x0a42), XInput (14-button cap), RAWINPUT (needs real HID reports). So BLE Series X|S pad on Windows: only XInput claims it → Share/Guide invisible via HID/XInput/WGI.
- Remaining routes for Share/Guide: (A) WIRED (USB-C cable) or Xbox Wireless Adapter → raw HID GIP frames — PoC ready to verify payload offsets 44/46/60 live; (B) GameInput GIP COM API — unsafe interop, conflicts with `#![forbid(unsafe_code)]`/windows-crate rejection; (C) accept BLE limitation.

## get_input_report route — hidapi C backend on Windows (2026-11-02)

- User research (hidapi issue #282): `hid_read` never delivers BLE HID reports on Windows; suggested probing `get_input_report` first.
- windows-native backend does NOT expose `get_input_report` (trait method gated `#[cfg(any(hidapi, target_os="linux"))]`, vendor lib.rs:497). Switched cfg(windows) dep to C backend: `features = []`; build.rs compile_windows() cc-compiles etc/hidapi/windows/hid.c via MinGW (offline OK, libhidapi.a built), sets cfg(hidapi).
- C impl detail: hid_get_input_report (vendor hid.c:1391-1395) uses DeviceIoControl(IOCTL_HID_GET_INPUT_REPORT) + GetOverlappedResult(wait=TRUE) — synchronous, not HidD_GetInputReport (chosen because HidD returns no actual length). Buffer convention: buf[0] = report ID, length includes it.
- C backend write() pads short writes to caps.OutputReportByteLength (windows-native required exact size → earlier os 1784); read() semantics unchanged (Ok(0) on timeout, report-ID byte stripped).
- New mode `--poll-ir` (src/poll_ir.rs): gamepad candidates (usage_page 0x01, usage 0x04/0x05), no init writes; probe IDs 0x01..0x04 x sizes [17,18,20,24,28,32,42,44,46,64], prints ok(len)/err per combo; successes polled every 50 ms, hex dump on change, decode transitions for Bluetooth transport; 0 successes -> "ALL attempts failed (N combos)", exit 0. --probe priority over --poll-ir; --bluetooth/--wired overrides honored.
- Gates: linux tests 51/51, clippy -D warnings both targets, fmt clean, windows-gnu build offline OK (exe 7,082,811 B, 2026-11-02 09:25:52 +0300), src/ zero unsafe/unwrap.
- Live expectation: if facade declares no input reports, ioctl likely fails per combo (ERROR_NOT_SUPPORTED / os 50-ish) — decisive either way; success means poll_ir becomes the transport and decode.rs table gets live reconciliation.

## Live: --poll-ir — get_input_report works but returns a frozen snapshot (2026-11-02)

- id=0x01 succeeds on every size (ioctl returns actual len 17 regardless of requested buffer): `01 00 80 00 80 00 82 00 82 00 00 00 00 00 00 00 00` — a valid BT state report (mask data[14]/[15]=0, hat=0, share byte absent within 17 total). ids 0x02..0x04 -> err 0x57 (ERROR_INVALID_BUFFER_SIZE).
- Polling at 50 ms while pressing ALL buttons: report never changes. HID driver cache is static; GATT notifications never update it. DECISIVE: raw-HID (async ReadFile AND sync get_input_report) cannot deliver live input from Windows BLE Xbox facade. Route B (GameInput) confirmed as sole Windows-BLE source.

## GameInput::v3 layout obtained — official header vendored (2026-11-02)

- Source: https://github.com/microsoftconnect/GameInput/raw/refs/heads/main/include/GameInput.h (47,688 B) -> .tmp/hidpad-poc/vendor/GameInput.h (authoritative for shim layout).
- IID_IGameInput = 20EFC1C7-5D9A-43BA-B26F-B807FA48609C (header:1003); export GameInputInitialize(riid, ppv) (header:1282); GameInputCreate is an inline thunk over it.
- IGameInput slots (header:1003-1079): 3 GetCurrentTimestamp()->u64, 4 GetCurrentReading, 7 RegisterReadingCallback, 8 RegisterDeviceCallback(device,kind,statusFilter,enumerationKind,context,cb,token*), 9 RegisterSystemButtonCallback(device,filter,context,cb,token*), 11 StopCallback, 12 UnregisterCallback->bool. IGameInputDevice slot 3 GetDeviceInfo(**const GameInputDeviceInfo) (header:1155+). Reading slot 6/7 GetControllerButtonCount/State (bool = 1 byte).
- Callback ABI (header:550-585): (token u64, context ptr, device ptr, timestamp u64, enum cur i32, enum prev i32). System buttons: None=0, Guide=0x1, Share=0x2 (header:300-307); BlockingEnumeration=2; DeviceAnyStatus=0xFFFFFFFF.
- Implemented `--gameinput` (src/gameinput.rs, windows-only, 746 lines): LoadLibraryW("gameinput.dll")+GetProcAddress("GameInputInitialize"), blocking device enumeration, AddRef+GetDeviceInfo per device, RegisterSystemButtonCallback(Guide|Share) on devices with supportedSystemButtons, share/guide DOWN/UP prints, 50 ms GetCurrentReading/GetControllerButtonState poll printing button index mask on change, heartbeat. Unsafe exception: crate #![forbid] -> #![deny] + #![allow(unsafe_code)] ONLY in gameinput module (user-approved PoC exception); zero unsafe elsewhere (rg-verified). Gates: 51/51 linux tests, clippy -D warnings both targets, fmt clean, offline windows-gnu build (exe 7,517,462 B, 2026-11-02 10:09:56 +0300).
- Layout risk noted: v3 header vs an older installed gameinput.dll (v1/v2 = shorter vtables, no Share) -> live-run garbage/crash symptoms would force v1/v2 revision; live test pending on user host.

## GameInput export fix + multi-version shim (2026-11-02)

- Live: LoadLibraryW("gameinput.dll") OK but GetProcAddress("GameInputInitialize") missing — OS inbox DLL exports GameInputCreate only. SDK impl (clone .tmp/GameInput, releases/v3.1.../lib/GameInput.cpp:529) delegates GameInputCreate -> CreateWithVersion(IID_IGameInput_v0) => create hands back a v0 object; version views resolved via QueryInterface (per-version GUIDs: v0 11BE2A7E…, v2 BBAA66D2…, v3 20EFC1C7…).
- Header cross-check (clone include/v0,v2,GameInput.h): v0 IGameInput has NO RegisterSystemButtonCallback / GameInputSystemButtons / Share — Guide equivalent = RegisterGuideButtonCallback slot 10 (v0:1137-1142); v0 GetDeviceInfo returns struct ptr directly (slot 3); v0 callbacks token-first; AnyStatus v0=0x00FFFFFF; v0 device info has no supportedSystemButtons (gate = supportedInput & Controller). v2/v3: device cb slot 8, system-button cb slot 9, GetDeviceInfo(hr,out**), Share=0x2 Guide=0x1. Reading buttons: v0 slots 10/11, v2/v3 slots 8/9.
- Rewrote src/gameinput.rs (1511 lines): export GameInputCreate first (fallback Initialize with IID attempts v3→v2→v0), QI negotiation prints `interface version chosen = v3|v2|v0`, per-version vtable/callback/layout tables cited to header lines, v0 guide callback + v2/v3 Share/Guide callback, button polling per version, registry dedup. Unsafe still confined to gameinput module.
- Gates: fmt/clippy both targets -D warnings, 51/51 linux tests, offline build exe 7,581,607 B (2026-11-02 11:09:11 +0300).
- Expectation: inbox DLL create=v0; if QI succeeds to v2/v3 on Win10+/Win11 (COM object implements all version views) Share arrives via slot-9; if only v0 view, Share is NOT available through this DLL and only Guide prints — decisive test pending on user host.

## Live: all registrations succeed but zero events — GameInput focus gating is the root cause (2026-11-02)

- Diagnostic run (dispatcher build): QI→v3 ok, sysbtn NULL + 3 async device callbacks + blocking callback all token-ok, dispatcher created, dispatch_ticks ~3000, Dispatch never executed callbacks, devices=0 across button presses and pad power-cycle.
- Root cause: GameInputDefaultFocusPolicy=0x0 (include/GameInput.h:45-56) delivers input/callback events only to foreground-focused apps; the PoC is a console process that never takes foreground focus. SDL remapper works because it has a foreground window.
- SDL proven sequence (SDL_gameinputjoystick.c:268-276): RegisterDeviceCallback(NULL, KindController, statusFilter=GameInputDeviceConnected, BlockingEnumeration, NULL, cb, &token) on the base create object — no QI, no dispatcher.
- Fix shipped (subagent e183b8ed): SetFocusPolicy(EnableBackgroundInput 0x40 | EnableBackgroundGuideButton 0x80 | EnableBackgroundShareButton 0x100 = 0x1C0) before any registration; slots transcribed per version — v3 slot 16 (include/GameInput.h:1070-1071), v2 slot 16 (include/v2/GameInput.h:922-923), v0 slot 21 (include/v0/GameInput.h:1184-1185, macro block :1448-1469); v0 enum has no EnableBackground bits (default already allows background; DisableBackgroundInput=0x1 is the opt-out) → policy 0x0 there. No enumeration-specific background flag exists in any version. Blocking variant statusFilter switched to GameInputDeviceConnected (0x1) per SDL.
- Gates green; exe 7,629,710 B (2026-11-02 12:22:05 +0300). Pending live re-run: expected devices>0, Dispatch executed callbacks, share/guide DOWN/UP from unfocused console = background-input proof for virshot (console/tray app).

## Live: focus policy lifted gating — callbacks fire, handler crashes (2026-11-02)

- SetFocusPolicy(0x1C0 = EnableBackgroundInput|Guide|Share) works: after `focus policy set = 0x1C0 (slot 16)` the process now dies immediately after async device registrations instead of beating forever — GameInput device callbacks fire on internal threads and handle_device_event crashed on a deref (silent abort, no Rust panic message across FFI). Both pad-on and pad-off runs die (device stays enumerated when off).
- Hardened (subagent fd58718e, .tmp/hidpad-poc/src/gameinput.rs 2148 lines): every callback entry prints args-only before any deref (print_flush survives abort), AddRef refcount line, GetDeviceInfo hr/null checked before out-param, numeric device fields before strings, safe_cstr null guards, sysbtn/guide callbacks use registry name lookup only (never deref device pointer), poll guards for null reading/count==0, debug line before blocking registration. Gates green, exe 7,672,120 B 12:49:24 +0300. Localization pending next live run.

## PoC moved into git as tracked standalone crate ./hidpad-poc (2026-11-02)

- User decision: PoC should live in the repo, not .tmp. ./hidpad-poc = standalone nested crate (own Cargo.toml/Cargo.lock/.gitignore; NOT a virshot workspace member; root virshot cargo check unaffected). Contains src/*.rs (hardened gameinput shim), vendor/hidapi (verbatim 2.6.7 crates.io mirror, [patch.crates-io] path dep — build.rs compiles windows/hid.c via cc/MinGW, cfg(hidapi) enables get_input_report), vendor/GameInput.h (47,688 B official header), vendor/NOTICE.md (provenance/licenses), README.md (build/run modes).
- Gates in ./hidpad-poc: fmt, clippy linux + windows-gnu(-D), test 51/51, offline windows-gnu build → exe 7,671,086 B (shorter embedded paths than .tmp copy).

## Live: device seen, crash localized — APP_LOCAL_DEVICE_ID is 32 bytes (2026-11-02)

- Pad ON: device callback fired on internal thread (`cb: device event ... status 0x0->0x1`, vid=0x045E pid=0x0B13 = Xbox Wireless), AddRef->4, numeric line printed, died at string deref; supportedSystemButtons=0x7E5CBA80 garbage.
- Root cause: mirrored GameInputDeviceInfo used APP_LOCAL_DEVICE_ID=[u8;16]; real = 32-byte hash (windows-rs APP_LOCAL_DEVICE_ID.value [u8;32]; .tmp/GameInput companion/c#interop/GameInputInteropV3.cs SizeConst=32; GDK docs "256-bit hash"). All fields after deviceId shifted 16B → systemButtons read inside deviceRootId, displayName pointer garbage → abort.
- Verified offsets (offset_of!/size_of tests, src/layout.rs, unsafe-free, linux-runnable): v3 size 256 — usage@6 hw@10 fw@18 deviceId@26(32) rootId@58(32) family@92 supportedInput@96 supportedRumbleMotors@100 systemButtons@104 containerId@108 displayName@128 pnpPath@136 …; v2 size 272; v0 size 320 (displayName@296 GameInputString*). Correction: GameInputRumbleMotors is a single 4-byte bit-flag enum (None/LowFreq/HighFreq/LeftTrigger/RightTrigger), not a 2-enum struct.
- Build stamp added (user request): build.rs embeds POC_BUILD_EPOCH/PROFILE/SHA; first line every run `hidpad-poc build <epoch> (<UTC>) profile=debug sha=<short>`; 4 stamp tests. Tests now 63/63 linux. Both crates synced (diff empty), exes 7,694,305 B (.tmp 13:27) / 7,693,271 B (tracked 13:39). Next live run: plausible systemButtons mask + real displayName + surviving process = Share path.

## LIVE SUCCESS: Share + Guide work via GameInput (2026-11-02)

- Clean run (pad ON): device enumerated once (name "Совместимое устройство ввода Bluetooth LE XINPUT", vid 0x045E pid 0x0B13, usage 0x0005, supportedInput 0x1040006, supportedRumbleMotors 0xF, supportedSystemButtons 0x3 = Guide|Share), 4 device-event callbacks (async kinds 0xE/0x40000/0x4 + blocking Connected) deduped to one registry entry; controller button polling prints bit changes (bits map: 0x01 View?, 0x02 Menu, 0x04 A, 0x08 B, 0x10 X?, 0x20 Y?, 0x40/0x80 LB/RB, 0x400-0x2000 D-pad); `cb: system button ... cur=0x2` → share DOWN/UP twice; `cur=0x1` → guide DOWN/UP. Process survives; dispatcher pump ticks. PoC objective ACHIEVED: Share readable from a console process thanks to SetFocusPolicy(0x1C0) before registrations.
- LT/RT not buttons: they are GameInput analog axes (KindControllerAxis) — out of PoC scope; axes not polled.
- Cleanup (subagent 4aba7bcb): dead raw-HID route deleted (probe/poll_ir/reader/decode.rs + vendored hidapi 69 files + [patch.crates-io]); gameinput.rs 1932 lines with debug scaffolding prints removed, safety kept (null guards, dedup, focus policy, bit-in-cur); single mode (--gameinput default); README/NOTICE rewritten; vendor/GameInput.h kept as ABI authority; linux tests now 12 (8 layout offsets + 4 build stamp); exe 6,084,020 B 14:37 +0300.
