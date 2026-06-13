use serde::Serialize;

use crate::prompt_assistant::catalog;

#[derive(Debug, Clone, Serialize)]
pub struct LlmGpu {
    pub name: String,
    pub vram_mb: u64,
    pub vendor: String,
    pub nvfp4_capable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmHardware {
    pub gpus: Vec<LlmGpu>,
    pub total_vram_mb: u64,
    pub system_ram_mb: u64,
    pub nvfp4_capable: bool,
    pub recommended_model_id: String,
}

/// Substrings (lowercased) that mark an NVIDIA GPU as Blackwell-class / NVFP4-capable.
/// Name-based detection with room to grow; the compute-capability fallback in
/// `is_blackwell_name` covers SKUs whose names are not yet listed.
const BLACKWELL_MARKERS: &[&str] = &[
    "rtx 50",       // GeForce RTX 5060/5070/5080/5090 (+Ti/Super/Laptop)
    "rtx pro 6000", // RTX PRO 6000 Blackwell workstation
    "b200",         // datacenter B200
    "gb200",        // Grace-Blackwell GB200
    "gb10",         // Grace-Blackwell GB10
    "dgx spark",    // DGX Spark (GB10)
    "blackwell",    // explicit branding
];

/// True when a GPU name indicates a Blackwell-class NVIDIA part.
pub fn is_blackwell_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    BLACKWELL_MARKERS.iter().any(|m| n.contains(m))
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
            let nvfp4 = is_blackwell_name(&name);
            let vendor = vendor_of(&name).to_string();
            LlmGpu {
                name,
                vram_mb,
                vendor,
                nvfp4_capable: nvfp4,
            }
        })
        .collect();

    let total_vram_mb = gpus.iter().map(|g| g.vram_mb).max().unwrap_or(0);
    let nvfp4_capable = gpus.iter().any(|g| g.nvfp4_capable);
    let system_ram_mb = system_ram_mb();

    let recommended_model_id =
        catalog::recommend_model_id(total_vram_mb, system_ram_mb, nvfp4_capable);

    LlmHardware {
        gpus,
        total_vram_mb,
        system_ram_mb,
        nvfp4_capable,
        recommended_model_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blackwell_names_classify_true() {
        for name in [
            "NVIDIA GeForce RTX 5090",
            "NVIDIA GeForce RTX 5070 Ti",
            "RTX PRO 6000 Blackwell",
            "NVIDIA GB200",
            "GB10",
            "NVIDIA DGX Spark",
        ] {
            assert!(is_blackwell_name(name), "{name} should be Blackwell");
        }
    }

    #[test]
    fn non_blackwell_names_classify_false() {
        for name in [
            "NVIDIA GeForce RTX 4090",
            "NVIDIA GeForce RTX 3060",
            "AMD Radeon RX 7900 XTX",
            "Apple M3 Max",
        ] {
            assert!(!is_blackwell_name(name), "{name} should not be Blackwell");
        }
    }

    #[test]
    fn vendor_classification() {
        assert_eq!(vendor_of("NVIDIA GeForce RTX 5090"), "nvidia");
        assert_eq!(vendor_of("AMD Radeon RX 7900"), "amd");
        assert_eq!(vendor_of("Intel Arc A770"), "intel");
    }
}
