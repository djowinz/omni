//! Serde roundtrip verification for DeviceId. Runs only with the `serde` feature.

#![cfg(feature = "serde")]

use omni_guard::DeviceId;

#[test]
fn device_id_json_roundtrip() {
    let original = DeviceId([0xde; 32]);
    let json = serde_json::to_string(&original).expect("serialize");
    let back: DeviceId = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(original, back);
}

#[test]
fn device_id_json_encoding_is_byte_array() {
    let id = DeviceId([0x00; 32]);
    let json = serde_json::to_string(&id).expect("serialize");
    assert!(json.starts_with('['));
    assert!(json.ends_with(']'));
    assert_eq!(json.matches(',').count(), 31);
}
