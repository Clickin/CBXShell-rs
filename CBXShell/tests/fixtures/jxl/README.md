# Generated JPEG XL fixtures

Original synthetic 2 x 1 pixel images, generated for this repository using official libjxl v0.12.0 `cjxl` (https://github.com/libjxl/libjxl/releases/tag/v0.12.0). No third-party artwork.

- `rgb.jxl`: RGB8 red, green; raw codestream.
- `rgba.jxl`: RGBA8 red/255 alpha, green/128 alpha; raw codestream.
- `gray16.jxl`: grayscale16 black (0), white (65535); encoder emits container with metadata.
- `container.jxl`: same pixels as rgb.jxl in a JXL container.

Encode PNG inputs with `cjxl input.png output.jxl -d 0 -e 3 --container=0`; use `--container=1` for container.jxl. Exact expected RGBA8 pixels are asserted in codec tests. The encoder is a development tool only, not a build or runtime dependency.

- `rgba-associated.jxl`: stored RGBA8 samples (64,32,16,255), (32,64,16,128), encoded with `--premultiply=1`. This option marks existing samples as associated without multiplying them. Expected straight RGBA8 is (64,32,16,255), (64,128,32,128).

- `animated.jxl`: two 2 x 1 frames (red then green), encoded from a synthetic GIF with `-d 0 -e 3`; first-frame output is red.
- `lossy.jxl`: 16 x 16 solid RGB (128,64,32), encoded with `-d 1 -e 3 --modular=0` to exercise lossy VarDCT decoding.

- `large-vardct.jxl`: synthetic 4310 x 6083 solid RGB (128,64,32), encoded with libjxl v0.12.0 `cjxl -d 1 -e 3 --modular=0`. Only 2,673 bytes compressed, but its full-resolution working buffers exceed 256 MiB. Run the memory regression with `cargo test --release -p cbxshell-image-codec --test jpeg_xl jpeg_xl_high_resolution_working_memory`.

- `display-p3.jxl`: 2 x 1 RGB (128,64,32), lossless `cjxl -d 0 -e 3 -x color_space=DisplayP3`; reference sRGB (138,59,21) from LittleCMS via Pillow.
- `gray-gamma.jxl`: 2 x 1 gray 128 with a gamma-1.8 ICC profile created by moxcms 0.8.1, encoded losslessly from PNG; LittleCMS reference sRGB (146,146,146).

User archives are never included. Opt-in tests in `CBXShell/tests/test_jpeg_xl.rs` accept `CBXSHELL_TEST_ARCHIVE` and `CBXSHELL_TEST_DLL` to verify archive extraction, every JXL page, or a built DLL through its actual COM interface without registration.
