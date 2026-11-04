//! Image to textured 3D mesh with Pixal3D on ComfyUI's native TRELLIS.2 nodes.
//!
//! Pixal3D (Tencent ARC, MIT, repackaged as Comfy-Org/Pixal3D) is a
//! pixel-aligned image-to-3D model on the TRELLIS.2 pipeline. The graph
//! follows ComfyUI's "Pixal3D & TRELLIS.2: Image to Model" template,
//! Pixal3D branch only:
//!
//! 1. BiRefNet mask (or the image's alpha), `ImageCropToMask` at pad 1.1.
//! 2. MoGe-2 estimates the horizontal field of view; `Pixal3DConditioning`
//!    encodes the crop with DINOv3-L + NAF at that camera.
//! 3. Four sampling passes on the Pixal3D model: sparse structure (shift 5,
//!    cfg 7.5 with CFG off for the last third, rescale 0.7), shape (cfg 7.5,
//!    rescale 0.5), the 1536 upsample pass, then texture (cfg 1).
//! 4. Mesh clean-up and baking: remesh at 768 (the template's watertight
//!    "solid" mode on ComfyUI v0.43+, its earlier "udf" mode before), decimate, smooth normals, UV
//!    unwrap, bake base colour / metallic / roughness from the texture
//!    voxels, normal and ambient occlusion from the remeshed high-poly mesh.
//! 5. `SaveGLB` writes one textured `.glb`.
//!
//! Like music and the splat graph, it has its own terminal node, so nothing
//! lands in the image gallery.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Every node the graph uses. All ship with ComfyUI v0.41.1 and newer.
pub const REQUIRED_NODES: &[&str] = &[
    "LoadImage",
    "InvertMask",
    "LoadBackgroundRemovalModel",
    "RemoveBackground",
    "ImageCropToMask",
    "LoadMoGeModel",
    "MoGeInference",
    "MoGeGeometryToFOV",
    "CLIPVisionLoader",
    "Pixal3DConditioning",
    "UNETLoader",
    "CFGOverride",
    "RescaleCFG",
    "ModelSamplingSD3",
    "EmptyTrellis2LatentStructure",
    "KSampler",
    "VAELoader",
    "VaeDecodeStructureTrellis2",
    "Trellis2ShapeStage",
    "Trellis2UpsampleStage",
    "VaeDecodeShapeTrellis",
    "Trellis2TextureStage",
    "VaeDecodeTextureTrellis",
    "RemeshMesh",
    "DecimateMesh",
    "MeshSmoothNormals",
    "UnwrapMesh",
    "BakeTextureFromVoxel",
    "BakeNormalMapFromMesh",
    "BakeAmbientOcclusion",
    "ApplyTextureToMesh",
    "SaveGLB",
];

/// ComfyUI output subfolder and filename prefix of the saved mesh. Only
/// outputs that match both are read back.
pub const OUTPUT_SUBFOLDER: &str = "3d";
pub const OUTPUT_PREFIX: &str = "mooshie_mesh_";

/// Node ids the frontend maps to progress stages (see `model3d.svelte.ts`).
pub const NODE_STRUCTURE_SAMPLER: &str = "16";
pub const NODE_SHAPE_SAMPLER: &str = "22";
pub const NODE_UPSAMPLE_SAMPLER: &str = "24";
pub const NODE_TEXTURE_SAMPLER: &str = "27";
pub const NODE_SAVE: &str = "39";

/// How much mesh and texture the result keeps. `High` is the official
/// template; `Standard` keeps fewer faces and smaller textures, for a file
/// a third the size that bakes faster.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshDetail {
    Standard,
    #[default]
    High,
}

impl MeshDetail {
    /// (target faces, base colour / metallic / roughness texture size).
    fn budget(self) -> (u32, u32) {
        match self {
            MeshDetail::Standard => (300_000, 2048),
            MeshDetail::High => (700_000, 4096),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshParams {
    /// Pixal3D model in `diffusion_models`, e.g. `pixal3d_int8_convrot.safetensors`.
    pub model: String,
    #[serde(default = "default_true")]
    pub remove_background: bool,
    #[serde(default)]
    pub detail: MeshDetail,
    #[serde(with = "crate::comfyui::types::seed_string")]
    pub seed: i64,
}

fn default_true() -> bool {
    true
}

/// The model files one run needs, resolved from the worker's model lists.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshModels {
    pub unet: String,
    pub clip_vision: String,
    pub shape_vae: String,
    pub texture_vae: String,
    pub moge: String,
    /// Only needed when background removal is on.
    pub bg_removal: Option<String>,
    /// `RemeshMesh` offers the "solid" sign mode (ComfyUI v0.43+).
    pub solid_remesh: bool,
}

/// Whether this ComfyUI's `RemeshMesh` has the watertight "solid" sign mode,
/// which the current template uses (added in ComfyUI v0.43.0).
pub fn has_solid_remesh(object_info: &Value) -> bool {
    object_info["RemeshMesh"]["input"]["required"]["sign_mode"][1]["options"]
        .as_array()
        .is_some_and(|options| options.iter().any(|o| o["key"] == "solid"))
}

fn base(name: &str) -> String {
    let l = name.to_lowercase().replace('\\', "/");
    l.rsplit('/').next().unwrap_or(&l).to_string()
}

/// `RemeshMesh` inputs. The current template uses "solid" (one watertight
/// shell: double walls removed, small holes closed, fill 20); before ComfyUI
/// v0.43 the template used "udf" with every clean-up option off.
fn remesh_inputs(solid: bool) -> Value {
    let mut inputs = json!({
        "mesh": ["25", 0], "resolution": 768, "band": 1.0, "project_back": 0.0,
        "fix_poles": false, "smooth_iters": 20, "drop_small_components": 0.01,
        "precluster_max_verts": 20_000_000,
    });
    let mode = if solid {
        json!({"sign_mode": "solid", "sign_mode.fill": 20.0, "sign_mode.qef": false})
    } else {
        json!({
            "sign_mode": "udf", "sign_mode.qef": false,
            "sign_mode.drop_inverted_components": false,
            "sign_mode.drop_enclosed_components": false,
        })
    };
    for (key, value) in mode.as_object().into_iter().flatten() {
        inputs[key] = value.clone();
    }
    inputs
}

/// Single-image Pixal3D models in `diffusion_models`. The multi-view model
/// needs a different conditioning node.
pub fn is_pixal3d_unet(name: &str) -> bool {
    let b = base(name);
    b.contains("pixal3d") && !b.contains("multiview") && b.ends_with(".safetensors")
}

/// DINOv3 ViT-L/16 with the bundled NAF upsampler, which Pixal3D needs.
pub fn is_dinov3_naf(name: &str) -> bool {
    let b = base(name);
    (b.contains("dino_v3") || b.contains("dinov3")) && b.contains("naf")
}

pub fn is_trellis2_shape_vae(name: &str) -> bool {
    let b = base(name);
    b.contains("trellis_2_shape") || b.contains("trellis2_shape")
}

pub fn is_trellis2_texture_vae(name: &str) -> bool {
    let b = base(name);
    b.contains("trellis_2_texture") || b.contains("trellis2_texture")
}

/// MoGe geometry models in `geometry_estimation`; MoGe-2 is preferred.
pub fn is_moge(name: &str) -> bool {
    base(name).contains("moge")
}

pub fn pick_moge(models: &[String]) -> Option<&String> {
    models
        .iter()
        .find(|m| base(m).contains("moge_2"))
        .or_else(|| models.first())
}

pub fn validate(params: &MeshParams) -> Result<(), String> {
    if !is_pixal3d_unet(&params.model) {
        return Err("Select a Pixal3D model from ComfyUI's diffusion_models folder.".into());
    }
    Ok(())
}

/// The seed ComfyUI's KSampler accepts (0 to 2^64 - 1), offset per pass.
fn pass_seed(seed: i64, offset: u64) -> u64 {
    (seed as u64).wrapping_add(offset)
}

fn node(g: &mut Map<String, Value>, id: &str, class: &str, inputs: Value) {
    g.insert(id.into(), json!({"class_type": class, "inputs": inputs}));
}

/// Build the ComfyUI API graph for one image (already uploaded as `image`).
pub fn build(params: &MeshParams, models: &MeshModels, image: &str, seed: i64) -> Value {
    let (faces, texture) = params.detail.budget();
    let mut g = Map::new();
    node(&mut g, "1", "LoadImage", json!({"image": image}));
    node(
        &mut g,
        "5",
        "ImageCropToMask",
        json!({ "images": ["1", 0], "masks": ["4", 0], "width": 1024, "height": 1024, "pad_factor": 1.1, "grow_mask": 0, "background": "#000000"}),
    );
    node(
        &mut g,
        "6",
        "LoadMoGeModel",
        json!({"model_name": models.moge}),
    );
    node(
        &mut g,
        "7",
        "MoGeInference",
        json!({ "moge_model": ["6", 0], "image": ["5", 0], "resolution_level": 9, "fov_x_degrees": 0.0, "batch_size": 4, "force_projection": true, "apply_mask": true, "refine_steps": 3}),
    );
    node(
        &mut g,
        "8",
        "MoGeGeometryToFOV",
        json!({ "moge_geometry": ["7", 0], "axis": "horizontal", "unit": "degrees"}),
    );
    node(
        &mut g,
        "9",
        "CLIPVisionLoader",
        json!({"clip_name": models.clip_vision}),
    );
    node(
        &mut g,
        "10",
        "Pixal3DConditioning",
        json!({ "clip_vision_model": ["9", 0], "image": ["5", 0], "camera_angle_x": ["8", 0]}),
    );
    node(
        &mut g,
        "11",
        "UNETLoader",
        json!({ "unet_name": models.unet, "weight_dtype": "default"}),
    );
    // Sparse structure pass.
    node(
        &mut g,
        "12",
        "CFGOverride",
        json!({ "model": ["11", 0], "cfg": 1.0, "start_percent": 0.667, "end_percent": 1.0}),
    );
    node(
        &mut g,
        "13",
        "RescaleCFG",
        json!({"model": ["12", 0], "multiplier": 0.7}),
    );
    node(
        &mut g,
        "14",
        "ModelSamplingSD3",
        json!({"model": ["13", 0], "shift": 5.0}),
    );
    node(
        &mut g,
        "15",
        "EmptyTrellis2LatentStructure",
        json!({"batch_size": 1}),
    );
    node(
        &mut g,
        "16",
        "KSampler",
        json!({ "model": ["14", 0], "seed": pass_seed(seed, 14), "steps": 12, "cfg": 7.5, "sampler_name": "euler", "scheduler": "normal", "positive": ["10", 0], "negative": ["10", 1], "latent_image": ["15", 0], "denoise": 1.0}),
    );
    node(
        &mut g,
        "17",
        "VAELoader",
        json!({"vae_name": models.shape_vae}),
    );
    node(
        &mut g,
        "18",
        "VaeDecodeStructureTrellis2",
        json!({ "samples": ["16", 0], "vae": ["17", 0], "resolution": "32"}),
    );
    // Shape pass, then the 1536 upsample pass.
    node(
        &mut g,
        "19",
        "Trellis2ShapeStage",
        json!({ "positive": ["10", 0], "negative": ["10", 1], "voxel": ["18", 0]}),
    );
    node(
        &mut g,
        "20",
        "CFGOverride",
        json!({ "model": ["11", 0], "cfg": 1.0, "start_percent": 0.769, "end_percent": 1.0}),
    );
    node(
        &mut g,
        "21",
        "RescaleCFG",
        json!({"model": ["20", 0], "multiplier": 0.5}),
    );
    node(
        &mut g,
        "22",
        "KSampler",
        json!({ "model": ["21", 0], "seed": pass_seed(seed, 0), "steps": 20, "cfg": 7.5, "sampler_name": "euler", "scheduler": "normal", "positive": ["19", 0], "negative": ["19", 1], "latent_image": ["19", 2], "denoise": 1.0}),
    );
    node(
        &mut g,
        "23",
        "Trellis2UpsampleStage",
        json!({ "positive": ["19", 0], "negative": ["19", 1], "shape_latent": ["22", 0], "vae": ["17", 0], "target_resolution": 1536}),
    );
    node(
        &mut g,
        "24",
        "KSampler",
        json!({ "model": ["21", 0], "seed": pass_seed(seed, 0), "steps": 12, "cfg": 7.5, "sampler_name": "euler", "scheduler": "simple", "positive": ["23", 0], "negative": ["23", 1], "latent_image": ["23", 2], "denoise": 1.0}),
    );
    node(
        &mut g,
        "25",
        "VaeDecodeShapeTrellis",
        json!({ "samples": ["24", 0], "vae": ["17", 0]}),
    );
    // Texture pass.
    node(
        &mut g,
        "26",
        "Trellis2TextureStage",
        json!({ "positive": ["23", 0], "negative": ["23", 1], "shape_latent": ["24", 0]}),
    );
    node(
        &mut g,
        "27",
        "KSampler",
        json!({ "model": ["11", 0], "seed": pass_seed(seed, 1), "steps": 12, "cfg": 1.0, "sampler_name": "euler", "scheduler": "normal", "positive": ["26", 0], "negative": ["26", 1], "latent_image": ["26", 2], "denoise": 1.0}),
    );
    node(
        &mut g,
        "28",
        "VAELoader",
        json!({"vae_name": models.texture_vae}),
    );
    node(
        &mut g,
        "29",
        "VaeDecodeTextureTrellis",
        json!({ "samples": ["27", 0], "vae": ["28", 0], "shape_subdivides": ["25", 1]}),
    );
    // Mesh clean-up and texture baking.
    node(
        &mut g,
        "30",
        "RemeshMesh",
        remesh_inputs(models.solid_remesh),
    );
    node(
        &mut g,
        "31",
        "DecimateMesh",
        json!({ "mesh": ["30", 0], "target_face_count": faces, "placement_mode": "midpoint"}),
    );
    node(
        &mut g,
        "32",
        "MeshSmoothNormals",
        json!({"mesh": ["31", 0], "crease_angle": 180.0}),
    );
    node(
        &mut g,
        "33",
        "UnwrapMesh",
        json!({ "mesh": ["32", 0], "segmenter": "pec", "resolution": texture, "padding": 1, "weld_distance": 0.0002}),
    );
    node(
        &mut g,
        "34",
        "BakeTextureFromVoxel",
        json!({ "mesh": ["33", 0], "voxel_colors": ["29", 0], "texture_size": texture, "reference_mesh": ["25", 0]}),
    );
    node(
        &mut g,
        "35",
        "BakeNormalMapFromMesh",
        json!({ "low_poly": ["33", 0], "high_poly": ["30", 0], "resolution": 2048, "cage_distance": 0.05, "ignore_backfaces": true}),
    );
    node(
        &mut g,
        "36",
        "BakeAmbientOcclusion",
        json!({ "low_poly": ["33", 0], "high_poly": ["30", 0], "resolution": 1024, "samples": 64, "max_distance": 0.71, "strength": 1.0, "bias": 0.01}),
    );
    node(
        &mut g,
        "37",
        "ApplyTextureToMesh",
        json!({ "mesh": ["33", 0], "base_color": ["34", 0], "metallic": ["34", 1], "roughness": ["34", 2], "occlusion": ["36", 0], "normal_map": ["35", 0]}),
    );
    node(
        &mut g,
        "38",
        "MeshSmoothNormals",
        json!({"mesh": ["37", 0], "crease_angle": 180.0}),
    );
    node(
        &mut g,
        "39",
        "SaveGLB",
        json!({ "mesh": ["38", 0], "filename_prefix": format!("{OUTPUT_SUBFOLDER}/mooshie_mesh")}),
    );
    let mut g = Value::Object(g);
    match &models.bg_removal {
        Some(bg) => {
            g["2"] = json!({"class_type": "LoadBackgroundRemovalModel",
                "inputs": {"bg_removal_name": bg}});
            g["3"] = json!({"class_type": "RemoveBackground",
                "inputs": {"bg_removal_model": ["2", 0], "image": ["1", 0]}});
            g["5"]["inputs"]["masks"] = json!(["3", 0]);
        }
        None => {
            // LoadImage's mask is 1 - alpha; the crop wants the subject as 1.
            g["4"] = json!({"class_type": "InvertMask", "inputs": {"mask": ["1", 1]}});
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> MeshParams {
        MeshParams {
            model: "pixal3d_int8_convrot.safetensors".into(),
            remove_background: true,
            detail: MeshDetail::High,
            seed: 42,
        }
    }

    fn models() -> MeshModels {
        MeshModels {
            unet: "pixal3d_int8_convrot.safetensors".into(),
            clip_vision: "dino_v3_L_naf_fp32.safetensors".into(),
            shape_vae: "trellis_2_shape_vae_bf16.safetensors".into(),
            texture_vae: "trellis_2_texture_vae_bf16.safetensors".into(),
            moge: "moge_2_vitl_normal_fp16.safetensors".into(),
            bg_removal: Some("birefnet.safetensors".into()),
            solid_remesh: true,
        }
    }

    #[test]
    fn remesh_mode_follows_comfyui() {
        let g = build(&params(), &models(), "in.png", 1);
        let r = &g["30"]["inputs"];
        assert_eq!(r["sign_mode"], "solid");
        assert_eq!(r["sign_mode.fill"], 20.0);
        assert_eq!(r["sign_mode.qef"], false);
        assert!(r.get("sign_mode.drop_inverted_components").is_none());
        let older = MeshModels {
            solid_remesh: false,
            ..models()
        };
        let g = build(&params(), &older, "in.png", 1);
        let r = &g["30"]["inputs"];
        assert_eq!(r["sign_mode"], "udf");
        assert_eq!(r["sign_mode.drop_enclosed_components"], false);
        assert!(r.get("sign_mode.fill").is_none());
        assert_eq!(r["resolution"], 768);
        assert_eq!(r["mesh"], json!(["25", 0]));
    }

    #[test]
    fn solid_remesh_is_read_from_object_info() {
        let mut info = json!({"RemeshMesh": {"input": {"required": {"sign_mode": [
            "COMFY_DYNAMICCOMBO_V3", {"options": [{"key": "udf"}, {"key": "sdf"}]}
        ]}}}});
        assert!(!has_solid_remesh(&info));
        info["RemeshMesh"]["input"]["required"]["sign_mode"][1]["options"]
            .as_array_mut()
            .unwrap()
            .push(json!({"key": "solid"}));
        assert!(has_solid_remesh(&info));
        assert!(!has_solid_remesh(&json!({})));
    }

    /// Every `[node, slot]` reference points at a node in the graph.
    fn assert_links_resolve(g: &Value) {
        for (id, node) in g.as_object().unwrap() {
            for (name, value) in node["inputs"].as_object().unwrap() {
                if let Some(link) = value.as_array() {
                    if link.len() == 2 && link[0].is_string() && link[1].is_u64() {
                        let target = link[0].as_str().unwrap();
                        assert!(g.get(target).is_some(), "{id}.{name} -> missing {target}");
                    }
                }
            }
        }
    }

    #[test]
    fn graph_uses_only_required_nodes_and_resolves() {
        for bg in [true, false] {
            let mut p = params();
            p.remove_background = bg;
            let mut m = models();
            if !bg {
                m.bg_removal = None;
            }
            let g = build(&p, &m, "in.png", 7);
            for node in g.as_object().unwrap().values() {
                let class = node["class_type"].as_str().unwrap();
                assert!(
                    REQUIRED_NODES.contains(&class),
                    "{class} not in REQUIRED_NODES"
                );
            }
            assert_links_resolve(&g);
        }
    }

    #[test]
    fn background_removal_feeds_the_crop() {
        let g = build(&params(), &models(), "in.png", 7);
        assert_eq!(g["5"]["inputs"]["masks"], json!(["3", 0]));
        assert!(g.get("4").is_none());
        let mut m = models();
        m.bg_removal = None;
        let mut p = params();
        p.remove_background = false;
        let g = build(&p, &m, "in.png", 7);
        assert_eq!(g["5"]["inputs"]["masks"], json!(["4", 0]));
        assert_eq!(g["4"]["inputs"]["mask"], json!(["1", 1]));
        assert!(g.get("2").is_none() && g.get("3").is_none());
    }

    #[test]
    fn sampling_matches_the_official_template() {
        let g = build(&params(), &models(), "in.png", 7);
        let ks = |id: &str| &g[id]["inputs"];
        assert_eq!(ks(NODE_STRUCTURE_SAMPLER)["steps"], 12);
        assert_eq!(ks(NODE_STRUCTURE_SAMPLER)["model"], json!(["14", 0]));
        assert_eq!(g["14"]["inputs"]["shift"], 5.0);
        assert_eq!(g["12"]["inputs"]["start_percent"], 0.667);
        assert_eq!(ks(NODE_SHAPE_SAMPLER)["steps"], 20);
        assert_eq!(ks(NODE_UPSAMPLE_SAMPLER)["scheduler"], "simple");
        assert_eq!(ks(NODE_TEXTURE_SAMPLER)["cfg"], 1.0);
        assert_eq!(ks(NODE_TEXTURE_SAMPLER)["model"], json!(["11", 0]));
        assert_eq!(ks(NODE_TEXTURE_SAMPLER)["seed"], 8);
        assert_eq!(g["23"]["inputs"]["target_resolution"], 1536);
        assert_eq!(g["10"]["inputs"]["camera_angle_x"], json!(["8", 0]));
        assert_eq!(g["5"]["inputs"]["pad_factor"], 1.1);
    }

    #[test]
    fn detail_sets_faces_and_texture_size() {
        let mut p = params();
        let g = build(&p, &models(), "in.png", 7);
        assert_eq!(g["31"]["inputs"]["target_face_count"], 700_000);
        assert_eq!(g["33"]["inputs"]["resolution"], 4096);
        assert_eq!(g["34"]["inputs"]["texture_size"], 4096);
        p.detail = MeshDetail::Standard;
        let g = build(&p, &models(), "in.png", 7);
        assert_eq!(g["31"]["inputs"]["target_face_count"], 300_000);
        assert_eq!(g["34"]["inputs"]["texture_size"], 2048);
    }

    #[test]
    fn save_path_matches_the_read_back_check() {
        let g = build(&params(), &models(), "in.png", 7);
        assert_eq!(g[NODE_SAVE]["class_type"], "SaveGLB");
        assert_eq!(g[NODE_SAVE]["inputs"]["filename_prefix"], "3d/mooshie_mesh");
        assert!(OUTPUT_PREFIX.starts_with("mooshie_mesh"));
    }

    #[test]
    fn seeds_stay_in_ksampler_range() {
        let g = build(&params(), &models(), "in.png", i64::MAX);
        assert!(g[NODE_TEXTURE_SAMPLER]["inputs"]["seed"].is_u64());
    }

    #[test]
    fn classifiers() {
        assert!(is_pixal3d_unet("pixal3d_int8_convrot.safetensors"));
        assert!(is_pixal3d_unet("sub/Pixal3D_bf16.safetensors"));
        assert!(!is_pixal3d_unet(
            "pixal3d_multiview_int8_convrot.safetensors"
        ));
        assert!(!is_pixal3d_unet("trellis_2_int8_convrot.safetensors"));
        assert!(is_dinov3_naf("dino_v3_L_naf_fp32.safetensors"));
        assert!(!is_dinov3_naf("dino_v3_vit_h_bf16.safetensors"));
        assert!(is_trellis2_shape_vae(
            "trellis_2_shape_vae_bf16.safetensors"
        ));
        assert!(is_trellis2_texture_vae(
            "trellis_2_texture_vae_bf16.safetensors"
        ));
        let moge = vec![
            "moge_1_vitl_fp16.safetensors".to_string(),
            "moge_2_vitl_normal_fp16.safetensors".to_string(),
        ];
        assert_eq!(pick_moge(&moge).unwrap(), &moge[1]);
    }

    #[test]
    fn params_deserialize_with_defaults() {
        let p: MeshParams = serde_json::from_value(
            json!({"model": "pixal3d_int8_convrot.safetensors", "seed": "-1"}),
        )
        .unwrap();
        assert!(p.remove_background);
        assert_eq!(p.detail, MeshDetail::High);
        assert_eq!(p.seed, -1);
        let p: MeshParams = serde_json::from_value(
            json!({"model": "pixal3d_int8_convrot.safetensors", "seed": "5", "detail": "standard"}),
        )
        .unwrap();
        assert_eq!(p.detail, MeshDetail::Standard);
        assert!(validate(&p).is_ok());
        assert!(validate(&MeshParams {
            model: "idol_fp16.safetensors".into(),
            ..p
        })
        .is_err());
    }
}
