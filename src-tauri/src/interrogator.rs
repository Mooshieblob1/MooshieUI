use std::path::PathBuf;
use std::time::Instant;

use ort::session::Session;
use serde::Serialize;
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter};

use crate::config;
use crate::error::AppError;
#[cfg(feature = "desktop")]
use crate::setup::DownloadProgress;

/// ONNX Runtime version matching ort-sys 2.0.0-rc.12 pre-built binaries.
const ORT_VERSION: &str = "1.24.2";

#[cfg(target_os = "linux")]
const ORT_LIB_NAME: &str = "libonnxruntime.so";
#[cfg(target_os = "macos")]
const ORT_LIB_NAME: &str = "libonnxruntime.dylib";
#[cfg(target_os = "windows")]
const ORT_LIB_NAME: &str = "onnxruntime.dll";

const MODEL_FILENAME: &str = "model.onnx";
const TAGS_FILENAME: &str = "selected_tags.csv";

/// The tagger MooshieUI used before the model picker existed. Installs that
/// predate the picker keep their files under this id (see `migrate_legacy_layout`).
pub const DEFAULT_INTERROGATOR_MODEL: &str = "wd-eva02-large-tagger-v3";

/// A tagger the user can pick in Settings.
///
/// Every entry must be a WD v3-family ONNX tagger: identical padded-square
/// NHWC/BGR preprocessing, identical `selected_tags.csv` schema, and the same
/// 10861-class sigmoid output. A model that breaks any of those assumptions
/// needs more than a new row here.
#[derive(Debug, Clone, Serialize)]
pub struct InterrogatorModelInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub repo: &'static str,
    /// Size of `model.onnx`, shown in Settings before the user commits to a download.
    pub size_bytes: u64,
    pub input_size: u32,
}

/// The first entry is the default and the fallback for unknown ids.
pub const INTERROGATOR_MODELS: &[InterrogatorModelInfo] = &[
    InterrogatorModelInfo {
        id: "wd-eva02-large-tagger-v3",
        label: "WD EVA02 Large v3",
        repo: "SmilingWolf/wd-eva02-large-tagger-v3",
        size_bytes: 1_260_435_999,
        input_size: 448,
    },
    InterrogatorModelInfo {
        id: "wd-vit-large-tagger-v3",
        label: "WD ViT Large v3",
        repo: "SmilingWolf/wd-vit-large-tagger-v3",
        size_bytes: 1_260_645_673,
        input_size: 448,
    },
    InterrogatorModelInfo {
        id: "wd-swinv2-tagger-v3",
        label: "WD SwinV2 v3",
        repo: "SmilingWolf/wd-swinv2-tagger-v3",
        size_bytes: 467_460_978,
        input_size: 448,
    },
    InterrogatorModelInfo {
        id: "wd-convnext-tagger-v3",
        label: "WD ConvNeXt v3",
        repo: "SmilingWolf/wd-convnext-tagger-v3",
        size_bytes: 394_990_732,
        input_size: 448,
    },
    InterrogatorModelInfo {
        id: "wd-vit-tagger-v3",
        label: "WD ViT v3",
        repo: "SmilingWolf/wd-vit-tagger-v3",
        size_bytes: 378_536_310,
        input_size: 448,
    },
];

/// A registry entry plus whether its files are already on disk. Settings uses
/// the flag to label a model as downloaded and to offer the delete action.
#[derive(Debug, Clone, Serialize)]
pub struct InterrogatorModelStatus {
    #[serde(flatten)]
    pub info: &'static InterrogatorModelInfo,
    pub downloaded: bool,
}

/// Status of every selectable model, given the root that holds the per-model
/// subdirectories.
pub fn model_statuses_at(root_dir: &std::path::Path) -> Vec<InterrogatorModelStatus> {
    INTERROGATOR_MODELS
        .iter()
        .map(|info| InterrogatorModelStatus {
            info,
            downloaded: is_model_downloaded_at(&root_dir.join(info.id)),
        })
        .collect()
}

/// Look up a model by id, falling back to the default so a stale or hand-edited
/// config id can never leave the interrogator without a model to load.
pub fn find_model(id: &str) -> &'static InterrogatorModelInfo {
    INTERROGATOR_MODELS
        .iter()
        .find(|m| m.id == id)
        .unwrap_or(&INTERROGATOR_MODELS[0])
}

fn file_url(info: &InterrogatorModelInfo, filename: &str) -> String {
    format!(
        "https://huggingface.co/{}/resolve/main/{}",
        info.repo, filename
    )
}

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
    /// Holds the shared ONNX Runtime library plus one subdirectory per model.
    root_dir: PathBuf,
    model_id: String,
}

impl Default for InterrogatorState {
    fn default() -> Self {
        Self::new()
    }
}

impl InterrogatorState {
    pub fn new() -> Self {
        let root_dir = config::app_data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("interrogator");
        migrate_legacy_layout(&root_dir);
        Self {
            session: None,
            tag_list: Vec::new(),
            root_dir,
            model_id: DEFAULT_INTERROGATOR_MODEL.to_string(),
        }
    }

    /// The directory holding the shared ONNX Runtime library and the per-model
    /// subdirectories. Callers clone this so they can run downloads without
    /// holding the interrogator lock across I/O.
    pub fn root_dir(&self) -> PathBuf {
        self.root_dir.clone()
    }

    /// The directory holding the selected model's ONNX and tag files.
    pub fn model_dir(&self) -> PathBuf {
        self.root_dir.join(&self.model_id)
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Select the active tagger. Switching drops the cached session and tag list
    /// so the next `load_session` picks up the new model. Unknown ids resolve to
    /// the default rather than leaving the interrogator pointed at nothing.
    pub fn set_model(&mut self, id: &str) {
        let resolved = find_model(id).id;
        if resolved != self.model_id {
            self.session = None;
            self.tag_list = Vec::new();
            self.model_id = resolved.to_string();
        }
    }

    /// Drop the cached session and tag list without changing the selection.
    /// Used after the active model's files are deleted, so the next run
    /// re-downloads instead of inferring against files that are gone.
    pub fn unload_session(&mut self) {
        self.session = None;
        self.tag_list = Vec::new();
    }

    fn model_path(&self) -> PathBuf {
        model_path_in(&self.model_dir())
    }

    fn tags_path(&self) -> PathBuf {
        tags_path_in(&self.model_dir())
    }

    pub fn is_model_downloaded(&self) -> bool {
        is_model_downloaded_at(&self.model_dir())
    }

    pub fn ort_library_path(&self) -> PathBuf {
        ort_library_path_in(&self.root_dir)
    }

    pub fn is_ort_library_present(&self) -> bool {
        is_ort_library_present_at(&self.root_dir)
    }

    pub fn session_not_loaded(&self) -> bool {
        self.session.is_none()
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
            .commit_from_file(self.model_path())
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
        let input_size = find_model(&self.model_id).input_size;

        let session = self
            .session
            .as_mut()
            .ok_or_else(|| AppError::InterrogatorError("Model not loaded".into()))?;

        let t = Instant::now();
        // Pre-downscale large images with fast Nearest filter before quality resize
        let img = if img.width() > input_size * 3 || img.height() > input_size * 3 {
            let pre_size = input_size * 2;
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
            input_size,
            input_size,
            image::imageops::FilterType::CatmullRom,
        );
        eprintln!("[interrogator] Image resized in {:.1?}", t.elapsed());

        // Build input tensor: [1, H, W, 3] float32 (NHWC, BGR, 0-255 range)
        // WD tagger expects raw pixel values, NOT normalized to [0,1]
        let pixels = (input_size * input_size) as usize;
        let mut input_data = vec![0.0f32; pixels * 3];
        for y in 0..input_size {
            for x in 0..input_size {
                let pixel = resized.get_pixel(x, y);
                let idx = (y * input_size + x) as usize;
                // NHWC: [y * W + x, channel] — BGR order
                input_data[idx * 3] = pixel[2] as f32; // B
                input_data[idx * 3 + 1] = pixel[1] as f32; // G
                input_data[idx * 3 + 2] = pixel[0] as f32; // R
            }
        }

        let input_shape = vec![1_i64, input_size as i64, input_size as i64, 3];
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

// ---------------------------------------------------------------------------
// Path-based download/status helpers.
//
// These operate on a plain `model_dir` path so callers can clone the directory
// out from under the interrogator's RwLock and run the (multi-second, network)
// downloads WITHOUT holding the guard across the await — per the project rule
// that guards must be dropped before awaiting I/O.
// ---------------------------------------------------------------------------

fn model_path_in(model_dir: &std::path::Path) -> PathBuf {
    model_dir.join(MODEL_FILENAME)
}

fn tags_path_in(model_dir: &std::path::Path) -> PathBuf {
    model_dir.join(TAGS_FILENAME)
}

fn ort_library_path_in(model_dir: &std::path::Path) -> PathBuf {
    model_dir.join(ORT_LIB_NAME)
}

pub fn is_model_downloaded_at(model_dir: &std::path::Path) -> bool {
    model_path_in(model_dir).exists() && tags_path_in(model_dir).exists()
}

pub fn is_ort_library_present_at(root_dir: &std::path::Path) -> bool {
    ort_library_path_in(root_dir).exists()
}

/// Move a pre-picker install (`interrogator/model.onnx`) into the default
/// model's subdirectory so existing users are not made to re-download 1.2 GB.
///
/// Best-effort and idempotent: any failure leaves the legacy files where they
/// are and the model simply downloads into the new layout.
pub fn migrate_legacy_layout(root_dir: &std::path::Path) {
    let legacy_model = root_dir.join(MODEL_FILENAME);
    if !legacy_model.exists() {
        return;
    }
    let dest_dir = root_dir.join(DEFAULT_INTERROGATOR_MODEL);
    if dest_dir.join(MODEL_FILENAME).exists() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        eprintln!("[interrogator] Legacy migration skipped (mkdir failed): {e}");
        return;
    }
    if let Err(e) = std::fs::rename(&legacy_model, dest_dir.join(MODEL_FILENAME)) {
        eprintln!("[interrogator] Legacy migration skipped (rename failed): {e}");
        return;
    }
    let legacy_tags = root_dir.join(TAGS_FILENAME);
    if legacy_tags.exists() {
        std::fs::rename(&legacy_tags, dest_dir.join(TAGS_FILENAME)).ok();
    }
    eprintln!(
        "[interrogator] Migrated legacy tagger files into {}",
        dest_dir.display()
    );
}

/// Delete a downloaded model's files, freeing the 0.4-1.2 GB it occupies.
/// The shared ONNX Runtime library lives at the root and is left alone.
pub fn delete_model_files_at(root_dir: &std::path::Path, model_id: &str) -> Result<(), AppError> {
    let dir = root_dir.join(find_model(model_id).id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// Download model files from HuggingFace if not already present.
#[cfg(feature = "desktop")]
pub async fn ensure_model_downloaded_at(
    app: &AppHandle,
    client: &reqwest::Client,
    model_dir: &std::path::Path,
    model_id: &str,
) -> Result<(), AppError> {
    let info = find_model(model_id);
    std::fs::create_dir_all(model_dir)?;
    if !model_path_in(model_dir).exists() {
        let url = file_url(info, MODEL_FILENAME);
        download_with_progress(app, client, &url, &model_path_in(model_dir), info.label).await?;
    }
    if !tags_path_in(model_dir).exists() {
        let url = file_url(info, TAGS_FILENAME);
        download_with_progress(app, client, &url, &tags_path_in(model_dir), TAGS_FILENAME).await?;
    }
    Ok(())
}

/// Download the ONNX Runtime shared library if not already present.
#[cfg(feature = "desktop")]
pub async fn ensure_ort_library_at(
    app: &AppHandle,
    client: &reqwest::Client,
    root_dir: &std::path::Path,
) -> Result<(), AppError> {
    if is_ort_library_present_at(root_dir) {
        return Ok(());
    }
    std::fs::create_dir_all(root_dir)?;
    let (url, archive_name) = ort_download_info();
    let archive_path = root_dir.join(archive_name);
    download_with_progress(app, client, &url, &archive_path, "ONNX Runtime").await?;
    extract_ort_library(&archive_path, &ort_library_path_in(root_dir))?;
    std::fs::remove_file(&archive_path).ok();
    Ok(())
}

/// Download model files without AppHandle (for browser mode).
pub async fn ensure_model_downloaded_headless_at(
    client: &reqwest::Client,
    model_dir: &std::path::Path,
    model_id: &str,
) -> Result<(), AppError> {
    let info = find_model(model_id);
    std::fs::create_dir_all(model_dir)?;
    if !model_path_in(model_dir).exists() {
        let url = file_url(info, MODEL_FILENAME);
        download_simple(client, &url, &model_path_in(model_dir)).await?;
    }
    if !tags_path_in(model_dir).exists() {
        let url = file_url(info, TAGS_FILENAME);
        download_simple(client, &url, &tags_path_in(model_dir)).await?;
    }
    Ok(())
}

/// Download ONNX Runtime without AppHandle (for browser mode).
pub async fn ensure_ort_library_headless_at(
    client: &reqwest::Client,
    root_dir: &std::path::Path,
) -> Result<(), AppError> {
    if is_ort_library_present_at(root_dir) {
        return Ok(());
    }
    std::fs::create_dir_all(root_dir)?;
    let (url, archive_name) = ort_download_info();
    let archive_path = root_dir.join(archive_name);
    download_simple(client, &url, &archive_path).await?;
    extract_ort_library(&archive_path, &ort_library_path_in(root_dir))?;
    std::fs::remove_file(&archive_path).ok();
    Ok(())
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
#[cfg(feature = "desktop")]
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

/// Simple download without progress events (for browser mode headless usage).
async fn download_simple(
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
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
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::InterrogatorError(format!("Download read error: {}", e)))?;
    std::fs::write(dest, &bytes)?;
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
    #[cfg(target_os = "macos")]
    {
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        (
            format!(
                "https://github.com/microsoft/onnxruntime/releases/download/v{}/onnxruntime-osx-{}-{}.tgz",
                ORT_VERSION, arch, ORT_VERSION
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

    #[cfg(target_os = "macos")]
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

            // Look for the versioned .dylib file (e.g. libonnxruntime.1.24.2.dylib)
            if let Some(name) = path.file_name() {
                let name = name.to_string_lossy();
                if name.starts_with("libonnxruntime.")
                    && name.ends_with(".dylib")
                    && name != "libonnxruntime.dylib"
                {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mooshieui-interrogator-test-{}-{}",
            std::process::id(),
            name
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_model_is_first_registry_entry() {
        assert_eq!(INTERROGATOR_MODELS[0].id, DEFAULT_INTERROGATOR_MODEL);
    }

    #[test]
    fn registry_ids_are_unique_and_well_formed() {
        let mut ids: Vec<&str> = INTERROGATOR_MODELS.iter().map(|m| m.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate interrogator model id");

        for m in INTERROGATOR_MODELS {
            assert!(!m.label.is_empty(), "{} has no label", m.id);
            assert!(m.repo.contains('/'), "{} repo is not owner/name", m.id);
            assert!(m.size_bytes > 0, "{} has no size", m.id);
            assert!(m.input_size > 0, "{} has no input size", m.id);
        }
    }

    #[test]
    fn find_model_resolves_known_ids() {
        for m in INTERROGATOR_MODELS {
            assert_eq!(find_model(m.id).id, m.id);
        }
    }

    #[test]
    fn find_model_falls_back_to_default_for_unknown_id() {
        assert_eq!(find_model("").id, DEFAULT_INTERROGATOR_MODEL);
        assert_eq!(
            find_model("not-a-real-tagger").id,
            DEFAULT_INTERROGATOR_MODEL
        );
    }

    #[test]
    fn file_url_points_at_the_repo_resolve_path() {
        let info = find_model("wd-vit-tagger-v3");
        assert_eq!(
            file_url(info, MODEL_FILENAME),
            "https://huggingface.co/SmilingWolf/wd-vit-tagger-v3/resolve/main/model.onnx"
        );
    }

    #[test]
    fn migration_moves_legacy_files_into_the_default_model_dir() {
        let root = scratch_dir("migrate");
        std::fs::write(root.join(MODEL_FILENAME), b"onnx").unwrap();
        std::fs::write(root.join(TAGS_FILENAME), b"tags").unwrap();

        migrate_legacy_layout(&root);

        let dest = root.join(DEFAULT_INTERROGATOR_MODEL);
        assert!(is_model_downloaded_at(&dest));
        assert!(!root.join(MODEL_FILENAME).exists());
        assert!(!root.join(TAGS_FILENAME).exists());
        assert_eq!(std::fs::read(dest.join(MODEL_FILENAME)).unwrap(), b"onnx");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn migration_is_a_noop_without_legacy_files() {
        let root = scratch_dir("migrate-noop");
        migrate_legacy_layout(&root);
        assert!(!root.join(DEFAULT_INTERROGATOR_MODEL).exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn migration_does_not_clobber_an_already_migrated_model() {
        let root = scratch_dir("migrate-existing");
        let dest = root.join(DEFAULT_INTERROGATOR_MODEL);
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join(MODEL_FILENAME), b"new").unwrap();
        std::fs::write(root.join(MODEL_FILENAME), b"legacy").unwrap();

        migrate_legacy_layout(&root);

        assert_eq!(std::fs::read(dest.join(MODEL_FILENAME)).unwrap(), b"new");
        assert!(root.join(MODEL_FILENAME).exists());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ort_library_lives_at_the_root_not_inside_a_model_dir() {
        let root = scratch_dir("ort-root");
        std::fs::write(ort_library_path_in(&root), b"lib").unwrap();
        assert!(is_ort_library_present_at(&root));
        assert!(!is_ort_library_present_at(
            &root.join(DEFAULT_INTERROGATOR_MODEL)
        ));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_removes_only_the_named_model() {
        let root = scratch_dir("delete");
        for id in ["wd-vit-tagger-v3", "wd-swinv2-tagger-v3"] {
            let dir = root.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(MODEL_FILENAME), b"onnx").unwrap();
            std::fs::write(dir.join(TAGS_FILENAME), b"tags").unwrap();
        }
        std::fs::write(ort_library_path_in(&root), b"lib").unwrap();

        delete_model_files_at(&root, "wd-vit-tagger-v3").unwrap();

        assert!(!root.join("wd-vit-tagger-v3").exists());
        assert!(is_model_downloaded_at(&root.join("wd-swinv2-tagger-v3")));
        assert!(is_ort_library_present_at(&root));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn deleting_a_model_that_was_never_downloaded_succeeds() {
        let root = scratch_dir("delete-missing");
        assert!(delete_model_files_at(&root, "wd-convnext-tagger-v3").is_ok());
        std::fs::remove_dir_all(&root).ok();
    }
}
