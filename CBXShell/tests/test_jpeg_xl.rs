use cbxshell::{archive::open_archive_from_stream, create_thumbnail_with_size};
use std::io::{Cursor, Write};

const RGB: &[u8] = include_bytes!("fixtures/jxl/rgb.jxl");

#[test]
fn jpeg_xl_archive_entries_are_counted_selected_and_extracted() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["readme.txt", "page10.JXL", "page2.jxl"] {
        writer
            .start_file(name, zip::write::FileOptions::default())
            .unwrap();
        writer
            .write_all(if name.ends_with("txt") {
                b"not an image"
            } else {
                RGB
            })
            .unwrap();
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
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(bitmap);
    }
}

/// Opt-in test for a real local archive; never commits user images to the repo.
#[test]
#[ignore = "set CBXSHELL_TEST_ARCHIVE to a local archive path"]
fn local_archive_thumbnail() {
    let path =
        std::env::var_os("CBXSHELL_TEST_ARCHIVE").expect("CBXSHELL_TEST_ARCHIVE is required");
    let archive = open_archive_from_stream(std::fs::File::open(path).unwrap()).unwrap();
    for sort in [false, true] {
        let entry = archive.find_first_image(sort).unwrap();
        let data = archive.extract_entry(&entry).unwrap();
        let bitmap = create_thumbnail_with_size(&data, 1280, 1280)
            .unwrap_or_else(|error| panic!("{} ({} bytes): {error}", entry.name, data.len()));
        assert!(!bitmap.is_invalid());
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::DeleteObject(bitmap);
        }
    }
}

/// Exercise the exported COM factory in the built DLL without registering it.
#[test]
#[ignore = "set CBXSHELL_TEST_ARCHIVE and CBXSHELL_TEST_DLL to local paths"]
fn local_dll_com_thumbnail() {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{ComInterface, Interface, GUID, HRESULT, PCSTR, PCWSTR};
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::Graphics::Gdi::{DeleteObject, GetObjectW, BITMAP, HBITMAP};
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, IClassFactory, COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::UI::Shell::PropertiesSystem::IInitializeWithFile;
    use windows::Win32::UI::Shell::{IThumbnailProvider, WTS_ALPHATYPE};

    let wide = |key| {
        std::env::var_os(key)
            .expect(key)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let archive = wide("CBXSHELL_TEST_ARCHIVE");
    let dll = wide("CBXSHELL_TEST_DLL");
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap();
        let module = LoadLibraryW(PCWSTR(dll.as_ptr())).unwrap();
        let result = (|| -> windows::core::Result<()> {
            type GetClass = unsafe extern "system" fn(
                *const GUID,
                *const GUID,
                *mut *mut std::ffi::c_void,
            ) -> HRESULT;
            let entry = GetProcAddress(module, PCSTR(b"DllGetClassObject\0".as_ptr()))
                .expect("DllGetClassObject export");
            let get_class: GetClass = std::mem::transmute(entry);
            let mut raw = std::ptr::null_mut();
            get_class(
                &cbxshell::com::CLSID_CBXSHELL,
                &IClassFactory::IID,
                &mut raw,
            )
            .ok()?;
            let factory = IClassFactory::from_raw(raw);
            let provider: IThumbnailProvider = factory.CreateInstance(None)?;
            let init: IInitializeWithFile = provider.cast()?;
            init.Initialize(PCWSTR(archive.as_ptr()), STGM_READ.0)?;
            let mut bitmap = HBITMAP::default();
            let mut alpha = WTS_ALPHATYPE::default();
            provider.GetThumbnail(1280, &mut bitmap, &mut alpha)?;
            let mut info = BITMAP::default();
            let size = GetObjectW(
                bitmap,
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut info as *mut _ as *mut _),
            );
            let valid = size != 0
                && info.bmWidth > 0
                && info.bmHeight > 0
                && info.bmWidth.max(info.bmHeight) == 1280;
            println!(
                "COM thumbnail: {}x{}, {} bits/pixel",
                info.bmWidth, info.bmHeight, info.bmBitsPixel
            );
            DeleteObject(bitmap).ok()?;
            assert!(valid, "DLL returned unexpected thumbnail dimensions");
            Ok(())
        })();
        FreeLibrary(module).unwrap();
        CoUninitialize();
        result.expect("real DLL IThumbnailProvider::GetThumbnail");
    }
}

#[test]
#[ignore = "set CBXSHELL_TEST_ARCHIVE; decodes every JXL entry"]
fn local_archive_all_jxl_thumbnails() {
    use std::io::Read;
    let path =
        std::env::var_os("CBXSHELL_TEST_ARCHIVE").expect("CBXSHELL_TEST_ARCHIVE is required");
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut checked = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        if !entry.name().to_ascii_lowercase().ends_with(".jxl") {
            continue;
        }
        assert!(
            entry.size() <= 32 * 1024 * 1024,
            "entry exceeds archive size limit"
        );
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        let bitmap = create_thumbnail_with_size(&data, 1280, 1280)
            .unwrap_or_else(|error| panic!("{} ({} bytes): {error}", entry.name(), data.len()));
        assert!(!bitmap.is_invalid());
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::DeleteObject(bitmap);
        }
        checked += 1;
        println!("JXL thumbnail {checked}: {} bytes OK", data.len());
    }
    assert!(checked > 0, "archive contains no JXL images");
}
