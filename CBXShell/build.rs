fn main() {
    let crate_name = std::env::var("CARGO_CRATE_NAME").unwrap_or_default();
    if crate_name == "cbxshell" {
        println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS");
    }

    println!("cargo:rustc-link-lib=dylib=ole32");
    println!("cargo:rustc-link-lib=dylib=oleaut32");
    println!("cargo:rustc-link-lib=dylib=uuid");
    println!("cargo:rustc-link-lib=dylib=shell32");
    println!("cargo:rustc-link-lib=dylib=gdi32");
    println!("cargo:rustc-link-lib=dylib=user32");
    println!("cargo:rustc-link-lib=dylib=comctl32");

    println!("cargo:rerun-if-changed=build.rs");
}
