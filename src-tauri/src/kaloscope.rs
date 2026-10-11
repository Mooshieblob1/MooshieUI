//! Kaloscope 3.0 artist detection for the interrogator.
//!
//! Kaloscope 3.0 is a DINOv3 ViT-B/16 classifier that scores an illustration
//! against 44,129 Danbooru artists. It runs on the interrogator's ONNX Runtime
//! next to the WD tagger and fills `InterrogationResult::artist_tags`, which WD
//! v3 taggers leave empty because their tag lists have no artist category.
//!
//! The ONNX file is the browser export in `Mooshie/kaloscope-web`: 8-bit
//! weight-only quantised (`MatMulNBits`), 169 MB, with the classifier's input
//! normalisation built into the graph. It takes `pixel_values` [N, 3, 512, 512]
//! and returns `logits` [N, 44129] plus a 256-d `style` embedding we ignore.

use std::path::{Path, PathBuf};
use std::time::Instant;

use ort::session::Session;
#[cfg(feature = "desktop")]
use tauri::AppHandle;

use crate::error::AppError;
use crate::interrogator::TagResult;

/// Id used for the model's directory and for the Settings downloaded list.
pub const MODEL_ID: &str = "kaloscope-3";
pub const MODEL_LABEL: &str = "Kaloscope 3.0 (artists)";
pub const REPO: &str = "Mooshie/kaloscope-web";
pub const MODEL_FILENAME: &str = "kaloscope3_v1_artist_web.onnx";
pub const LABELS_FILENAME: &str = "labels_v3.json";
/// The DINOv3 License has to travel with the weights, so it is downloaded too.
pub const LICENSE_FILENAME: &str = "LICENSE.md";
/// Size of the ONNX file, shown in Settings before the first download.
pub const MODEL_SIZE_BYTES: u64 = 169_079_820;
pub const INPUT_SIZE: u32 = 512;
pub const NUM_CLASSES: usize = 44_129;
/// Most artist tags one run adds. The model picks a single artist per image,
/// so anything past the top few is noise even when it clears the threshold.
pub const MAX_ARTISTS: usize = 3;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

pub fn model_dir(root_dir: &Path) -> PathBuf {
    root_dir.join(MODEL_ID)
}

pub fn is_downloaded_at(root_dir: &Path) -> bool {
    let dir = model_dir(root_dir);
    dir.join(MODEL_FILENAME).exists() && dir.join(LABELS_FILENAME).exists()
}

fn file_url(filename: &str) -> String {
    format!("https://huggingface.co/{}/resolve/main/{}", REPO, filename)
}

/// Download the model, labels and licence if any are missing.
#[cfg(feature = "desktop")]
pub async fn ensure_downloaded_at(
    app: &AppHandle,
    client: &reqwest::Client,
    root_dir: &Path,
) -> Result<(), AppError> {
    let dir = model_dir(root_dir);
    std::fs::create_dir_all(&dir)?;
    for (filename, label) in [
        (MODEL_FILENAME, MODEL_LABEL),
        (LABELS_FILENAME, LABELS_FILENAME),
        (LICENSE_FILENAME, LICENSE_FILENAME),
    ] {
        let dest = dir.join(filename);
        if !dest.exists() {
            crate::interrogator::download_with_progress(
                app,
                client,
                &file_url(filename),
                &dest,
                label,
            )
            .await?;
        }
    }
    Ok(())
}

/// Download the model files without AppHandle (for browser mode).
pub async fn ensure_downloaded_headless_at(
    client: &reqwest::Client,
    root_dir: &Path,
) -> Result<(), AppError> {
    let dir = model_dir(root_dir);
    std::fs::create_dir_all(&dir)?;
    for filename in [MODEL_FILENAME, LABELS_FILENAME, LICENSE_FILENAME] {
        let dest = dir.join(filename);
        if !dest.exists() {
            crate::interrogator::download_simple(client, &file_url(filename), &dest).await?;
        }
    }
    Ok(())
}

/// Remove the downloaded model directory. A missing directory is not an error.
pub fn delete_files_at(root_dir: &Path) -> Result<(), AppError> {
    let dir = model_dir(root_dir);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// Cached Kaloscope session and artist names. Lives inside `InterrogatorState`
/// so it shares the interrogator's lock and ONNX Runtime library.
#[derive(Default)]
pub struct ArtistDetector {
    session: Option<Session>,
    labels: Vec<String>,
}

impl ArtistDetector {
    pub fn is_loaded(&self) -> bool {
        self.session.is_some()
    }

    pub fn unload(&mut self) {
        self.session = None;
        self.labels = Vec::new();
    }

    /// Load the session and labels from `root_dir/kaloscope-3`. The caller must
    /// have initialised ONNX Runtime first (`InterrogatorState::init_ort`).
    pub fn load(&mut self, root_dir: &Path) -> Result<(), AppError> {
        if self.session.is_some() {
            return Ok(());
        }
        let dir = model_dir(root_dir);
        let t = Instant::now();

        let labels_json = std::fs::read_to_string(dir.join(LABELS_FILENAME)).map_err(|e| {
            AppError::InterrogatorError(format!("Failed to read Kaloscope labels: {}", e))
        })?;
        let labels = parse_labels(&labels_json)?;

        let thread_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let session = Session::builder()
            .map_err(|e| {
                AppError::InterrogatorError(format!("Failed to create session builder: {}", e))
            })?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)
            .map_err(|e| {
                AppError::InterrogatorError(format!("Failed to set optimization level: {}", e))
            })?
            .with_intra_threads(thread_count)
            .map_err(|e| AppError::InterrogatorError(format!("Failed to set thread count: {}", e)))?
            .commit_from_file(dir.join(MODEL_FILENAME))
            .map_err(|e| {
                AppError::InterrogatorError(format!("Failed to load Kaloscope model: {}", e))
            })?;

        eprintln!("[kaloscope] Model loaded in {:.1?}", t.elapsed());
        self.labels = labels;
        self.session = Some(session);
        Ok(())
    }

    /// Score `img` and return up to `MAX_ARTISTS` artists whose softmax score
    /// is at least `threshold`, highest first.
    pub fn detect(
        &mut self,
        img: &image::DynamicImage,
        threshold: f32,
    ) -> Result<Vec<TagResult>, AppError> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| AppError::InterrogatorError("Kaloscope model not loaded".into()))?;

        let t = Instant::now();
        let input_data = preprocess(img);
        let shape = vec![1_i64, 3, INPUT_SIZE as i64, INPUT_SIZE as i64];
        let input_tensor = ort::value::Tensor::from_array((shape, input_data)).map_err(|e| {
            AppError::InterrogatorError(format!("Failed to create Kaloscope input: {}", e))
        })?;

        let outputs = session
            .run(ort::inputs!["pixel_values" => input_tensor])
            .map_err(|e| {
                AppError::InterrogatorError(format!("Kaloscope inference failed: {}", e))
            })?;
        let logits = outputs
            .get("logits")
            .ok_or_else(|| AppError::InterrogatorError("Kaloscope output 'logits' missing".into()))?
            .downcast_ref::<ort::value::DynTensorValueType>()
            .map_err(|e| AppError::InterrogatorError(format!("Output is not a tensor: {}", e)))?
            .try_extract_tensor::<f32>()
            .map_err(|e| AppError::InterrogatorError(format!("Failed to extract logits: {}", e)))?
            .1
            .to_vec();
        eprintln!("[kaloscope] Inference completed in {:.1?}", t.elapsed());

        if logits.len() != self.labels.len() {
            return Err(AppError::InterrogatorError(format!(
                "Kaloscope output has {} classes but {} has {} artists; the files do not match.",
                logits.len(),
                LABELS_FILENAME,
                self.labels.len()
            )));
        }
        Ok(top_artists(
            &softmax(&logits),
            &self.labels,
            threshold,
            MAX_ARTISTS,
        ))
    }
}

/// Parse `labels_v3.json`: a JSON array of artist names indexed by class id.
fn parse_labels(json: &str) -> Result<Vec<String>, AppError> {
    let labels: Vec<String> = serde_json::from_str(json)
        .map_err(|e| AppError::InterrogatorError(format!("Invalid Kaloscope labels: {}", e)))?;
    if labels.len() != NUM_CLASSES {
        return Err(AppError::InterrogatorError(format!(
            "{} lists {} artists; expected {}.",
            LABELS_FILENAME,
            labels.len(),
            NUM_CLASSES
        )));
    }
    Ok(labels)
}

/// Size of the image after torchvision's `Resize(size)`: the short side becomes
/// `size` and the long side scales with it, truncated.
fn resize_dims(width: u32, height: u32, size: u32) -> (u32, u32) {
    if width <= height {
        let h = (size as u64 * height as u64 / width.max(1) as u64) as u32;
        (size, h.max(size))
    } else {
        let w = (size as u64 * width as u64 / height.max(1) as u64) as u32;
        (w.max(size), size)
    }
}

/// Model-card preprocessing: composite transparency onto white, resize the
/// short side to 512, centre-crop 512x512 (no stretching), ImageNet mean/std,
/// planar RGB (NCHW).
fn preprocess(img: &image::DynamicImage) -> Vec<f32> {
    let rgb = if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        let mut out = image::RgbImage::new(rgba.width(), rgba.height());
        for (dst, src) in out.pixels_mut().zip(rgba.pixels()) {
            let a = src[3] as f32 / 255.0;
            for c in 0..3 {
                dst[c] = (src[c] as f32 * a + 255.0 * (1.0 - a)).round() as u8;
            }
        }
        out
    } else {
        img.to_rgb8()
    };

    let (w, h) = resize_dims(rgb.width(), rgb.height(), INPUT_SIZE);
    let resized = image::imageops::resize(&rgb, w, h, image::imageops::FilterType::Triangle);
    // torchvision rounds half to even here; flooring is at most one pixel off.
    let left = (w - INPUT_SIZE) / 2;
    let top = (h - INPUT_SIZE) / 2;

    let plane = (INPUT_SIZE * INPUT_SIZE) as usize;
    let mut data = vec![0.0f32; plane * 3];
    for y in 0..INPUT_SIZE {
        for x in 0..INPUT_SIZE {
            let px = resized.get_pixel(left + x, top + y);
            let i = (y * INPUT_SIZE + x) as usize;
            for c in 0..3 {
                data[c * plane + i] = (px[c] as f32 / 255.0 - MEAN[c]) / STD[c];
            }
        }
    }
    data
}

fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = logits.iter().map(|&l| (l - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    exps.into_iter().map(|e| e / sum).collect()
}

/// The `max` highest-scoring labels at or above `threshold`, highest first.
fn top_artists(probs: &[f32], labels: &[String], threshold: f32, max: usize) -> Vec<TagResult> {
    let mut hits: Vec<(usize, f32)> = probs
        .iter()
        .enumerate()
        .filter(|(_, &p)| p >= threshold)
        .map(|(i, &p)| (i, p))
        .collect();
    hits.sort_by(|a, b| b.1.total_cmp(&a.1));
    hits.truncate(max);
    hits.into_iter()
        .filter_map(|(i, p)| {
            labels.get(i).map(|name| TagResult {
                name: name.clone(),
                confidence: p,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_dims_scales_the_short_side_to_512() {
        assert_eq!(resize_dims(1000, 2000, 512), (512, 1024));
        assert_eq!(resize_dims(2000, 1000, 512), (1024, 512));
        assert_eq!(resize_dims(800, 800, 512), (512, 512));
        // Upscales small images too, matching torchvision.
        assert_eq!(resize_dims(256, 300, 512), (512, 600));
        // Truncates like torchvision's int().
        assert_eq!(resize_dims(700, 1001, 512), (512, 732));
    }

    #[test]
    fn preprocess_outputs_a_normalised_planar_512_square() {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            640,
            900,
            image::Rgb([255, 0, 128]),
        ));
        let data = preprocess(&img);
        let plane = (INPUT_SIZE * INPUT_SIZE) as usize;
        assert_eq!(data.len(), plane * 3);
        let expect = |v: f32, c: usize| (v / 255.0 - MEAN[c]) / STD[c];
        assert!((data[0] - expect(255.0, 0)).abs() < 1e-5);
        assert!((data[plane] - expect(0.0, 1)).abs() < 1e-5);
        assert!((data[2 * plane + plane - 1] - expect(128.0, 2)).abs() < 1e-5);
    }

    #[test]
    fn preprocess_composites_transparency_onto_white() {
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            512,
            512,
            image::Rgba([0, 0, 0, 0]),
        ));
        let data = preprocess(&img);
        let white_r = (1.0 - MEAN[0]) / STD[0];
        assert!(data.iter().take(10).all(|&v| (v - white_r).abs() < 1e-5));
    }

    #[test]
    fn preprocess_centre_crops_the_long_side() {
        // Left third black, middle white, right third black: after a centre
        // crop of a 3:1 image only the white middle should remain.
        let mut rgb = image::RgbImage::from_pixel(1536, 512, image::Rgb([0, 0, 0]));
        for y in 0..512 {
            for x in 512..1024 {
                rgb.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
        let data = preprocess(&image::DynamicImage::ImageRgb8(rgb));
        let white_r = (1.0 - MEAN[0]) / STD[0];
        let row = 256 * INPUT_SIZE as usize;
        assert!((data[row + 2] - white_r).abs() < 1e-3);
        assert!((data[row + 509] - white_r).abs() < 1e-3);
    }

    #[test]
    fn softmax_sums_to_one_and_keeps_order() {
        let p = softmax(&[1.0, 3.0, 2.0]);
        assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(p[1] > p[2] && p[2] > p[0]);
        // Large logits must not overflow.
        let p = softmax(&[1000.0, 0.0]);
        assert!(p[0] > 0.999 && p[0].is_finite());
    }

    #[test]
    fn top_artists_filters_sorts_and_caps() {
        let labels: Vec<String> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let probs = [0.05, 0.4, 0.2, 0.15, 0.2];
        let hits = top_artists(&probs, &labels, 0.1, 3);
        let names: Vec<&str> = hits.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 3);
        assert_eq!(names[0], "b");
        assert!(names.contains(&"c") && names.contains(&"e"));
        assert!(top_artists(&probs, &labels, 0.5, 3).is_empty());
    }

    #[test]
    fn parse_labels_requires_the_full_class_list() {
        assert!(parse_labels("[\"a\",\"b\"]").is_err());
        assert!(parse_labels("not json").is_err());
        let full = serde_json::to_string(&vec!["x"; NUM_CLASSES]).unwrap();
        assert_eq!(parse_labels(&full).unwrap().len(), NUM_CLASSES);
    }

    #[test]
    fn delete_removes_only_the_kaloscope_dir() {
        let root =
            std::env::temp_dir().join(format!("mooshieui-kaloscope-test-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(model_dir(&root)).unwrap();
        std::fs::write(model_dir(&root).join(MODEL_FILENAME), b"x").unwrap();
        std::fs::write(model_dir(&root).join(LABELS_FILENAME), b"[]").unwrap();
        std::fs::write(root.join("keep.txt"), b"x").unwrap();
        assert!(is_downloaded_at(&root));
        delete_files_at(&root).unwrap();
        assert!(!is_downloaded_at(&root));
        assert!(root.join("keep.txt").exists());
        // Deleting again is fine.
        delete_files_at(&root).unwrap();
        std::fs::remove_dir_all(&root).ok();
    }
}
