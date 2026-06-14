use serde::Serialize;

use crate::prompt_assistant::catalog;

#[derive(Debug, Clone, Serialize)]
pub struct LlmGpu {
    pub name: String,
    pub vram_mb: u64,
    pub vendor: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmHardware {
    pub gpus: Vec<LlmGpu>,
    pub total_vram_mb: u64,
    pub system_ram_mb: u64,
    pub recommended_model_id: String,
}

/// Coarse vendor classification from the GPU name.
pub fn vendor_of(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.contains("nvidia")
        || n.contains("geforce")
        || n.contains("rtx")
        || n.contains("gtx")
        || n.contains("quadro")
        || n.contains("tesla")
    {
        "nvidia"
    } else if n.contains("radeon") || n.contains("amd") || n.contains("instinct") {
        "amd"
    } else if n.contains("intel") || n.contains("arc") {
        "intel"
    } else if n.contains("apple") {
        "apple"
    } else {
        "unknown"
    }
}

/// Read total system RAM in MB.
pub fn system_ram_mb() -> u64 {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.total_memory() / 1024 / 1024 // bytes → MB
}

/// Build the full hardware report and pick a recommended catalog model.
pub fn detect() -> LlmHardware {
    let raw = crate::comfyui::gpu_manager::detect_gpus(); // Vec<(index, name, vram_mb)>
    let gpus: Vec<LlmGpu> = raw
        .into_iter()
        .map(|(_idx, name, vram_mb)| {
            let vendor = vendor_of(&name).to_string();
            LlmGpu {
                name,
                vram_mb,
                vendor,
            }
        })
        .collect();

    let total_vram_mb = gpus.iter().map(|g| g.vram_mb).max().unwrap_or(0);
    let system_ram_mb = system_ram_mb();

    let recommended_model_id = catalog::recommend_model_id(total_vram_mb, system_ram_mb);

    LlmHardware {
        gpus,
        total_vram_mb,
        system_ram_mb,
        recommended_model_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_classification() {
        assert_eq!(vendor_of("NVIDIA GeForce RTX 5090"), "nvidia");
        assert_eq!(vendor_of("AMD Radeon RX 7900"), "amd");
        assert_eq!(vendor_of("Intel Arc A770"), "intel");
    }
}
