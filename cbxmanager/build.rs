fn main() {
    let mut res = winres::WindowsResource::new();
    // The icon is an optional local asset; the application manifest is mandatory.
    if std::path::Path::new("../Assets/cbx_icon.ico").is_file() {
        res.set_icon("../Assets/cbx_icon.ico");
    }
    res.set_manifest_file("cbxmanager.exe.manifest");

    res.compile()
        .expect("Failed to compile required CBXManager Windows resources");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=cbxmanager.exe.manifest");
    println!("cargo:rerun-if-changed=../Assets/cbx_icon.ico");
}
