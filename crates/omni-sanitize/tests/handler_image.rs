use omni_sanitize::{sanitize_bundle, SanitizeError};

mod common;

fn tiny_png() -> Vec<u8> {
    use image::{ImageBuffer, Rgba};
    let buf: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
    let mut out = Vec::new();
    buf.write_to(&mut std::io::Cursor::new(&mut out), image::ImageOutputFormat::Png)
        .unwrap();
    out
}

#[test]
fn accepts_png_roundtrips_to_png() {
    let src = tiny_png();
    let (manifest, files) = common::bundle_with_image("images/x.png", src);
    let (out, report) = sanitize_bundle(&manifest, files).unwrap();
    assert!(out.contains_key("images/x.png"));
    assert_eq!(&out["images/x.png"][0..4], b"\x89PNG");
    assert!(report.files.iter().any(|f| f.path == "images/x.png"));
}

#[test]
fn rejects_non_image_bytes() {
    // "not an image" bytes — does NOT match exec magic (no MZ/ELF/etc.),
    // so dispatch reaches ImageHandler which rejects as non-decodable.
    let (manifest, files) = common::bundle_with_image("images/x.png", b"not an image".to_vec());
    let err = sanitize_bundle(&manifest, files).unwrap_err();
    assert!(matches!(err, SanitizeError::Handler { kind: "image", .. }));
}
