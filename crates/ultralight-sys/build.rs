fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let vendor_lib = std::path::Path::new(&manifest_dir).join("../../vendor/ultralight/lib");
    let vendor_bin = std::path::Path::new(&manifest_dir).join("../../vendor/ultralight/bin");

    // Tell the linker where to find the import libraries (.lib files)
    println!("cargo:rustc-link-search=native={}", vendor_lib.display());

    // Link against all four Ultralight libraries
    println!("cargo:rustc-link-lib=dylib=Ultralight");
    println!("cargo:rustc-link-lib=dylib=UltralightCore");
    println!("cargo:rustc-link-lib=dylib=WebCore");
    println!("cargo:rustc-link-lib=dylib=AppCore");

    // Copy DLLs to the output directory so the binary can find them at runtime
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let target_dir = std::path::Path::new(&out_dir)
        .ancestors()
        .find(|p| p.ends_with("debug") || p.ends_with("release"))
        .expect("Could not find target profile dir");

    for dll in &[
        "Ultralight.dll",
        "UltralightCore.dll",
        "WebCore.dll",
        "AppCore.dll",
    ] {
        let src = vendor_bin.join(dll);
        let dst = target_dir.join(dll);
        if src.exists() {
            let _ = std::fs::copy(&src, &dst);
        }
    }

    // Copy resources directory
    let vendor_resources =
        std::path::Path::new(&manifest_dir).join("../../vendor/ultralight/resources");
    let target_resources = target_dir.join("resources");
    if vendor_resources.exists() {
        let _ = std::fs::create_dir_all(&target_resources);
        for entry in std::fs::read_dir(&vendor_resources).unwrap() {
            let entry = entry.unwrap();
            let _ = std::fs::copy(entry.path(), target_resources.join(entry.file_name()));
        }
    }
}
