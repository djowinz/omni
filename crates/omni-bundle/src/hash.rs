use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::manifest::{canonical_manifest_bytes, Manifest};

/// SHA-256 of a byte slice. Shared across pack / unpack / canonical_hash to
/// avoid duplicated inline implementations.
pub(crate) fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

/// SHA-256 over a deterministic uncompressed ustar tar stream built from the
/// canonical manifest bytes followed by every file in sorted path order.
/// All metadata (mode 0644, uid/gid 0, mtime 0) is fixed, so the digest is
/// independent of filesystem and time of day.
pub fn canonical_hash(manifest: &Manifest, files: &BTreeMap<String, Vec<u8>>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut tar = UstarWriter::new(&mut hasher);

    let manifest_bytes = canonical_manifest_bytes(manifest)
        .expect("canonical manifest serialization must not fail for validated manifest");
    tar.append("manifest.json", &manifest_bytes);

    for (path, bytes) in files.iter() {
        if path == "manifest.json" {
            continue;
        }
        tar.append(path, bytes);
    }

    tar.finish();
    hasher.finalize().into()
}

const ZERO_BLOCK: [u8; 512] = [0u8; 512];

struct UstarWriter<'a, W: Digest> {
    hasher: &'a mut W,
}

impl<'a, W: Digest> UstarWriter<'a, W> {
    fn new(hasher: &'a mut W) -> Self {
        Self { hasher }
    }

    fn append(&mut self, path: &str, data: &[u8]) {
        let header = build_header(path, data.len() as u64);
        self.hasher.update(header);
        self.hasher.update(data);
        let pad = (512 - (data.len() % 512)) % 512;
        if pad > 0 {
            self.hasher.update(&ZERO_BLOCK[..pad]);
        }
    }

    fn finish(self) {
        self.hasher.update(ZERO_BLOCK);
        self.hasher.update(ZERO_BLOCK);
    }
}

fn build_header(path: &str, size: u64) -> [u8; 512] {
    // validate_path() rejects paths >100 bytes. Callers of canonical_hash must
    // have run validate_path (via pack/unpack or directly) on every path first.
    assert!(
        path.len() <= 100,
        "canonical_hash: path >100 bytes — caller must validate_path first: {path}"
    );
    let mut h = [0u8; 512];
    h[..path.len()].copy_from_slice(path.as_bytes());

    write_octal(&mut h[100..108], 0o644, 7);
    write_octal(&mut h[108..116], 0, 7);
    write_octal(&mut h[116..124], 0, 7);
    write_octal(&mut h[124..136], size, 11);
    write_octal(&mut h[136..148], 0, 11);

    // Checksum field placeholder: 8 spaces per ustar spec.
    h[148..156].copy_from_slice(b"        ");
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");

    let sum: u32 = h.iter().map(|b| *b as u32).sum();
    // Checksum stored as 6 octal digits, NUL, space — exactly 8 bytes.
    let mut cksum = [0u8; 8];
    write_octal(&mut cksum[..7], sum as u64, 6);
    cksum[7] = b' ';
    h[148..156].copy_from_slice(&cksum);

    h
}

/// Write a fixed-width octal number followed by a trailing NUL byte into `dst`.
/// `dst` must be exactly `digits + 1` bytes. Right-aligned, zero-padded.
fn write_octal(dst: &mut [u8], mut value: u64, digits: usize) {
    assert_eq!(dst.len(), digits + 1);
    dst[digits] = 0;
    for i in (0..digits).rev() {
        dst[i] = b'0' + (value & 0o7) as u8;
        value >>= 3;
    }
    debug_assert_eq!(value, 0, "value overflowed {digits}-digit octal");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{FileEntry, Tag};

    fn sample() -> (Manifest, BTreeMap<String, Vec<u8>>) {
        let mut files = BTreeMap::new();
        files.insert("overlay.omni".into(), b"<overlay/>".to_vec());
        files.insert("themes/default.css".into(), b":root{--a:0}".to_vec());

        let m = Manifest {
            schema_version: 1,
            name: "Sample".into(),
            version: "1.0.0".parse().unwrap(),
            omni_min_version: "0.1.0".parse().unwrap(),
            description: "d".into(),
            tags: vec![Tag::Dark],
            license: "MIT".into(),
            entry_overlay: "overlay.omni".into(),
            default_theme: Some("themes/default.css".into()),
            sensor_requirements: vec![],
            files: vec![
                FileEntry { path: "overlay.omni".into(), sha256: [0u8; 32] },
                FileEntry { path: "themes/default.css".into(), sha256: [0u8; 32] },
            ],
            signature: None,
        };
        (m, files)
    }

    #[test]
    fn canonical_hash_is_stable() {
        let (m, f) = sample();
        assert_eq!(canonical_hash(&m, &f), canonical_hash(&m, &f));
    }

    #[test]
    fn canonical_hash_changes_when_content_changes() {
        let (m, mut f) = sample();
        let before = canonical_hash(&m, &f);
        f.insert("overlay.omni".into(), b"<overlay2/>".to_vec());
        let after = canonical_hash(&m, &f);
        assert_ne!(before, after);
    }

    /// Golden hash locks the canonical_hash byte format (ustar header layout,
    /// field order, checksum encoding). Drift here breaks host/Worker dedup parity.
    #[test]
    fn canonical_hash_matches_golden() {
        let (m, f) = sample();
        let expected = "092e759315415125e73d91a682c05283934bdadcf2de5c02399de0c4d1b5d024";
        let got = canonical_hash(&m, &f);
        assert_eq!(hex::encode(got), expected);
    }

    #[test]
    fn canonical_hash_order_independent_of_insert_order() {
        let (m, _) = sample();
        let mut a = BTreeMap::new();
        a.insert("overlay.omni".into(), b"x".to_vec());
        a.insert("themes/default.css".into(), b"y".to_vec());
        let mut b = BTreeMap::new();
        b.insert("themes/default.css".into(), b"y".to_vec());
        b.insert("overlay.omni".into(), b"x".to_vec());
        assert_eq!(canonical_hash(&m, &a), canonical_hash(&m, &b));
    }
}
