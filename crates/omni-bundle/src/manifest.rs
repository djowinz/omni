use serde::{Deserialize, Serialize};

use crate::error::BundleError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tag {
    Dark, Light, Minimal, Gaming, Neon, Retro, Cyberpunk,
    Pastel, HighContrast, Monospace, Racing, Flightsim,
    Mmo, Fps, Productivity, Creative,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub name: String,
    pub version: semver::Version,
    pub omni_min_version: semver::Version,
    pub description: String,
    pub tags: Vec<Tag>,
    pub license: String,
    pub entry_overlay: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_theme: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sensor_requirements: Vec<String>,
    pub files: Vec<FileEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub path: String,
    #[serde(with = "hex_sha256")]
    pub sha256: [u8; 32],
}

pub(crate) mod hex_sha256 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("sha256 hex must be 64 chars"))?;
        Ok(arr)
    }
}

/// Enforce intra-manifest references: no duplicate paths; `entry_overlay`
/// and `default_theme` (if present) must appear in `files`. Used by pack and
/// unpack to keep their validation consistent.
pub(crate) fn validate_manifest_references(m: &Manifest) -> Result<(), BundleError> {
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for e in &m.files {
        if !seen.insert(e.path.as_str()) {
            return Err(BundleError::UnsafePath(format!(
                "duplicate manifest entry: {}",
                e.path
            )));
        }
    }
    if !m.files.iter().any(|e| e.path == m.entry_overlay) {
        return Err(BundleError::FileMissing(m.entry_overlay.clone()));
    }
    if let Some(theme) = &m.default_theme {
        if !m.files.iter().any(|e| &e.path == theme) {
            return Err(BundleError::FileMissing(theme.clone()));
        }
    }
    Ok(())
}

/// Canonical compact JSON form: object keys sorted lexicographically (recursive), no whitespace.
pub(crate) fn canonical_manifest_bytes(m: &Manifest) -> Result<Vec<u8>, serde_json::Error> {
    let v = serde_json::to_value(m)?;
    let sorted = sort_value(v);
    serde_json::to_vec(&sorted)
}

/// Pretty human-readable form for the zip `manifest.json` entry. Sorted keys, 2-space indent, trailing newline.
pub(crate) fn pretty_manifest_bytes(m: &Manifest) -> Result<Vec<u8>, serde_json::Error> {
    let v = serde_json::to_value(m)?;
    let sorted = sort_value(v);
    let mut out = serde_json::to_vec_pretty(&sorted)?;
    out.push(b'\n');
    Ok(out)
}

fn sort_value(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> =
                map.into_iter().map(|(k, val)| (k, sort_value(val))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::new();
            for (k, val) in entries {
                out.insert(k, val);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(sort_value).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            schema_version: 1,
            name: "Sample".into(),
            version: "1.0.0".parse().unwrap(),
            omni_min_version: "0.1.0".parse().unwrap(),
            description: "d".into(),
            tags: vec![Tag::Dark, Tag::HighContrast],
            license: "MIT".into(),
            entry_overlay: "overlay.omni".into(),
            default_theme: Some("themes/default.css".into()),
            sensor_requirements: vec!["cpu.usage".into()],
            files: vec![FileEntry { path: "overlay.omni".into(), sha256: [1u8; 32] }],
            signature: None,
        }
    }

    #[test]
    fn tag_serializes_kebab_case() {
        let s = serde_json::to_string(&Tag::HighContrast).unwrap();
        assert_eq!(s, "\"high-contrast\"");
    }

    #[test]
    fn deny_unknown_fields_rejects_extras() {
        let json = r#"{
            "schema_version":1,"name":"x","version":"1.0.0","omni_min_version":"0.1.0",
            "description":"","tags":[],"license":"MIT","entry_overlay":"overlay.omni",
            "files":[{"path":"overlay.omni","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}],
            "wat":"nope"
        }"#;
        assert!(serde_json::from_str::<Manifest>(json).is_err());
    }

    #[test]
    fn canonical_is_byte_stable() {
        let a = canonical_manifest_bytes(&sample()).unwrap();
        let b = canonical_manifest_bytes(&sample()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn hex_sha256_roundtrips() {
        let fe = FileEntry { path: "x".into(), sha256: [0xab; 32] };
        let s = serde_json::to_string(&fe).unwrap();
        assert!(s.contains(&"ab".repeat(32)));
        let back: FileEntry = serde_json::from_str(&s).unwrap();
        assert_eq!(fe, back);
    }

    #[test]
    fn hex_sha256_rejects_wrong_length() {
        let bad = r#"{"path":"x","sha256":"abcd"}"#;
        assert!(serde_json::from_str::<FileEntry>(bad).is_err());
    }
}
