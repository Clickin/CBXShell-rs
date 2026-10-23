use cbxshell::{archive::open_archive_from_stream, create_thumbnail_with_size};
use std::io::{Cursor, Write};

const RGB: &[u8] = include_bytes!("fixtures/jxl/rgb.jxl");

#[test]
fn jpeg_xl_archive_entries_are_counted_selected_and_extracted() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["readme.txt", "page10.JXL", "page2.jxl"] {
        writer.start_file(name, zip::write::FileOptions::default()).unwrap();
        writer.write_all(if name.ends_with("txt") { b"not an image" } else { RGB }).unwrap();
    }
    let bytes = writer.finish().unwrap().into_inner();
    let archive = open_archive_from_stream(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.get_metadata().unwrap().image_count, 2);
    assert_eq!(archive.find_first_image(false).unwrap().name, "page10.JXL");
    let entry = archive.find_first_image(true).unwrap();
    assert_eq!(entry.name, "page2.jxl");
    let data = archive.extract_entry(&entry).unwrap();
    assert_eq!(data, RGB);
    let bitmap = create_thumbnail_with_size(&data, 32, 32).unwrap();
    assert!(!bitmap.is_invalid());
    unsafe { let _ = windows::Win32::Graphics::Gdi::DeleteObject(bitmap); }
}
