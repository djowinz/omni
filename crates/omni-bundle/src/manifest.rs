use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{BundleError, IntegrityKind};

/// Format-validated tag string. Semantic vocabulary (which tags are recognized)
/// is enforced server-side via the Worker's `config:vocab` KV, per retro-005 D6.
/// This type only enforces format: kebab-case, 2–32 chars, starts with a letter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag(String);

impl Tag {
    pub fn new(s: impl Into<String>) -> Result<Self, crate::error::BundleError> {
        let s = s.into();
        if !Self::is_valid(&s) {
            return Err(crate::error::BundleError::Malformed {
                message: format!("tag format invalid: {s:?}"),
                source: None,
            });
        }
        Ok(Tag(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(s: &str) -> bool {
        // ^[a-z][a-z0-9-]{1,31}$
        let bytes = s.as_bytes();
        if bytes.len() < 2 || bytes.len() > 32 {
            return false;
        }
        if !bytes[0].is_ascii_lowercase() {
            return false;
        }
        bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
    }
}

impl Serialize for Tag {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Tag::new(&s).map_err(|e| serde::de::Error::custom(e.to_string()))
    }
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
///
/// LOAD-BEARING: depends on `serde_json`'s `preserve_order` feature (enabled
/// in Cargo.toml). Without it, `from_slice` would re-hash keys into a
/// BTreeMap/HashMap and we'd lose the JCS-sorted order through the roundtrip,
/// breaking byte-deterministic `pack` output.
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
            tags: vec![Tag::new("dark").unwrap(), Tag::new("high-contrast").unwrap()],
            license: "MIT".into(),
            entry_overlay: "overlay.omni".into(),
            default_theme: Some("themes/default.css".into()),
            sensor_requirements: vec!["cpu.usage".into()],
            files: vec![FileEntry { path: "overlay.omni".into(), sha256: [1u8; 32] }],
        }
    }

    #[test]
    fn tag_accepts_valid_formats() {
        assert!(Tag::new("dark").is_ok());
        assert!(Tag::new("high-contrast").is_ok());
        assert!(Tag::new("a1").is_ok());
        assert!(Tag::new("my-cool-theme").is_ok());
    }

    #[test]
    fn tag_rejects_invalid_formats() {
        assert!(Tag::new("").is_err());
        assert!(Tag::new("a").is_err()); // too short
        assert!(Tag::new(&"a".repeat(33)).is_err()); // too long
        assert!(Tag::new("1starts-with-digit").is_err());
        assert!(Tag::new("-starts-with-hyphen").is_err());
        assert!(Tag::new("has space").is_err());
        assert!(Tag::new("UPPERCASE").is_err());
        assert!(Tag::new("snake_case").is_err());
    }

    #[test]
    fn tag_serializes_as_plain_string() {
        let t = Tag::new("dark").unwrap();
        assert_eq!(serde_json::to_string(&t).unwrap(), "\"dark\"");
    }

    #[test]
    fn tag_deserializes_from_valid_string() {
        let t: Tag = serde_json::from_str("\"dark\"").unwrap();
        assert_eq!(t.as_str(), "dark");
    }

    #[test]
    fn tag_deserialize_rejects_invalid_string() {
        assert!(serde_json::from_str::<Tag>("\"UPPERCASE\"").is_err());
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
