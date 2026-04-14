//! Serde roundtrip verification for DeviceId.
//! Runs only when the `serde` feature is enabled.

#![cfg(feature = "serde")]

use omni_guard_trait::DeviceId;

#[test]
fn device_id_json_roundtrip() {
    let original = DeviceId([0xde; 32]);
    let json = serde_json::to_string(&original).expect("serialize");
    let back: DeviceId = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(original, back);
}

#[test]
fn device_id_json_encoding_is_byte_array() {
    // Confirm the on-wire shape is a JSON array of 32 bytes, not a hex string.
    // Consumers (the Worker, sub-spec #008) need a stable encoding.
    let id = DeviceId([0x00; 32]);
    let json = serde_json::to_string(&id).expect("serialize");
    assert!(json.starts_with('['));
    assert!(json.ends_with(']'));
    // 32 zero entries separated by commas.
    assert_eq!(json.matches(',').count(), 31);
}
