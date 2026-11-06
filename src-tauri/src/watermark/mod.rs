//! Metadata that survives stripping.
//!
//! Text chunks and stealth alpha are lost the moment a site re-encodes an
//! image, it is resized, or someone takes a screenshot. Two invisible layers
//! point a stripped copy back at its gallery record, which still holds every
//! setting:
//!
//! - **Fingerprint** (always on): a perceptual hash of each saved image goes
//!   into the gallery index. Nothing in the file changes. It only finds images
//!   in this gallery.
//! - **Watermark** (opt in): a TrustMark payload, a MooshieUI tag plus a random
//!   ID stored on the gallery row, is woven into the pixels. It survives
//!   re-encoding, resizing and screenshots, and tells anyone who reads it that
//!   the image came from MooshieUI even when the record is not theirs.
//!
//! Reading tries the file's own metadata first; only a file with none goes
//! through [`recover`]. Everything here is best effort: a failure saves or
//! reads the image exactly as before.

pub mod fingerprint;
#[cfg(any(feature = "desktop", feature = "server"))]
mod trustmark;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use image::DynamicImage;

use crate::metadata::ImageFormat;

/// Key added to recovered metadata saying where it came from: `watermark`,
/// `fingerprint`, or `watermark_foreign` (a MooshieUI mark with no record in
/// this gallery, so no settings).
pub const RECOVERED_KEY: &str = "mooshie_recovered";

/// Leading payload bits that mark a MooshieUI watermark (0xB2, then version 1
/// of this layout). A decode without them is someone else's TrustMark.
const TAG_BITS: &str = "10110010";
/// ID bits after the tag: the rest of TrustMark's 61-bit BCH-5 payload.
const ID_BITS: usize = 53;

/// Images smaller than this on either side are not watermarked: the residual
/// is upscaled from 256 pixels, so on a smaller image it would show.
const MIN_SIDE: u32 = 256;

/// Where the TrustMark models are kept.
pub fn models_dir() -> Option<PathBuf> {
    crate::config::app_data_dir().map(|d| d.join("watermark"))
}

/// A fresh watermark ID: random, non-zero, `ID_BITS` wide.
pub fn new_id() -> u64 {
    loop {
        let id = rand::random::<u64>() & ((1u64 << ID_BITS) - 1);
        if id != 0 {
            return id;
        }
    }
}

/// The TrustMark data bits for `id`.
fn payload_for(id: u64) -> String {
    format!("{TAG_BITS}{id:0width$b}", width = ID_BITS)
}

/// The ID in decoded data bits, or `None` when they are not a MooshieUI mark.
fn id_from_payload(bits: &str) -> Option<u64> {
    let rest = bits.strip_prefix(TAG_BITS)?;
    if rest.len() != ID_BITS {
        return None;
    }
    u64::from_str_radix(rest, 2).ok().filter(|id| *id != 0)
}

/// Decode an image file's pixels: PNG, JPEG and WebP through `image`, JXL
/// through `jxl-oxide`. `None` for anything else.
pub fn decode_pixels(bytes: &[u8]) -> Option<DynamicImage> {
    if crate::metadata::detect_format(bytes) == ImageFormat::Jxl {
        let d = crate::jxl::decode_to_rgba8(bytes).ok()?;
        return image::RgbaImage::from_raw(d.width, d.height, d.rgba).map(DynamicImage::ImageRgba8);
    }
    image::load_from_memory(bytes).ok()
}

static OWNER_CHOICE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Remember the owner's watermark setting from their latest save, so saves the
/// app makes on its own (MCP generations) follow it.
pub fn remember_owner_choice(enabled: bool) {
    OWNER_CHOICE.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// The owner's watermark setting as of their latest save (off until then).
pub fn owner_choice() -> bool {
    OWNER_CHOICE.load(std::sync::atomic::Ordering::Relaxed)
}

/// What the save path writes after [`protect_for_save`].
pub struct Protected {
    /// The bytes to embed metadata into and write: watermarked, or the input.
    pub bytes: Vec<u8>,
    /// The watermark ID to record, when one was embedded and read back.
    pub watermark_id: Option<u64>,
    /// The fingerprint of the final pixels, when they were decoded anyway.
    pub fingerprint: Option<u64>,
}

impl Protected {
    fn untouched(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
            watermark_id: None,
            fingerprint: None,
        }
    }
}

/// Watermark `bytes` before they are saved, when `enabled` and the models are
/// ready. Keeps the container format. Leaves alone: NovelAI PNGs (their pixels
/// carry NovelAI's own signed payload), 16-bit images, videos and anything
/// under [`MIN_SIDE`]. Blocking: run off the async runtime for large images.
pub fn protect_for_save(bytes: &[u8], enabled: bool) -> Protected {
    #[cfg(any(feature = "desktop", feature = "server"))]
    if enabled {
        match off_the_runtime(|| engine::watermark_file(bytes)) {
            Ok(Some(p)) => return p,
            Ok(None) => {}
            Err(e) => log::warn!("watermark: skipped, saving without: {e}"),
        }
    }
    let _ = enabled;
    Protected::untouched(bytes)
}

/// Run `f`, telling a multi-threaded tokio runtime this worker is busy so its
/// other tasks move on. The save paths that call [`protect_for_save`] are
/// synchronous functions running inside async handlers.
#[cfg(any(feature = "desktop", feature = "server"))]
fn off_the_runtime<T>(f: impl FnOnce() -> T) -> T {
    use tokio::runtime::{Handle, RuntimeFlavor};
    match Handle::try_current() {
        Ok(h) if h.runtime_flavor() == RuntimeFlavor::MultiThread => tokio::task::block_in_place(f),
        _ => f(),
    }
}

/// Record the fingerprint (and watermark ID) of a just-saved gallery file.
/// Runs on its own thread when the pixels still need decoding, so the save
/// returns at once.
pub fn index_saved(path: &Path, bytes: &[u8], protected: &Protected) {
    let path = path.to_path_buf();
    let watermark_id = protected.watermark_id;
    if let Some(fp) = protected.fingerprint {
        crate::gallery_index::set_protection(&path, fp, watermark_id);
        return;
    }
    let bytes = bytes.to_vec();
    std::thread::spawn(move || {
        if let Some(img) = decode_pixels(&bytes) {
            let fp = fingerprint::fingerprint(&img);
            crate::gallery_index::set_protection(&path, fp, watermark_id);
        }
    });
}

/// Find the gallery record of an image whose own metadata is gone: by its
/// watermark first, then by fingerprint. Only rows directly inside `dir` are
/// considered. The returned map carries [`RECOVERED_KEY`]. Blocking.
pub fn recover(bytes: &[u8], dir: &Path) -> Option<HashMap<String, String>> {
    let img = decode_pixels(bytes)?;

    #[cfg(any(feature = "desktop", feature = "server"))]
    match engine::read_id(&img) {
        Ok(Some(id)) => {
            let meta = crate::gallery_index::path_for_watermark(id, dir)
                .and_then(|p| crate::metadata::read_file_metadata(&p));
            return Some(match meta {
                Some(m) => tagged(m, "watermark"),
                None => tagged(HashMap::new(), "watermark_foreign"),
            });
        }
        Ok(None) => {}
        Err(e) => log::info!("watermark: not read ({e}); trying the fingerprint"),
    }

    let fp = fingerprint::fingerprint(&img);
    let path = crate::gallery_index::nearest_fingerprint(fp, dir, fingerprint::MATCH_DISTANCE)?;
    let meta = crate::metadata::read_file_metadata(&path)?;
    Some(tagged(meta, "fingerprint"))
}

/// The file's own metadata, or, when it has none, what [`recover`] finds in
/// the gallery at `dir`. Blocking.
pub fn read_or_recover(
    bytes: &[u8],
    dir: Option<&Path>,
) -> Result<Option<HashMap<String, String>>, String> {
    if let Some(meta) = crate::metadata::read_image_metadata(bytes)? {
        return Ok(Some(meta));
    }
    Ok(dir.and_then(|d| recover(bytes, d)))
}

fn tagged(mut meta: HashMap<String, String>, source: &str) -> HashMap<String, String> {
    meta.insert(RECOVERED_KEY.to_string(), source.to_string());
    meta
}

/// Whether the watermark can run right now: models and ONNX Runtime on disk.
pub fn is_ready() -> bool {
    #[cfg(any(feature = "desktop", feature = "server"))]
    {
        engine::files_present()
    }
    #[cfg(not(any(feature = "desktop", feature = "server")))]
    {
        false
    }
}

/// Download whatever the watermark still needs (ONNX Runtime, shared with the
/// tagger, and the two TrustMark models, about 65 MB) and load it.
#[cfg(any(feature = "desktop", feature = "server"))]
pub async fn prepare(client: &reqwest::Client) -> Result<(), crate::error::AppError> {
    engine::download(client).await?;
    tokio::task::spawn_blocking(engine::ensure_loaded)
        .await
        .map_err(|e| crate::error::AppError::Other(format!("watermark load task: {e}")))?
        .map_err(crate::error::AppError::Other)
}

#[cfg(any(feature = "desktop", feature = "server"))]
mod engine {
    use super::*;
    use crate::error::AppError;
    use sha2::{Digest, Sha256};
    use std::sync::Mutex;

    static ENGINE: Mutex<Option<trustmark::TrustMark>> = Mutex::new(None);

    const MODEL_BASE_URL: &str = "https://cai-watermark.adobe.net/watermarking/trustmark-models";
    /// The two variant P models and their SHA-256, pinned so a changed upload
    /// is refused instead of silently swapping the watermark.
    const MODELS: [(&str, &str); 2] = [
        (
            trustmark::ENCODER_FILENAME,
            "053441c9c9f05fc158ccba71c610d9d58fcd2c82d1912bf0ffcee988cf2f74c8",
        ),
        (
            trustmark::DECODER_FILENAME,
            "be6d7c33f8a7b376f179e75f3f7c58ff816a9ac7bb6d37fd0a729a635f624c35",
        ),
    ];

    fn ort_root() -> Option<PathBuf> {
        crate::config::app_data_dir().map(|d| d.join("interrogator"))
    }

    pub(super) fn files_present() -> bool {
        let Some(dir) = models_dir() else {
            return false;
        };
        MODELS.iter().all(|(f, _)| dir.join(f).exists())
            && ort_root().is_some_and(|r| crate::interrogator::is_ort_library_present_at(&r))
    }

    pub(super) async fn download(client: &reqwest::Client) -> Result<(), AppError> {
        let root = ort_root().ok_or_else(|| AppError::Other("No app data directory".into()))?;
        crate::interrogator::ensure_ort_library_headless_at(client, &root).await?;
        let dir = models_dir().ok_or_else(|| AppError::Other("No app data directory".into()))?;
        std::fs::create_dir_all(&dir)?;
        for (file, sha) in MODELS {
            let dest = dir.join(file);
            if dest.exists() {
                continue;
            }
            let url = format!("{MODEL_BASE_URL}/{file}");
            crate::interrogator::download_simple(client, &url, &dest).await?;
            let got = hex::encode(Sha256::digest(std::fs::read(&dest)?));
            if got != sha {
                let _ = std::fs::remove_file(&dest);
                return Err(AppError::Other(format!(
                    "{file} failed its checksum (got {got}); not using it"
                )));
            }
        }
        Ok(())
    }

    /// Load the models once. Errors when they are not downloaded.
    pub(super) fn ensure_loaded() -> Result<(), String> {
        let mut slot = ENGINE
            .lock()
            .map_err(|_| "watermark engine lock poisoned")?;
        if slot.is_some() {
            return Ok(());
        }
        if !files_present() {
            return Err("watermark models are not downloaded".into());
        }
        let lib = crate::interrogator::ort_library_path_in(&ort_root().ok_or("no app data dir")?);
        ort::init_from(&lib)
            .map_err(|e| format!("ONNX Runtime at {}: {e}", lib.display()))?
            .commit();
        let dir = models_dir().ok_or("no app data dir")?;
        let started = std::time::Instant::now();
        *slot = Some(trustmark::TrustMark::load(&dir, trustmark::Version::Bch5)?);
        log::info!("watermark: models loaded in {:.1?}", started.elapsed());
        Ok(())
    }

    fn with_engine<T>(
        f: impl FnOnce(&mut trustmark::TrustMark) -> Result<T, String>,
    ) -> Result<T, String> {
        ensure_loaded()?;
        let mut slot = ENGINE
            .lock()
            .map_err(|_| "watermark engine lock poisoned")?;
        let tm = slot.as_mut().ok_or("watermark engine not loaded")?;
        f(tm)
    }

    /// `Ok(None)` for files the watermark deliberately skips.
    pub(super) fn watermark_file(bytes: &[u8]) -> Result<Option<Protected>, String> {
        let format = crate::metadata::detect_format(bytes);
        match format {
            ImageFormat::Png => {
                if crate::metadata::png_carries_novelai_metadata(bytes)
                    || crate::metadata::is_png_16bit(bytes)?
                {
                    return Ok(None);
                }
            }
            ImageFormat::Jxl => {
                if crate::jxl::bits_per_sample(bytes).is_none_or(|b| b > 8) {
                    return Ok(None);
                }
            }
            ImageFormat::WebP => {}
            _ => return Ok(None),
        }
        let img = decode_pixels(bytes).ok_or("could not decode the image")?;
        if img.width() < MIN_SIDE || img.height() < MIN_SIDE {
            return Ok(None);
        }

        // An edit of an image that is already marked (Photopea, a re-save)
        // keeps its mark: a second residual on top would blur both.
        if let Some(existing) = read_id(&img)? {
            return Ok(Some(Protected {
                bytes: bytes.to_vec(),
                watermark_id: Some(existing),
                fingerprint: Some(fingerprint::fingerprint(&img)),
            }));
        }

        let id = new_id();
        let started = std::time::Instant::now();
        let marked = with_engine(|tm| {
            let marked = tm.encode(&img, &payload_for(id), trustmark::DEFAULT_STRENGTH)?;
            let back = tm.decode(&DynamicImage::ImageRgba8(marked.clone()))?;
            if back.as_deref().and_then(id_from_payload) != Some(id) {
                return Err("the watermark did not read back".into());
            }
            Ok(marked)
        })?;
        let (w, h) = marked.dimensions();
        let fp = fingerprint::fingerprint(&DynamicImage::ImageRgba8(marked.clone()));
        let raw = marked.into_raw();
        let out = match format {
            ImageFormat::Png => {
                let png = crate::jxl::encode_rgba8_png(&raw, w, h).map_err(|e| e.to_string())?;
                // Keep ComfyUI's own chunks (workflow, prompt) on the new PNG.
                crate::metadata::copy_png_text_chunks(bytes, &png).unwrap_or(png)
            }
            ImageFormat::Jxl => {
                crate::jxl::encode_rgba8_visually_lossless(&raw, w, h).map_err(|e| e.to_string())?
            }
            _ => crate::jxl::encode_rgba8_webp_from_raw(&raw, w, h, false)
                .map_err(|e| e.to_string())?,
        };
        log::info!(
            "watermark: embedded in {w}x{h} {format:?} in {:.0?}",
            started.elapsed()
        );
        Ok(Some(Protected {
            bytes: out,
            watermark_id: Some(id),
            fingerprint: Some(fp),
        }))
    }

    /// The MooshieUI watermark ID in `img`. `Err` when the models are not
    /// available; `Ok(None)` when there is no MooshieUI mark.
    pub(super) fn read_id(img: &DynamicImage) -> Result<Option<u64>, String> {
        with_engine(|tm| Ok(tm.decode(img)?.as_deref().and_then(id_from_payload)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_fit_the_payload_and_are_never_zero() {
        for _ in 0..1000 {
            let id = new_id();
            assert!(id != 0 && id < (1u64 << ID_BITS));
        }
    }

    #[test]
    fn payload_round_trips_and_fills_bch5() {
        let id = 0x1F_2345_6789_ABCD;
        let bits = payload_for(id);
        assert_eq!(bits.len(), 61);
        assert_eq!(id_from_payload(&bits), Some(id));
    }

    #[test]
    fn a_foreign_trustmark_is_not_ours() {
        let mut bits = payload_for(42);
        bits.replace_range(0..1, "0");
        assert_eq!(id_from_payload(&bits), None);
        assert_eq!(id_from_payload(&"0".repeat(61)), None);
        assert_eq!(
            id_from_payload(&format!("{TAG_BITS}{}", "0".repeat(ID_BITS))),
            None
        );
    }
}

/// End-to-end checks against the real TrustMark models. They need the models
/// and ONNX Runtime on disk, so they are ignored by default. Run with
/// `MOOSHIE_TRUSTMARK_DIR=<models> MOOSHIE_ORT_LIB=<libonnxruntime> cargo test
/// watermark::model_tests -- --ignored`.
#[cfg(all(test, any(feature = "desktop", feature = "server")))]
mod model_tests {
    use super::*;

    fn load() -> trustmark::TrustMark {
        let lib = std::env::var("MOOSHIE_ORT_LIB").expect("MOOSHIE_ORT_LIB");
        ort::init_from(&lib).unwrap().commit();
        let dir = std::env::var("MOOSHIE_TRUSTMARK_DIR").expect("MOOSHIE_TRUSTMARK_DIR");
        trustmark::TrustMark::load(Path::new(&dir), trustmark::Version::Bch5).unwrap()
    }

    fn sample(w: u32, h: u32) -> DynamicImage {
        // Flat colour fields with soft edges, like an anime render.
        DynamicImage::ImageRgba8(image::RgbaImage::from_fn(w, h, |x, y| {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            let r = if (u - 0.5).powi(2) + (v - 0.4).powi(2) < 0.06 {
                240
            } else {
                90
            };
            let g = (v * 200.0) as u8 + 30;
            let b = if u > 0.7 { 210 } else { 60 + (u * 80.0) as u8 };
            image::Rgba([r, g, b, 255])
        }))
    }

    fn jpeg(img: &DynamicImage, q: u8) -> DynamicImage {
        let mut buf = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, q)
            .encode_image(&img.to_rgb8())
            .unwrap();
        image::load_from_memory(&buf).unwrap()
    }

    fn psnr(a: &image::RgbaImage, b: &image::RgbaImage) -> f64 {
        let mse: f64 = a
            .pixels()
            .zip(b.pixels())
            .flat_map(|(p, q)| (0..3).map(move |c| (p[c] as f64 - q[c] as f64).powi(2)))
            .sum::<f64>()
            / (a.width() * a.height() * 3) as f64;
        10.0 * (255.0f64.powi(2) / mse).log10()
    }

    #[test]
    #[ignore]
    fn survives_jpeg_resize_and_crop() {
        let mut tm = load();
        let original = sample(832, 1216);
        let id = new_id();
        let t = std::time::Instant::now();
        let marked = tm
            .encode(&original, &payload_for(id), trustmark::DEFAULT_STRENGTH)
            .unwrap();
        eprintln!(
            "encode {:?}, PSNR {:.1} dB",
            t.elapsed(),
            psnr(&original.to_rgba8(), &marked)
        );
        let marked = DynamicImage::ImageRgba8(marked);

        let read = |tm: &mut trustmark::TrustMark, img: &DynamicImage| {
            tm.decode(img).unwrap().as_deref().and_then(id_from_payload)
        };
        assert_eq!(read(&mut tm, &marked), Some(id), "lossless");
        assert_eq!(read(&mut tm, &jpeg(&marked, 75)), Some(id), "jpeg 75");
        let half = marked.resize_exact(416, 608, image::imageops::FilterType::Lanczos3);
        assert_eq!(
            read(&mut tm, &jpeg(&half, 80)),
            Some(id),
            "half size + jpeg 80"
        );
        let mut png = Vec::new();
        marked
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(
            read(&mut tm, &decode_pixels(&png).unwrap()),
            Some(id),
            "png file"
        );
        let crop = marked.crop_imm(20, 30, 792, 1150);
        assert_eq!(
            read(&mut tm, &jpeg(&crop, 85)),
            Some(id),
            "small crop + jpeg 85"
        );
        assert_eq!(read(&mut tm, &original), None, "unmarked");
    }
}
