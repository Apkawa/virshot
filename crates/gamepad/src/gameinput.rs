//! GameInput COM vtable shim: enumerate controller devices, report
//! system-button (Guide/Share) transitions, and poll all controller buttons —
//! without any extra crates. Every event the PoC printed on stdout is delivered
//! to the crate's public `Observer` (see `lib.rs`); internal failure notes go to
//! stderr through `note!`, so a host process's stdout stays clean.
//!
//! Version authority: the official Microsoft GameInput SDK headers — v0 =
//! `include/v0/GameInput.h`, v2 = `include/v2/GameInput.h`, v3 =
//! `include/GameInput.h` (the vendored `vendor/GameInput.h` copy is
//! byte-identical). v0, v2 and v3 are *different*
//! COM interfaces: different `IID_IGameInput` GUIDs, different vtable slot
//! order, different callback signatures and different `GameInputDeviceInfo`
//! layouts. Every slot table below cites the header lines it was transcribed
//! from; slots 0-2 are the inherited `IUnknown` methods in all versions.
//!
//! The `#[repr(C)]` struct mirrors themselves live in `layout.rs`, which is
//! compiled on every platform so the Linux test run can assert their offsets;
//! this module only imports them.
//!
//! Unsafe exception, scoped to this module: a raw COM vtable cannot be
//! mirrored without `unsafe` (borrowing the vtable stored in an object's first
//! word, `transmute` of exported function pointers, defensive pointer derefs of
//! GameInput-owned info blocks). The crate root keeps `deny(unsafe_code)`, and
//! nothing above this boundary uses `unsafe`.
#![allow(unsafe_code)] // GameInput COM vtable shim: raw COM vtables need `unsafe`.

use crate::DeviceInfo;
use crate::Error;
use crate::Observer;
use crate::SystemButton;
use crate::button_name;

use std::ffi::CStr;
use std::ffi::c_char;
use std::ffi::c_void;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

/// `HRESULT` is a 32-bit signed value; the sign bit marks failure.
type Hresult = i32;

/// Print an `HRESULT` as the unsigned 8-digit hex the Windows headers use.
fn hr_hex(hr: Hresult) -> String {
    format!("0x{:08X}", hr as u32)
}

/// Interface API version negotiated from the created object. v1 exists
/// (`include/v1/GameInput.h:775`) but is deliberately not part of the
/// negotiation chain: the SDK loader (`lib/GameInput.cpp:472-512`) only ever
/// hands out v0/v2/v3 for the IIDs below.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ApiVersion {
    V0,
    V2,
    V3,
}

impl ApiVersion {
    fn label(self) -> &'static str {
        match self {
            ApiVersion::V0 => "v0",
            ApiVersion::V2 => "v2",
            ApiVersion::V3 => "v3",
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            ApiVersion::V0 => 0,
            ApiVersion::V2 => 2,
            ApiVersion::V3 => 3,
        }
    }
}

/// Decode the stored version; anything unknown behaves as v0 (the fallback
/// path where the base object is used as-is).
fn chosen_version() -> ApiVersion {
    match VERSION.load(Ordering::Relaxed) {
        3 => ApiVersion::V3,
        2 => ApiVersion::V2,
        _ => ApiVersion::V0,
    }
}

/// `GameInputKindController` / `GameInputKindControllerButton`: the same bit
/// values in every version (v0:20-37, v2:19-34, v3:19-34).
const KIND_CONTROLLER: i32 = 0x0000_000E;
const KIND_CONTROLLER_BUTTON: i32 = 0x0000_0004;
/// `GameInputKindGamepad`: 0x40000 in ALL three headers (v0:34, v2:31,
/// v3:32), so the gamepad-specific device callback is registered on every
/// negotiated version, not only v2/v3.
const KIND_GAMEPAD: i32 = 0x0004_0000;
/// `GameInputDeviceAnyStatus`: v0 caps at 0x00FFFFFF (v0:218-231); v2/v3 use
/// the full 32-bit mask (v2:235-241, v3:346-352).
const DEVICE_ANY_STATUS_V0: i32 = 0x00FF_FFFF;
const DEVICE_ANY_STATUS_V2_V3: i32 = 0xFFFF_FFFF_u32 as i32;
/// `GameInputDeviceConnected`: 0x1 in every version (v0:221, v2:238, v3:349).
/// SDL's proven blocking device registration uses this filter instead of
/// AnyStatus (`SDL_gameinputjoystick.c:268-276`), so the blocking variant of
/// this PoC does too; the async variants keep AnyStatus.
const DEVICE_CONNECTED: i32 = 0x0000_0001;
/// `GameInputFocusPolicy` background-enable bits (v3:45-54, v2:45-54):
/// `GameInputDefaultFocusPolicy = 0x0` (v3:47) delivers callbacks/reading
/// events only to a foreground-focused app, which a console process never is
/// — the live root cause of registrations succeeding with zero dispatched
/// events. No version's enum has an enumeration-specific background flag
/// (checked v0:48-53, v1:44-54, v2:45-54, v3:45-54), so
/// `EnableBackgroundInput` gates device enumeration events as well.
const FOCUS_ENABLE_BACKGROUND_INPUT: i32 = 0x0000_0040;
const FOCUS_ENABLE_BACKGROUND_GUIDE: i32 = 0x0000_0080;
const FOCUS_ENABLE_BACKGROUND_SHARE: i32 = 0x0000_0100;
const FOCUS_POLICY_BACKGROUND: i32 =
    FOCUS_ENABLE_BACKGROUND_INPUT | FOCUS_ENABLE_BACKGROUND_GUIDE | FOCUS_ENABLE_BACKGROUND_SHARE;
/// `GameInputBlockingEnumeration`: value 2 in all versions (v0:41-46, v2:36-42,
/// v3:38-43). `GameInputAsyncEnumeration` is 1 in the same enum blocks: the
/// callback fires on GameInput's own threads / inside `Dispatch` instead of
/// synchronously inside the registration call.
const ENUMERATION_BLOCKING: i32 = 2;
const ENUMERATION_ASYNC: i32 = 1;
/// `GameInputSystemButtonGuide` / `GameInputSystemButtonShare` (v2:226-233,
/// v3:300-307). v0 has no `GameInputSystemButtons` enum at all — see
/// `register_guide_button_callback`.
const SYSTEM_BUTTON_GUIDE: i32 = 0x0000_0001;
const SYSTEM_BUTTON_SHARE: i32 = 0x0000_0002;
const SYSTEM_BUTTON_FILTER: i32 = SYSTEM_BUTTON_GUIDE | SYSTEM_BUTTON_SHARE;
/// `GAMEINPUT_E_DEVICE_DISCONNECTED` / `GAMEINPUT_E_READING_NOT_FOUND`
/// (v0:1559-1583; the same codes appear in the v2/v3 headers).
const E_DEVICE_DISCONNECTED: u32 = 0x838A_0001;
const E_READING_NOT_FOUND: u32 = 0x838A_0003;

/// Button arrays larger than this are capped (no controller has that many).
const MAX_BUTTON_COUNT: usize = 64;
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Heartbeat cadence: one concise `alive:` line every 5 s.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// `IGameInputDispatcher::Dispatch(quotaInMicroseconds)` pump cadence and
/// per-pump callback quota (slot 3 in every version — see `DispatcherVtbl`).
const DISPATCH_INTERVAL: Duration = Duration::from_millis(10);
const DISPATCH_QUOTA_MICROSECONDS: u64 = 5000;

/// Export names resolved by `GetProcAddress`. The Windows inbox DLL exports
/// only `GameInputCreate(IUnknown**)` — it hands back the v0 interface — while
/// the SDK DLL exports both (`lib/GameInput.cpp:522-535`: the exported
/// `GameInputCreate` delegates to `GameInputCreateWithVersion(IID_IGameInput_v0)`,
/// which itself prefers the `GameInputInitialize` export). A `c""` literal is
/// NUL-terminated.
const CREATE_EXPORT: &CStr = c"GameInputCreate";
const INITIALIZE_EXPORT: &CStr = c"GameInputInitialize";

// The struct mirrors live in `layout.rs`: compiled on every platform (no
// Windows linkage, no `unsafe`) so the Linux `cargo test` run can assert their
// offsets against the header transcriptions there. `layout.rs` documents the
// 32-byte `APP_LOCAL_DEVICE_ID` authority; a 16-byte mirror would shift every
// field after `deviceId`, so those offset tests are the regression guard.
use crate::layout::{GameInputDeviceInfoV0, GameInputDeviceInfoV2, GameInputDeviceInfoV3, Guid};

/// `IID_IGameInput` for each interface version, copied from the
/// `DECLARE_INTERFACE_IID_` lines: v3 (v3:1001-1003), v2 (v2:853-855),
/// v0 (v0:1083-1085).
static IID_IGAMEINPUT_V3: Guid = Guid {
    data1: 0x20EFC1C7,
    data2: 0x5D9A,
    data3: 0x43BA,
    data4: [0xB2, 0x6F, 0xB8, 0x07, 0xFA, 0x48, 0x60, 0x9C],
};
static IID_IGAMEINPUT_V2: Guid = Guid {
    data1: 0xBBAA66D2,
    data2: 0x837A,
    data3: 0x40F7,
    data4: [0xA3, 0x03, 0x91, 0x7D, 0x50, 0x09, 0x55, 0xF4],
};
static IID_IGAMEINPUT_V0: Guid = Guid {
    data1: 0x11BE2A7E,
    data2: 0x4254,
    data3: 0x445A,
    data4: [0x9C, 0x09, 0xFF, 0xC4, 0x0F, 0x00, 0x69, 0x18],
};

/// Callback signatures, one per version, token first in every version:
///
/// * v0 `GameInputDeviceCallback` (v0:537-543) — same shape as v2/v3.
/// * v0 `GameInputGuideButtonCallback` (v0:545-551): `bool isPressed`, no
///   previous/current bitmask — v0 has no `GameInputSystemButtons`.
/// * v2 `GameInputDeviceCallback` (v2:453-459) / `GameInputSystemButtonCallback`
///   (v2:461-467).
/// * v3 `GameInputDeviceCallback` (v3:566-572) / `GameInputSystemButtonCallback`
///   (v3:574-580).
type DeviceCallbackFn = unsafe extern "system" fn(
    token: u64,
    context: *mut c_void,
    device: *mut c_void,
    timestamp: u64,
    current_status: i32,
    previous_status: i32,
);
type SystemButtonCallbackFn = unsafe extern "system" fn(
    token: u64,
    context: *mut c_void,
    device: *mut c_void,
    timestamp: u64,
    current_buttons: i32,
    previous_buttons: i32,
);
type GuideButtonCallbackFnV0 = unsafe extern "system" fn(
    token: u64,
    context: *mut c_void,
    device: *mut c_void,
    timestamp: u64,
    is_pressed: bool,
);

/// Shared `IUnknown` prefix (slots 0-2, inherited identically by every
/// GameInput interface in every version).
#[repr(C)]
struct IUnknownVtbl {
    query_interface: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult,
    >,
    add_ref: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    release: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
}

/// Placeholder slot: vtable entries we never call are still pointer-sized;
/// typing them keeps the slot indexing exact.
type UnusedSlot = Option<extern "system" fn(*mut c_void) -> Hresult>;

/// `IGameInput` v0 vtable (v0:1085-1186, 22 slots): slots 0-2 `IUnknown`,
/// 3 `GetCurrentTimestamp`, 4 `GetCurrentReading`, 5 `GetNextReading`,
/// 6 `GetPreviousReading`, 7 `GetTemporalReading` (removed in v2/v3),
/// 8 `RegisterReadingCallback` (extra `float analogThreshold` arg),
/// 9 `RegisterDeviceCallback`, 10 `RegisterGuideButtonCallback`
/// (v0:1137-1142 — v0's only system-button callback), 11 keyboard layout,
/// 12 `StopCallback`, 13 `UnregisterCallback(token, timeout)` (extra timeout
/// arg), 14 `CreateDispatcher` (v0:1155-1156), 15 `CreateAggregateDevice`,
/// 16-19 finders (incl. FromObject/FromPlatformHandle), 20
/// `EnableOemDeviceSupport`, 21 `SetFocusPolicy` (v0:1184-1185; the
/// `lpVtbl` macro block v0:1448-1469 confirms the exact declared order).
/// Mirrored up to slot 21 so `SetFocusPolicy` is reachable; slots after
/// `CreateDispatcher` except 21 are unused.
#[repr(C)]
#[allow(dead_code)] // Declared for layout fidelity; only a subset is called.
struct GameInputVtblV0 {
    query_interface: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult,
    >,
    add_ref: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    release: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    get_current_timestamp: Option<unsafe extern "system" fn(this: *mut c_void) -> u64>,
    get_current_reading: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            input_kind: i32,
            device: *mut c_void,
            reading: *mut *mut c_void,
        ) -> Hresult,
    >,
    get_next_reading: UnusedSlot,
    get_previous_reading: UnusedSlot,
    get_temporal_reading: UnusedSlot,
    register_reading_callback: UnusedSlot,
    register_device_callback: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            device: *mut c_void,
            input_kind: i32,
            status_filter: i32,
            enumeration_kind: i32,
            context: *mut c_void,
            callback: DeviceCallbackFn,
            token: *mut u64,
        ) -> Hresult,
    >,
    register_guide_button_callback: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            device: *mut c_void,
            context: *mut c_void,
            callback: GuideButtonCallbackFnV0,
            token: *mut u64,
        ) -> Hresult,
    >,
    // Slots 11-13: keyboard layout (v0:1143-1148), `StopCallback` (void, token
    // arg) and `UnregisterCallback(token, timeout)` (bool) — pointer-sized
    // placeholders only, never called.
    register_keyboard_layout_callback: UnusedSlot,
    stop_callback: UnusedSlot,
    unregister_callback: UnusedSlot,
    // Slot 14 (v0:1155-1156): `CreateDispatcher(IGameInputDispatcher**)`.
    create_dispatcher: Option<
        unsafe extern "system" fn(this: *mut c_void, dispatcher: *mut *mut c_void) -> Hresult,
    >,
    // Slots 15-20 (v0:1157-1183): `CreateAggregateDevice`, the four finders
    // (`FromId`, `FromObject`, `FromPlatformHandle`, `FromPlatformString`)
    // and `EnableOemDeviceSupport` — pointer-sized placeholders only, never
    // called; they keep slot 21's index exact.
    create_aggregate_device: UnusedSlot,
    find_device_from_id: UnusedSlot,
    find_device_from_object: UnusedSlot,
    find_device_from_platform_handle: UnusedSlot,
    find_device_from_platform_string: UnusedSlot,
    enable_oem_device_support: UnusedSlot,
    // Slot 21 (v0:1184-1185, macro v0:1469): `SetFocusPolicy(policy)`, void.
    set_focus_policy: Option<SetFocusPolicyFn>,
}

/// `IGameInput` v2 vtable (v2:855-920, 17 slots): `GetTemporalReading` is
/// gone, so from slot 7 on the layout shifts down by one vs v0; 7
/// `RegisterReadingCallback` (no analogThreshold), 8 `RegisterDeviceCallback`,
/// 9 `RegisterSystemButtonCallback` (v2:888-895), 10 keyboard layout, 11
/// `StopCallback`, 12 `UnregisterCallback(token)`, 13 `CreateDispatcher`
/// (v2:911-912), 14/15 finders, 16 `SetFocusPolicy` (v2:922-923 — the v2
/// interface ends there). Mirrored up to slot 16 so `SetFocusPolicy` is
/// reachable; slots 14/15 are unused placeholders.
#[repr(C)]
#[allow(dead_code)] // Declared for layout fidelity; only a subset is called.
struct GameInputVtblV2 {
    query_interface: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult,
    >,
    add_ref: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    release: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    get_current_timestamp: Option<unsafe extern "system" fn(this: *mut c_void) -> u64>,
    get_current_reading: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            input_kind: i32,
            device: *mut c_void,
            reading: *mut *mut c_void,
        ) -> Hresult,
    >,
    get_next_reading: UnusedSlot,
    get_previous_reading: UnusedSlot,
    register_reading_callback: UnusedSlot,
    register_device_callback: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            device: *mut c_void,
            input_kind: i32,
            status_filter: i32,
            enumeration_kind: i32,
            context: *mut c_void,
            callback: DeviceCallbackFn,
            token: *mut u64,
        ) -> Hresult,
    >,
    register_system_button_callback: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            device: *mut c_void,
            button_filter: i32,
            context: *mut c_void,
            callback: SystemButtonCallbackFn,
            token: *mut u64,
        ) -> Hresult,
    >,
    // Slots 10-12: keyboard layout, `StopCallback` (void) and
    // `UnregisterCallback(token)` (bool) — pointer-sized placeholders only,
    // never called.
    register_keyboard_layout_callback: UnusedSlot,
    stop_callback: UnusedSlot,
    unregister_callback: UnusedSlot,
    // Slot 13 (v2:911-912, v3:1059-1060): `CreateDispatcher(IGameInputDispatcher**)`.
    create_dispatcher: Option<
        unsafe extern "system" fn(this: *mut c_void, dispatcher: *mut *mut c_void) -> Hresult,
    >,
    // Slots 14-15 (v2:914-919): `FindDeviceFromId` / `FindDeviceFromPlatformString`
    // — pointer-sized placeholders only, never called.
    find_device_from_id: UnusedSlot,
    find_device_from_platform_string: UnusedSlot,
    // Slot 16 (v2:922-923): `SetFocusPolicy(policy)`, void — the tail of v2.
    set_focus_policy: Option<SetFocusPolicyFn>,
}

/// `IGameInput` v3 vtable (v3:1003-1079, 19 slots): identical to v2 for slots
/// 3-16 (device callback slot 8, system-button callback slot 9, keyboard
/// layout 10, `StopCallback` 11, `UnregisterCallback` 12, `CreateDispatcher`
/// 13 at v3:1059-1060, finders 14/15, `SetFocusPolicy` 16 at v3:1070-1071), plus
/// `CreateAggregateDevice` (17) and `DisableAggregateDevice` (18). Mirrored
/// up to slot 18 so `SetFocusPolicy` is reachable; slots 14/15 and 17/18 are
/// unused placeholders.
#[repr(C)]
#[allow(dead_code)] // Declared for layout fidelity; only a subset is called.
struct GameInputVtblV3 {
    query_interface: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult,
    >,
    add_ref: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    release: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    get_current_timestamp: Option<unsafe extern "system" fn(this: *mut c_void) -> u64>,
    get_current_reading: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            input_kind: i32,
            device: *mut c_void,
            reading: *mut *mut c_void,
        ) -> Hresult,
    >,
    get_next_reading: UnusedSlot,
    get_previous_reading: UnusedSlot,
    register_reading_callback: UnusedSlot,
    register_device_callback: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            device: *mut c_void,
            input_kind: i32,
            status_filter: i32,
            enumeration_kind: i32,
            context: *mut c_void,
            callback: DeviceCallbackFn,
            token: *mut u64,
        ) -> Hresult,
    >,
    register_system_button_callback: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            device: *mut c_void,
            button_filter: i32,
            context: *mut c_void,
            callback: SystemButtonCallbackFn,
            token: *mut u64,
        ) -> Hresult,
    >,
    // Slots 10-12: keyboard layout, `StopCallback` (void) and
    // `UnregisterCallback(token)` (bool) — pointer-sized placeholders only,
    // never called.
    register_keyboard_layout_callback: UnusedSlot,
    stop_callback: UnusedSlot,
    unregister_callback: UnusedSlot,
    // Slot 13 (v2:911-912, v3:1059-1060): `CreateDispatcher(IGameInputDispatcher**)`.
    create_dispatcher: Option<
        unsafe extern "system" fn(this: *mut c_void, dispatcher: *mut *mut c_void) -> Hresult,
    >,
    // Slots 14-15 (v3:1062-1067): `FindDeviceFromId` / `FindDeviceFromPlatformString`
    // — pointer-sized placeholders only, never called.
    find_device_from_id: UnusedSlot,
    find_device_from_platform_string: UnusedSlot,
    // Slot 16 (v3:1070-1071): `SetFocusPolicy(policy)`, void.
    set_focus_policy: Option<SetFocusPolicyFn>,
    // Slots 17-18 (v3:1073-1079): `CreateAggregateDevice` /
    // `DisableAggregateDevice` — pointer-sized placeholders only, never called.
    create_aggregate_device: UnusedSlot,
    disable_aggregate_device: UnusedSlot,
}

/// `IGameInputReading` v0 vtable (v0:1190-1263): slot 4 `GetSequenceNumber`,
/// slot 7 `GetRawReport`, and controller accessors start at slot 8 — button
/// count/state live at slots 10/11 (v0:1219-1223).
#[repr(C)]
#[allow(dead_code)] // Declared for layout fidelity; only a subset is called.
struct ReadingVtblV0 {
    query_interface: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult,
    >,
    add_ref: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    release: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    get_input_kind: UnusedSlot,
    get_sequence_number: UnusedSlot,
    get_timestamp: UnusedSlot,
    get_device: UnusedSlot,
    get_raw_report: UnusedSlot,
    get_controller_axis_count: UnusedSlot,
    get_controller_axis_state: UnusedSlot,
    get_controller_button_count: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    get_controller_button_state:
        Option<unsafe extern "system" fn(this: *mut c_void, count: u32, states: *mut u8) -> u32>,
}

/// `IGameInputReading` v2/v3 vtable (v2:926-946, v3:1100-1119): no
/// `GetSequenceNumber` / `GetRawReport`, so button count/state are slots 8/9.
/// `bool*` is 1 byte in the MSVC ABI, hence the `*mut u8` state array.
#[repr(C)]
#[allow(dead_code)] // Declared for layout fidelity; only a subset is called.
struct ReadingVtblV2V3 {
    query_interface: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult,
    >,
    add_ref: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    release: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    get_input_kind: UnusedSlot,
    get_timestamp: UnusedSlot,
    get_device: UnusedSlot,
    get_controller_axis_count: UnusedSlot,
    get_controller_axis_state: UnusedSlot,
    get_controller_button_count: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    get_controller_button_state:
        Option<unsafe extern "system" fn(this: *mut c_void, count: u32, states: *mut u8) -> u32>,
}

/// `IGameInputDevice` v0 vtable (v0:1267-1279): `GetDeviceInfo` (slot 3)
/// RETURNS the info pointer directly (`GameInputDeviceInfo const *`,
/// v0:1277) — no `HRESULT`, no out-param — and `GetDeviceStatus` (slot 4,
/// v0:1279) returns the status. The info block is owned by GameInput.
#[repr(C)]
#[allow(dead_code)] // Declared for layout fidelity; only a subset is called.
struct DeviceVtblV0 {
    query_interface: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult,
    >,
    add_ref: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    release: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    get_device_info:
        Option<unsafe extern "system" fn(this: *mut c_void) -> *const GameInputDeviceInfoV0>,
    get_device_status: Option<unsafe extern "system" fn(this: *mut c_void) -> i32>,
}

/// `IGameInputDevice` v2/v3 vtable (v2:981-990, v3:1155-1164): `GetDeviceInfo`
/// (slot 3) takes an out-param `const GameInputDeviceInfo**` and returns an
/// `HRESULT`; slot 4 is `GetHapticInfo`; `GetDeviceStatus` is slot 5.
#[repr(C)]
#[allow(dead_code)] // Declared for layout fidelity; only a subset is called.
struct DeviceVtblV2V3 {
    query_interface: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult,
    >,
    add_ref: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    release: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    get_device_info:
        Option<unsafe extern "system" fn(this: *mut c_void, info: *mut *const c_void) -> Hresult>,
    get_haptic_info: UnusedSlot,
    get_device_status: Option<unsafe extern "system" fn(this: *mut c_void) -> i32>,
}

/// `IGameInputDispatcher` vtable — the same interface in all three versions
/// (same IID `"415EED2E-98CB-42C2-8F28-B94601074E31"`: v3:1218, v2:1015,
/// v0:1344) with the same slot order: slots 0-2 `IUnknown`, 3
/// `Dispatch(quotaInMicroseconds) -> bool` (v3:1220-1221, v2:1017-1018,
/// v0:1356-1357), 4 `OpenWaitHandle(HANDLE*)` (v3:1223-1225, v2:1020-1022,
/// v0:1358-1360). `Dispatch` runs pending callbacks on the CALLING thread,
/// which is why a process with no dispatcher sees no callback events.
#[repr(C)]
#[allow(dead_code)] // Declared for layout fidelity; only slot 3 is called.
struct DispatcherVtbl {
    query_interface: Option<
        unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult,
    >,
    add_ref: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    release: Option<unsafe extern "system" fn(this: *mut c_void) -> u32>,
    dispatch: Option<DispatchFn>,
    open_wait_handle: UnusedSlot,
}

/// `IGameInputDispatcher::Dispatch`: returns whether callbacks were dispatched
/// within `quotaInMicroseconds` (header says `IFACEMETHOD_(bool, Dispatch)`;
/// `bool` is 1 byte in the MSVC ABI).
type DispatchFn = unsafe extern "system" fn(this: *mut c_void, quota_in_microseconds: u64) -> bool;

/// `IGameInput::RegisterSystemButtonCallback` shared signature (slot 9):
/// v3:1040-1046, v2:892-898. `device` is `_In_opt_`, so NULL registers for
/// system-button events from ALL devices.
type SystemButtonRegisterFn = unsafe extern "system" fn(
    this: *mut c_void,
    device: *mut c_void,
    button_filter: i32,
    context: *mut c_void,
    callback: SystemButtonCallbackFn,
    token: *mut u64,
) -> Hresult;

/// `IGameInput::RegisterGuideButtonCallback` v0 signature (slot 10,
/// v0:1136-1142). `device` is `_In_opt_` as well, so NULL works here too.
type GuideRegisterFnV0 = unsafe extern "system" fn(
    this: *mut c_void,
    device: *mut c_void,
    context: *mut c_void,
    callback: GuideButtonCallbackFnV0,
    token: *mut u64,
) -> Hresult;

/// `IGameInput::RegisterDeviceCallback` shared signature (slot 9 in v0, slot 8
/// in v2/v3): v0:1127-1135, v2:883-888, v3:1031-1038.
type DeviceRegisterFn = unsafe extern "system" fn(
    this: *mut c_void,
    device: *mut c_void,
    input_kind: i32,
    status_filter: i32,
    enumeration_kind: i32,
    context: *mut c_void,
    callback: DeviceCallbackFn,
    token: *mut u64,
) -> Hresult;

/// `IGameInput::SetFocusPolicy`: v3 slot 16 (include/GameInput.h:1070-1071),
/// v2 slot 16 (include/v2/GameInput.h:922-923, the interface's last method),
/// v0 slot 21 (include/v0/GameInput.h:1184-1185, order confirmed by the
/// `lpVtbl` macros at v0:1448-1469). Declared `IFACEMETHOD_(void,
/// SetFocusPolicy)` — no `HRESULT`; the policy enum is 32-bit (`i32`).
type SetFocusPolicyFn = unsafe extern "system" fn(this: *mut c_void, policy: i32);

/// `GameInputCreate(void** ppv)` export — the Windows inbox DLL's entry point;
/// creates the v0 interface (SDK mirror: `lib/GameInput.cpp:522-535`).
type GameInputCreateFn = unsafe extern "system" fn(ppv: *mut *mut c_void) -> Hresult;

/// `GameInputInitialize(REFIID riid, void** ppv)` export (v3 header:1282-1284).
type GameInputInitializeFn =
    unsafe extern "system" fn(riid: *const Guid, ppv: *mut *mut c_void) -> Hresult;

unsafe extern "system" {
    fn LoadLibraryW(lib_filename: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, proc_name: *const c_char) -> *mut c_void;
}

/// The negotiated live `IGameInput` object, shared with callback threads.
static GAMEINPUT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Negotiated interface version (see `ApiVersion::as_u8`).
static VERSION: AtomicU8 = AtomicU8::new(0);

/// Controller devices reported by the device callback, deduplicated by pointer:
/// a device reported again by another kind filter or a later status change is
/// not added a second time, so nothing re-registers or re-prints.
struct RegisteredDevice {
    device: *mut c_void,
    name: String,
    supported_input: i32,
    last_buttons: u64,
    last_error: u32,
}

/// Raw COM pointers, but the registry is only ever touched under the mutex, and
/// GameInput owns the objects (this PoC holds one `AddRef` per device).
unsafe impl Send for RegisteredDevice {}

static DEVICES: Mutex<Vec<RegisteredDevice>> = Mutex::new(Vec::new());

/// Set once the version's system-button/guide callback is registered with
/// `device = NULL` (all devices). The per-device registration inside the
/// device callback is then redundant, so `handle_device_event` skips it; the
/// per-device path stays the fallback when the NULL registration failed.
static GLOBAL_SYSBTN_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Printed once when the chosen version's reading layout lacks the controller
/// button slots (defensive; all three mirrored versions do have them).
static POLL_UNSUPPORTED_PRINTED: AtomicBool = AtomicBool::new(false);

/// The observer for the current run. Callbacks fire on GameInput's own threads
/// and inside `Dispatch` on the pump thread, so the reference lives in a
/// process-wide static — the same shape as the other globals above.
static OBSERVER: Mutex<Option<&'static dyn Observer>> = Mutex::new(None);

/// Record the observer for this run. The PoC's state (`GAMEINPUT`, `VERSION`,
/// `DEVICES`) is process-wide, so one `run()` per process is the supported
/// shape; a second call would re-register callbacks on the same interface.
fn set_observer(observer: &dyn Observer) {
    // SAFETY: `run()` holds the caller's borrow for the whole pump/poll loop,
    // and the callbacks below only dereference it inside that window, so the
    // erased lifetime cannot outlive the borrow.
    let stored: &'static dyn Observer =
        unsafe { std::mem::transmute::<&dyn Observer, &'static dyn Observer>(observer) };
    if let Ok(mut slot) = OBSERVER.lock() {
        *slot = Some(stored);
    }
}

/// Deliver one event to the observer. A poisoned mutex drops the event, the
/// same tolerance the registry lock gets above.
fn emit<F: FnOnce(&dyn Observer)>(event: F) {
    if let Ok(slot) = OBSERVER.lock()
        && let Some(observer) = *slot
    {
        event(observer);
    }
}

/// Device name looked up from the registry by pointer comparison: the device
/// pointer is compared, NEVER dereferenced. `<null>` when the device has not
/// been reported by the device callback (so the registry has no name for it),
/// which keeps system-button/guide callbacks free of any device deref.
fn registry_name(device: *mut c_void) -> String {
    match DEVICES.lock() {
        Ok(devices) => devices
            .iter()
            .find(|entry| entry.device == device)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| String::from("<null>")),
        Err(_) => String::from("<null>"),
    }
}

/// NUL-terminated UTF-16 for `LoadLibraryW`, built without unsafe.
fn utf16_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Diagnostic note: one line on **stderr**, flushed immediately. Callback paths
/// must survive a native abort (GameInput can terminate the process inside a
/// callback on its own threads), so the handle is locked and flushed instead of
/// relying on stdout buffering. Events never land here — they go to `Observer`.
macro_rules! note {
    ($($arg:tt)*) => {{
        let stderr = std::io::stderr();
        let mut handle = stderr.lock();
        let _ = writeln!(handle, $($arg)*);
        let _ = handle.flush();
    }};
}

/// Read a GameInput-owned C string defensively: `<null>` when the pointer is
/// null; `CStr::from_ptr` is only reached after the null guard.
fn safe_cstr(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::from("<null>");
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// Borrow the vtable stored in the first word of a COM object. `None` when the
/// object or the vtable pointer is null; the borrow only reads slot function
/// pointers while GameInput still owns the object.
fn vtbl_of<T>(object: *mut c_void) -> Option<&'static T> {
    if object.is_null() {
        return None;
    }
    let vtbl_ptr = unsafe { *(object as *const *const T) };
    if vtbl_ptr.is_null() {
        return None;
    }
    Some(unsafe { &*vtbl_ptr })
}

/// Numeric fields of `GameInputDeviceInfo` for the device line. v0 reports
/// `capabilities` where v2/v3 report `supported_system_buttons` (v0 has no such
/// field). `supportedRumbleMotors` — the 4-byte flag enum
/// `GameInputRumbleMotors` (v3:532-538 + `DEFINE_ENUM_FLAG_OPERATORS` :541;
/// v2:420-428; v0:508-515) — exists in all three layouts but is not part of the
/// line. Every field is zero when the info block is unavailable.
struct DeviceNumeric {
    vendor_id: u16,
    product_id: u16,
    usage_id: u16,
    supported_input: i32,
    system_buttons_or_capabilities: i32,
    v0_layout: bool,
}

impl DeviceNumeric {
    /// All-zero fallback used when `GetDeviceInfo` failed or handed back a
    /// null info pointer, so the device line still prints.
    fn fallback(v0_layout: bool) -> Self {
        DeviceNumeric {
            vendor_id: 0,
            product_id: 0,
            usage_id: 0,
            supported_input: 0,
            system_buttons_or_capabilities: 0,
            v0_layout,
        }
    }
}

/// Raw `displayName` pointer from the info block, read only through
/// `safe_cstr`, which null-guards it.
struct DeviceStrings {
    display_name: *const c_char,
}

impl DeviceStrings {
    /// Null pointer: the fallback when `GetDeviceInfo` failed, so the device
    /// line prints `<null>` for the name instead of dereferencing anything.
    fn fallback() -> Self {
        DeviceStrings {
            display_name: std::ptr::null(),
        }
    }
}

/// v0 `IGameInputDevice::GetDeviceInfo` (slot 3, v0:1277): direct info
/// pointer, no `HRESULT`; `displayName` is a `GameInputString` (v0:663-668).
/// The returned pointer is null-checked before ANY dereference; the info
/// block's numeric fields are read first, and the `displayName` pointer is
/// kept raw for `safe_cstr`, which null-guards it.
fn read_device_info_v0(device: *mut c_void) -> Option<(DeviceNumeric, DeviceStrings)> {
    let Some(vtbl) = vtbl_of::<DeviceVtblV0>(device) else {
        note!("device object or vtable pointer is null");
        return None;
    };
    let Some(get_device_info) = vtbl.get_device_info else {
        note!("device vtable missing GetDeviceInfo");
        return None;
    };
    let info = unsafe { get_device_info(device) };
    if info.is_null() {
        note!("GetDeviceInfo returned ok with a null info pointer");
        return None;
    }
    // GameInput owns the info block; copy the numeric fields and keep the raw
    // string pointers, never dereferenced without a null guard.
    let info = unsafe { &*info };
    // `displayName` points to a `GameInputString`: null-guard the struct
    // pointer before reading `data`; `data` itself is guarded by `safe_cstr`.
    let display_name = if info.display_name.is_null() {
        std::ptr::null()
    } else {
        // Field-only copy (`*const c_char` is `Copy`): no move of the whole
        // `GameInputString` out of the shared info borrow.
        unsafe { (*info.display_name).data }
    };
    Some((
        DeviceNumeric {
            vendor_id: info.vendor_id,
            product_id: info.product_id,
            usage_id: info.usage.id,
            supported_input: info.supported_input,
            system_buttons_or_capabilities: info.capabilities,
            v0_layout: true,
        },
        DeviceStrings { display_name },
    ))
}

/// v2 `IGameInputDevice::GetDeviceInfo` (slot 3, v2:983-984): `HRESULT` +
/// out-param pointer. The `HRESULT` is checked BEFORE the info pointer is
/// touched, and the pointer is null-checked before any dereference;
/// `displayName` is a bare `const char*` (v2:738-739), kept raw and
/// null-guarded by `safe_cstr`.
fn read_device_info_v2(device: *mut c_void) -> Option<(DeviceNumeric, DeviceStrings)> {
    let Some(vtbl) = vtbl_of::<DeviceVtblV2V3>(device) else {
        note!("device object or vtable pointer is null");
        return None;
    };
    let Some(get_device_info) = vtbl.get_device_info else {
        note!("device vtable missing GetDeviceInfo");
        return None;
    };
    let mut info: *const c_void = std::ptr::null();
    let hr = unsafe { get_device_info(device, &mut info) };
    if hr < 0 {
        note!("GetDeviceInfo failed: hr={}", hr_hex(hr));
        return None;
    }
    if info.is_null() {
        note!("GetDeviceInfo returned ok with a null info pointer");
        return None;
    }
    let info = unsafe { &*(info as *const GameInputDeviceInfoV2) };
    Some((
        DeviceNumeric {
            vendor_id: info.vendor_id,
            product_id: info.product_id,
            usage_id: info.usage.id,
            supported_input: info.supported_input,
            system_buttons_or_capabilities: info.supported_system_buttons,
            v0_layout: false,
        },
        DeviceStrings {
            display_name: info.display_name,
        },
    ))
}

/// v3 `IGameInputDevice::GetDeviceInfo` (slot 3, v3:1157-1158), same shape as
/// v2 but transcribed from the v3 header (`displayName` at v3:853). Same
/// `HRESULT`-then-pointer discipline as v2.
fn read_device_info_v3(device: *mut c_void) -> Option<(DeviceNumeric, DeviceStrings)> {
    let Some(vtbl) = vtbl_of::<DeviceVtblV2V3>(device) else {
        note!("device object or vtable pointer is null");
        return None;
    };
    let Some(get_device_info) = vtbl.get_device_info else {
        note!("device vtable missing GetDeviceInfo");
        return None;
    };
    let mut info: *const c_void = std::ptr::null();
    let hr = unsafe { get_device_info(device, &mut info) };
    if hr < 0 {
        note!("GetDeviceInfo failed: hr={}", hr_hex(hr));
        return None;
    }
    if info.is_null() {
        note!("GetDeviceInfo returned ok with a null info pointer");
        return None;
    }
    let info = unsafe { &*(info as *const GameInputDeviceInfoV3) };
    Some((
        DeviceNumeric {
            vendor_id: info.vendor_id,
            product_id: info.product_id,
            usage_id: info.usage.id,
            supported_input: info.supported_input,
            system_buttons_or_capabilities: info.supported_system_buttons,
            v0_layout: false,
        },
        DeviceStrings {
            display_name: info.display_name,
        },
    ))
}

/// Device info dispatch on the negotiated version.
fn read_device_info(device: *mut c_void) -> Option<(DeviceNumeric, DeviceStrings)> {
    match chosen_version() {
        ApiVersion::V0 => read_device_info_v0(device),
        ApiVersion::V2 => read_device_info_v2(device),
        ApiVersion::V3 => read_device_info_v3(device),
    }
}

/// `IUnknown::AddRef` (slot 1): keep the device alive across callbacks.
/// Returns the raw refcount the COM call reported (`None` when the object,
/// vtable, or slot is null/missing — that failure is printed here).
fn add_ref(device: *mut c_void) -> Option<u32> {
    let Some(vtbl) = vtbl_of::<IUnknownVtbl>(device) else {
        note!("device object or vtable pointer is null (AddRef skipped)");
        return None;
    };
    let Some(add_ref_fn) = vtbl.add_ref else {
        note!("device vtable missing AddRef");
        return None;
    };
    Some(unsafe { add_ref_fn(device) })
}

/// `IUnknown::Release` (slot 2): drop the create-reference after a successful
/// `QueryInterface` handed us a different object, and release readings each
/// polling pass.
fn release_interface(object: *mut c_void) {
    let Some(vtbl) = vtbl_of::<IUnknownVtbl>(object) else {
        note!("object or vtable pointer is null (Release skipped)");
        return;
    };
    let Some(release_fn) = vtbl.release else {
        note!("vtable missing Release");
        return;
    };
    unsafe { release_fn(object) };
}

/// Release an `IGameInputReading` via its vtable slot 2.
fn release_reading(reading: *mut c_void) {
    let Some(vtbl) = vtbl_of::<IUnknownVtbl>(reading) else {
        note!("reading object or vtable pointer is null (Release skipped)");
        return;
    };
    let Some(release_fn) = vtbl.release else {
        note!("reading vtable missing Release");
        return;
    };
    unsafe { release_fn(reading) };
}

/// `IUnknown::QueryInterface` (slot 0, inherited by every interface version).
/// `None` = object/slot missing; otherwise `(returned pointer, HRESULT)`.
fn query_interface(object: *mut c_void, riid: &Guid) -> Option<(*mut c_void, Hresult)> {
    let vtbl = vtbl_of::<IUnknownVtbl>(object)?;
    let query = vtbl.query_interface?;
    let mut ppv: *mut c_void = std::ptr::null_mut();
    let hr = unsafe { query(object, riid, &mut ppv) };
    Some((ppv, hr))
}

/// Version negotiation: `QueryInterface` slot 0 with `IID_IGameInput_v3`
/// (v3:1001-1003) first; if that fails, `IID_IGameInput_v2` (v2:853-855); if
/// that fails too, the created object is kept as v0 (v0:1083-1085). A
/// successful QI returns a pointer with its own reference, so the base object
/// is released only when the QI'd pointer differs from it; the chosen pointer
/// stays alive for the whole run (the PoC never exits cleanly).
fn negotiate(base: *mut c_void) -> (ApiVersion, *mut c_void) {
    if let Some((ppv, hr)) = query_interface(base, &IID_IGAMEINPUT_V3)
        && hr >= 0
        && !ppv.is_null()
    {
        note!("gameinput: interface version chosen = v3");
        if ppv != base {
            release_interface(base);
        }
        return (ApiVersion::V3, ppv);
    }
    if let Some((ppv, hr)) = query_interface(base, &IID_IGAMEINPUT_V2)
        && hr >= 0
        && !ppv.is_null()
    {
        note!("gameinput: interface version chosen = v2");
        if ppv != base {
            release_interface(base);
        }
        return (ApiVersion::V2, ppv);
    }
    note!("gameinput: interface version chosen = v0");
    (ApiVersion::V0, base)
}

/// Focus-gating fix: call `IGameInput::SetFocusPolicy` ONCE on the
/// negotiated interface before ANY registration. Under the default policy
/// `GameInputDefaultFocusPolicy = 0x0` (include/GameInput.h:47; v2:47) events
/// are delivered only to a foreground-focused app, so a windowless console
/// process registers callbacks successfully yet receives nothing. The v2/v3
/// background-enable bits (v3:51-53, v2:51-53) lift that
/// gate for input, Guide and Share; no enumeration-specific background flag
/// exists in any version's enum (v0:48-53, v1:44-54, v2:45-54, v3:45-54), so
/// device enumeration rides on `EnableBackgroundInput`. v0 HAS the slot (21)
/// but its enum (include/v0/GameInput.h:48-53) has no `EnableBackground*`
/// bits — v0's default already allows background input
/// (`GameInputDisableBackgroundInput = 0x1` is the opt-out) — so v0 is called
/// with policy 0x0 and the difference is noted.
fn set_focus_policy(object: *mut c_void, version: ApiVersion) {
    let (policy, slot) = match version {
        ApiVersion::V0 => (0, 21),
        ApiVersion::V2 => (FOCUS_POLICY_BACKGROUND, 16),
        ApiVersion::V3 => (FOCUS_POLICY_BACKGROUND, 16),
    };
    let set = match version {
        ApiVersion::V0 => vtbl_of::<GameInputVtblV0>(object).and_then(|vtbl| vtbl.set_focus_policy),
        ApiVersion::V2 => vtbl_of::<GameInputVtblV2>(object).and_then(|vtbl| vtbl.set_focus_policy),
        ApiVersion::V3 => vtbl_of::<GameInputVtblV3>(object).and_then(|vtbl| vtbl.set_focus_policy),
    };
    let Some(set) = set else {
        note!(
            "IGameInput object or vtable pointer is null; SetFocusPolicy ({} slot {slot}) skipped",
            version.label()
        );
        return;
    };
    if version == ApiVersion::V0 {
        note!(
            "gameinput: v0 enum has no EnableBackground* flags (include/v0/GameInput.h:48-53); \
             default policy already allows background input"
        );
    }
    unsafe { set(object, policy) }; // void return, no HRESULT to check
    note!("gameinput: focus policy set = 0x{policy:X} (slot {slot})");
}

/// The version-specific `RegisterSystemButtonCallback` slot (9) plus its
/// callback trampoline. `None` for v0, which has no `GameInputSystemButtons`
/// enum and no such slot — v0's guide-only entry point is slot 10,
/// `RegisterGuideButtonCallback` (v0:1136-1142). A missing slot or null object
/// is printed here.
fn system_button_slot_and_callback() -> Option<(SystemButtonRegisterFn, SystemButtonCallbackFn)> {
    let gameinput = GAMEINPUT.load(Ordering::Relaxed);
    match chosen_version() {
        ApiVersion::V0 => {
            note!("v0 has no RegisterSystemButtonCallback; guide-only path used instead");
            None
        }
        ApiVersion::V2 => {
            let Some(vtbl) = vtbl_of::<GameInputVtblV2>(gameinput) else {
                note!(
                    "IGameInput object or vtable pointer is null; system-button registration skipped",
                );
                return None;
            };
            let Some(register) = vtbl.register_system_button_callback else {
                note!("IGameInput vtable missing RegisterSystemButtonCallback");
                return None;
            };
            Some((register, on_system_button_event_v2))
        }
        ApiVersion::V3 => {
            let Some(vtbl) = vtbl_of::<GameInputVtblV3>(gameinput) else {
                note!(
                    "IGameInput object or vtable pointer is null; system-button registration skipped",
                );
                return None;
            };
            let Some(register) = vtbl.register_system_button_callback else {
                note!("IGameInput vtable missing RegisterSystemButtonCallback");
                return None;
            };
            Some((register, on_system_button_event_v3))
        }
    }
}

/// Call `RegisterSystemButtonCallback` (slot 9, v2:888-895, v3:1040-1045) with
/// `buttonFilter = Guide|Share` for `device` — NULL means events from ALL
/// devices (`_In_opt_ device`, v3:1041, v2:893). `None` = slot/object
/// unavailable (already printed); otherwise `(HRESULT, token)`.
fn call_register_system_button_callback(device: *mut c_void) -> Option<(Hresult, u64)> {
    let (register, callback) = system_button_slot_and_callback()?;
    let gameinput = GAMEINPUT.load(Ordering::Relaxed);
    let mut token: u64 = 0;
    let hr = unsafe {
        register(
            gameinput,
            device,
            SYSTEM_BUTTON_FILTER,
            std::ptr::null_mut(),
            callback,
            &mut token,
        )
    };
    Some((hr, token))
}

/// v2/v3: the FIRST registration of the run — system buttons from ALL devices
/// via `device = NULL`. This alone delivers Share/Guide without any device
/// enumeration, so a failure here is a decisive negative result. The success is
/// reported by the one summary note, not per token.
/// `None` = the negotiated interface lacks the slot (fatal for the library:
/// `Error::InterfaceUnavailable`); `Some(false)` = the call returned a failing
/// HRESULT, which the PoC tolerated and continued past.
fn register_system_button_callback_all_devices() -> Option<bool> {
    let (hr, _token) = call_register_system_button_callback(std::ptr::null_mut())?;
    if hr < 0 {
        note!(
            "RegisterSystemButtonCallback(device=NULL) failed: hr={}",
            hr_hex(hr)
        );
        return Some(false);
    }
    Some(true)
}

/// v2/v3: register `RegisterSystemButtonCallback` (slot 9, v2:888-895,
/// v3:1040-1045) for Guide|Share on one device. Called from the device
/// callback only when the all-devices (NULL) registration did NOT succeed:
/// with `GameInputBlockingEnumeration` GameInput runs device callbacks on the
/// calling thread, so this is a re-entrant call (see the PoC caveats).
fn register_system_button_callback(device: *mut c_void, name: &str) -> bool {
    let Some((hr, _token)) = call_register_system_button_callback(device) else {
        return false;
    };
    if hr < 0 {
        note!(
            "RegisterSystemButtonCallback failed on {name}: hr={}",
            hr_hex(hr)
        );
        return false;
    }
    note!("system-button callback registered on {name}");
    true
}

/// v0 slot 10 `RegisterGuideButtonCallback(device, context, callback, token)`
/// (v0:1136-1142). `None` = object/slot unavailable (already printed);
/// otherwise `(HRESULT, token)`.
fn call_register_guide_button_callback(device: *mut c_void) -> Option<(Hresult, u64)> {
    let gameinput = GAMEINPUT.load(Ordering::Relaxed);
    let Some(vtbl) = vtbl_of::<GameInputVtblV0>(gameinput) else {
        note!("IGameInput object or vtable pointer is null; guide-button registration skipped",);
        return None;
    };
    let Some(register) = vtbl.register_guide_button_callback else {
        note!("IGameInput vtable missing RegisterGuideButtonCallback");
        return None;
    };
    // Slot-typed binding: `GuideRegisterFnV0` is the header signature for v0 slot 10.
    let register: GuideRegisterFnV0 = register;
    let mut token: u64 = 0;
    let hr = unsafe {
        register(
            gameinput,
            device,
            std::ptr::null_mut(),
            on_guide_button_event_v0,
            &mut token,
        )
    };
    Some((hr, token))
}

/// v0: the FIRST registration of the run — guide events from ALL devices via
/// `device = NULL` (`_In_opt_ device` at v0:1137, so NULL is allowed). v0 has
/// no `GameInputSystemButtons` enum and no Share button (Share starts at v2,
/// v2:226-233); slot 10 is v0's only system-button entry point. The success is
/// reported by the one summary note, not per token.
/// `None` = the negotiated interface lacks the slot (`Error::InterfaceUnavailable`).
fn register_guide_button_callback_all_devices() -> Option<bool> {
    let (hr, _token) = call_register_guide_button_callback(std::ptr::null_mut())?;
    if hr < 0 {
        note!(
            "RegisterGuideButtonCallback(device=NULL) failed: hr={}",
            hr_hex(hr)
        );
        return Some(false);
    }
    Some(true)
}

/// v0: register `RegisterGuideButtonCallback` (slot 10, v0:1137-1142). v0 has
/// no `GameInputSystemButtons` enum and no `RegisterSystemButtonCallback`;
/// slot 10 `RegisterGuideButtonCallback(device, context, callback, token)` is
/// v0's only system-button entry point — Share support starts at v2
/// (v2:226-233). SDL relies on v0 for Guide, which this slot provides. Called
/// from the device callback only when the all-devices (NULL) registration did
/// NOT succeed.
fn register_guide_button_callback(device: *mut c_void, name: &str) -> bool {
    let Some((hr, _token)) = call_register_guide_button_callback(device) else {
        return false;
    };
    if hr < 0 {
        note!(
            "RegisterGuideButtonCallback failed on {name}: hr={}",
            hr_hex(hr)
        );
        return false;
    }
    note!("guide-button callback registered on {name}");
    true
}

/// Shared device-event body for the three version-specific callbacks.
///
/// Safety order: `AddRef` (`IUnknown` slot 1) keeps the
/// device alive across callbacks, `GetDeviceInfo` runs its version-specific
/// discipline (v2/v3: `HRESULT` checked before the out-param pointer is
/// touched; v0: the returned info pointer null-checked), numeric fields are read
/// FIRST and the raw `displayName` pointer LAST through the null-guarded
/// `safe_cstr`.
///
/// Output: one `device_connected` event per device pointer — the registry
/// dedups by pointer, so a device reported by several kind filters or by a
/// later status change emits nothing again. The per-device system-button
/// registration is skipped when the all-devices (`device = NULL`) registration
/// succeeded.
fn handle_device_event(
    version: ApiVersion,
    _token: u64,
    device: *mut c_void,
    _timestamp: u64,
    _current_status: i32,
    _previous_status: i32,
) {
    // `AddRef` (`IUnknown` slot 1): keep the device alive; its refcount is not
    // printed (a null object/vtable is reported inside `add_ref`).
    let _ = add_ref(device);
    // `known` plus the registry length: the length is the index a NEW device
    // occupies when it is pushed below, which is the id the observer gets.
    let (known, index) = match DEVICES.lock() {
        Ok(devices) => (
            devices.iter().any(|entry| entry.device == device),
            devices.len(),
        ),
        Err(_) => (false, 0),
    };
    // Numeric fields first (all zeros when the info block is unavailable), then
    // the raw string pointer. v0 has no `supportedSystemButtons` field, so its
    // `capabilities` mask prints in that position with a note.
    let (numeric, strings) = match read_device_info(device) {
        Some(pair) => pair,
        None => (
            DeviceNumeric::fallback(version == ApiVersion::V0),
            DeviceStrings::fallback(),
        ),
    };
    let name = safe_cstr(strings.display_name);
    if !known {
        // The PoC's one `device:` line per new device pointer becomes one
        // `device_connected` event, carrying the same numeric fields.
        if numeric.v0_layout {
            note!(
                "v0 info block has no supportedSystemButtons; capabilities mask reported instead"
            );
        }
        emit(|observer| {
            observer.device_connected(&DeviceInfo {
                index,
                name: name.clone(),
                vendor_id: numeric.vendor_id,
                product_id: numeric.product_id,
                usage_id: numeric.usage_id,
                supported_input: numeric.supported_input,
                supported_system_buttons: numeric.system_buttons_or_capabilities,
            })
        });
    }

    // With the all-devices (NULL) callback registered, a per-device
    // registration would only duplicate callbacks, so it is skipped; the
    // per-device path stays the fallback when the NULL registration failed.
    let global_sysbtn = GLOBAL_SYSBTN_REGISTERED.load(Ordering::Relaxed);
    if !known && !global_sysbtn {
        match version {
            // v0: no system-button bitmask exists; register the Guide-only
            // callback for controller-capable devices (slot 10, v0:1137-1142).
            ApiVersion::V0 => {
                if numeric.supported_input & KIND_CONTROLLER != 0 {
                    register_guide_button_callback(device, &name);
                }
            }
            ApiVersion::V2 | ApiVersion::V3 => {
                if numeric.system_buttons_or_capabilities & SYSTEM_BUTTON_FILTER != 0 {
                    register_system_button_callback(device, &name);
                }
            }
        }
    }

    match DEVICES.lock() {
        Ok(mut devices) => {
            if !devices.iter().any(|entry| entry.device == device) {
                devices.push(RegisteredDevice {
                    device,
                    name,
                    supported_input: numeric.supported_input,
                    last_buttons: 0,
                    last_error: 0,
                });
            }
        }
        Err(_) => note!("device registry poisoned"),
    }
}

/// v0 `GameInputDeviceCallback` (v0:537-543).
unsafe extern "system" fn on_device_event_v0(
    token: u64,
    _context: *mut c_void,
    device: *mut c_void,
    timestamp: u64,
    current_status: i32,
    previous_status: i32,
) {
    handle_device_event(
        ApiVersion::V0,
        token,
        device,
        timestamp,
        current_status,
        previous_status,
    );
}

/// v2 `GameInputDeviceCallback` (v2:453-459).
unsafe extern "system" fn on_device_event_v2(
    token: u64,
    _context: *mut c_void,
    device: *mut c_void,
    timestamp: u64,
    current_status: i32,
    previous_status: i32,
) {
    handle_device_event(
        ApiVersion::V2,
        token,
        device,
        timestamp,
        current_status,
        previous_status,
    );
}

/// v3 `GameInputDeviceCallback` (v3:566-572).
unsafe extern "system" fn on_device_event_v3(
    token: u64,
    _context: *mut c_void,
    device: *mut c_void,
    timestamp: u64,
    current_status: i32,
    previous_status: i32,
) {
    handle_device_event(
        ApiVersion::V3,
        token,
        device,
        timestamp,
        current_status,
        previous_status,
    );
}

/// Shared system-button body for the v2/v3 callbacks: a bit set in
/// `current_buttons` means pressed, so a changed bit maps straight to DOWN/UP.
/// `GameInputIgnorePrevious`
/// gives `previous = 0`, so the first report prints the current state instead of
/// inventing an edge. The device pointer is used ONLY for a registry name lookup
/// (`registry_name` compares pointers, never dereferences): a device the device
/// callback has not reported yet prints `<null>`.
fn handle_system_button_event(
    _token: u64,
    device: *mut c_void,
    _timestamp: u64,
    current: i32,
    previous: i32,
) {
    let changed = current ^ previous;
    let name = registry_name(device);
    // Same detection as the PoC: a changed bit present in `current` is DOWN,
    // otherwise UP. The PoC's `[timestamp] name share/guide EDGE` lines become
    // `system_button` events in the same order (Share first, then Guide); the
    // timestamp is not part of the observer API.
    if changed & SYSTEM_BUTTON_SHARE != 0 {
        let down = current & SYSTEM_BUTTON_SHARE != 0;
        emit(|observer| observer.system_button(SystemButton::Share, down, &name));
    }
    if changed & SYSTEM_BUTTON_GUIDE != 0 {
        let down = current & SYSTEM_BUTTON_GUIDE != 0;
        emit(|observer| observer.system_button(SystemButton::Guide, down, &name));
    }
}

/// v2 `GameInputSystemButtonCallback` (v2:461-467).
unsafe extern "system" fn on_system_button_event_v2(
    token: u64,
    _context: *mut c_void,
    device: *mut c_void,
    timestamp: u64,
    current_buttons: i32,
    previous_buttons: i32,
) {
    handle_system_button_event(token, device, timestamp, current_buttons, previous_buttons);
}

/// v3 `GameInputSystemButtonCallback` (v3:574-580).
unsafe extern "system" fn on_system_button_event_v3(
    token: u64,
    _context: *mut c_void,
    device: *mut c_void,
    timestamp: u64,
    current_buttons: i32,
    previous_buttons: i32,
) {
    handle_system_button_event(token, device, timestamp, current_buttons, previous_buttons);
}

/// v0 `GameInputGuideButtonCallback` (v0:545-551): `bool isPressed` — v0
/// carries no previous/current bitmask and no Share button. The name comes from
/// the registry only, so the device pointer is never dereferenced here.
unsafe extern "system" fn on_guide_button_event_v0(
    _token: u64,
    _context: *mut c_void,
    device: *mut c_void,
    _timestamp: u64,
    is_pressed: bool,
) {
    let name = registry_name(device);
    let down = is_pressed;
    emit(|observer| observer.system_button(SystemButton::Guide, down, &name));
}

/// `IGameInput::GetCurrentReading` is slot 4 in every version (v0:1099-1103,
/// v2:863-867, v3:1007-1010); dispatch on the negotiated layout.
fn get_current_reading_slot(object: *mut c_void) -> Option<GameInputReadingFn> {
    match chosen_version() {
        ApiVersion::V0 => vtbl_of::<GameInputVtblV0>(object)?.get_current_reading,
        ApiVersion::V2 => vtbl_of::<GameInputVtblV2>(object)?.get_current_reading,
        ApiVersion::V3 => vtbl_of::<GameInputVtblV3>(object)?.get_current_reading,
    }
}

/// Shared signature of the version-specific `GetCurrentReading` slots.
type GameInputReadingFn = unsafe extern "system" fn(
    this: *mut c_void,
    input_kind: i32,
    device: *mut c_void,
    reading: *mut *mut c_void,
) -> Hresult;

/// Print the "unsupported" note once and make `poll_loop` skip all further
/// passes. All three mirrored layouts expose button slots, so this only fires
/// for an unexpectedly truncated vtable.
fn mark_polling_unsupported() {
    if !POLL_UNSUPPORTED_PRINTED.swap(true, Ordering::Relaxed) {
        note!("button polling unsupported on {}", chosen_version().label());
    }
}

/// Controller button count/state slots on the reading object, per version:
/// v0 slots 10/11 (v0:1219-1223), v2/v3 slots 8/9 (v2:938-942, v3:1115-1119).
/// `None` = null reading/vtable or missing slots; the exact failure is printed
/// here, and missing slots permanently skip polling for the chosen version.
fn button_slots(reading: *mut c_void) -> Option<(ButtonCountFn, ButtonStateFn)> {
    let (count_slot, state_slot) = match chosen_version() {
        ApiVersion::V0 => {
            let Some(vtbl) = vtbl_of::<ReadingVtblV0>(reading) else {
                note!("reading object or vtable pointer is null");
                return None;
            };
            (
                vtbl.get_controller_button_count,
                vtbl.get_controller_button_state,
            )
        }
        ApiVersion::V2 | ApiVersion::V3 => {
            let Some(vtbl) = vtbl_of::<ReadingVtblV2V3>(reading) else {
                note!("reading object or vtable pointer is null");
                return None;
            };
            (
                vtbl.get_controller_button_count,
                vtbl.get_controller_button_state,
            )
        }
    };
    let Some(count) = count_slot else {
        note!("reading vtable missing GetControllerButtonCount");
        mark_polling_unsupported();
        return None;
    };
    let Some(state) = state_slot else {
        note!("reading vtable missing GetControllerButtonState");
        mark_polling_unsupported();
        return None;
    };
    Some((count, state))
}

type ButtonCountFn = unsafe extern "system" fn(this: *mut c_void) -> u32;
type ButtonStateFn =
    unsafe extern "system" fn(this: *mut c_void, count: u32, states: *mut u8) -> u32;

/// Outcome of one button poll of one registry entry, collected by `poll_pass`
/// and delivered to the observer after the registry lock is released.
enum PollOutcome {
    /// Nothing to report: null pointer (including a pointer nulled by an
    /// earlier disconnect), no reading, or a mask identical to the last one.
    Nothing,
    /// The state mask changed. `bits` is the NEW mask and `pressed` lists the
    /// currently pressed indices of that mask — a STATE change, not a 0→1 diff
    /// (a release changes the mask too). Consumers needing DOWN edges diff
    /// masks themselves (`bits & !previous`).
    Changed(u64, Vec<u8>),
    /// `GAMEINPUT_E_DEVICE_DISCONNECTED`: the device pointer was nulled, the
    /// registry entry stays (index stable), and no further poll reports
    /// anything for it — so this is delivered exactly once per entry.
    Disconnected,
}

/// Poll one controller device's button state. The PoC printed a changed mask
/// line; the library returns the new mask plus its pressed indices instead, and
/// `poll_pass` delivers it to the observer AFTER the registry lock is released
/// (so an observer may call `button_label`, which locks the registry itself).
/// LT/RT are analog axes (`GetControllerAxisState`), not controller buttons —
/// intentionally out of scope.
fn poll_device_buttons(entry: &mut RegisteredDevice) -> PollOutcome {
    let gameinput = GAMEINPUT.load(Ordering::Relaxed);
    if entry.device.is_null() {
        return PollOutcome::Nothing;
    }
    let Some(get_current_reading) = get_current_reading_slot(gameinput) else {
        note!("IGameInput vtable missing GetCurrentReading");
        return PollOutcome::Nothing;
    };

    // Controller-ish kinds only (axes + buttons + switches), masked by what the
    // device actually supports.
    let kind = entry.supported_input & KIND_CONTROLLER;
    let mut reading: *mut c_void = std::ptr::null_mut();
    let hr = unsafe { get_current_reading(gameinput, kind, entry.device, &mut reading) };
    if hr < 0 {
        let code = hr as u32;
        if code == E_DEVICE_DISCONNECTED {
            note!("{} disconnected (hr={}); dropping", entry.name, hr_hex(hr));
            entry.device = std::ptr::null_mut();
            return PollOutcome::Disconnected;
        }
        // `GAMEINPUT_E_READING_NOT_FOUND` is normal while idle; other codes are
        // printed once per code so the 50 ms loop does not spam.
        if code != E_READING_NOT_FOUND && entry.last_error != code {
            entry.last_error = code;
            note!(
                "GetCurrentReading failed on {}: hr={}",
                entry.name,
                hr_hex(hr)
            );
        }
        return PollOutcome::Nothing;
    }
    if reading.is_null() {
        // Never hand a null reading to the button slots below.
        return PollOutcome::Nothing;
    }

    let Some((button_count, button_state)) = button_slots(reading) else {
        release_reading(reading);
        return PollOutcome::Nothing;
    };

    // `GetControllerButtonCount` dereferences the reading: non-null checked
    // above and the slot itself is checked by `button_slots`.
    let raw_count = unsafe { button_count(reading) };
    let count = std::cmp::min(raw_count as usize, MAX_BUTTON_COUNT);
    if count == 0 {
        // Skip `GetControllerButtonState` entirely: an empty `Vec` would hand
        // it a dangling pointer, and there is nothing to read anyway.
        release_reading(reading);
        return PollOutcome::Nothing;
    }
    // `GetControllerButtonState` dereferences the reading (non-null checked
    // above) and the state buffer, whose pointer is valid for `count` > 0.
    let mut states: Vec<u8> = vec![0u8; count];
    let written = unsafe { button_state(reading, count as u32, states.as_mut_ptr()) };
    // Release the reading every pass, per the header contract.
    release_reading(reading);

    let filled = std::cmp::min(written as usize, count);
    let mut mask: u64 = 0;
    for (index, pressed) in states.iter().take(filled).enumerate() {
        if *pressed != 0 {
            mask |= 1 << index;
        }
    }
    if mask != entry.last_buttons {
        entry.last_buttons = mask;
        let pressed: Vec<u8> = (0..filled)
            .filter(|index| mask & (1 << index) != 0)
            .map(|index| index as u8)
            .collect();
        PollOutcome::Changed(mask, pressed)
    } else {
        PollOutcome::Nothing
    }
}

/// One button-polling pass over the device registry (per-version reading
/// slots, unchanged). Used by the polling thread AND by the dispatcher pump
/// loop, at `POLL_INTERVAL` cadence.
fn poll_pass() {
    // If the chosen version lacks controller button state methods, polling
    // is skipped (the note was printed once by `mark_polling_unsupported`).
    if POLL_UNSUPPORTED_PRINTED.load(Ordering::Relaxed) {
        return;
    }
    let mut changes: Vec<(usize, u64, Vec<u8>)> = Vec::new();
    let mut disconnected: Vec<(usize, String)> = Vec::new();
    match DEVICES.lock() {
        Ok(mut devices) => {
            // Registry index identifies the device to the observer; entries are
            // never removed (a disconnect nulls the pointer and the entry stays),
            // so an index is stable for the whole run.
            for (index, entry) in devices.iter_mut().enumerate() {
                if entry.supported_input & KIND_CONTROLLER_BUTTON == 0 {
                    continue;
                }
                match poll_device_buttons(entry) {
                    PollOutcome::Changed(mask, pressed) => changes.push((index, mask, pressed)),
                    PollOutcome::Disconnected => disconnected.push((index, entry.name.clone())),
                    PollOutcome::Nothing => {}
                }
            }
        }
        Err(_) => note!("device registry poisoned"),
    }
    // Delivered AFTER the registry lock is dropped, so an observer may call
    // `button_label` (which locks the registry) without deadlocking. Button
    // changes first, then the disconnects of the same pass: a disconnect is the
    // last event for that entry.
    for (index, bits, pressed) in changes {
        emit(|observer| observer.buttons_changed(index, bits, &pressed));
    }
    for (index, name) in disconnected {
        emit(|observer| observer.device_disconnected(index, &name));
    }
}

/// Button polling thread body (fallback mode, no dispatcher): one pass over
/// the registry per `POLL_INTERVAL`, until `stop` is set or the GameInput
/// object pointer is cleared.
fn poll_loop(stop: &AtomicBool) {
    while !GAMEINPUT.load(Ordering::Relaxed).is_null() && !stop.load(Ordering::Relaxed) {
        sleep(POLL_INTERVAL);
        poll_pass();
    }
}

/// Device count for the heartbeat, tolerating a poisoned mutex.
fn device_count() -> usize {
    match DEVICES.lock() {
        Ok(guard) => guard.len(),
        Err(_) => 0,
    }
}

/// The version-specific `RegisterDeviceCallback` slot: slot 9 in v0
/// (v0:1127-1135) and slot 8 in v2/v3 (v2:883-888, v3:1031-1038). Signatures
/// are identical across versions (`DeviceRegisterFn`).
fn device_callback_slot(object: *mut c_void, version: ApiVersion) -> Option<DeviceRegisterFn> {
    match version {
        ApiVersion::V0 => {
            vtbl_of::<GameInputVtblV0>(object).and_then(|vtbl| vtbl.register_device_callback)
        }
        ApiVersion::V2 => {
            vtbl_of::<GameInputVtblV2>(object).and_then(|vtbl| vtbl.register_device_callback)
        }
        ApiVersion::V3 => {
            vtbl_of::<GameInputVtblV3>(object).and_then(|vtbl| vtbl.register_device_callback)
        }
    }
}

/// One `RegisterDeviceCallback` registration with `device = NULL` (all
/// devices), an explicit kind + enumeration filter, and `statusFilter` =
/// `GameInputDeviceConnected` (0x1: v0:221, v2:238, v3:349) for the blocking
/// variant (SDL's proven sequence, `SDL_gameinputjoystick.c:268-276`) or
/// `GameInputDeviceAnyStatus` for the async variants; failures are printed, and
/// successes are covered by the one summary line in `run_gameinput`.
///
/// Three kind filters are registered because an Xbox gamepad advertises the specific
/// `GameInputKindGamepad` bit (0x40000 — v3:32, v2:31, v0:34) alongside the
/// generic Controller bits, so a device whose `supportedInput` lacks 0x0E
/// never matches a `GameInputKindController` registration — hence the three
/// async variants (Controller, Gamepad, ControllerButton).
fn register_device_callback_variant(
    object: *mut c_void,
    version: ApiVersion,
    kind: i32,
    enumeration_kind: i32,
) -> Option<bool> {
    // `None` = the negotiated interface lacks `RegisterDeviceCallback`
    // (`Error::InterfaceUnavailable`); `Some(false)` = a failing HRESULT, which
    // the PoC tolerated and continued past.
    let Some(register) = device_callback_slot(object, version) else {
        note!("IGameInput object or vtable pointer is null");
        return None;
    };
    let blocking = enumeration_kind == ENUMERATION_BLOCKING;
    let status_filter = if blocking {
        DEVICE_CONNECTED
    } else if version == ApiVersion::V0 {
        DEVICE_ANY_STATUS_V0
    } else {
        DEVICE_ANY_STATUS_V2_V3
    };
    let callback: DeviceCallbackFn = match version {
        ApiVersion::V0 => on_device_event_v0,
        ApiVersion::V2 => on_device_event_v2,
        ApiVersion::V3 => on_device_event_v3,
    };
    let enum_label = if enumeration_kind == ENUMERATION_BLOCKING {
        "blocking"
    } else {
        "async"
    };
    let mut token: u64 = 0;
    let hr = unsafe {
        register(
            object,
            std::ptr::null_mut(),
            kind,
            status_filter,
            enumeration_kind,
            std::ptr::null_mut(),
            callback,
            &mut token,
        )
    };
    if hr < 0 {
        note!(
            "RegisterDeviceCallback(kind=0x{kind:X}, enumeration={enum_label}) failed: hr={}",
            hr_hex(hr)
        );
        return Some(false);
    }
    Some(true)
}

/// `IGameInput::CreateDispatcher`: slot 13 in v3 (v3:1059-1060) and slot 13 in
/// v2 (v2:911-912 — same order as v3, no `GetTemporalReading`), slot 14 in v0
/// (v0:1155-1156, after `UnregisterCallback(token, timeout)`). Returns the
/// dispatcher object; failures are printed and the caller falls back to
/// GameInput's internal callback threads.
fn create_dispatcher(object: *mut c_void, version: ApiVersion) -> Option<*mut c_void> {
    let (create, slot) = match version {
        ApiVersion::V0 => (
            vtbl_of::<GameInputVtblV0>(object).and_then(|vtbl| vtbl.create_dispatcher),
            14,
        ),
        ApiVersion::V2 => (
            vtbl_of::<GameInputVtblV2>(object).and_then(|vtbl| vtbl.create_dispatcher),
            13,
        ),
        ApiVersion::V3 => (
            vtbl_of::<GameInputVtblV3>(object).and_then(|vtbl| vtbl.create_dispatcher),
            13,
        ),
    };
    let Some(create) = create else {
        note!(
            "IGameInput object or vtable pointer is null; CreateDispatcher ({} slot {slot}) skipped",
            version.label()
        );
        return None;
    };
    let mut dispatcher: *mut c_void = std::ptr::null_mut();
    let hr = unsafe { create(object, &mut dispatcher) };
    if hr < 0 {
        note!(
            "CreateDispatcher failed ({} slot {slot}): hr={}",
            version.label(),
            hr_hex(hr)
        );
        return None;
    }
    if dispatcher.is_null() {
        note!(
            "CreateDispatcher succeeded ({} slot {slot}) with a null dispatcher pointer",
            version.label()
        );
        return None;
    }
    note!(
        "gameinput: dispatcher created ({} slot {slot})",
        version.label()
    );
    Some(dispatcher)
}

/// Dispatcher pump mode: GameInput runs registered callbacks on the thread
/// that calls `IGameInputDispatcher::Dispatch(quotaInMicroseconds)` (slot 3,
/// `DispatcherVtbl`; the header returns `bool`). Without a pump — and without
/// blocking enumeration — a process receives no callback events at all. Each
/// pump executes pending callbacks
/// within `DISPATCH_QUOTA_MICROSECONDS`; the loop pumps every
/// `DISPATCH_INTERVAL`, polls buttons every `POLL_INTERVAL` on the same
/// thread, and emits one `heartbeat` event every `HEARTBEAT_INTERVAL`.
/// Returns `false` only when `Dispatch` is missing from the dispatcher vtable;
/// `true` when the pump ends (stop flag set, or the GameInput object cleared).
/// The PoC's loop never returned — Ctrl+C killed the process — so the stop flag
/// is the library's only way to leave this loop.
fn pump_loop(dispatcher: *mut c_void, stop: &AtomicBool) -> bool {
    let Some(dispatch) = vtbl_of::<DispatcherVtbl>(dispatcher).and_then(|vtbl| vtbl.dispatch)
    else {
        note!("dispatcher object or vtable pointer is null; Dispatch unavailable");
        return false;
    };
    let mut ticks: u64 = 0;
    let mut pumped_once = false;
    let mut last_poll = Instant::now();
    let mut last_beat = Instant::now();
    while !GAMEINPUT.load(Ordering::Relaxed).is_null() && !stop.load(Ordering::Relaxed) {
        sleep(DISPATCH_INTERVAL);
        let dispatched = unsafe { dispatch(dispatcher, DISPATCH_QUOTA_MICROSECONDS) };
        ticks += 1;
        if dispatched && !pumped_once {
            pumped_once = true;
            note!("gameinput: Dispatch executed callbacks on the pump thread");
        }
        if last_poll.elapsed() >= POLL_INTERVAL {
            last_poll = Instant::now();
            poll_pass();
        }
        if last_beat.elapsed() >= HEARTBEAT_INTERVAL {
            last_beat = Instant::now();
            let devices = device_count();
            emit(|observer| observer.heartbeat(devices, ticks));
        }
    }
    true
}

/// Fallback mode (dispatcher creation failed): callbacks run on GameInput's
/// internal threads, polling stays on its own thread, and the heartbeat reports
/// device count only (no dispatch ticks). `thread::scope` borrows the caller's
/// stop flag for the polling thread and joins it when this loop ends.
fn fallback_loop(stop: &AtomicBool) {
    note!("gameinput: no dispatcher; callbacks rely on GameInput's internal threads");
    std::thread::scope(|scope| {
        scope.spawn(|| poll_loop(stop));
        while !GAMEINPUT.load(Ordering::Relaxed).is_null() && !stop.load(Ordering::Relaxed) {
            sleep(HEARTBEAT_INTERVAL);
            let devices = device_count();
            emit(|observer| observer.heartbeat(devices, 0));
        }
    });
}

/// v3 `GameInputControllerInfo`, transcribed from the vendored header
/// (`vendor/GameInput.h:692-699`: `controllerAxisCount`@0 (line 694),
/// `controllerAxisLabels`@8, `controllerButtonCount`@16,
/// `controllerButtonCount`@16 (line 696),
/// `controllerButtonLabels`@24 (line 697), `controllerSwitchCount`@32,
/// `controllerSwitchInfo`@40). `controllerButtonLabels` is a
/// `const GameInputLabel*` (:697) — one 4-byte enum value per button.
#[repr(C)]
struct ControllerInfoV3 {
    controller_axis_count: u32,
    controller_axis_labels: *const c_void,
    controller_button_count: u32,
    controller_button_labels: *const i32,
    controller_switch_count: u32,
    controller_switch_info: *const c_void,
}

/// Layout asserts for the label block, mirroring the `layout.rs` style.
const _: () = assert!(std::mem::size_of::<ControllerInfoV3>() == 48);
const _: () = assert!(std::mem::offset_of!(ControllerInfoV3, controller_button_count) == 16);
const _: () = assert!(std::mem::offset_of!(ControllerInfoV3, controller_button_labels) == 24);

/// v2 per-button entry, transcribed from the SDK v2 header
/// (`include/v2/GameInput.h:582-586`: `GameInputControllerButtonInfo {
/// GameInputKind mappedInputKinds; GameInputLabel label; }`) — 8 bytes, label
/// at offset 4.
#[repr(C)]
struct ControllerButtonInfoV2 {
    mapped_input_kinds: i32,
    label: i32,
}

const _: () = assert!(std::mem::size_of::<ControllerButtonInfoV2>() == 8);
const _: () = assert!(std::mem::offset_of!(ControllerButtonInfoV2, label) == 4);

/// `IGameInputDevice::GetDeviceInfo` (slot 3, v2:983-984 / v3:1157-1158),
/// shared by the label lookups: same discipline as `read_device_info_v2/v3` —
/// `HRESULT` checked BEFORE the out-param is used, pointer null-checked before
/// any dereference.
fn device_info_pointer(device: *mut c_void) -> Option<*const c_void> {
    let vtbl = vtbl_of::<DeviceVtblV2V3>(device)?;
    let get_device_info = vtbl.get_device_info?;
    let mut info: *const c_void = std::ptr::null();
    let hr = unsafe { get_device_info(device, &mut info) };
    if hr < 0 {
        note!("GetDeviceInfo failed: hr={}", hr_hex(hr));
        return None;
    }
    if info.is_null() {
        note!("GetDeviceInfo returned ok with a null info pointer");
        return None;
    }
    Some(info)
}

/// v3 label: `GameInputDeviceInfo.controllerInfo` (vendored header :859,
/// mirrored offset 168 — asserted in `layout.rs`) -> the `GameInputLabel` at
/// `button_index`, bounded by `controllerButtonCount`.
fn controller_label_v3(device: *mut c_void, button_index: u8) -> Option<i32> {
    let info = device_info_pointer(device)?;
    let info = unsafe { &*(info as *const GameInputDeviceInfoV3) };
    let controller = info.controller_info;
    if controller.is_null() {
        return None;
    }
    let controller = unsafe { &*(controller as *const ControllerInfoV3) };
    let count = std::cmp::min(
        controller.controller_button_count as usize,
        MAX_BUTTON_COUNT,
    );
    if button_index as usize >= count {
        return None;
    }
    let labels = controller.controller_button_labels;
    if labels.is_null() {
        return None;
    }
    Some(unsafe { *labels.add(button_index as usize) })
}

/// v2 label: `controllerButtonCount` (offset 224) and
/// `controllerButtonInfo` (offset 232), both asserted in `layout.rs` -> the
/// `label` field of the entry at `button_index`.
fn controller_label_v2(device: *mut c_void, button_index: u8) -> Option<i32> {
    let info = device_info_pointer(device)?;
    let info = unsafe { &*(info as *const GameInputDeviceInfoV2) };
    let count = std::cmp::min(info.controller_button_count as usize, MAX_BUTTON_COUNT);
    if button_index as usize >= count {
        return None;
    }
    let buttons = info.controller_button_info;
    if buttons.is_null() {
        return None;
    }
    let entry = unsafe { &*(buttons as *const ControllerButtonInfoV2).add(button_index as usize) };
    Some(entry.label)
}

/// Additive label lookup for one button of a registered device.
///
/// `IGameInputDevice` has NO `GetButtonLabel` slot: the vendored header lists
/// slots 3-17 (`vendor/GameInput.h:1155-1217` — GetDeviceInfo,
/// GetCurrentReading, GetReadingTimestamp, GetDeviceStatus, ...,
/// SendRawDeviceOutput) and none of them is a label getter; `GetButtonLabel` is
/// the WinRT `Windows.Gaming.Input` name (`AGENTS.md` / `docs/spec.md` used it
/// for the planned `windows`-crate path). Labels therefore come from the device
/// info block instead — v3 via `controllerInfo` (:692-699), v2 via
/// `controllerButtonInfo` (:582-586). v0 mirrors no per-button label path, so
/// it returns `None`.
///
/// `device_index` is the registry index from `DeviceInfo`/`buttons_changed`; a
/// disconnected (nulled) device, an out-of-range index, and an unlabeled button
/// (`GameInputLabelNone` / `Unknown`) all return `None`.
pub(crate) fn button_label(device_index: usize, button_index: u8) -> Option<String> {
    let device = DEVICES.lock().ok()?.get(device_index)?.device;
    if device.is_null() {
        return None;
    }
    let label = match chosen_version() {
        ApiVersion::V0 => return None,
        ApiVersion::V2 => controller_label_v2(device, button_index)?,
        ApiVersion::V3 => controller_label_v3(device, button_index)?,
    };
    button_name(label).map(str::to_owned)
}

/// Library entry point: init, focus policy, registrations, then the pump —
/// the PoC's `run_gameinput` order, unchanged: load the DLL, resolve exports,
/// create the `IGameInput` object, negotiate v3/v2/v0, set the focus policy
/// BEFORE any registration, register system buttons (`device = NULL`), register
/// device callbacks (three async kinds + one blocking Connected), then pump.
///
/// Failure mapping (the PoC's print-and-exit points):
/// * `Error::LibraryUnavailable` — `gameinput.dll` cannot be loaded, or it
///   exports neither `GameInputCreate` nor `GameInputInitialize`.
/// * `Error::InitFailed(_)` — object creation failed or handed back a null
///   `IGameInput` pointer (the PoC's nonzero-exit cases).
/// * `Error::InterfaceUnavailable` — the negotiated interface lacks a callback
///   slot this library drives (the PoC reported and exited cleanly).
///
/// Returns `Ok(())` when `stop` is set (or the object pointer is cleared).
/// GameInput state lives in process-wide statics, so `run` is designed for one
/// call per process; a second call would re-register callbacks on the same
/// interface.
pub(crate) fn run(observer: &dyn Observer, stop: &AtomicBool) -> Result<(), Error> {
    set_observer(observer);

    let module_name = utf16_nul("gameinput.dll");
    let module = unsafe { LoadLibraryW(module_name.as_ptr()) };
    if module.is_null() {
        note!("LoadLibraryW(\"gameinput.dll\") failed: module missing");
        return Err(Error::LibraryUnavailable);
    }

    // `GameInputCreate` FIRST: the Windows inbox gameinput.dll exports only
    // this symbol (it hands back the v0 interface). `GameInputInitialize` is
    // the fallback (the SDK DLL exports both; its GameInputCreate delegates
    // to CreateWithVersion(IID_IGameInput_v0), which itself prefers
    // Initialize — lib/GameInput.cpp:472-535).
    let create_ptr = unsafe { GetProcAddress(module, CREATE_EXPORT.as_ptr()) };
    let initialize_ptr = unsafe { GetProcAddress(module, INITIALIZE_EXPORT.as_ptr()) };
    if create_ptr.is_null() && initialize_ptr.is_null() {
        note!(
            "GetProcAddress(\"GameInputCreate\") and GetProcAddress(\"GameInputInitialize\") failed: exports missing"
        );
        return Err(Error::LibraryUnavailable);
    }

    let mut base: *mut c_void = std::ptr::null_mut();
    if !create_ptr.is_null() {
        note!("gameinput: export used = GameInputCreate");
        let create = unsafe { std::mem::transmute::<*mut c_void, GameInputCreateFn>(create_ptr) };
        let hr = unsafe { create(&mut base) };
        if hr < 0 {
            note!("GameInputCreate failed: hr={}", hr_hex(hr));
            return Err(Error::InitFailed("GameInputCreate failed"));
        }
        if base.is_null() {
            note!("GameInputCreate succeeded with a null IGameInput pointer");
            return Err(Error::InitFailed(
                "GameInputCreate returned a null IGameInput pointer",
            ));
        }
    } else {
        note!("gameinput: export used = GameInputInitialize");
        let initialize =
            unsafe { std::mem::transmute::<*mut c_void, GameInputInitializeFn>(initialize_ptr) };
        // Ask for the newest IID the export honors; QI negotiation below
        // settles the layout regardless of which create call succeeded.
        for (iid, label) in [
            (&IID_IGAMEINPUT_V3, "v3"),
            (&IID_IGAMEINPUT_V2, "v2"),
            (&IID_IGAMEINPUT_V0, "v0"),
        ] {
            let hr = unsafe { initialize(iid, &mut base) };
            if hr >= 0 {
                if base.is_null() {
                    note!("GameInputInitialize succeeded with a null IGameInput pointer");
                } else {
                    note!("gameinput: created via GameInputInitialize ({label})");
                    break;
                }
            } else {
                note!("GameInputInitialize({label}) failed: hr={}", hr_hex(hr));
            }
            base = std::ptr::null_mut();
        }
        if base.is_null() {
            return Err(Error::InitFailed(
                "GameInputInitialize returned a null IGameInput pointer",
            ));
        }
    }

    // QI slot 0 negotiation: v3 -> v2 -> v0 (IIDs cited on the statics).
    let (version, object) = negotiate(base);
    GAMEINPUT.store(object, Ordering::Relaxed);
    VERSION.store(version.as_u8(), Ordering::Relaxed);

    // Focus gating, BEFORE any registration: the default focus policy
    // (GameInputDefaultFocusPolicy = 0x0, include/GameInput.h:45-56) delivers
    // events only to the foreground-focused app, which is why the PoC
    // registered everything but dispatched nothing. Set the background policy
    // once on the negotiated interface (see `set_focus_policy`).
    set_focus_policy(object, version);

    // Registration order, per the headers:
    // 1. System buttons from ALL devices FIRST — `device = NULL` is allowed in
    //    every version (`_In_opt_ device`: v3:1041, v2:893, v0:1137). v2/v3 use
    //    slot 9 `RegisterSystemButtonCallback(buttonFilter = Guide|Share)`
    //    (v3:1040-1046, v2:892-898); v0 has no system-button bitmask, so slot
    //    10 `RegisterGuideButtonCallback` (v0:1136-1142) is the equivalent and
    //    delivers Guide only (Share starts at v2, v2:226-233). This
    //    registration alone should produce Share/Guide events even if no device
    //    callback ever fires.
    let sysbtn_registered = match version {
        ApiVersion::V0 => register_guide_button_callback_all_devices(),
        ApiVersion::V2 | ApiVersion::V3 => register_system_button_callback_all_devices(),
    };
    let Some(sysbtn_registered) = sysbtn_registered else {
        return Err(Error::InterfaceUnavailable);
    };
    GLOBAL_SYSBTN_REGISTERED.store(sysbtn_registered, Ordering::Relaxed);

    // 2. Async device enumeration (`GameInputAsyncEnumeration` = 1, v0:41-46,
    //    v2:36-42, v3:38-43) with three different kind filters, all
    //    `device = NULL` and `statusFilter = GameInputDeviceAnyStatus` (v0
    //    caps at 0x00FFFFFF, v0:218-231; v2/v3 use 0xFFFFFFFF, v2:235-241,
    //    v3:346-352): Controller (0x0E), Gamepad (0x40000) and ControllerButton
    //    (0x04). All three callbacks share one registry, deduped by device
    //    pointer, so a device reported by several kind filters lands once.
    for kind in [KIND_CONTROLLER, KIND_GAMEPAD, KIND_CONTROLLER_BUTTON] {
        if register_device_callback_variant(object, version, kind, ENUMERATION_ASYNC).is_none() {
            return Err(Error::InterfaceUnavailable);
        }
    }

    // 3. The original blocking variant last (kind 0x0E,
    //    `GameInputBlockingEnumeration` = 2): every connected controller is
    //    reported synchronously inside this call, on the calling thread
    //    (RegisterDeviceCallback: v0 slot 9 at v0:1127-1135, v2 slot 8 at
    //    v2:883-888, v3 slot 8 at v3:1031-1038). Its `statusFilter` is
    //    `GameInputDeviceConnected` (0x1) like SDL's registration
    //    (`SDL_gameinputjoystick.c:268-276`), not AnyStatus.
    if register_device_callback_variant(object, version, KIND_CONTROLLER, ENUMERATION_BLOCKING)
        .is_none()
    {
        return Err(Error::InterfaceUnavailable);
    }

    // ONE summary note for the whole registration set; individual token lines
    // are gone, and a failed registration is reported by its own helper.
    note!("registered: sysbtn=NULL, device kinds 0xE/0x40000/0x4 async, blocking Connected");

    // Pump GameInput's callbacks on the calling thread when a dispatcher is
    // available; otherwise rely on GameInput's internal threads. The PoC's pump
    // never returned (Ctrl+C terminated the process), so it released the
    // dispatcher only on the Dispatch bail-out; here `stop` ends the pump, so
    // the dispatcher reference is released on every path that leaves the loop.
    let Some(dispatcher) = create_dispatcher(object, version) else {
        fallback_loop(stop);
        return Ok(());
    };
    let pumped = pump_loop(dispatcher, stop);
    release_interface(dispatcher);
    if pumped {
        return Ok(());
    }
    // pump_loop bailed out because the dispatcher vtable lacks `Dispatch`:
    // keep running with callbacks on GameInput's internal threads instead.
    fallback_loop(stop);
    Ok(())
}
