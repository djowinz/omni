use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::manifest::{canonical_manifest_bytes, Manifest};

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
    let out = hasher.finalize();
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&out);
    digest
}

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
            self.hasher.update(vec![0u8; pad]);
        }
    }

    fn finish(self) {
        self.hasher.update([0u8; 1024]);
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

    for b in &mut h[148..156] {
        *b = b' ';
    }
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");

    let sum: u32 = h.iter().map(|b| *b as u32).sum();
    let s = format!("{:06o}\0 ", sum);
    h[148..156].copy_from_slice(s.as_bytes());

    h
}

fn write_octal(dst: &mut [u8], value: u64, digits: usize) {
    let s = format!("{:0>width$o}\0", value, width = digits);
    assert_eq!(s.len(), digits + 1);
    dst[..s.len()].copy_from_slice(s.as_bytes());
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

    /// Golden hash for the `sample()` fixture. Locks the canonical_hash byte
    /// format (ustar header layout, field order, checksum encoding). If a future
    /// change to `canonical_hash`, `canonical_manifest_bytes`, or the ustar
    /// writer changes this value, the WASM Worker will compute a different
    /// dedup hash than the host — this test catches that before merge.
    #[test]
    fn canonical_hash_matches_golden() {
        let (m, f) = sample();
        let expected = "092e759315415125e73d91a682c05283934bdadcf2de5c02399de0c4d1b5d024";
        let got = canonical_hash(&m, &f);
        assert_eq!(hex_encode(&got), expected);
    }

    fn hex_encode(b: &[u8; 32]) -> String {
        let mut s = String::with_capacity(64);
        for x in b {
            use std::fmt::Write;
            write!(s, "{:02x}", x).unwrap();
        }
        s
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
