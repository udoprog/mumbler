use std::io::Cursor;

use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, imageops};

/// Process raw image bytes into a square `size x size` PNG.
///
/// The image is first downscaled so its longer side equals `size`, then
/// composited onto a square canvas filled with the average colour of the
/// resized image.
pub(crate) fn process(data: &[u8], size: u32) -> Result<Vec<u8>> {
    let image = image::load_from_memory(data)?;
    let rgba = image.to_rgba8();

    let (w, h) = rgba.dimensions();

    anyhow::ensure!(w > 0 && h > 0, "image has zero-sized dimension ({w}x{h})");

    let (new_w, new_h) = if w >= h {
        (size, (size * h / w).max(1))
    } else {
        ((size * w / h).max(1), size)
    };

    let small = imageops::resize(&rgba, new_w, new_h, FilterType::Lanczos3);

    let (r, g, b, a) = small
        .pixels()
        .try_fold((0u64, 0u64, 0u64, 0u64), |(r, g, b, a), p| {
            Some((
                r.checked_add(p[0] as u64)?,
                g.checked_add(p[1] as u64)?,
                b.checked_add(p[2] as u64)?,
                a.checked_add(p[3] as u64)?,
            ))
        })
        .context("image is too large to process")?;

    let count = (new_w * new_h) as u64;

    let avg = image::Rgba([
        (r / count) as u8,
        (g / count) as u8,
        (b / count) as u8,
        (a / count) as u8,
    ]);

    // Create a `size×size` canvas filled with the average colour and center
    // the resized image onto it.
    let mut canvas = image::RgbaImage::from_pixel(size, size, avg);
    let x_offset = ((size - new_w) / 2) as i64;
    let y_offset = ((size - new_h) / 2) as i64;
    imageops::overlay(&mut canvas, &small, x_offset, y_offset);

    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(canvas).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(bytes.into_inner())
}
