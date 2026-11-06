//! TrustMark invisible watermarking, variant P.
//!
//! A port of Adobe's TrustMark Rust crate (trustmark 0.4.0, MIT License,
//! Copyright (c) 2024 Adobe, <https://github.com/adobe/trustmark>), which
//! implements "TrustMark - Universal Watermarking for Arbitrary Resolution
//! Images" (Bui, Agarwal, Collomosse, arXiv:2311.18297).
//!
//! Why it is vendored instead of depended on: the crate pins
//! `ort = "=2.0.0-rc.10"`, and `ort-sys` links the native runtime, so it can't
//! sit next to the app's own `ort` 2.0 release candidate. The port keeps the
//! crate's maths (`bits.rs`, the verbatim `bch.rs`) and swaps its
//! `ndarray`/`fast_image_resize` plumbing for plain `f32` buffers and the
//! `image` crate already in the tree. Only variant P (the "perceptual" model,
//! tuned for creative imagery) is carried.
//!
//! How it works: the encoder sees a 256x256 copy of the image's centre square
//! plus 100 payload bits and returns a watermarked 256x256 copy. The
//! difference (the residual) is upscaled and added to the full-size image, so
//! the cost is the same at any output size and the mark survives resizing.

#[allow(clippy::all)]
mod bch;
pub mod bits;

use image::{imageops::FilterType, DynamicImage, GenericImageView as _, Rgb32FImage};
use ort::session::Session;

pub use bits::Version;

/// Side of the square the encoder works on.
const ENCODE_SIZE: u32 = 256;
/// Side of the square the variant P decoder reads.
const DECODE_SIZE: u32 = 224;
/// Variant P's residual gain.
const STRENGTH_MULTIPLIER: f32 = 1.25;
/// TrustMark's "normal" strength.
pub const DEFAULT_STRENGTH: f32 = 0.95;
/// Residual bound, in model units ([-1, 1] pixel range).
const RESIDUAL_LIMIT: f32 = 0.2;
/// Border, in residual pixels, blended to the mean to hide the crop edge.
const BOUNDARY: usize = 2;

pub const ENCODER_FILENAME: &str = "encoder_P.onnx";
pub const DECODER_FILENAME: &str = "decoder_P.onnx";

/// Loaded encoder and decoder sessions.
pub struct TrustMark {
    encoder: Session,
    decoder: Session,
    version: Version,
}

fn ort_err<E: std::fmt::Display>(what: &str) -> impl Fn(E) -> String + '_ {
    move |e| format!("TrustMark {what}: {e}")
}

fn session(path: &std::path::Path, threads: usize) -> Result<Session, String> {
    Session::builder()
        .map_err(ort_err("session builder"))?
        .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
        .map_err(ort_err("optimization level"))?
        .with_intra_threads(threads)
        .map_err(ort_err("thread count"))?
        .commit_from_file(path)
        .map_err(|e| format!("TrustMark model {}: {e}", path.display()))
}

impl TrustMark {
    /// Load both models from `dir`. ONNX Runtime must already be initialised.
    pub fn load(dir: &std::path::Path, version: Version) -> Result<Self, String> {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4);
        Ok(Self {
            encoder: session(&dir.join(ENCODER_FILENAME), threads)?,
            decoder: session(&dir.join(DECODER_FILENAME), threads)?,
            version,
        })
    }

    /// Watermark `img` with `data` (a '0'/'1' string of at most
    /// [`Self::data_bits`] bits). Returns 8-bit RGBA, alpha untouched.
    pub fn encode(
        &mut self,
        img: &DynamicImage,
        data: &str,
        strength: f32,
    ) -> Result<image::RgbaImage, String> {
        let payload = bits::Bits::apply_error_correction_and_schema(data.to_owned(), self.version)
            .map_err(|e| format!("TrustMark payload: {e}"))?;
        let cover = model_input(img, ENCODE_SIZE);

        let size = ENCODE_SIZE as i64;
        let image_in = ort::value::Tensor::from_array((vec![1_i64, 3, size, size], cover.clone()))
            .map_err(ort_err("input tensor"))?;
        let bits_in = ort::value::Tensor::from_array((
            vec![1_i64, bits::PAYLOAD_BITS as i64],
            payload.to_floats(),
        ))
        .map_err(ort_err("payload tensor"))?;
        let outputs = self
            .encoder
            .run(ort::inputs![
                "onnx::Concat_0" => image_in,
                "onnx::Gemm_1" => bits_in,
            ])
            .map_err(ort_err("encode"))?;
        let stego = outputs
            .get("image")
            .ok_or("TrustMark encoder returned no image")?
            .downcast_ref::<ort::value::DynTensorValueType>()
            .map_err(ort_err("encoder output"))?
            .try_extract_tensor::<f32>()
            .map_err(ort_err("encoder output"))?
            .1
            .to_vec();
        if stego.len() != cover.len() {
            return Err(format!(
                "TrustMark encoder returned {} values, expected {}",
                stego.len(),
                cover.len()
            ));
        }

        let gain = STRENGTH_MULTIPLIER * strength;
        let residual: Vec<f32> = stego
            .iter()
            .zip(&cover)
            .map(|(s, c)| (gain * (s - c)).clamp(-RESIDUAL_LIMIT, RESIDUAL_LIMIT))
            .collect();
        let (w, h) = img.dimensions();
        let residual = pad_to_aspect(residual, w as usize, h as usize);
        Ok(apply_residual(img, &residual))
    }

    /// Read the data bits back, or `None` when there is no readable mark.
    pub fn decode(&mut self, img: &DynamicImage) -> Result<Option<String>, String> {
        let size = DECODE_SIZE as i64;
        let input = ort::value::Tensor::from_array((
            vec![1_i64, 3, size, size],
            model_input(img, DECODE_SIZE),
        ))
        .map_err(ort_err("input tensor"))?;
        let outputs = self
            .decoder
            .run(ort::inputs!["image" => input])
            .map_err(ort_err("decode"))?;
        let logits = outputs
            .get("output")
            .ok_or("TrustMark decoder returned no output")?
            .downcast_ref::<ort::value::DynTensorValueType>()
            .map_err(ort_err("decoder output"))?
            .try_extract_tensor::<f32>()
            .map_err(ort_err("decoder output"))?
            .1
            .to_vec();
        if logits.len() != bits::PAYLOAD_BITS {
            return Err(format!(
                "TrustMark decoder returned {} bits, expected {}",
                logits.len(),
                bits::PAYLOAD_BITS
            ));
        }
        match bits::Bits::from_logits(&logits) {
            Ok(b) => Ok(Some(b.get_data())),
            Err(bits::Error::CorruptWatermark) => Ok(None),
            Err(e) => Err(format!("TrustMark payload: {e}")),
        }
    }
}

/// Centre square of `(w, h)`: `(x, y, side)`. Variant P always crops.
fn centre_square(w: u32, h: u32) -> (u32, u32, u32) {
    let side = w.min(h);
    let offset = (w.max(h) - side) / 2;
    if h > w {
        (0, offset, side)
    } else {
        (offset, 0, side)
    }
}

/// The centre square resized to `size`, as NCHW floats in [-1, 1].
fn model_input(img: &DynamicImage, size: u32) -> Vec<f32> {
    let (w, h) = img.dimensions();
    let (x, y, side) = centre_square(w, h);
    let square = img
        .crop_imm(x, y, side, side)
        .resize_exact(size, size, FilterType::Triangle)
        .into_rgb32f();
    let plane = (size * size) as usize;
    let mut out = vec![0f32; plane * 3];
    for (i, px) in square.pixels().enumerate() {
        for c in 0..3 {
            out[c * plane + i] = px[c] * 2.0 - 1.0;
        }
    }
    out
}

/// A residual laid out for the full image: the 256x256 centre-square residual
/// sits in the middle of a canvas with the image's aspect ratio (one side 256),
/// and everything outside it, plus a thin border around it, takes the
/// channel's mean so the crop edge does not show. Returned as an RGB float
/// image in the [0, 1] encoding (`(r + 1) / 2`) so it can be resampled.
fn pad_to_aspect(mut residual: Vec<f32>, width: usize, height: usize) -> Rgb32FImage {
    let n = ENCODE_SIZE as usize;
    let plane = n * n;
    let means: Vec<f32> = (0..3)
        .map(|c| residual[c * plane..(c + 1) * plane].iter().sum::<f32>() / plane as f32)
        .collect();
    for (c, mean) in means.iter().enumerate() {
        let ch = &mut residual[c * plane..(c + 1) * plane];
        for y in 0..n {
            for x in 0..n {
                if y < BOUNDARY || y >= n - BOUNDARY || x < BOUNDARY || x >= n - BOUNDARY {
                    ch[y * n + x] = *mean;
                }
            }
        }
    }

    let (out_w, out_h) = if width > height {
        (((width as f32 / height as f32) * n as f32) as usize, n)
    } else {
        (n, ((height as f32 / width as f32) * n as f32) as usize)
    };
    let (off_x, off_y) = ((out_w - n) / 2, (out_h - n) / 2);

    Rgb32FImage::from_fn(out_w as u32, out_h as u32, |x, y| {
        let (x, y) = (x as usize, y as usize);
        let inside = x >= off_x && x < off_x + n && y >= off_y && y < off_y + n;
        let value = |c: usize| {
            let r = if inside {
                residual[c * plane + (y - off_y) * n + (x - off_x)]
            } else {
                means[c]
            };
            (r + 1.0) / 2.0
        };
        image::Rgb([value(0), value(1), value(2)])
    })
}

/// Upscale `residual` to the image and add it. In [0, 1] pixel units the
/// change is `r / 2`, matching the reference `(min(2p - 1 + r, 1) + 1) / 2`.
fn apply_residual(img: &DynamicImage, residual: &Rgb32FImage) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    let residual = image::imageops::resize(residual, w, h, FilterType::Triangle);
    let mut out = img.to_rgba8();
    for (px, r) in out.pixels_mut().zip(residual.pixels()) {
        for c in 0..3 {
            let delta = r[c] * 2.0 - 1.0;
            let v = (px[c] as f32 / 255.0 + delta / 2.0).min(1.0);
            px[c] = (v * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_square_crops_the_long_side() {
        assert_eq!(centre_square(100, 110), (0, 5, 100));
        assert_eq!(centre_square(101, 10), (45, 0, 10));
        assert_eq!(centre_square(64, 64), (0, 0, 64));
    }

    #[test]
    fn model_input_is_nchw_in_minus_one_to_one() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            300,
            200,
            image::Rgb([255, 0, 128]),
        ));
        let t = model_input(&img, 8);
        assert_eq!(t.len(), 3 * 64);
        assert!(t[..64].iter().all(|v| (*v - 1.0).abs() < 1e-6));
        assert!(t[64..128].iter().all(|v| (*v + 1.0).abs() < 1e-6));
        assert!(t[128..].iter().all(|v| v.abs() < 0.01));
    }

    #[test]
    fn padding_follows_the_aspect_ratio() {
        let residual = vec![0.1f32; 3 * 256 * 256];
        assert_eq!(
            pad_to_aspect(residual.clone(), 832, 1216).dimensions(),
            (256, 374)
        );
        assert_eq!(
            pad_to_aspect(residual.clone(), 1216, 832).dimensions(),
            (374, 256)
        );
        assert_eq!(pad_to_aspect(residual, 512, 512).dimensions(), (256, 256));
    }

    #[test]
    fn a_zero_residual_leaves_pixels_alone() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(40, 30, |x, y| {
            image::Rgba([(x * 6) as u8, (y * 8) as u8, 77, 200])
        }));
        let residual = pad_to_aspect(vec![0.0; 3 * 256 * 256], 40, 30);
        assert_eq!(apply_residual(&img, &residual), img.to_rgba8());
    }
}
