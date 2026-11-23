//! Perceptual fingerprint (a 64-bit DCT hash) of an image.
//!
//! Two copies of the same picture hash to nearby values even after resizing,
//! JPEG or WebP re-encoding, a screenshot or small colour shifts, so the
//! gallery index can find the original of a copy whose metadata was stripped.
//! Unlike a watermark it changes nothing in the file.
//!
//! The hash is the classic pHash: the image goes to 32x32 greyscale, a 2D DCT
//! is taken, and each of the 8x8 lowest frequencies (the DC term aside) sets a
//! bit when it is above their median.

use image::{imageops::FilterType, DynamicImage};

const SIDE: usize = 32;
const LOW: usize = 8;

/// Hamming distance at or under which two fingerprints are the same picture.
/// Re-encodes and resizes land at 0 to 6; unrelated images sit near 32.
pub const MATCH_DISTANCE: u32 = 10;

/// The fingerprint of `img`.
pub fn fingerprint(img: &DynamicImage) -> u64 {
    // Shrink in two steps: a large image straight to 32x32 with a 3-tap
    // filter aliases, and the hash then shifts with the resize filter.
    let small = if img.width() > 256 || img.height() > 256 {
        img.resize_exact(256, 256, FilterType::Triangle)
    } else {
        img.clone()
    };
    let grey = small
        .resize_exact(SIDE as u32, SIDE as u32, FilterType::Triangle)
        .to_luma32f();
    let pixels: Vec<f32> = grey.pixels().map(|p| p[0]).collect();
    let coeffs = dct_low(&pixels);

    let mut ac: Vec<f32> = coeffs[1..].to_vec();
    ac.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = (ac[ac.len() / 2 - 1] + ac[ac.len() / 2]) / 2.0;

    coeffs
        .iter()
        .enumerate()
        .skip(1)
        .fold(0u64, |acc, (i, c)| acc | (((*c > median) as u64) << i))
}

/// Hamming distance between two fingerprints.
pub fn distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// The `LOW` x `LOW` lowest-frequency coefficients of the 2D DCT-II of a
/// `SIDE` x `SIDE` image, row-major.
fn dct_low(pixels: &[f32]) -> Vec<f32> {
    let basis: Vec<f32> = (0..LOW)
        .flat_map(|k| {
            (0..SIDE).map(move |n| {
                (std::f32::consts::PI / SIDE as f32 * (n as f32 + 0.5) * k as f32).cos()
            })
        })
        .collect();
    // Rows first: SIDE rows x LOW frequencies.
    let mut rows = vec![0f32; SIDE * LOW];
    for y in 0..SIDE {
        for k in 0..LOW {
            rows[y * LOW + k] = (0..SIDE)
                .map(|x| pixels[y * SIDE + x] * basis[k * SIDE + x])
                .sum();
        }
    }
    let mut out = vec![0f32; LOW * LOW];
    for v in 0..LOW {
        for u in 0..LOW {
            out[v * LOW + u] = (0..SIDE)
                .map(|y| rows[y * LOW + u] * basis[v * SIDE + y])
                .sum();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smooth test picture with structure at several scales.
    fn scene(w: u32, h: u32, seed: f32) -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            let a = ((u * 7.0 + seed).sin() * (v * 5.0 - seed).cos() * 0.5 + 0.5) * 255.0;
            let b = (((u - 0.3).powi(2) + (v - 0.6).powi(2)).sqrt() * 400.0 + seed * 40.0) % 255.0;
            let c = if (u * 4.0 + seed) as u32 % 2 == (v * 3.0) as u32 % 2 {
                200.0
            } else {
                40.0
            };
            image::Rgb([a as u8, b as u8, c as u8])
        }))
    }

    fn jpeg(img: &DynamicImage, quality: u8) -> DynamicImage {
        let mut buf = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
            .encode_image(&img.to_rgb8())
            .unwrap();
        image::load_from_memory(&buf).unwrap()
    }

    #[test]
    fn survives_resize_and_jpeg() {
        let original = scene(832, 1216, 0.4);
        let fp = fingerprint(&original);
        let copy = jpeg(&original.resize_exact(416, 608, FilterType::Lanczos3), 70);
        assert!(distance(fp, fingerprint(&copy)) <= MATCH_DISTANCE / 2);
    }

    #[test]
    fn different_pictures_are_far_apart() {
        let a = fingerprint(&scene(512, 512, 0.4));
        let b = fingerprint(&scene(512, 512, 2.9));
        assert!(
            distance(a, b) > MATCH_DISTANCE,
            "distance {}",
            distance(a, b)
        );
    }

    #[test]
    fn the_same_pixels_hash_the_same() {
        let img = scene(300, 200, 1.0);
        assert_eq!(fingerprint(&img), fingerprint(&img.clone()));
    }

    #[test]
    fn distance_counts_differing_bits() {
        assert_eq!(distance(0b1011, 0b0010), 2);
        assert_eq!(distance(u64::MAX, 0), 64);
    }
}
