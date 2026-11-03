# Pictures to pages

How a picture becomes a teletext page of mosaic graphics in `neoteletext-picture` (DEC-2142, DEC-2325). The requirements are REQ-2327 and REQ-2328 to REQ-2337 in rdm; this document explains them. The Python script `scripts/image-to-teletext.py`, which this library was ported from, stays as it is beside it (DEC-2142 item 1).

## From bytes to page

1. **Decoding** (`decode.rs`, REQ-2330). The bytes are refused over 64 MiB. The `image` crate reads the header, refused over 32,768 pixels a side; the decoded size, known from the header, is refused over 512 MiB before a pixel is decoded, since the crate's own memory limit lets a buffer through that the system has not yet backed. The picture is turned upright by its orientation tag, and any alpha is laid over the background colour (REQ-2328). An animation gives its first frame.
2. **Fitting** (`fit.rs`, `raster.rs`, REQ-2331 to REQ-2334). A crop region is cut first. The picture is then cropped, contained or stretched to the page's raster as the engine draws it, 480 dots by 20 lines a row (rows 0 to 24, or 1 to 24 when the header is kept), for a display whose dots are square or, on a television, 16/13 as wide as tall. Scaling is in fixed point, by a box or a Lanczos filter, with the rounding and pass order of the library the script used, so the pages match it. Each sixel is then the mean of the 6 dots by 6, 8 or 6 lines it covers. Pixel art skips the raster: each pixel is a sixel, enlarged by a whole number.
3. **Encoding** (`level1.rs`, `enhanced.rs`, REQ-2335). At Levels 1 and 1.5 a dynamic programme along each row chooses each cell's mosaic or spacing attribute for the least error, each sixel's error weighted by its height, the receiver's eight columns of row 0 weighted nothing. At Levels 2.5 and 3.5 the palette is fitted from three reductions of the picture (`quant.rs`) to the rule of two colours a cell, and colour changes are X/26 triplets, in the page's own sixteen packets by default or in Active objects on page 1A0.
4. **Writing** (`tti.rs`). The page is page 100 of a TTI file, with Fastext links to itself so row 24 shows; `Page::t42` reads the file back with the engine's TTI reader and encodes it with its T42 writer.

## Where untrusted input is handled

The picture's sizes come from the input, so `decode`, `raster` and `fit` use only checked access and saturating arithmetic. Everything after them works on the fixed 80 by 75 sixels and palettes of at most 32 colours; the functions there that index allow it one by one with that reason. The crate opens nothing (REQ-SEC-016), so a program can run it in a confined process: the browser does, in a decoder process with no rights. The fuzz target `picture_convert` feeds arbitrary bytes and options; its first run, 37,405 inputs in two minutes on 3 November 2026, found no fault.

## Measured

On 3 November 2026, on Linux x86_64, the library's pages were byte-identical to the prototype's for 24 conversions (six pictures, PNG and JPEG, at Level 1.5, Level 2.5 with objects, and Level 3.5 with and without objects), and its resampler equals Pillow 13.0.0's output for ten scalings of a synthetic picture. A conversion took at most 1.5 seconds at Level 3.5 without objects, and 0.01 seconds at Level 1.5, in a release build.
