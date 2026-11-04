//! `#[repr(C)]` mirrors of the GameInput structs, transcribed field-for-field
//! from the official headers, with a Linux-side layout test.
//!
//! This module is compiled on EVERY platform (no Windows linkage, no `unsafe`)
//! so `cargo test` on Linux can assert the mirrored offsets against the byte
//! offsets computed from the header transcriptions below — the live-run bug was
//! a mirrored `APP_LOCAL_DEVICE_ID` of 16 bytes instead of the real 32 bytes,
//! which shifted every field after `deviceId` and made `supportedInput` /
//! `supportedSystemButtons` read hash bytes and `displayName` a bogus pointer.
//!
//! Authority (all offsets are x64, MSVC C alignment):
//! * v3: `include/GameInput.h` — `GameInputDeviceInfo` at :838-873,
//!   `GameInputUsage` :678-682, `GameInputVersion` :610-615,
//!   `GameInputDeviceFamily` :356-364, `GameInputKind` :19-34,
//!   `GameInputRumbleMotors` :532-538 (+ `DEFINE_ENUM_FLAG_OPERATORS` :541),
//!   `GameInputSystemButtons` :300-306 (+ flag operators :307).
//! * v2: `include/v2/GameInput.h` — `GameInputDeviceInfo` at :723-761,
//!   `GameInputUsage` :570-574, `GameInputVersion` :497-503,
//!   `GameInputRumbleMotors` :420-428 (+ :429), `GameInputSystemButtons`
//!   :226-231 (+ :233), `GameInputDeviceFamily` :245-253.
//! * v0: `include/v0/GameInput.h` — `GameInputDeviceInfo` at :940-991,
//!   `GameInputString` :663-668, `GameInputUsage` :670-674,
//!   `GameInputVersion` :676-681, `GameInputDeviceFamily` :244-252,
//!   `GameInputRumbleMotors` :508-515 (+ :517). v0 has no
//!   `GameInputSystemButtons` enum.
//!
//! `APP_LOCAL_DEVICE_ID` is a **32-byte** (256-bit) application-local device
//! hash, not 16 bytes: Microsoft's own C# interop marshals it with
//! `SizeConst = 32` (`companion/c#interop/GameInputInteropV3.cs:11-14`, struct
//! at :881-891), the GDK docs describe `deviceId` as "a 256-bit hash value"
//! (<https://devdocs.xbox.com/reference/input/gameinput/structs/gameinputdeviceinfo>),
//! and the Windows SDK Rust mirror is `APP_LOCAL_DEVICE_ID { value: [u8; 32] }`
//! (<https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Foundation/struct.APP_LOCAL_DEVICE_ID.html>).
//!
//! `GameInputRumbleMotors` is a bit-flag **enum** (4 bytes), NOT a struct:
//! there is no `GameInputRumbleMotor` (singular) enum in any header; the
//! members are `RumbleNone/LowFrequency/HighFrequency/LeftTrigger/RightTrigger`
//! (v3:532-538, v2:420-428, v0:508-515). So `supportedRumbleMotors` occupies
//! exactly 4 bytes between `supportedInput` and `supportedSystemButtons`.

/// `APP_LOCAL_DEVICE_ID` (Windows SDK `appmodel.h`): fixed 32-byte array.
pub(crate) type AppLocalDeviceId = [u8; 32];

/// `GUID` (Windows layout, 16 bytes, alignment 4): `unsigned long`, two
/// `unsigned short`, `unsigned char[8]`.
#[repr(C)]
#[allow(dead_code)] // Mirrored for layout fidelity; `containerId` is not printed.
pub(crate) struct Guid {
    pub(crate) data1: u32,
    pub(crate) data2: u16,
    pub(crate) data3: u16,
    pub(crate) data4: [u8; 8],
}

/// `GameInputUsage`: two `uint16_t` (v0:670-674, v2:570-574, v3:678-682).
#[repr(C)]
#[allow(dead_code)] // `page` mirrored for layout fidelity; only `id` is printed.
pub(crate) struct GameInputUsage {
    pub(crate) page: u16,
    pub(crate) id: u16,
}

/// `GameInputVersion`: four `uint16_t` (v0:676-681, v2:497-503, v3:610-615).
#[repr(C)]
#[allow(dead_code)] // Mirrored for layout fidelity only; not printed.
pub(crate) struct GameInputVersion {
    pub(crate) major: u16,
    pub(crate) minor: u16,
    pub(crate) build: u16,
    pub(crate) revision: u16,
}

/// `GameInputString` (v0:663-668): `data` is a UTF-8, NUL-terminated C string
/// (`char const *` at offset 8 on x64).
#[repr(C)]
#[allow(dead_code)] // `size_in_bytes`/`code_point_count` mirrored for fidelity only.
pub(crate) struct GameInputStringV0 {
    pub(crate) size_in_bytes: u32,
    pub(crate) code_point_count: u32,
    pub(crate) data: *const std::ffi::c_char,
}

/// `GameInputDeviceInfo` v0 layout, full transcription of v0:940-991. Unlike
/// v2/v3 this struct has `infoSize`, `interfaceNumber`, `collectionNumber`, a
/// `capabilities` field, and `displayName` points to a `GameInputString`, not
/// to a bare C string. There is no `supportedSystemButtons` field in v0.
#[repr(C)]
#[allow(dead_code)] // Mirrored for layout fidelity; only a prefix is read.
pub(crate) struct GameInputDeviceInfoV0 {
    pub(crate) info_size: u32,
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) revision_number: u16,
    pub(crate) interface_number: u8,
    pub(crate) collection_number: u8,
    pub(crate) usage: GameInputUsage,
    pub(crate) hardware_version: GameInputVersion,
    pub(crate) firmware_version: GameInputVersion,
    pub(crate) device_id: AppLocalDeviceId,
    pub(crate) device_root_id: AppLocalDeviceId,
    pub(crate) device_family: i32,
    pub(crate) capabilities: i32,
    pub(crate) supported_input: i32,
    pub(crate) supported_rumble_motors: i32,
    pub(crate) input_report_count: u32,
    pub(crate) output_report_count: u32,
    pub(crate) feature_report_count: u32,
    pub(crate) controller_axis_count: u32,
    pub(crate) controller_button_count: u32,
    pub(crate) controller_switch_count: u32,
    pub(crate) touch_point_count: u32,
    pub(crate) touch_sensor_count: u32,
    pub(crate) force_feedback_motor_count: u32,
    pub(crate) haptic_feedback_motor_count: u32,
    pub(crate) device_string_count: u32,
    pub(crate) device_descriptor_size: u32,
    pub(crate) input_report_info: *const std::ffi::c_void,
    pub(crate) output_report_info: *const std::ffi::c_void,
    pub(crate) feature_report_info: *const std::ffi::c_void,
    pub(crate) controller_axis_info: *const std::ffi::c_void,
    pub(crate) controller_button_info: *const std::ffi::c_void,
    pub(crate) controller_switch_info: *const std::ffi::c_void,
    pub(crate) keyboard_info: *const std::ffi::c_void,
    pub(crate) mouse_info: *const std::ffi::c_void,
    pub(crate) touch_sensor_info: *const std::ffi::c_void,
    pub(crate) motion_info: *const std::ffi::c_void,
    pub(crate) arcade_stick_info: *const std::ffi::c_void,
    pub(crate) flight_stick_info: *const std::ffi::c_void,
    pub(crate) gamepad_info: *const std::ffi::c_void,
    pub(crate) racing_wheel_info: *const std::ffi::c_void,
    pub(crate) ui_navigation_info: *const std::ffi::c_void,
    pub(crate) force_feedback_motor_info: *const std::ffi::c_void,
    pub(crate) haptic_feedback_motor_info: *const std::ffi::c_void,
    pub(crate) display_name: *const GameInputStringV0,
    pub(crate) device_strings: *const GameInputStringV0,
    pub(crate) device_descriptor_data: *const std::ffi::c_void,
}

/// `GameInputDeviceInfo` v2 layout, full transcription of v2:723-761. The
/// prefix through `pnpPath` is field-for-field identical to v3 (v3:838-854),
/// but the tail differs: v2 has `uiNavigationInfo` and the three
/// controller-count/pointer pairs before `forceFeedbackMotorCount`, and no
/// report-count fields.
#[repr(C)]
#[allow(dead_code)] // Mirrored for layout fidelity; only a prefix is read.
pub(crate) struct GameInputDeviceInfoV2 {
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) revision_number: u16,
    pub(crate) usage: GameInputUsage,
    pub(crate) hardware_version: GameInputVersion,
    pub(crate) firmware_version: GameInputVersion,
    pub(crate) device_id: AppLocalDeviceId,
    pub(crate) device_root_id: AppLocalDeviceId,
    pub(crate) device_family: i32,
    pub(crate) supported_input: i32,
    pub(crate) supported_rumble_motors: i32,
    pub(crate) supported_system_buttons: i32,
    pub(crate) container_id: Guid,
    pub(crate) display_name: *const std::ffi::c_char,
    pub(crate) pnp_path: *const std::ffi::c_char,
    pub(crate) keyboard_info: *const std::ffi::c_void,
    pub(crate) mouse_info: *const std::ffi::c_void,
    pub(crate) sensors_info: *const std::ffi::c_void,
    pub(crate) arcade_stick_info: *const std::ffi::c_void,
    pub(crate) flight_stick_info: *const std::ffi::c_void,
    pub(crate) gamepad_info: *const std::ffi::c_void,
    pub(crate) racing_wheel_info: *const std::ffi::c_void,
    pub(crate) ui_navigation_info: *const std::ffi::c_void,
    pub(crate) controller_axis_count: u32,
    pub(crate) controller_axis_info: *const std::ffi::c_void,
    pub(crate) controller_button_count: u32,
    pub(crate) controller_button_info: *const std::ffi::c_void,
    pub(crate) controller_switch_count: u32,
    pub(crate) controller_switch_info: *const std::ffi::c_void,
    pub(crate) force_feedback_motor_count: u32,
    pub(crate) force_feedback_motor_info: *const std::ffi::c_void,
}

/// `GameInputDeviceInfo` v3 layout, full transcription of v3:838-873,
/// including the report-count fields at :865-872.
#[repr(C)]
#[allow(dead_code)] // Mirrored for layout fidelity; only a prefix is read.
pub(crate) struct GameInputDeviceInfoV3 {
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) revision_number: u16,
    pub(crate) usage: GameInputUsage,
    pub(crate) hardware_version: GameInputVersion,
    pub(crate) firmware_version: GameInputVersion,
    pub(crate) device_id: AppLocalDeviceId,
    pub(crate) device_root_id: AppLocalDeviceId,
    pub(crate) device_family: i32,
    pub(crate) supported_input: i32,
    pub(crate) supported_rumble_motors: i32,
    pub(crate) supported_system_buttons: i32,
    pub(crate) container_id: Guid,
    pub(crate) display_name: *const std::ffi::c_char,
    pub(crate) pnp_path: *const std::ffi::c_char,
    pub(crate) keyboard_info: *const std::ffi::c_void,
    pub(crate) mouse_info: *const std::ffi::c_void,
    pub(crate) sensors_info: *const std::ffi::c_void,
    pub(crate) controller_info: *const std::ffi::c_void,
    pub(crate) arcade_stick_info: *const std::ffi::c_void,
    pub(crate) flight_stick_info: *const std::ffi::c_void,
    pub(crate) gamepad_info: *const std::ffi::c_void,
    pub(crate) racing_wheel_info: *const std::ffi::c_void,
    pub(crate) force_feedback_motor_count: u32,
    pub(crate) force_feedback_motor_info: *const std::ffi::c_void,
    pub(crate) input_report_count: u32,
    pub(crate) input_report_info: *const std::ffi::c_void,
    pub(crate) output_report_count: u32,
    pub(crate) output_report_info: *const std::ffi::c_void,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Byte offsets below are the MSVC x64 layout of the transcribed headers:
    // enums are 4-byte ints, `GUID` is 16 bytes at alignment 4, pointers are
    // 8 bytes at alignment 8, so a `uint32_t` count before a pointer leaves a
    // 4-byte padding gap (the C compiler inserts it; `#[repr(C)]` does too).

    #[test]
    fn device_id_is_32_byte_hash_not_16() {
        assert_eq!(std::mem::size_of::<AppLocalDeviceId>(), 32);
        assert_eq!(std::mem::align_of::<AppLocalDeviceId>(), 1);
    }

    #[test]
    fn supporting_types_have_header_sizes() {
        assert_eq!(std::mem::size_of::<Guid>(), 16);
        assert_eq!(std::mem::align_of::<Guid>(), 4);
        assert_eq!(std::mem::size_of::<GameInputUsage>(), 4);
        assert_eq!(std::mem::size_of::<GameInputVersion>(), 8);
        assert_eq!(std::mem::size_of::<GameInputStringV0>(), 16);
        assert_eq!(std::mem::offset_of!(GameInputStringV0, data), 8);
    }

    #[test]
    fn v3_device_info_prefix_offsets_match_header() {
        // v3:840-854 under C alignment: `GameInputUsage` is 4 bytes at
        // alignment 2, so `hardwareVersion` follows it at 10 (not 8) and
        // `firmwareVersion` at 18; the 32-byte ids sit at 26/58; the 4-byte
        // enum `deviceFamily` is padded to 92, giving supportedInput@96,
        // supportedRumbleMotors@100, supportedSystemButtons@104, GUID@108.
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV3, vendor_id), 0);
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV3, product_id), 2);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, revision_number),
            4
        );
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV3, usage), 6);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, hardware_version),
            10
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, firmware_version),
            18
        );
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV3, device_id), 26);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, device_root_id),
            58
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, device_family),
            92
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, supported_input),
            96
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, supported_rumble_motors),
            100
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, supported_system_buttons),
            104
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, container_id),
            108
        );
    }

    #[test]
    fn v3_device_info_string_and_tail_offsets_match_header() {
        // v3:853-872: the GUID ends at 124, so the 8-byte pointers are padded
        // to displayName@128, pnpPath@136, then the eight info pointers
        // 144..200; each `uint32_t` count before a pointer leaves a 4-byte gap.
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, display_name),
            128
        );
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV3, pnp_path), 136);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, keyboard_info),
            144
        );
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV3, mouse_info), 152);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, sensors_info),
            160
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, controller_info),
            168
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, arcade_stick_info),
            176
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, flight_stick_info),
            184
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, gamepad_info),
            192
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, racing_wheel_info),
            200
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, force_feedback_motor_count),
            208
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, force_feedback_motor_info),
            216
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, input_report_count),
            224
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, input_report_info),
            232
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, output_report_count),
            240
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, output_report_info),
            248
        );
        assert_eq!(std::mem::size_of::<GameInputDeviceInfoV3>(), 256);
        assert_eq!(std::mem::align_of::<GameInputDeviceInfoV3>(), 8);
    }

    #[test]
    fn v2_device_info_offsets_match_header() {
        // v2:723-761: identical prefix through pnpPath@136, then seven info
        // pointers + uiNavigationInfo@200, then controller count/pointer
        // pairs, each count followed by a 4-byte padding gap.
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV2, device_id), 26);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, device_root_id),
            58
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, supported_input),
            96
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, supported_rumble_motors),
            100
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, supported_system_buttons),
            104
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, container_id),
            108
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, display_name),
            128
        );
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV2, pnp_path), 136);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, keyboard_info),
            144
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, ui_navigation_info),
            200
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, controller_axis_count),
            208
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, controller_axis_info),
            216
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, controller_button_count),
            224
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, controller_button_info),
            232
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, controller_switch_count),
            240
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, controller_switch_info),
            248
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, force_feedback_motor_count),
            256
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, force_feedback_motor_info),
            264
        );
        assert_eq!(std::mem::size_of::<GameInputDeviceInfoV2>(), 272);
    }

    #[test]
    fn v0_device_info_offsets_match_header() {
        // v0:940-991: infoSize@0, ids@4/@6/@8, interface/collection bytes@10/11,
        // usage@12, versions@16/@24, 32-byte ids@32/@64, family@96,
        // capabilities@100, supportedInput@104, rumble@108, twelve counts
        // @112..156, then pointers@160.. with displayName@296 as GameInputString.
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV0, info_size), 0);
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV0, vendor_id), 4);
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV0, product_id), 6);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, revision_number),
            8
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, interface_number),
            10
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, collection_number),
            11
        );
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV0, usage), 12);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, hardware_version),
            16
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, firmware_version),
            24
        );
        assert_eq!(std::mem::offset_of!(GameInputDeviceInfoV0, device_id), 32);
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, device_root_id),
            64
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, device_family),
            96
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, capabilities),
            100
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, supported_input),
            104
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, supported_rumble_motors),
            108
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, input_report_count),
            112
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, device_descriptor_size),
            156
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, input_report_info),
            160
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, gamepad_info),
            256
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, display_name),
            296
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, device_strings),
            304
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV0, device_descriptor_data),
            312
        );
        assert_eq!(std::mem::size_of::<GameInputDeviceInfoV0>(), 320);
    }

    #[test]
    fn rumble_motors_field_is_a_4_byte_enum() {
        // `GameInputRumbleMotors` is a flag enum (v3:532-538 + :541), i.e. a
        // 4-byte value: supportedSystemButtons sits exactly 4 bytes after
        // supportedRumbleMotors in both v2 and v3.
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV3, supported_system_buttons)
                - std::mem::offset_of!(GameInputDeviceInfoV3, supported_rumble_motors),
            4
        );
        assert_eq!(
            std::mem::offset_of!(GameInputDeviceInfoV2, supported_system_buttons)
                - std::mem::offset_of!(GameInputDeviceInfoV2, supported_rumble_motors),
            4
        );
    }

    #[test]
    fn supported_fields_do_not_overlap_container_id() {
        // Regression guard for the live bug: the three enum fields must sit
        // BEFORE `containerId`, never inside its 16 hash/GUID bytes.
        let sysbtn = std::mem::offset_of!(GameInputDeviceInfoV3, supported_system_buttons);
        let container = std::mem::offset_of!(GameInputDeviceInfoV3, container_id);
        assert!(sysbtn + 4 <= container);
        assert!(std::mem::offset_of!(GameInputDeviceInfoV3, supported_input) + 4 <= container);
        assert!(
            std::mem::offset_of!(GameInputDeviceInfoV3, supported_rumble_motors) + 4 <= container
        );
        // And the string pointers must sit AFTER the whole 16-byte GUID.
        assert!(std::mem::offset_of!(GameInputDeviceInfoV3, display_name) >= container + 16);
        assert!(std::mem::offset_of!(GameInputDeviceInfoV3, pnp_path) >= container + 16);
    }
}
