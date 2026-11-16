//! Images an assistant hands to a tool: a gallery file name, or the picture
//! itself as base64 (for one the assistant got from somewhere else).
//!
//! Every source is decoded and re-encoded as a plain PNG before it goes
//! anywhere, so nothing but pixels leaves this module: no metadata chunks, and
//! no path an assistant could point outside the gallery.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;

use crate::state::AppState;

/// Largest base64 image accepted, decoded.
const MAX_INLINE_BYTES: usize = 25 * 1024 * 1024;
/// Longest side a source may have.
const MAX_SOURCE_SIDE: u32 = 8192;

pub struct SourceImage {
    pub image: image::DynamicImage,
    /// Generation settings read from a gallery file; empty for inline data.
    pub metadata: HashMap<String, String>,
    /// The gallery file name, for the job record.
    pub filename: Option<String>,
}

impl SourceImage {
    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }
}

/// Whether `spec` is image data rather than a gallery file name. Base64 has
/// no `.`, and a file name always has an extension.
fn is_inline(spec: &str) -> bool {
    spec.starts_with("data:") || (spec.len() > 256 && !spec.contains('.'))
}

fn decode_inline(spec: &str) -> Result<Vec<u8>, String> {
    let data = match spec.strip_prefix("data:") {
        Some(rest) => {
            let (mime, data) = rest
                .split_once(";base64,")
                .ok_or("A data URL must be base64 (data:image/png;base64,...)")?;
            if !mime.starts_with("image/") {
                return Err(format!("{mime} is not an image"));
            }
            data
        }
        None => spec,
    };
    let cleaned: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() / 4 * 3 > MAX_INLINE_BYTES {
        return Err("The image is over 25 MB".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|e| format!("The image isn't valid base64: {e}"))
}

/// Load a source: a gallery file name, or base64 / a data URL.
pub async fn load(spec: &str) -> Result<SourceImage, String> {
    let spec = spec.trim().to_string();
    if spec.is_empty() {
        return Err("Empty image".into());
    }
    tokio::task::spawn_blocking(move || {
        let (bytes, filename) = if is_inline(&spec) {
            (decode_inline(&spec)?, None)
        } else {
            let dir = super::tools::gallery_dir()?;
            let path = super::tools::gallery_image_path(&dir, &spec)?;
            (std::fs::read(&path).map_err(|e| e.to_string())?, Some(spec))
        };
        let image = crate::commands::api::decode_gallery_image(&bytes)
            .map_err(|e| format!("Could not read the image: {e}"))?;
        if image.width().max(image.height()) > MAX_SOURCE_SIDE {
            return Err(format!(
                "The image is {}x{}; {MAX_SOURCE_SIDE} px on a side at most.",
                image.width(),
                image.height()
            ));
        }
        let metadata = if filename.is_some() {
            crate::metadata::read_image_metadata(&bytes)
                .ok()
                .flatten()
                .unwrap_or_default()
        } else {
            HashMap::new()
        };
        Ok(SourceImage {
            image,
            metadata,
            filename,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn encode_png(image: &image::DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(out.into_inner())
}

/// Upload a source to ComfyUI's input folder and return the name a workflow
/// loads it by.
pub async fn upload(state: &Arc<AppState>, src: &SourceImage) -> Result<String, String> {
    let image = src.image.clone();
    let png = tokio::task::spawn_blocking(move || encode_png(&image))
        .await
        .map_err(|e| e.to_string())??;
    let name = format!("mcp_{}.png", uuid::Uuid::new_v4().simple());
    let resp = state
        .upload_image_from_bytes(png, name)
        .await
        .map_err(|e| format!("Could not upload the image to ComfyUI (is it running?): {e}"))?;
    Ok(if resp.subfolder.is_empty() {
        resp.name
    } else {
        format!("{}/{}", resp.subfolder, resp.name)
    })
}

/// The source resized to exactly `width` x `height`, as base64 PNG: what
/// NovelAI's img2img takes.
pub async fn base64_png_at(src: &SourceImage, width: u32, height: u32) -> Result<String, String> {
    let image = src.image.clone();
    tokio::task::spawn_blocking(move || {
        let fitted = if image.width() == width && image.height() == height {
            image
        } else {
            image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
        };
        let png = encode_png(&fitted)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(png))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A size with the source's aspect ratio and about `area` pixels, snapped to
/// `step` and kept between 256 and 4096 px a side.
pub fn fit_size(src_w: u32, src_h: u32, area: u64, step: u32) -> (u32, u32) {
    let (w, h) = (f64::from(src_w.max(1)), f64::from(src_h.max(1)));
    let scale = (area as f64 / (w * h)).sqrt();
    let snap = |v: f64| {
        let v = (v / f64::from(step)).round() as u32 * step;
        v.clamp(256, 4096)
    };
    let (mut fw, mut fh) = (snap(w * scale), snap(h * scale));
    // Rounding up on both sides can overshoot the area; step one side down.
    while u64::from(fw) * u64::from(fh) > area && fw.max(fh) > 256 {
        if fw >= fh {
            fw -= step;
        } else {
            fh -= step;
        }
    }
    (fw, fh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_and_data_are_told_apart() {
        assert!(!is_inline("ComfyUI_00001_.png"));
        assert!(is_inline("data:image/png;base64,AAAA"));
        assert!(is_inline(&"A".repeat(400)));
        assert!(!is_inline("short"));
    }

    #[test]
    fn data_urls_must_be_base64_images() {
        assert!(decode_inline("data:text/plain;base64,AAAA").is_err());
        assert!(decode_inline("data:image/png,raw").is_err());
        assert_eq!(
            decode_inline("data:image/png;base64,AAEC").unwrap(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn fitted_sizes_keep_the_aspect_and_stay_under_the_area() {
        let (w, h) = fit_size(2000, 3000, 832 * 1216, 64);
        assert_eq!((w % 64, h % 64), (0, 0));
        assert!(u64::from(w) * u64::from(h) <= 832 * 1216);
        assert!((f64::from(w) / f64::from(h) - 2.0 / 3.0).abs() < 0.08);
        assert_eq!(fit_size(1024, 1024, 1024 * 1024, 16), (1024, 1024));
    }
}
