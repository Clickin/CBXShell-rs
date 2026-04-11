fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("../Assets/cbx_icon.ico");
    res.set_manifest_file("cbxmanager.exe.manifest");

    let _ = res.compile();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=cbxmanager.exe.manifest");
    println!("cargo:rerun-if-changed=../Assets/cbx_icon.ico");
}
