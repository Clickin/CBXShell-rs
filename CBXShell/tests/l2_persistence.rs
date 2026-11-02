use cbxshell::CBXShell;
use std::io::Write;
use windows::core::{ComInterface, HSTRING};
use windows::Win32::Graphics::Gdi::{DeleteObject, HBITMAP};
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, IStream, COINIT_APARTMENTTHREADED, STATFLAG_DEFAULT, STATSTG,
};
use windows::Win32::UI::Shell::PropertiesSystem::IInitializeWithStream;
use windows::Win32::UI::Shell::{
    BHID_Stream, IShellItem, SHCreateItemFromParsingName, WTS_ALPHATYPE,
};

#[test]
fn shell_stream_persists_l2() {
    let root = std::env::temp_dir().join(format!("cbxshell-persistence-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("LOCALAPPDATA", &root);
    let path = root.join("cover.cbz");
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(32, 32)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    archive
        .start_file("cover.png", zip::write::FileOptions::default())
        .unwrap();
    archive.write_all(png.get_ref()).unwrap();
    drop(archive.finish().unwrap());
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap();
        {
            let item: IShellItem =
                SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).unwrap();
            let stream: IStream = item.BindToHandler(None, &BHID_Stream).unwrap();
            let mut stat = STATSTG::default();
            stream.Stat(&mut stat, STATFLAG_DEFAULT).unwrap();
            eprintln!(
                "stream name={:?}, size={}, mtime={:?}",
                stat.pwcsName.to_string(),
                stat.cbSize,
                stat.mtime
            );
            windows::Win32::System::Com::CoTaskMemFree(Some(stat.pwcsName.0.cast()));
            let provider = CBXShell::new().unwrap();
            let init: IInitializeWithStream = provider.cast().unwrap();
            init.Initialize(&stream, 0).unwrap();
            let mut bitmap = HBITMAP::default();
            let mut alpha = WTS_ALPHATYPE::default();
            provider.GetThumbnail(256, &mut bitmap, &mut alpha).unwrap();
            // A completed request must not prevent moving the source while
            // the provider is still alive.
            drop(stream);
            drop(item);
            std::fs::rename(&path, root.join("moved.cbz")).unwrap();
            // Cache publication is asynchronous; wait for the first entry
            // before requesting another size to exercise the hit path.
            let cache_root = root.join("CBXShell/thumbnail-cache/v1");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                let published = std::fs::read_dir(&cache_root)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .flat_map(|shard| {
                        std::fs::read_dir(shard.path())
                            .into_iter()
                            .flatten()
                            .flatten()
                    })
                    .flat_map(|source| {
                        std::fs::read_dir(source.path())
                            .into_iter()
                            .flatten()
                            .flatten()
                    })
                    .any(|entry| entry.path().extension().is_some_and(|ext| ext == "webp"));
                if published {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "cache was not published: {:?}",
                    cache_root
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            DeleteObject(bitmap).unwrap();
            provider.GetThumbnail(128, &mut bitmap, &mut alpha).unwrap();
            DeleteObject(bitmap).unwrap();
        }
        CoUninitialize();
    }
    let cache_root = root.join("CBXShell/thumbnail-cache/v1");
    let shard = std::fs::read_dir(&cache_root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let source = std::fs::read_dir(shard)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let entries: Vec<_> = std::fs::read_dir(source).unwrap().collect();
    assert_eq!(entries.len(), 1);
    let encoded = std::fs::read(entries[0].as_ref().unwrap().path()).unwrap();
    assert_eq!(&encoded[..4], b"RIFF");
    assert_eq!(&encoded[8..12], b"WEBP");
    let decoded = image::load_from_memory(&encoded).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (32, 32));
    std::fs::remove_dir_all(root).unwrap();
}
