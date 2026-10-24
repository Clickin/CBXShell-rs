//! First-frame JPEG XL decoding, with bounded output and sRGB conversion.
use crate::DecodedRgbaImage;
use cbxshell_core::{CbxError, Result};
use jxl::api::{
    AlphaMode, Event, ExtraChannel, JxlColorEncoding, JxlColorProfile, JxlColorType, JxlDataFormat,
    JxlDecoder, JxlDecoderOptions, JxlOutputBuffer, JxlPixelFormat,
};

type DecodeResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const OUTPUT_LIMIT: usize = 256 * 1024 * 1024;
const MAX_DECODE_SAMPLES: usize = 256 * 1024 * 1024;

pub(crate) fn decode(data: &[u8]) -> Result<DecodedRgbaImage> {
    decode_inner(data).map_err(|error| CbxError::Image(format!("JPEG XL decoding failed: {error}")))
}

fn decode_inner(data: &[u8]) -> DecodeResult<DecodedRgbaImage> {
    let mut options = JxlDecoderOptions::default();
    // Counts samples (pixels * color/extra channels), including reference frames.
    // This is a complexity guard, not a total process-memory quota.
    options.sample_limit = Some(MAX_DECODE_SAMPLES);
    options.alpha_mode = AlphaMode::Unpremultiplied;
    let mut decoder = JxlDecoder::new(options);
    let mut input = data;
    if decoder.process(&mut input, None, None)? != Event::BasicInfo {
        return Err("Incomplete JPEG XL header".into());
    }
    let info = decoder.basic_info().ok_or("Missing JPEG XL basic info")?;
    let (width, height) = info.size;
    let output_len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .filter(|&n| n > 0 && n <= OUTPUT_LIMIT)
        .ok_or("JPEG XL image exceeds 256 MiB RGBA output limit")?;
    let cmyk = decoder
        .embedded_color_profile()
        .is_some_and(JxlColorProfile::is_cmyk);
    let alpha_index = info
        .extra_channels
        .iter()
        .position(|channel| channel.ec_type == ExtraChannel::Alpha);
    let mut format = JxlPixelFormat::rgba8(info.extra_channels.len());
    if cmyk {
        format.color_type = JxlColorType::Cmyk;
        if let Some(index) = alpha_index {
            format.extra_channel_format[index] = Some(JxlDataFormat::U8 { bit_depth: 8 });
        }
    }
    decoder.set_pixel_format(format)?;
    let profile = decoder
        .output_color_profile()
        .ok_or("Missing JPEG XL output profile")?
        .clone();
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(output_len)?;
    pixels.resize(output_len, 0);
    let mut alpha = if cmyk && alpha_index.is_some() {
        vec![0u8; width * height]
    } else {
        Vec::new()
    };
    {
        let mut buffers = vec![JxlOutputBuffer::new(&mut pixels, height, width * 4)];
        if !alpha.is_empty() {
            buffers.push(JxlOutputBuffer::new(&mut alpha, height, width));
        }
        loop {
            match decoder.process(&mut input, Some(&mut buffers), None)? {
                Event::FrameHeader => {}
                Event::FrameComplete { .. } => break,
                _ => return Err("Incomplete JPEG XL first frame".into()),
            }
        }
    }
    // No decoding caches or worker threads survive conversion or DLL unloading.
    drop(decoder);
    convert_to_srgb(&mut pixels, width, &profile, cmyk, &alpha)?;
    cbxshell_core::utils::debug_log::debug_log(&format!(
        "JPEG XL decoded by jxl-rs: {width}x{height}, {} input bytes, {output_len} RGBA bytes",
        data.len()
    ));
    Ok(DecodedRgbaImage::new(
        width.try_into()?,
        height.try_into()?,
        pixels,
    ))
}

fn convert_to_srgb(
    pixels: &mut [u8],
    width: usize,
    profile: &JxlColorProfile,
    cmyk: bool,
    alpha: &[u8],
) -> DecodeResult<()> {
    use moxcms::{ColorProfile, Layout, TransformOptions};
    if !cmyk
        && [false, true].into_iter().any(|gray| {
            profile.same_color_encoding(&JxlColorProfile::Simple(JxlColorEncoding::srgb(gray)))
        })
    {
        return Ok(());
    }
    let icc = profile
        .try_as_icc()
        .ok_or("Cannot convert JPEG XL color profile to sRGB")?;
    let source = ColorProfile::new_from_slice(&icc)?;
    let gray = profile.channels() == 1;
    let layout = if gray {
        Layout::GrayAlpha
    } else if cmyk && !alpha.is_empty() {
        Layout::Cmyka
    } else {
        Layout::Rgba
    };
    let channels = if gray {
        2
    } else if cmyk && !alpha.is_empty() {
        5
    } else {
        4
    };
    let options = TransformOptions {
        rendering_intent: moxcms::RenderingIntent::RelativeColorimetric,
        ..Default::default()
    };
    let transform =
        source.create_transform_8bit(layout, &ColorProfile::new_srgb(), Layout::Rgba, options)?;
    // Transform one row at a time, avoiding another full-resolution RGBA buffer.
    let mut input_row = vec![0u8; width * channels];
    for (y, row) in pixels.chunks_exact_mut(width * 4).enumerate() {
        if gray {
            for (source, dest) in row
                .as_chunks::<4>()
                .0
                .iter()
                .zip(input_row.as_chunks_mut::<2>().0.iter_mut())
            {
                dest.copy_from_slice(&[source[0], source[3]]);
            }
        } else if channels == 5 {
            for (x, (source, dest)) in row
                .as_chunks::<4>()
                .0
                .iter()
                .zip(input_row.as_chunks_mut::<5>().0.iter_mut())
                .enumerate()
            {
                dest[..4].copy_from_slice(source);
                dest[4] = alpha[y * width + x];
            }
        } else {
            input_row.copy_from_slice(row);
        }
        transform.transform(&input_row, row)?;
    }
    Ok(())
}
