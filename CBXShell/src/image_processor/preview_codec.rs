//! Canonical preview-proxy cache codec (plan sections 2.3 and 3)
//!
//! The persistent L2 thumbnail cache stores one canonical preview proxy per
//! archive variant. The proxy is always a *fully rendered* image: the RGBA
//! passed to [`PreviewCacheCodec::encode`] must already reflect the visible
//! rendering policy (resized to at most [`PROXY_MAX_DIMENSION`] on the
//! longest edge, background composited, alpha forced opaque) exactly as
//! produced by the thumbnail rendering boundary (`render_canonical_proxy` /
//! `RenderedThumbnail`).
//!
//! # Trait shape deviation from the plan sketch
//!
//! The plan sketches `encode(image: &RenderedThumbnail)`. This crate sits
//! *below* the rendering boundary in the dependency layering
//! (`cbxshell-thumbnail-win` depends on `cbxshell-image-codec`), so
//! referencing the rendering-boundary type here would create a dependency
//! cycle. The trait therefore takes plain `(width, height, rgba)` and
//! callers bridge through the rendering type's public fields.
//!
//! # Codec choice
//!
//! [`JpegPreviewCodec`] is the sanctioned shipping codec (plan section 2.3
//! fallback): the `image` crate's WebP encoder is lossless-only, and lossy
//! WebP would require libwebp C bindings — a build/CI constraint for the
//! x64 + ARM64 shell DLL. JPEG runs on the existing pure-Rust stack.
//! [`WebpLosslessPreviewCodec`] exists purely as a benchmark comparator and
//! must not become the default. Final quality tuning still requires
//! benchmarking with representative comic covers (plan section 2.3).

use crate::utils::error::CbxError;

type Result<T, E = CbxError> = std::result::Result<T, E>;

/// Canonical proxy maximum dimension in pixels (plan section 2.2)
///
/// Sources whose longest edge exceeds this are downscaled preserving the
/// aspect ratio; smaller sources are stored at their natural decoded size
/// (never upscaled).
pub const PROXY_MAX_DIMENSION: u16 = 1024;

/// Revision of the cache proxy encode format
///
/// Bump this when the encode format or quality level changes so that stale
/// persisted proxies become unreachable and are regenerated.
pub const CACHE_CODEC_REVISION: u16 = 1;

/// Decoded canonical preview proxy
///
/// Plain RGBA pixel data (`width * height * 4` bytes) as produced by
/// [`PreviewCacheCodec::decode`]. Pixels are fully opaque (alpha = 255):
/// the canonical proxy is a rendered image, and the shipping JPEG codec has
/// no alpha channel to preserve anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPreview {
    /// Proxy width in pixels
    pub width: u32,

    /// Proxy height in pixels
    pub height: u32,

    /// RGBA pixel bytes (`width * height * 4`)
    pub rgba: Vec<u8>,
}

impl DecodedPreview {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self {
            width,
            height,
            rgba,
        }
    }
}

/// Encode/decode contract for persisted canonical preview proxies
///
/// Implementations must be deterministic: encoding the same pixels with the
/// same configuration must produce byte-identical output, so the L2 cache
/// can verify or regenerate entries reproducibly.
///
/// See the module docs for why this takes `(width, height, rgba)` instead
/// of the plan's `&RenderedThumbnail`.
pub trait PreviewCacheCodec {
    /// Encode fully rendered RGBA pixels (`width * height * 4` bytes) into
    /// the persisted proxy byte stream.
    ///
    /// The input must already be fully rendered (background applied, alpha
    /// opaque); encoders may drop or flatten any remaining alpha.
    fn encode(&self, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>>;

    /// Decode persisted proxy bytes back into RGBA pixels.
    fn decode(&self, data: &[u8]) -> Result<DecodedPreview>;
}

/// JPEG canonical proxy codec (shipping default)
///
/// Lossy JPEG via the `image` crate's pure-Rust encoder
/// (`JpegEncoder::new_with_quality`). JPEG has no alpha channel, so alpha is
/// dropped on encode — safe by contract because proxies are fully rendered
/// (alpha = 255) before they reach the codec.
#[derive(Debug, Clone)]
pub struct JpegPreviewCodec {
    /// JPEG quality, 1 (worst) to 100 (best). Default: 90.
    pub quality: u8,
}

impl Default for JpegPreviewCodec {
    fn default() -> Self {
        Self { quality: 90 }
    }
}

impl PreviewCacheCodec for JpegPreviewCodec {
    fn encode(&self, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
        // JPEG has no alpha channel and the image-crate encoder rejects
        // Rgba8 input, so flatten RGBA to RGB. Safe by contract: proxies
        // are fully rendered (alpha = 255) before they reach the codec.
        let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
        for px in rgba.chunks_exact(4) {
            rgb.extend_from_slice(&[px[0], px[1], px[2]]);
        }

        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, self.quality)
            .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
            .map_err(|e| CbxError::Image(format!("JPEG proxy encode failed: {}", e)))?;
        Ok(encoded)
    }

    fn decode(&self, data: &[u8]) -> Result<DecodedPreview> {
        let img = image::load_from_memory(data)
            .map_err(|e| CbxError::Image(format!("JPEG proxy decode failed: {}", e)))?;
        Ok(DecodedPreview::new(
            img.width(),
            img.height(),
            img.to_rgba8().into_raw(),
        ))
    }
}

/// Lossless WebP canonical proxy codec (benchmark comparator only)
///
/// Uses the `image` crate's lossless WebP encoder. Kept solely to compare
/// size/latency against [`JpegPreviewCodec`] in the codec benchmark; it is
/// not the shipping default (see the module docs for the rationale).
#[derive(Debug, Clone, Default)]
pub struct WebpLosslessPreviewCodec;

impl PreviewCacheCodec for WebpLosslessPreviewCodec {
    fn encode(&self, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
        let mut encoded = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
            .encode(rgba, width, height, image::ExtendedColorType::Rgba8)
            .map_err(|e| CbxError::Image(format!("WebP proxy encode failed: {}", e)))?;
        Ok(encoded)
    }

    fn decode(&self, data: &[u8]) -> Result<DecodedPreview> {
        let img = image::load_from_memory(data)
            .map_err(|e| CbxError::Image(format!("WebP proxy decode failed: {}", e)))?;
        Ok(DecodedPreview::new(
            img.width(),
            img.height(),
            img.to_rgba8().into_raw(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic non-uniform RGBA test pattern of `width` x `height`
    fn test_pattern(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let r = (x * 7 % 256) as u8;
                let g = (y * 11 % 256) as u8;
                let b = ((x + y) * 3 % 256) as u8;
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        rgba
    }

    #[test]
    fn test_jpeg_roundtrip_preserves_dimensions() {
        let codec = JpegPreviewCodec::default();
        let rgba = test_pattern(17, 13);

        let encoded = codec
            .encode(17, 13, &rgba)
            .expect("JPEG encode should succeed");
        let decoded = codec.decode(&encoded).expect("JPEG decode should succeed");

        assert_eq!((decoded.width, decoded.height), (17, 13));
        assert_eq!(decoded.rgba.len(), (17 * 13 * 4) as usize);
    }

    #[test]
    fn test_webp_lossless_roundtrip_preserves_dimensions() {
        let codec = WebpLosslessPreviewCodec;
        let rgba = test_pattern(17, 13);

        let encoded = codec
            .encode(17, 13, &rgba)
            .expect("WebP encode should succeed");
        let decoded = codec.decode(&encoded).expect("WebP decode should succeed");

        assert_eq!((decoded.width, decoded.height), (17, 13));
        assert_eq!(decoded.rgba.len(), (17 * 13 * 4) as usize);
    }

    #[test]
    fn test_decoded_preview_alpha_fully_opaque() {
        let rgba = test_pattern(16, 12);

        let jpeg = JpegPreviewCodec::default();
        let jpeg_decoded = jpeg
            .decode(&jpeg.encode(16, 12, &rgba).expect("encode should succeed"))
            .expect("decode should succeed");
        assert!(
            jpeg_decoded.rgba.chunks_exact(4).all(|px| px[3] == 255),
            "JPEG-decoded proxy pixels must be fully opaque"
        );

        let webp = WebpLosslessPreviewCodec;
        let webp_decoded = webp
            .decode(&webp.encode(16, 12, &rgba).expect("encode should succeed"))
            .expect("decode should succeed");
        assert!(
            webp_decoded.rgba.chunks_exact(4).all(|px| px[3] == 255),
            "WebP-decoded proxy pixels must be fully opaque"
        );
    }

    #[test]
    fn test_jpeg_encode_deterministic() {
        let codec = JpegPreviewCodec::default();
        let rgba = test_pattern(16, 12);

        let first = codec.encode(16, 12, &rgba).expect("encode should succeed");
        let second = codec.encode(16, 12, &rgba).expect("encode should succeed");
        assert_eq!(first, second, "same input must encode to identical bytes");
    }

    #[test]
    fn test_webp_lossless_encode_deterministic() {
        let codec = WebpLosslessPreviewCodec;
        let rgba = test_pattern(16, 12);

        let first = codec.encode(16, 12, &rgba).expect("encode should succeed");
        let second = codec.encode(16, 12, &rgba).expect("encode should succeed");
        assert_eq!(first, second, "same input must encode to identical bytes");
    }

    #[test]
    fn test_jpeg_decode_invalid_data_errors() {
        let codec = JpegPreviewCodec::default();
        assert!(codec.decode(b"this is not an image").is_err());
        assert!(codec.decode(&[]).is_err());
    }

    #[test]
    fn test_webp_decode_invalid_data_errors() {
        let codec = WebpLosslessPreviewCodec;
        assert!(codec.decode(b"this is not an image").is_err());
        assert!(codec.decode(&[]).is_err());
    }

    #[test]
    fn test_jpeg_default_quality_is_90() {
        assert_eq!(JpegPreviewCodec::default().quality, 90);
    }
}

#[cfg(test)]
mod benches {
    use super::{
        DecodedPreview, JpegPreviewCodec, PreviewCacheCodec, WebpLosslessPreviewCodec,
        PROXY_MAX_DIMENSION,
    };
    use crate::decoder::decode_rgba_image;
    use image::{ImageBuffer, RgbaImage};
    use std::time::Instant;

    /// Real images from the repository, embedded at compile time.
    ///
    /// `include_bytes!` resolves relative to this source file
    /// (`CBXShell/src/image_processor/`), so three levels up is the repo
    /// root. Only git-tracked images are embedded so fresh clones compile.
    const BENCH_IMAGES: &[(&str, &[u8])] = &[
        (
            "Assets/SplashScreen.png (620x300)",
            include_bytes!("../../../Assets/SplashScreen.png"),
        ),
        (
            "Assets/Wide310x150Logo.png (310x150)",
            include_bytes!("../../../Assets/Wide310x150Logo.png"),
        ),
        (
            "Assets/Square150x150Logo.png (150x150)",
            include_bytes!("../../../Assets/Square150x150Logo.png"),
        ),
    ];

    /// Number of timed iterations per measurement
    const BENCH_ITERS: usize = 3;

    /// Canonical proxy dimensions (plan section 2.2): fit within
    /// [`PROXY_MAX_DIMENSION`] preserving aspect ratio, never upscaling.
    /// Benchmark-local mirror of the production size rule.
    fn canonical_dims(width: u32, height: u32) -> (u32, u32) {
        let max = u32::from(PROXY_MAX_DIMENSION);
        if width <= max && height <= max {
            return (width, height);
        }
        let scale = max as f64 / width.max(height) as f64;
        (
            ((width as f64 * scale).round() as u32).max(1),
            ((height as f64 * scale).round() as u32).max(1),
        )
    }

    /// Deterministic synthetic "cover": smooth gradient plus a checker
    /// pattern, sized to force the canonical downscale path.
    fn synthetic_source() -> (u32, u32, Vec<u8>) {
        let (w, h) = (1024u32, 1536u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let mut r = (x * 255 / w) as u8;
                let mut g = (y * 255 / h) as u8;
                let b = (((x ^ y) * 255) / w.max(h)) as u8;
                if (x / 32 + y / 32) % 2 == 0 {
                    r = r.saturating_add(40);
                    g = g.saturating_add(40);
                }
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        (w, h, rgba)
    }

    fn resample(rgba: &[u8], src: (u32, u32), dst: (u32, u32)) -> Vec<u8> {
        if src == dst {
            return rgba.to_vec();
        }
        let img = ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(src.0, src.1, rgba.to_vec())
            .expect("fixture pixel buffer size must match dimensions");
        let resized: RgbaImage =
            image::imageops::resize(&img, dst.0, dst.1, image::imageops::FilterType::Triangle);
        resized.into_raw()
    }

    /// Codec benchmark over real repo images and one synthetic cover
    /// (plan section 2.3 / section 18 Phase 3).
    ///
    /// Run with:
    /// `cargo test -p cbxshell-image-codec --release -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn benchmark_canonical_proxy_codecs() {
        // Assemble proxy inputs: decode, then apply the canonical dimension
        // rule. (Resampling here uses the image crate, not the production
        // fast_image_resize path; only codec size/latency is measured.)
        let mut sources: Vec<(String, u32, u32, Vec<u8>)> = Vec::new();
        for (label, bytes) in BENCH_IMAGES {
            let img = decode_rgba_image(bytes).expect("bench fixture must decode");
            let (src_w, src_h) = (img.width(), img.height());
            let (w, h) = canonical_dims(src_w, src_h);
            let rgba = resample(&img.into_pixels(), (src_w, src_h), (w, h));
            sources.push(((*label).to_string(), w, h, rgba));
        }
        let (sw, sh, srgba) = synthetic_source();
        let (dw, dh) = canonical_dims(sw, sh);
        let srgba = resample(&srgba, (sw, sh), (dw, dh));
        sources.push((format!("synthetic gradient {sw}x{sh}"), dw, dh, srgba));

        let codecs: Vec<(String, Box<dyn PreviewCacheCodec>)> = vec![
            (
                "JPEG q75".to_string(),
                Box::new(JpegPreviewCodec { quality: 75 }) as _,
            ),
            (
                "JPEG q85".to_string(),
                Box::new(JpegPreviewCodec { quality: 85 }) as _,
            ),
            (
                "JPEG q90".to_string(),
                Box::new(JpegPreviewCodec { quality: 90 }) as _,
            ),
            (
                "JPEG q95".to_string(),
                Box::new(JpegPreviewCodec { quality: 95 }) as _,
            ),
            (
                "WebP lossless".to_string(),
                Box::new(WebpLosslessPreviewCodec) as _,
            ),
        ];

        let mut rows: Vec<(String, String, String, usize, f64, f64)> = Vec::new();
        for (codec_name, codec) in &codecs {
            for (label, w, h, rgba) in &sources {
                // Warmup + correctness sanity
                let encoded: Vec<u8> = codec.encode(*w, *h, rgba).expect("encode must succeed");
                let DecodedPreview {
                    width: dw2,
                    height: dh2,
                    ..
                } = codec.decode(&encoded).expect("decode must succeed");
                assert_eq!((dw2, dh2), (*w, *h), "roundtrip must preserve dims");

                let mut enc_total = std::time::Duration::ZERO;
                let mut dec_total = std::time::Duration::ZERO;
                let mut last = Vec::new();
                for _ in 0..BENCH_ITERS {
                    let t = Instant::now();
                    last = codec.encode(*w, *h, rgba).expect("encode must succeed");
                    enc_total += t.elapsed();

                    let t = Instant::now();
                    let _ = codec.decode(&last).expect("decode must succeed");
                    dec_total += t.elapsed();
                }

                rows.push((
                    label.clone(),
                    format!("{w}x{h}"),
                    codec_name.clone(),
                    last.len(),
                    enc_total.as_secs_f64() * 1000.0 / BENCH_ITERS as f64,
                    dec_total.as_secs_f64() * 1000.0 / BENCH_ITERS as f64,
                ));
            }
        }

        println!(
            "\n{:<42} {:>10} {:<14} {:>10} {:>10} {:>10}",
            "proxy source", "dims", "codec", "size KiB", "enc ms", "dec ms"
        );
        println!("{}", "-".repeat(100));
        for (label, dims, codec, size, enc, dec) in &rows {
            println!(
                "{:<42} {:>10} {:<14} {:>10.1} {:>10.2} {:>10.2}",
                label,
                dims,
                codec,
                *size as f64 / 1024.0,
                enc,
                dec
            );
        }

        // Storage projection: average proxy size x library sizes
        println!("\nStorage projection (avg proxy size x entries):");
        for (codec_name, _) in &codecs {
            let sizes: Vec<usize> = rows
                .iter()
                .filter(|(_, _, c, ..)| c == codec_name)
                .map(|(.., size, _, _)| *size)
                .collect();
            let avg = sizes.iter().sum::<usize>() as f64 / sizes.len() as f64;
            print!("{:<14} avg {:>8.1} KiB |", codec_name, avg / 1024.0);
            for entries in [10_000usize, 30_000, 50_000] {
                let mib = avg * entries as f64 / (1024.0 * 1024.0);
                print!(" {:>2}k: {:>6.0} MiB |", entries / 1_000, mib);
            }
            println!();
        }
    }
}
