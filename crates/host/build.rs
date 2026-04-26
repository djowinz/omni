fn main() {
    let manifest = std::path::Path::new("resources/app.manifest");
    if manifest.exists() {
        let abs = manifest.canonicalize().expect("manifest path");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", abs.display());
    }
}
