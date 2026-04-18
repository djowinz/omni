use omni_bundle::{unpack, BundleLimits};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn unpack_never_panics_on_arbitrary_bytes(data in prop::collection::vec(any::<u8>(), 0..16_384)) {
        // Must not panic. Either Ok (vanishingly unlikely) or Err is fine.
        let _ = unpack(&data, &BundleLimits::DEFAULT);
    }

    #[test]
    fn unpack_never_panics_on_truncated_valid_zip(
        truncation in 0usize..200
    ) {
        // Build a minimal but valid zip, then chop trailing bytes.
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::<u8>::new()));
        let opts = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("manifest.json", opts).unwrap();
        use std::io::Write;
        zw.write_all(b"{}").unwrap();
        zw.start_file("overlay.omni", opts).unwrap();
        zw.write_all(b"<x/>").unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let cut = bytes.len().saturating_sub(truncation);
        let _ = unpack(&bytes[..cut], &BundleLimits::DEFAULT);
    }
}
