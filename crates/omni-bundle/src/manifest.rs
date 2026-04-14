use serde::{Deserialize, Serialize};

use crate::error::{BundleError, IntegrityKind};

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
            return Err(BundleError::Integrity {
                kind: IntegrityKind::DuplicatePath,
                detail: e.path.clone(),
            });
        }
    }
    if !m.files.iter().any(|e| e.path == m.entry_overlay) {
        return Err(BundleError::Integrity {
            kind: IntegrityKind::FileMissing,
            detail: m.entry_overlay.clone(),
        });
    }
    if let Some(theme) = &m.default_theme {
        if !m.files.iter().any(|e| &e.path == theme) {
            return Err(BundleError::Integrity {
                kind: IntegrityKind::FileMissing,
                detail: theme.clone(),
            });
        }
    }
    Ok(())
}

/// RFC 8785 (JCS) canonical form: deterministic JSON suitable for hashing
/// and signing. Object keys sorted in Unicode code-point order, no
/// insignificant whitespace, normalized number representation.
pub(crate) fn canonical_manifest_bytes(m: &Manifest) -> Result<Vec<u8>, serde_json::Error> {
    serde_jcs::to_vec(m)
}

/// Pretty-printed form for the human-readable `manifest.json` entry inside
/// the .omnipkg zip. Keys sorted (via JCS roundtrip) for determinism in the
/// zip output; this is distinct from the canonical form above (which is
/// what the hash consumes).
pub(crate) fn pretty_manifest_bytes(m: &Manifest) -> Result<Vec<u8>, serde_json::Error> {
    let canonical = serde_jcs::to_vec(m)?;
    let value: serde_json::Value = serde_json::from_slice(&canonical)?;
    let mut out = serde_json::to_vec_pretty(&value)?;
    out.push(b'\n');
    Ok(out)
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
