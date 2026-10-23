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
