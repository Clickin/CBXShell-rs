use cbxshell_image_codec::decode_rgba_image;

const RGB: &[u8] = include_bytes!("../../../CBXShell/tests/fixtures/jxl/rgb.jxl");
const CONTAINER: &[u8] = include_bytes!("../../../CBXShell/tests/fixtures/jxl/container.jxl");
const RGBA: &[u8] = include_bytes!("../../../CBXShell/tests/fixtures/jxl/rgba.jxl");
const GRAY16: &[u8] = include_bytes!("../../../CBXShell/tests/fixtures/jxl/gray16.jxl");

#[test]
fn jpeg_xl_codestream_and_container_decode_without_com() {
    for data in [RGB, CONTAINER] {
        let image = decode_rgba_image(data).unwrap();
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(image.as_pixels(), &[255, 0, 0, 255, 0, 255, 0, 255]);
    }
}

#[test]
fn jpeg_xl_alpha_is_preserved() {
    let image = decode_rgba_image(RGBA).unwrap();
    assert_eq!((image.width(), image.height()), (2, 1));
    assert_eq!(image.as_pixels(), &[255, 0, 0, 255, 0, 255, 0, 128]);
}

#[test]
fn jpeg_xl_gray16_converts_to_rgba8() {
    let image = decode_rgba_image(GRAY16).unwrap();
    assert_eq!((image.width(), image.height()), (2, 1));
    assert_eq!(image.as_pixels(), &[0, 0, 0, 255, 255, 255, 255, 255]);
}

#[test]
fn truncated_jpeg_xl_is_an_error_not_a_partial_thumbnail() {
    for data in [RGB, CONTAINER, RGBA, GRAY16] {
        for len in 0..data.len() {
            assert!(
                decode_rgba_image(&data[..len]).is_err(),
                "accepted {} of {} bytes",
                len,
                data.len()
            );
        }
    }
}

#[test]
fn jpeg_xl_associated_alpha_is_converted_to_straight_rgba() {
    let data = include_bytes!("../../../CBXShell/tests/fixtures/jxl/rgba-associated.jxl");
    let metadata = jxl_oxide::JxlImage::builder().read(&data[..]).unwrap();
    assert!(metadata
        .image_header()
        .metadata
        .ec_info
        .iter()
        .any(|info| info.alpha_associated() == Some(true)));
    let image = decode_rgba_image(data).unwrap();
    assert_eq!(image.as_pixels(), &[64, 32, 16, 255, 64, 128, 32, 128]);
}

#[test]
fn jpeg_xl_animation_uses_first_frame() {
    let data = include_bytes!("../../../CBXShell/tests/fixtures/jxl/animated.jxl");
    let image = decode_rgba_image(data).unwrap();
    assert_eq!((image.width(), image.height()), (2, 1));
    assert_eq!(image.as_pixels(), &[255, 0, 0, 255, 255, 0, 0, 255]);
}

#[test]
fn jpeg_xl_lossy_vardct_decodes() {
    let data = include_bytes!("../../../CBXShell/tests/fixtures/jxl/lossy.jxl");
    let image = decode_rgba_image(data).unwrap();
    assert_eq!((image.width(), image.height()), (16, 16));
    for pixel in image.as_pixels().as_chunks::<4>().0 {
        for (actual, expected) in pixel.iter().zip([128u8, 64, 32, 255]) {
            assert!(
                actual.abs_diff(expected) <= 4,
                "unexpected pixel: {pixel:?}"
            );
        }
    }
}
