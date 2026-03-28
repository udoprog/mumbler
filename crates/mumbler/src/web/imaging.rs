use std::io::Cursor;

use anyhow::{Context, Result};
use api::{ContentType, CropRegion, ImageSizing};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, Rgba, imageops};

/// Process raw image bytes into a PNG.
///
/// If `crop` is provided the image is first cropped to that region; otherwise
/// the full image is used. The cropped (or full) image is then downscaled so
/// its longer side equals `size`.
///
/// When `square` is `true` the result is composited onto a `size × size` canvas
/// filled with the average colour of the resized image. When `false` the
/// resized image is returned as-is, preserving aspect ratio.
pub(crate) fn process(
    data: &[u8],
    crop: CropRegion,
    sizing: ImageSizing,
    size: u32,
) -> Result<(ContentType, u32, u32, Vec<u8>)> {
    let image = image::load_from_memory(data)?;

    let image = if !crop.is_whole_image(image.width(), image.height()) {
        let img_w = image.width();
        let img_h = image.height();
        let x = crop.x1.min(img_w.saturating_sub(1));
        let y = crop.y1.min(img_h.saturating_sub(1));
        let w = crop.x2.saturating_sub(crop.x1).min(img_w - x).max(1);
        let h = crop.y2.saturating_sub(crop.y1).min(img_h - y).max(1);
        image.crop_imm(x, y, w, h)
    } else {
        image
    };

    let rgba = image.to_rgba8();

    let (w, h) = rgba.dimensions();

    anyhow::ensure!(w > 0 && h > 0, "image has zero-sized dimension ({w}x{h})");

    let (new_w, new_h) = if w >= h {
        (size, (size * h / w).max(1))
    } else {
        ((size * w / h).max(1), size)
    };

    let small = imageops::resize(&rgba, new_w, new_h, FilterType::Lanczos3);

    let [r, g, b, a] = small
        .pixels()
        .try_fold([0u64; 4], |[r0, g0, b0, a0], &Rgba([r1, g1, b1, a1])| {
            Some([
                r0.checked_add(u64::from(r1))?,
                g0.checked_add(u64::from(g1))?,
                b0.checked_add(u64::from(b1))?,
                a0.checked_add(u64::from(a1))?,
            ])
        })
        .context("image is too large to process")?;

    let count = (new_w * new_h) as u64;

    let avg = image::Rgba([
        u8::try_from(r / count).unwrap_or(u8::MAX),
        u8::try_from(g / count).unwrap_or(u8::MAX),
        u8::try_from(b / count).unwrap_or(u8::MAX),
        u8::try_from(a / count).unwrap_or(u8::MAX),
    ]);

    let (out_w, out_h) = if sizing.is_square() {
        (size, size)
    } else {
        (new_w, new_h)
    };

    let output = if sizing.is_square() {
        // Create a `size×size` canvas filled with the average colour and center
        // the resized image onto it.
        let mut canvas = image::RgbaImage::from_pixel(size, size, avg);
        let x_offset = ((size - new_w) / 2) as i64;
        let y_offset = ((size - new_h) / 2) as i64;
        imageops::overlay(&mut canvas, &small, x_offset, y_offset);
        DynamicImage::ImageRgba8(canvas)
    } else {
        DynamicImage::ImageRgba8(small)
    };

    let mut bytes = Cursor::new(Vec::new());
    output.write_to(&mut bytes, ImageFormat::Png)?;
    Ok((ContentType::PNG, out_w, out_h, bytes.into_inner()))
}
