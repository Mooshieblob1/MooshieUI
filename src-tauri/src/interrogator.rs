use std::path::PathBuf;
use std::time::Instant;

use ort::session::Session;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::config;
use crate::error::AppError;
use crate::setup::DownloadProgress;

/// ONNX Runtime version matching ort-sys 2.0.0-rc.12 pre-built binaries.
const ORT_VERSION: &str = "1.24.2";

#[cfg(target_os = "linux")]
const ORT_LIB_NAME: &str = "libonnxruntime.so";
#[cfg(target_os = "windows")]
const ORT_LIB_NAME: &str = "onnxruntime.dll";

const HF_BASE_URL: &str =
    "https://huggingface.co/SmilingWolf/wd-eva02-large-tagger-v3/resolve/main";
const MODEL_FILENAME: &str = "model.onnx";
const TAGS_FILENAME: &str = "selected_tags.csv";
const MODEL_INPUT_SIZE: u32 = 448;

#[derive(Debug, Clone, Serialize)]
pub struct TagResult {
    pub name: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterrogationResult {
    pub character_tags: Vec<TagResult>,
    pub artist_tags: Vec<TagResult>,
    pub general_tags: Vec<TagResult>,
    pub copyright_tags: Vec<TagResult>,
    pub rating_tags: Vec<TagResult>,
}

#[derive(Debug, Clone)]
pub struct TagDef {
    pub name: String,
    pub category: u8,
}

pub struct InterrogatorState {
    session: Option<Session>,
    tag_list: Vec<TagDef>,
    model_dir: PathBuf,
}

impl InterrogatorState {
    pub fn new() -> Self {
        let model_dir = config::app_data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("interrogator");
        Self {
            session: None,
            tag_list: Vec::new(),
            model_dir,
        }
    }

    fn model_path(&self) -> PathBuf {
        self.model_dir.join(MODEL_FILENAME)
    }

    fn tags_path(&self) -> PathBuf {
        self.model_dir.join(TAGS_FILENAME)
    }

    pub fn is_model_downloaded(&self) -> bool {
        self.model_path().exists() && self.tags_path().exists()
    }

    pub fn ort_library_path(&self) -> PathBuf {
        self.model_dir.join(ORT_LIB_NAME)
    }

    pub fn is_ort_library_present(&self) -> bool {
        self.ort_library_path().exists()
    }

    pub fn session_not_loaded(&self) -> bool {
        self.session.is_none()
    }

    /// Download model files from HuggingFace if not already present.
    pub async fn ensure_model_downloaded(
        &self,
        app: &AppHandle,
        client: &reqwest::Client,
    ) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.model_dir)?;

        if !self.model_path().exists() {
            let url = format!("{}/{}", HF_BASE_URL, MODEL_FILENAME);
            download_with_progress(app, client, &url, &self.model_path(), MODEL_FILENAME).await?;
        }
        if !self.tags_path().exists() {
            let url = format!("{}/{}", HF_BASE_URL, TAGS_FILENAME);
            download_with_progress(app, client, &url, &self.tags_path(), TAGS_FILENAME).await?;
        }
        Ok(())
    }

    /// Download the ONNX Runtime shared library if not already present.
    pub async fn ensure_ort_library(
        &self,
        app: &AppHandle,
        client: &reqwest::Client,
    ) -> Result<(), AppError> {
        if self.is_ort_library_present() {
            return Ok(());
        }

        std::fs::create_dir_all(&self.model_dir)?;

        let (url, archive_name) = ort_download_info();
        let archive_path = self.model_dir.join(archive_name);

        download_with_progress(app, client, &url, &archive_path, "ONNX Runtime").await?;
        extract_ort_library(&archive_path, &self.ort_library_path())?;
        std::fs::remove_file(&archive_path).ok();

        Ok(())
    }

    /// Load the ONNX session and tag list, caching for subsequent calls.
    /// Uses Level1 optimization only (constant folding) — fast even for large models.
    pub fn load_session(&mut self) -> Result<(), AppError> {
        if self.session.is_some() {
            return Ok(());
        }

        let t = Instant::now();

        // Initialize ONNX Runtime from downloaded shared library
        let lib_path = self.ort_library_path();
        let builder = ort::init_from(&lib_path).map_err(|e| {
            AppError::InterrogatorError(format!(
                "Failed to load ONNX Runtime library at '{}': {}",
                lib_path.display(),
                e
            ))
        })?;
        builder.commit();

        // Parse tag CSV
        self.tag_list = parse_tags_csv(&self.tags_path())?;

        // Load ONNX model — Level1 is fast (constant folding only).
        // Level3 does expensive transformer fusions that can take 10+ min on large models
        // with negligible inference speedup since intra_threads already parallelizes matmuls.
        let thread_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        eprintln!(
            "[interrogator] Loading model ({:.0} MB, {} threads)...",
            std::fs::metadata(self.model_path())
                .map(|m| m.len() as f64 / 1_048_576.0)
                .unwrap_or(0.0),
            thread_count
        );

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
            .commit_from_file(&self.model_path())
            .map_err(|e| {
                AppError::InterrogatorError(format!("Failed to load ONNX model: {}", e))
            })?;

        eprintln!("[interrogator] Model loaded in {:.1?}", t.elapsed());
        self.session = Some(session);
        Ok(())
    }

    /// Run inference on raw bytes (decodes first, then delegates).
    pub fn run_inference(
        &mut self,
        image_bytes: &[u8],
        general_threshold: f32,
        character_threshold: f32,
    ) -> Result<InterrogationResult, AppError> {
        let t = Instant::now();
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| AppError::InterrogatorError(format!("Failed to decode image: {}", e)))?;
        eprintln!("[interrogator] Image decoded in {:.1?}", t.elapsed());
        self.run_inference_from_image(img, general_threshold, character_threshold)
    }

    /// Run inference on an already-decoded DynamicImage.
    pub fn run_inference_from_image(
        &mut self,
        img: image::DynamicImage,
        general_threshold: f32,
        character_threshold: f32,
    ) -> Result<InterrogationResult, AppError> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| AppError::InterrogatorError("Model not loaded".into()))?;

        let t = Instant::now();
        // Pre-downscale large images with fast Nearest filter before quality resize
        let img = if img.width() > MODEL_INPUT_SIZE * 3 || img.height() > MODEL_INPUT_SIZE * 3 {
            let pre_size = MODEL_INPUT_SIZE * 2;
            img.resize(pre_size, pre_size, image::imageops::FilterType::Nearest)
        } else {
            img
        };

        // WD tagger preprocessing: pad to square with white fill, then resize
        let rgb = img.to_rgb8();
        let (w, h) = (rgb.width(), rgb.height());
        let max_dim = w.max(h);
        let mut padded = image::RgbImage::from_pixel(max_dim, max_dim, image::Rgb([255, 255, 255]));
        let pad_left = (max_dim - w) / 2;
        let pad_top = (max_dim - h) / 2;
        image::imageops::overlay(&mut padded, &rgb, pad_left as i64, pad_top as i64);

        let resized = image::imageops::resize(
            &padded,
            MODEL_INPUT_SIZE,
            MODEL_INPUT_SIZE,
            image::imageops::FilterType::CatmullRom,
        );
        eprintln!("[interrogator] Image resized in {:.1?}", t.elapsed());

        // Build input tensor: [1, H, W, 3] float32 (NHWC, BGR, 0-255 range)
        // WD tagger expects raw pixel values, NOT normalized to [0,1]
        let pixels = (MODEL_INPUT_SIZE * MODEL_INPUT_SIZE) as usize;
        let mut input_data = vec![0.0f32; pixels * 3];
        for y in 0..MODEL_INPUT_SIZE {
            for x in 0..MODEL_INPUT_SIZE {
                let pixel = resized.get_pixel(x, y);
                let idx = (y * MODEL_INPUT_SIZE + x) as usize;
                // NHWC: [y * W + x, channel] — BGR order
                input_data[idx * 3] = pixel[2] as f32; // B
                input_data[idx * 3 + 1] = pixel[1] as f32; // G
                input_data[idx * 3 + 2] = pixel[0] as f32; // R
            }
        }

        let input_shape = vec![1_i64, MODEL_INPUT_SIZE as i64, MODEL_INPUT_SIZE as i64, 3];
        let input_tensor =
            ort::value::Tensor::from_array((input_shape, input_data)).map_err(|e| {
                AppError::InterrogatorError(format!("Failed to create input tensor: {}", e))
            })?;

        // Log model input/output info for debugging
        let input_names: Vec<String> = session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        let output_names: Vec<String> = session
            .outputs()
            .iter()
            .map(|o| o.name().to_string())
            .collect();
        eprintln!("[interrogator] Model inputs: {:?}", input_names);
        eprintln!("[interrogator] Model outputs: {:?}", output_names);

        // Run inference
        let t = Instant::now();
        let outputs = session
            .run(ort::inputs![input_tensor])
            .map_err(|e| AppError::InterrogatorError(format!("Inference failed: {}", e)))?;
        eprintln!(
            "[interrogator] ONNX inference completed in {:.1?}",
            t.elapsed()
        );

        // Extract output probabilities — WD tagger outputs sigmoid probabilities
        let output_name: String = if let Some(name) = output_names.first() {
            name.clone()
        } else {
            return Err(AppError::InterrogatorError("No output tensor found".into()));
        };

        let output = outputs
            .get(&output_name)
            .ok_or_else(|| AppError::InterrogatorError("No output tensor found".into()))?;

        let tensor_ref = output
            .downcast_ref::<ort::value::DynTensorValueType>()
            .map_err(|e| AppError::InterrogatorError(format!("Output is not a tensor: {}", e)))?;

        let (_, probs_slice) = tensor_ref
            .try_extract_tensor::<f32>()
            .map_err(|e| AppError::InterrogatorError(format!("Failed to extract output: {}", e)))?;

        let probs: Vec<f32> = probs_slice.to_vec();

        eprintln!(
            "[interrogator] Output '{}': {} probabilities, tag_list: {} tags",
            output_name,
            probs.len(),
            self.tag_list.len()
        );
        if let Some(max_prob) = probs.iter().cloned().reduce(f32::max) {
            let above_threshold = probs.iter().filter(|&&p| p >= general_threshold).count();
            eprintln!(
                "[interrogator] Max prob: {:.4}, above general threshold ({:.2}): {}",
                max_prob, general_threshold, above_threshold
            );
        }

        // Map probabilities to tags with category-specific thresholds
        let mut character_tags = Vec::new();
        let mut artist_tags = Vec::new();
        let mut general_tags = Vec::new();
        let mut copyright_tags = Vec::new();
        let mut rating_tags = Vec::new();

        for (i, &prob) in probs.iter().enumerate() {
            if i >= self.tag_list.len() {
                break;
            }
            let tag = &self.tag_list[i];
            let result = TagResult {
                name: tag.name.clone(),
                confidence: prob,
            };

            match tag.category {
                0 => {
                    // General tags
                    if prob >= general_threshold {
                        general_tags.push(result);
                    }
                }
                1 => {
                    // Artist tags
                    if prob >= 0.5 {
                        artist_tags.push(result);
                    }
                }
                3 => {
                    // Copyright tags
                    if prob >= 0.5 {
                        copyright_tags.push(result);
                    }
                }
                4 => {
                    // Character tags
                    if prob >= character_threshold {
                        character_tags.push(result);
                    }
                }
                9 => {
                    // Rating tags — always include all with their probs
                    rating_tags.push(result);
                }
                _ => {
                    // Other categories — treat as general
                    if prob >= general_threshold {
                        general_tags.push(result);
                    }
                }
            }
        }

        // Sort each category by confidence descending
        character_tags.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        artist_tags.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        general_tags.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        copyright_tags.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        rating_tags.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        Ok(InterrogationResult {
            character_tags,
            artist_tags,
            general_tags,
            copyright_tags,
            rating_tags,
        })
    }
}

/// Parse the selected_tags.csv file to extract tag names and categories.
fn parse_tags_csv(path: &std::path::Path) -> Result<Vec<TagDef>, AppError> {
    let mut reader = csv::Reader::from_path(path)
        .map_err(|e| AppError::InterrogatorError(format!("Failed to read tags CSV: {}", e)))?;

    let mut tags = Vec::new();
    for result in reader.records() {
        let record =
            result.map_err(|e| AppError::InterrogatorError(format!("CSV parse error: {}", e)))?;
        // CSV format: tag_id, name, category, count
        let name = record.get(1).unwrap_or("").to_string();
        let category: u8 = record.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        if !name.is_empty() {
            tags.push(TagDef { name, category });
        }
    }
    Ok(tags)
}

/// Download a file with progress events emitted to the frontend.
async fn download_with_progress(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
    label: &str,
) -> Result<(), AppError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::InterrogatorError(format!("Download failed: {}", e)))?;

    if !resp.status().is_success() {
        return Err(AppError::InterrogatorError(format!(
            "Download returned status {}",
            resp.status()
        )));
    }

    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file = std::fs::File::create(dest)?;

    app.emit(
        "interrogator:download_progress",
        DownloadProgress {
            filename: label.to_string(),
            downloaded: 0,
            total,
            done: false,
        },
    )
    .ok();

    let mut last_emit: u64 = 0;
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| AppError::InterrogatorError(format!("Download read error: {}", e)))?
    {
        use std::io::Write;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;

        if downloaded - last_emit > 256 * 1024 || downloaded == total {
            last_emit = downloaded;
            app.emit(
                "interrogator:download_progress",
                DownloadProgress {
                    filename: label.to_string(),
                    downloaded,
                    total,
                    done: false,
                },
            )
            .ok();
        }
    }

    app.emit(
        "interrogator:download_progress",
        DownloadProgress {
            filename: label.to_string(),
            downloaded,
            total,
            done: true,
        },
    )
    .ok();

    Ok(())
}

/// Returns (download_url, archive_filename) for the platform-specific ONNX Runtime.
fn ort_download_info() -> (String, &'static str) {
    #[cfg(target_os = "linux")]
    {
        (
            format!(
                "https://github.com/microsoft/onnxruntime/releases/download/v{}/onnxruntime-linux-x64-{}.tgz",
                ORT_VERSION, ORT_VERSION
            ),
            "ort_runtime.tgz",
        )
    }
    #[cfg(target_os = "windows")]
    {
        (
            format!(
                "https://github.com/microsoft/onnxruntime/releases/download/v{}/onnxruntime-win-x64-{}.zip",
                ORT_VERSION, ORT_VERSION
            ),
            "ort_runtime.zip",
        )
    }
}

/// Extract the ONNX Runtime shared library from a downloaded archive.
fn extract_ort_library(
    archive_path: &std::path::Path,
    dest: &std::path::Path,
) -> Result<(), AppError> {
    #[cfg(target_os = "linux")]
    {
        let file = std::fs::File::open(archive_path)?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);

        for entry in archive
            .entries()
            .map_err(|e| AppError::InterrogatorError(format!("Failed to read tar: {}", e)))?
        {
            let mut entry = entry.map_err(|e| {
                AppError::InterrogatorError(format!("Failed to read tar entry: {}", e))
            })?;
            let path = entry
                .path()
                .map_err(|e| AppError::InterrogatorError(format!("Invalid path: {}", e)))?;

            // Look for the versioned .so file (e.g., libonnxruntime.so.1.24.2)
            if let Some(name) = path.file_name() {
                let name = name.to_string_lossy();
                if name.starts_with("libonnxruntime.so.1.") {
                    entry.unpack(dest).map_err(|e| {
                        AppError::InterrogatorError(format!("Failed to extract library: {}", e))
                    })?;
                    return Ok(());
                }
            }
        }
        Err(AppError::InterrogatorError(
            "ONNX Runtime library not found in archive".into(),
        ))
    }

    #[cfg(target_os = "windows")]
    {
        let file = std::fs::File::open(archive_path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| AppError::InterrogatorError(format!("Failed to read zip: {}", e)))?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| {
                AppError::InterrogatorError(format!("Failed to read zip entry: {}", e))
            })?;
            if entry.name().ends_with("onnxruntime.dll") {
                let mut outfile = std::fs::File::create(dest)?;
                std::io::copy(&mut entry, &mut outfile)?;
                return Ok(());
            }
        }
        Err(AppError::InterrogatorError(
            "ONNX Runtime DLL not found in archive".into(),
        ))
    }
}
