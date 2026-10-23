use cbxshell_image_magic::{detect_image_format, is_image_file, verify_image_data};

#[test]
fn jpeg_xl_signatures_are_recognized() {
    for data in [&b"\xff\x0a"[..], &b"\x00\x00\x00\x0cJXL \x0d\x0a\x87\x0a"[..]] {
        assert_eq!(detect_image_format(data).unwrap().as_str(), "JPEG XL");
        assert!(verify_image_data(data, "page.jxl").is_ok());
    }
}

#[test]
fn jpeg_xl_extension_is_case_insensitive() {
    assert!(is_image_file("folder/page.jxl"));
    assert!(is_image_file("PAGE.JXL"));
    assert!(!is_image_file("page.jxl.txt"));
}

#[test]
fn incomplete_or_false_jpeg_xl_signatures_are_rejected() {
    let signature = b"\x00\x00\x00\x0cJXL \x0d\x0a\x87\x0a";
    for len in 0..signature.len() {
        assert!(detect_image_format(&signature[..len]).is_err());
    }
    assert!(detect_image_format(b"\xff").is_err());
    assert!(detect_image_format(b"\xff\x0b\x00\x00").is_err());
    assert!(detect_image_format(b"\x00\x00\x00\x0cJXL \x0d\x0a\x87\x00").is_err());
}
