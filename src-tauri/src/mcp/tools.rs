//! The MCP tools: definitions for `tools/list` and their handlers.
//!
//! Every handler acts as the machine owner (the root gallery, admin queue
//! slot), which is who holds the token. Tool failures come back as a normal
//! result with `isError: true` so the agent sees the message.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use super::jobs::{self, Job};
use super::music::{self, MusicJob};
use super::{comic, expand, inputs, library, links, setups};
use crate::comfyui::types::GenerationParams;
use crate::state::AppState;

/// Longest a tool holds a request open before handing back a job id, for
/// clients on this machine. Tunnelled requests get less (see `mod.rs`).
pub const MAX_WAIT: Duration = Duration::from_secs(600);
/// Largest song `include_audio` embeds in a result.
const MAX_INLINE_AUDIO: u64 = 16 * 1024 * 1024;
const PREVIEW_SIZE: u32 = 768;
/// Newest gallery files a search looks at when filling in missing metadata.
const SEARCH_WINDOW: usize = 5000;
/// Files whose metadata a single search reads from disk at most.
const BACKFILL_PER_CALL: usize = 200;
/// Most reference images SnapRef takes (`style_ref::validate_snapref`).
const MAX_SNAPREF_IMAGES: usize = 8;

pub fn definitions() -> Value {
    // One json! per tool: a single literal this size hits the macro
    // recursion limit.
    Value::Array(vec![
        json!({
            "name": "get_status",
            "title": "MooshieUI status",
            "description": "Check whether ComfyUI is running, what is queued, recent image and music jobs, and which models have a saved generation setup (only those can be used by generate_image). Call this first.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "list_models",
            "title": "List models",
            "description": "List installed model files. Checkpoints and diffusion models marked has_setup can be passed to generate_image; LoRAs can be added to any generation. music lists the YuE2 checkpoints generate_music can use.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": { "type": "string", "enum": ["checkpoints", "diffusion_models", "loras", "upscale_models", "music"], "description": "Default checkpoints." },
                    "filter": { "type": "string", "description": "Case-insensitive substring to match." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Default 200." }
                }
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "generate_image",
            "title": "Generate image",
            "description": "Generate images with MooshieUI (txt2img), on a local model or NovelAI. Reuses the owner's last setup for the model (loaders, upscale, face fix, guidance; for NovelAI its quality tags, UC preset and sampler); you change the prompt and any of the optional fields. Prompts may use __name__ wildcards from the owner's wildcard files and {a|b|c} random choices (list_library kind wildcards). Saved characters, artist styles and prompt chunks can be added by name (list_library). NovelAI images must stay inside the free Opus window: at most 1 megapixel, 28 steps, one image. Images are saved to the MooshieUI gallery and returned as previews, each with a url that downloads the full-size file. Waits up to 10 minutes for the result unless wait is false; then poll get_job (get_status lists recent job ids).",
            "inputSchema": {
                "type": "object",
                "required": ["prompt"],
                "properties": {
                    "prompt": { "type": "string", "description": "Positive prompt, in the style the model expects (tags for anime SDXL models, sentences for Flux and similar)." },
                    "negative_prompt": { "type": "string", "description": "Replaces the setup's negative prompt." },
                    "model": { "type": "string", "description": "Model with a saved setup (see get_status), a file name or a NovelAI model id such as nai-diffusion-4-5-full. Default: the most recently used one." },
                    "characters": { "type": "array", "maxItems": 22, "items": { "type": "string" }, "description": "Saved character names (list_library). Each adds its prompt and UC; on NovelAI V4 and later it becomes its own character prompt." },
                    "styles": { "type": "array", "maxItems": 20, "items": { "type": "string" }, "description": "Saved artist style names (list_library kind styles): their weighted artist tags are appended and their UC added." },
                    "chunks": {
                        "type": "array",
                        "maxItems": 30,
                        "description": "Saved prompt chunk names (list_library kind chunks), or { name, mode } with mode prepend, append or random (one random line). Default: the mode the chunk is active with in MooshieUI, else append. Their UC is added.",
                        "items": {
                            "anyOf": [
                                { "type": "string" },
                                { "type": "object", "required": ["name"], "properties": { "name": { "type": "string" }, "mode": { "type": "string", "enum": ["prepend", "append", "random"] } } }
                            ]
                        }
                    },
                    "use_active_styles": { "type": "boolean", "description": "Also add the styles and chunks currently active in MooshieUI. Default false." },
                    "source_image": { "type": "string", "description": "Turns this into img2img: a gallery filename, or the image as base64 or a data URL. The size follows the source's aspect ratio unless width/height are given. Works on NovelAI too (free window)." },
                    "denoise": { "type": "number", "minimum": 0.05, "maximum": 1, "description": "With source_image: how much to change it. Default 0.6." },
                    "reference_images": { "type": "array", "maxItems": 8, "items": { "type": "string" }, "description": "Local models only: one or two character/style reference images (gallery filenames or base64) for the model's style reference (Anima Reference Adapter, Flux Redux, IP-Adapter), using the setup's reference settings. When the owner's Anima setup uses SnapRef, up to 8 images of the same character." },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168, "description": "How long the returned download links work. Default 24." },
                    "width": { "type": "integer", "minimum": 256, "maximum": 4096 },
                    "height": { "type": "integer", "minimum": 256, "maximum": 4096 },
                    "steps": { "type": "integer", "minimum": 1, "maximum": 150 },
                    "cfg": { "type": "number", "minimum": 0, "maximum": 30 },
                    "seed": { "type": "integer", "description": "Omit or -1 for random." },
                    "batch_size": { "type": "integer", "minimum": 1, "maximum": 8 },
                    "sampler": { "type": "string", "description": "ComfyUI sampler name, e.g. euler_ancestral. On NovelAI its own names, e.g. k_euler_ancestral." },
                    "scheduler": { "type": "string", "description": "ComfyUI scheduler name, e.g. karras. On NovelAI its noise schedule, e.g. karras." },
                    "loras": {
                        "type": "array",
                        "maxItems": 10,
                        "description": "Replaces the setup's LoRAs. [] removes them. Not for NovelAI.",
                        "items": {
                            "type": "object",
                            "required": ["name"],
                            "properties": {
                                "name": { "type": "string", "description": "LoRA filename from list_models." },
                                "strength": { "type": "number", "description": "Default 1." },
                                "strength_clip": { "type": "number", "description": "Default: same as strength." }
                            }
                        }
                    },
                    "transparent_background": { "type": "boolean", "description": "Ask for a see-through (RGBA) background, for logos, stickers and design assets. Only models that support it (Ming-Image Design) use it. Default: the setup's." },
                    "wait": { "type": "boolean", "description": "Default true." },
                    "include_images": { "type": "boolean", "description": "Return image previews. Default true." }
                }
            }
        }),
        json!({
            "name": "generate_music",
            "title": "Generate music",
            "description": "Generate a song with YuE2 (the MooshieUI Music page backend). Describe the style; add lyrics for vocals or leave them out for instrumental music. The FLAC (and the ABC score YuE2 planned) is saved on the MooshieUI machine and its path returned. Songs take minutes: this waits up to 10 minutes unless wait is false; then poll get_job.",
            "inputSchema": {
                "type": "object",
                "required": ["style"],
                "properties": {
                    "style": { "type": "string", "description": "Genre, instruments, mood, tempo and vocal type, e.g. \"upbeat synthpop, female vocals, 120 bpm, bright analog synths\"." },
                    "lyrics": { "type": "string", "description": "Song lyrics. Section tags such as [verse] and [chorus] on their own lines help structure. Omit for instrumental music." },
                    "duration_seconds": { "type": "number", "minimum": 1, "maximum": 360, "description": "Maximum length. Default 120." },
                    "checkpoint": { "type": "string", "description": "YuE2 checkpoint from list_models (category music). Default: the first installed." },
                    "planning": { "type": "string", "enum": ["full", "melody", "off"], "description": "full plans melody and chords as an ABC score first (default), melody plans only the melody, off skips planning." },
                    "score": { "type": "string", "description": "ABC notation score for the song to follow (needs planning full or melody)." },
                    "steps": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 32." },
                    "seed": { "type": "integer", "description": "Omit or -1 for random." },
                    "wait": { "type": "boolean", "description": "Default true." },
                    "include_audio": { "type": "boolean", "description": "Embed the FLAC in the result (up to 16 MB). Default false." }
                }
            }
        }),
        json!({
            "name": "create_comic",
            "title": "Make a comic page",
            "description": "Lay out a comic page, generate every panel and assemble the page, like the MooshieUI Comic page. Give one panels entry per panel in reading order. Each panel is generated on its own with the owner's setup for the model (NovelAI stays inside the free Opus window), then cropped into its box with gutters and black borders; panel images and the page are saved to the gallery. The cast's appearance tags, the page style and the setting are added to every panel, so a panel prompt only says what happens in it: framing, who (by count and name), pose, action, expression, background. On NovelAI each panel's dialogue is lettered in speech bubbles (a Text: block); other models draw no dialogue. For NovelAI V5 write each prompt as a tag line, a blank line, then one or two sentences of prose that name each speaker and quote their line word for word; put only the spoken words in dialogue, one string per bubble, at most two short bubbles per panel, no speaker names. mode one_image (NovelAI V4 or later, up to 6 panels) draws the whole page as one image with each panel as a positioned character prompt instead. A page takes minutes: this waits up to 10 minutes unless wait is false; then poll get_job. Use redo_comic_panels to remake panels you don't like.",
            "inputSchema": {
                "type": "object",
                "required": ["panels"],
                "properties": {
                    "layout": { "type": "string", "enum": ["full", "two_rows", "two_columns", "three_rows", "one_over_two", "two_over_one", "tall_left_two_right", "four_rows", "grid_2x2", "one_two_one", "one_one_two", "two_one_two", "one_two_two", "grid_2x3", "two_one_three"], "description": "Panel layout. The digits read top to bottom: one_two_one is 1 wide panel, 2 side by side, 1 wide (4 panels); grid_2x3 is 3 rows of 2. Default: a layout for the number of panels. Or use rows." },
                    "rows": {
                        "type": "array",
                        "maxItems": 9,
                        "description": "A custom layout instead of layout: rows top to bottom.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "height": { "type": "number", "description": "This row's share of the page height, relative to the others. Default 1." },
                                "panels": { "type": "integer", "minimum": 1, "maximum": 9, "description": "Panels side by side in this row, equal widths. Default 1." },
                                "widths": { "type": "array", "items": { "type": "number" }, "description": "Instead of panels: each panel's relative width, left to right." }
                            }
                        }
                    },
                    "panels": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 9,
                        "description": "One entry per panel, in reading order.",
                        "items": {
                            "type": "object",
                            "required": ["prompt"],
                            "properties": {
                                "prompt": { "type": "string", "description": "What this panel shows." },
                                "dialogue": { "type": "array", "maxItems": 4, "items": { "type": "string" }, "description": "Speech bubbles, the words only, exactly as lettered. NovelAI only." },
                                "cast": { "type": "array", "items": { "type": "string" }, "description": "Names from cast who are in this panel; their appearance tags are added." },
                                "characters": { "type": "array", "maxItems": 6, "items": { "type": "string" }, "description": "Saved MooshieUI character names (list_library) in this panel." },
                                "setting": { "type": "string", "description": "Replaces the page setting for this panel (a flashback, a cut away)." },
                                "negative_prompt": { "type": "string" }
                            }
                        }
                    },
                    "cast": {
                        "type": "array",
                        "maxItems": 12,
                        "description": "The page's characters, described once.",
                        "items": {
                            "type": "object",
                            "required": ["name"],
                            "properties": {
                                "name": { "type": "string" },
                                "tags": { "type": "string", "description": "Appearance tags: count, hair, eyes, outfit, e.g. 1girl, long black hair, red eyes, sailor uniform." }
                            }
                        }
                    },
                    "style": { "type": "string", "description": "Style tags for every panel, e.g. manga, monochrome, screentone." },
                    "setting": { "type": "string", "description": "Location, time and light for every panel, e.g. classroom, sunset." },
                    "page_shape": { "type": "string", "enum": ["portrait", "square", "landscape"], "description": "Default portrait (the ISO ratio of a manga page)." },
                    "right_to_left": { "type": "boolean", "description": "Manga reading order: panels within a row read right to left. Default false." },
                    "gutter": { "type": "integer", "minimum": 0, "maximum": 80, "description": "Space between panels in pixels on the 2048 px page. Default 24." },
                    "mode": { "type": "string", "enum": ["panels", "one_image"], "description": "Default panels." },
                    "model": { "type": "string", "description": "As in generate_image. Default: the most recently used one." },
                    "negative_prompt": { "type": "string", "description": "Replaces the setup's negative prompt for every panel." },
                    "styles": { "type": "array", "maxItems": 20, "items": { "type": "string" }, "description": "Saved artist style names, added to every panel." },
                    "chunks": { "type": "array", "maxItems": 30, "items": { "type": "string" }, "description": "Saved prompt chunk names, added to every panel." },
                    "use_active_styles": { "type": "boolean", "description": "Also add the styles and chunks active in MooshieUI. Default false." },
                    "seed": { "type": "integer", "description": "Panel n uses seed + n - 1. Omit for random." },
                    "steps": { "type": "integer", "minimum": 1, "maximum": 150 },
                    "cfg": { "type": "number", "minimum": 0, "maximum": 30 },
                    "sampler": { "type": "string" },
                    "scheduler": { "type": "string" },
                    "wait": { "type": "boolean", "description": "Default true." },
                    "include_images": { "type": "boolean", "description": "Return a preview of the finished page. Default true." },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168, "description": "How long the returned download links work. Default 24." }
                }
            }
        }),
        json!({
            "name": "redo_comic_panels",
            "title": "Redo comic panels",
            "description": "Make some panels of a finished create_comic page again, optionally with a new prompt or dialogue, keep the rest, and assemble a new page. Returns a new comic job.",
            "inputSchema": {
                "type": "object",
                "required": ["job_id"],
                "properties": {
                    "job_id": { "type": "string", "description": "The comic job (comic-...)." },
                    "panels": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "description": "Panel numbers to generate again as they are, with a new seed." },
                    "changes": {
                        "type": "array",
                        "description": "Panels to change and generate again.",
                        "items": {
                            "type": "object",
                            "required": ["panel"],
                            "properties": {
                                "panel": { "type": "integer", "minimum": 1 },
                                "prompt": { "type": "string" },
                                "dialogue": { "type": "array", "items": { "type": "string" } },
                                "setting": { "type": "string" },
                                "negative_prompt": { "type": "string" }
                            }
                        }
                    },
                    "seed": { "type": "integer", "description": "Omit for random." },
                    "wait": { "type": "boolean", "description": "Default true." },
                    "include_images": { "type": "boolean", "description": "Default true." },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168 }
                }
            }
        }),
        json!({
            "name": "get_job",
            "title": "Generation job",
            "description": "Status and results of a generate_image, create_comic or generate_music job.",
            "inputSchema": {
                "type": "object",
                "required": ["job_id"],
                "properties": {
                    "job_id": { "type": "string" },
                    "wait_seconds": { "type": "integer", "minimum": 0, "maximum": 600, "description": "Wait up to this long for the job to finish. Default 0." },
                    "include_images": { "type": "boolean", "description": "Image jobs: return previews. Default true." },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168, "description": "Image jobs: how long the download links work. Default 24." },
                    "include_audio": { "type": "boolean", "description": "Music jobs: embed the FLAC in the result (up to 16 MB). Default false; the file path is always returned." }
                }
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "cancel_generation",
            "title": "Cancel generation",
            "description": "Cancel a generate_image, create_comic or generate_music job, or every unfinished one when job_id is omitted. Generations started in the MooshieUI window are not touched.",
            "inputSchema": {
                "type": "object",
                "properties": { "job_id": { "type": "string" } }
            },
            "annotations": { "destructiveHint": false }
        }),
        json!({
            "name": "search_gallery",
            "title": "Search gallery",
            "description": "Find images in the MooshieUI gallery by words in their prompt, negative prompt or model name, or list the newest when query is empty. Filter by board, favourites or rating. Each result has a download link (url) for the full-size file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "board": { "type": "string", "description": "Only images on this board (list_boards). Unsorted is images on none." },
                    "favourites_only": { "type": "boolean" },
                    "min_rating": { "type": "integer", "minimum": 0, "maximum": 5, "description": "Only images rated at least this many stars." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 20." },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168, "description": "How long the download links work. Default 24." }
                }
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "get_image",
            "title": "Get gallery image",
            "description": "Look at a gallery image and read its generation settings, board, rating and a download link (url) for the full-size file.",
            "inputSchema": {
                "type": "object",
                "required": ["filename"],
                "properties": {
                    "filename": { "type": "string", "description": "Gallery filename from search_gallery or generate_image." },
                    "max_size": { "type": "integer", "minimum": 128, "maximum": 2048, "description": "Longest side of the returned preview. Default 1024." },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168, "description": "How long the download link works. Default 24." }
                }
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "list_library",
            "title": "List saved characters, styles, chunks and wildcards",
            "description": "The owner's saved characters (with the model families each is written for), artist styles, prompt chunks (named prompt snippets with an optional UC) and wildcard files. Use the names in generate_image.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["characters", "styles", "chunks", "wildcards"], "description": "Default characters." },
                    "filter": { "type": "string", "description": "Case-insensitive substring of the name." }
                }
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "get_character",
            "title": "Get saved character",
            "description": "A saved character's full prompt and UC for each model family, its memory files (notes on who they are, for writing or roleplay) and its portrait.",
            "inputSchema": {
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": { "type": "string" },
                    "architecture": { "type": "string", "description": "Which card to show first when the character has several, e.g. novelai, illustrious, flux. Default novelai." },
                    "include_portrait": { "type": "boolean", "description": "Default true." }
                }
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "edit_image",
            "title": "Edit image",
            "description": "Edit an image with an instruction using an Image Edit model (Qwen Image Edit, Flux Kontext, Flux.2 Klein, FLUX 3 Klein, Anima ReStyler). Reuses the owner's last Image Edit setup for the model (get_status lists setups with mode image_edit). The first image is edited; more images are references (Klein takes up to 10). Saved to the gallery like generate_image.",
            "inputSchema": {
                "type": "object",
                "required": ["images", "prompt"],
                "properties": {
                    "images": { "type": "array", "minItems": 1, "maxItems": 10, "items": { "type": "string" }, "description": "Gallery filenames or base64 images / data URLs." },
                    "prompt": { "type": "string", "description": "The edit instruction, e.g. \"make her hair blue\"." },
                    "negative_prompt": { "type": "string" },
                    "model": { "type": "string", "description": "Edit model with a saved Image Edit setup. Default: the most recent one." },
                    "width": { "type": "integer", "minimum": 256, "maximum": 4096 },
                    "height": { "type": "integer", "minimum": 256, "maximum": 4096 },
                    "steps": { "type": "integer", "minimum": 1, "maximum": 150 },
                    "cfg": { "type": "number", "minimum": 0, "maximum": 30 },
                    "seed": { "type": "integer" },
                    "wait": { "type": "boolean", "description": "Default true." },
                    "include_images": { "type": "boolean", "description": "Default true." },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168 }
                }
            }
        }),
        json!({
            "name": "upscale_image",
            "title": "Upscale image",
            "description": "Upscale a gallery image (or a base64 image) and refine it with a local model, the way MooshieUI's Refine does. Uses the setup of the model that made the image when it has one, else the given or most recent local setup, and the image's own prompt to guide the refine.",
            "inputSchema": {
                "type": "object",
                "required": ["image"],
                "properties": {
                    "image": { "type": "string", "description": "Gallery filename, or base64 / data URL." },
                    "scale": { "type": "number", "minimum": 1, "maximum": 4, "description": "Default 2." },
                    "denoise": { "type": "number", "minimum": 0.05, "maximum": 1, "description": "Refine strength. Default: the setup's upscale denoise." },
                    "upscale_model": { "type": "string", "description": "Upscale model file (list_models category upscale_models). Default: the setup's." },
                    "model": { "type": "string", "description": "Local model setup to refine with." },
                    "prompt": { "type": "string", "description": "Replaces the image's own prompt for the refine pass." },
                    "seed": { "type": "integer" },
                    "wait": { "type": "boolean", "description": "Default true." },
                    "include_images": { "type": "boolean", "description": "Default true." },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168 }
                }
            }
        }),
        json!({
            "name": "list_boards",
            "title": "List gallery boards",
            "description": "The owner's gallery boards with image counts, and how many images are favourited or rated.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "search_eagle",
            "title": "Search Eagle library",
            "description": "Search the owner's Eagle (eagle.cool) library, which must be open on the MooshieUI machine. Returns item ids, names, tags, notes and download links. list_folders: true lists the folders instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Keyword matched against names, tags and notes." },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "folder": { "type": "string", "description": "Folder name or id." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 20." },
                    "offset": { "type": "integer", "minimum": 0 },
                    "list_folders": { "type": "boolean" },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168 }
                }
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "get_eagle_image",
            "title": "Get Eagle image",
            "description": "Look at an Eagle item (preview up to 2048 px), with its tags, notes and a download link for the original.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id": { "type": "string", "description": "Item id from search_eagle." },
                    "max_size": { "type": "integer", "minimum": 128, "maximum": 2048, "description": "Default 1024." },
                    "link_hours": { "type": "integer", "minimum": 1, "maximum": 168 }
                }
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "send_to_eagle",
            "title": "Send to Eagle",
            "description": "Add gallery images to the owner's Eagle library, with prompt tags and the prompt, negative and seed as the note. Goes to the folder set in MooshieUI's Settings unless folder is given.",
            "inputSchema": {
                "type": "object",
                "required": ["filenames"],
                "properties": {
                    "filenames": { "type": "array", "minItems": 1, "maxItems": 100, "items": { "type": "string" } },
                    "folder": { "type": "string", "description": "Eagle folder name or id." }
                }
            }
        }),
    ])
}

fn text(value: Value) -> Value {
    json!({ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() })
}

fn ok(content: Vec<Value>) -> Value {
    json!({ "content": content, "isError": false })
}

fn error(message: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": message.into() }], "isError": true })
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// How a request reached the server.
#[derive(Debug, Clone)]
pub struct CallContext {
    /// Waiting tools give up after this and return the job id.
    pub max_wait: Duration,
    /// Came through the tunnel or the owner's own address, so links must use
    /// the public address.
    pub proxied: bool,
}

/// Run a tool. `None` for an unknown tool name.
pub async fn call(
    state: &Arc<AppState>,
    name: &str,
    args: &Value,
    ctx: &CallContext,
) -> Option<Value> {
    let max_wait = ctx.max_wait;
    let result = match name {
        "get_status" => get_status(state).await,
        "list_models" => list_models(state, args).await,
        "generate_image" => generate_image(state, args, ctx).await,
        "edit_image" => edit_image(state, args, ctx).await,
        "upscale_image" => upscale_image(state, args, ctx).await,
        "generate_music" => generate_music(state, args, max_wait).await,
        "create_comic" => create_comic(state, args, ctx).await,
        "redo_comic_panels" => redo_comic_panels(state, args, ctx).await,
        "get_job" => get_job(args, ctx).await,
        "cancel_generation" => cancel_generation(state, args).await,
        "search_gallery" => search_gallery(args, ctx).await,
        "get_image" => get_image(args, ctx).await,
        "list_boards" => list_boards().await,
        "list_library" => list_library(args).await,
        "get_character" => get_character(args).await,
        "search_eagle" => search_eagle(state, args, ctx).await,
        "get_eagle_image" => get_eagle_image(state, args, ctx).await,
        "send_to_eagle" => send_to_eagle(state, args).await,
        _ => return None,
    };
    Some(result.unwrap_or_else(error))
}

fn setup_summary(s: &setups::Setup) -> Value {
    let p = &s.params;
    json!({
        "model": s.model,
        "family": p.model_architecture,
        "width": p.width,
        "height": p.height,
        "steps": p.steps,
        "cfg": p.cfg,
        "sampler": p.sampler_name,
        "scheduler": p.scheduler,
        "negative_prompt": p.negative_prompt,
        "loras": p.loras.iter().map(|l| json!({ "name": l.name, "strength": l.strength_model })).collect::<Vec<_>>(),
        "upscale": p.upscale_enabled.then(|| json!({ "scale": p.upscale_scale, "model": p.upscale_model })),
        "face_fix": p.facefix_enabled,
    })
}

async fn get_status(state: &Arc<AppState>) -> Result<Value, String> {
    let comfyui = match state.get_system_stats_info().await {
        Ok(stats) => json!({
            "running": true,
            "version": stats.system.comfyui_version,
            "gpus": stats.devices.iter().map(|d| json!({
                "name": d.name,
                "vram_total_gb": (d.vram_total as f64 / 1e9 * 10.0).round() / 10.0,
                "vram_free_gb": (d.vram_free as f64 / 1e9 * 10.0).round() / 10.0,
            })).collect::<Vec<_>>(),
        }),
        Err(e) => json!({ "running": false, "error": e.to_string() }),
    };
    let setups: Vec<Value> = setups::list().iter().map(setup_summary).collect();
    let recent: Vec<Value> = jobs::recent(5)
        .iter()
        .map(|j| {
            json!({
                "job_id": j.job_id,
                "status": j.status,
                "prompt": j.prompt.chars().take(120).collect::<String>(),
                "images": j.images.iter().map(|i| &i.filename).collect::<Vec<_>>(),
                "error": j.error,
            })
        })
        .collect();
    let recent_music: Vec<Value> = music::recent(5)
        .iter()
        .map(|j| {
            json!({
                "job_id": j.job_id,
                "status": j.status,
                "style": j.style.chars().take(120).collect::<String>(),
                "file": j.file,
                "error": j.error,
            })
        })
        .collect();
    let recent_comics: Vec<Value> = comic::recent(5)
        .iter()
        .map(|j| {
            json!({
                "job_id": j.job_id,
                "status": j.status,
                "panels": j.panels.len(),
                "page": j.page.as_ref().map(|p| &p.filename),
                "error": j.error,
            })
        })
        .collect();
    Ok(ok(vec![text(json!({
        "mooshieui_version": env!("CARGO_PKG_VERSION"),
        "comfyui": comfyui,
        "queue_length": state.prompt_queue.len(),
        "recent_jobs": recent,
        "recent_music_jobs": recent_music,
        "recent_comic_jobs": recent_comics,
        "setups": setups,
        "note": if setups.is_empty() {
            "No saved setups yet: generate one txt2img image in MooshieUI with a model to make it usable here."
        } else {
            "generate_image can use any model in setups."
        },
    }))]))
}

async fn list_models(state: &Arc<AppState>, args: &Value) -> Result<Value, String> {
    let category = arg_str(args, "category").unwrap_or("checkpoints");
    if category == "music" {
        return list_music_models(state).await;
    }
    if !matches!(
        category,
        "checkpoints" | "diffusion_models" | "loras" | "upscale_models"
    ) {
        return Err(
            "category must be checkpoints, diffusion_models, loras, upscale_models or music".into(),
        );
    }
    let filter = arg_str(args, "filter").map(str::to_ascii_lowercase);
    let limit = args["limit"].as_u64().unwrap_or(200).clamp(1, 1000) as usize;
    let all = state
        .get_models_list(category)
        .await
        .map_err(|e| format!("Could not list models (is ComfyUI running?): {e}"))?;
    let with_setup: HashSet<String> = setups::list().into_iter().map(|s| s.model).collect();
    let matching: Vec<&String> = all
        .iter()
        .filter(|m| {
            filter
                .as_deref()
                .is_none_or(|f| m.to_ascii_lowercase().contains(f))
        })
        .collect();
    let models: Vec<Value> = matching
        .iter()
        .take(limit)
        .map(|m| {
            if matches!(category, "loras" | "upscale_models") {
                json!({ "name": m })
            } else {
                json!({ "name": m, "has_setup": with_setup.contains(m.as_str()) })
            }
        })
        .collect();
    Ok(ok(vec![text(json!({
        "category": category,
        "total": matching.len(),
        "models": models,
    }))]))
}

/// A seed for prompt expansion and the job: the agent's, or a fresh one
/// (NovelAI seeds are 32-bit).
fn resolve_seed(params: &GenerationParams) -> i64 {
    if params.novelai.is_some() {
        crate::novelai::resolve_seed(params.seed)
    } else if params.seed < 0 {
        (rand::random::<u64>() >> 1) as i64
    } else {
        params.seed
    }
}

/// Add saved characters, styles and chunks, then expand `__wildcards__` and
/// `{a|b}` in every prompt, with the job's seed so it can be reproduced.
/// Returns what was added and any wildcard names that matched no file.
pub(super) async fn finish_prompt(
    params: &mut GenerationParams,
    args: &Value,
) -> Result<(Vec<String>, Vec<String>), String> {
    params.seed = resolve_seed(params);
    let seed = params.seed as u32;
    let add = library::Additions::from_args(args)?;
    let used = if add.is_empty() {
        Vec::new()
    } else {
        let lib = library::Library::load().await;
        library::apply(params, &lib, &add, seed)?
    };
    let nai_texts = params.novelai.as_ref().map(|n| {
        n.characters
            .iter()
            .flat_map(|c| [c.prompt.as_str(), c.negative_prompt.as_str()])
            .any(expand::needs_expansion)
    });
    let mut unknown = Vec::new();
    if expand::needs_expansion(&params.positive_prompt)
        || expand::needs_expansion(&params.negative_prompt)
        || nai_texts == Some(true)
    {
        let table = tokio::task::spawn_blocking(expand::load_table)
            .await
            .map_err(|e| e.to_string())?;
        params.positive_prompt =
            expand::expand_prompt(&params.positive_prompt, &table, seed, 0, &mut unknown);
        params.negative_prompt =
            expand::expand_prompt(&params.negative_prompt, &table, seed, 1, &mut unknown);
        if let Some(nai) = params.novelai.as_mut() {
            for (i, c) in nai.characters.iter_mut().enumerate() {
                let salt = 101 + (i as u32) * 2;
                c.prompt = expand::expand_prompt(&c.prompt, &table, seed, salt, &mut unknown);
                c.negative_prompt =
                    expand::expand_prompt(&c.negative_prompt, &table, seed, salt + 1, &mut unknown);
            }
        }
    }
    Ok((used, unknown))
}

/// Start a job and answer the way every generating tool does.
async fn run_job(
    state: &Arc<AppState>,
    params: GenerationParams,
    args: &Value,
    ctx: &CallContext,
    added: Vec<String>,
    unknown: Vec<String>,
) -> Result<Value, String> {
    let rx = jobs::start(state.clone(), params);
    let job = if args["wait"].as_bool() == Some(false) {
        rx.borrow().clone()
    } else {
        jobs::wait(rx, ctx.max_wait).await
    };
    let mut result = job_result(&job, args, ctx).await?;
    let mut notes = serde_json::Map::new();
    if !added.is_empty() {
        notes.insert("added".into(), json!(added));
    }
    if !unknown.is_empty() {
        notes.insert(
            "unknown_wildcards".into(),
            json!(unknown
                .iter()
                .map(|n| format!("__{n}__"))
                .collect::<Vec<_>>()),
        );
    }
    if !notes.is_empty() {
        if let Some(content) = result["content"].as_array_mut() {
            content.push(text(Value::Object(notes)));
        }
    }
    Ok(result)
}

/// `denoise` from the arguments, checked.
fn arg_denoise(args: &Value, default: f64) -> Result<f64, String> {
    let d = args["denoise"].as_f64().unwrap_or(default);
    if !(0.05..=1.0).contains(&d) {
        return Err("denoise must be between 0.05 and 1".into());
    }
    Ok(d)
}

/// Images listed under `key`: gallery file names or base64 / data URLs.
fn arg_images(args: &Value, key: &str, max: usize) -> Result<Vec<String>, String> {
    let list: Vec<String> = match &args[key] {
        Value::Null => Vec::new(),
        Value::String(s) => vec![s.clone()],
        Value::Array(items) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{key} must be gallery file names or base64 images"))
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(format!("{key} must be a list")),
    };
    if list.len() > max {
        return Err(format!("{key} takes {max} images at most"));
    }
    Ok(list)
}

/// Turn a txt2img request into img2img on `source_image`, sized like the
/// source unless the agent gave a size.
async fn apply_source_image(
    state: &Arc<AppState>,
    params: &mut GenerationParams,
    args: &Value,
    spec: &str,
) -> Result<(), String> {
    let src = inputs::load(spec).await?;
    let nai = params.novelai.is_some();
    if args["width"].is_null() && args["height"].is_null() {
        let area = u64::from(params.width) * u64::from(params.height);
        let (w, h) = inputs::fit_size(src.width(), src.height(), area, if nai { 64 } else { 16 });
        params.width = w;
        params.height = h;
    }
    let denoise = arg_denoise(args, 0.6)?;
    if nai {
        setups::check_novelai_free_window(params)?;
        let data = inputs::base64_png_at(&src, params.width, params.height).await?;
        let block = params.novelai.as_mut().expect("checked above");
        block.action = "img2img".into();
        block.strength = denoise;
        params.mode = "img2img".into();
        params.input_image = Some(data);
    } else {
        params.mode = "img2img".into();
        params.denoise = denoise;
        params.input_image = Some(inputs::upload(state, &src).await?);
    }
    Ok(())
}

async fn generate_image(
    state: &Arc<AppState>,
    args: &Value,
    ctx: &CallContext,
) -> Result<Value, String> {
    let overrides: setups::Overrides =
        serde_json::from_value(args.clone()).map_err(|e| format!("Invalid arguments: {e}"))?;
    let setup = setups::find(
        &setups::list(),
        arg_str(args, "model"),
        setups::Purpose::Generate,
    )?;
    let mut params = setups::apply(&setup.params, &overrides)?;
    if let Some(spec) = arg_str(args, "source_image") {
        apply_source_image(state, &mut params, args, spec).await?;
    }
    let refs = arg_images(args, "reference_images", MAX_SNAPREF_IMAGES)?;
    if !refs.is_empty() {
        if params.novelai.is_some() {
            return Err("reference_images work on local models only (NovelAI references cost Anlas). Use characters for a saved NovelAI character.".into());
        }
        params.style_ref_enabled = true;
        let snapref = crate::templates::style_ref::snapref_active(&params);
        if !snapref && refs.len() > 2 {
            return Err("This setup's style reference takes one or two images (up to 8 only when the owner uses SnapRef on Anima).".into());
        }
        let mut names = Vec::new();
        for spec in &refs {
            names.push(inputs::upload(state, &inputs::load(spec).await?).await?);
        }
        if snapref {
            params.snapref_references = names
                .into_iter()
                .map(|name| crate::comfyui::types::SnapRefReference {
                    image: Some(name),
                    strength: 1.0,
                })
                .collect();
        } else {
            params.style_ref_image = names.first().cloned();
            params.style_ref_image_2 = names.get(1).cloned();
        }
    }
    let (added, unknown) = finish_prompt(&mut params, args).await?;
    run_job(state, params, args, ctx, added, unknown).await
}

async fn edit_image(
    state: &Arc<AppState>,
    args: &Value,
    ctx: &CallContext,
) -> Result<Value, String> {
    let images = arg_images(args, "images", 10)?;
    if images.is_empty() {
        return Err("images is required: the picture to edit first, then any references".into());
    }
    let mut overrides: setups::Overrides =
        serde_json::from_value(args.clone()).map_err(|e| format!("Invalid arguments: {e}"))?;
    if overrides.prompt.trim().is_empty() {
        overrides.prompt = arg_str(args, "instruction").unwrap_or("").to_string();
    }
    let setup = setups::find(
        &setups::list(),
        arg_str(args, "model"),
        setups::Purpose::Edit,
    )?;
    let mut params = setups::apply(&setup.params, &overrides)?;
    let mut uploaded = Vec::new();
    let mut first_size = None;
    for spec in &images {
        let src = inputs::load(spec).await?;
        first_size.get_or_insert((src.width(), src.height()));
        uploaded.push(inputs::upload(state, &src).await?);
    }
    if let (Some((w, h)), true) = (
        first_size,
        args["width"].is_null() && args["height"].is_null(),
    ) {
        let area = u64::from(params.width) * u64::from(params.height);
        (params.width, params.height) = inputs::fit_size(w, h, area, 16);
    }
    params.edit_reference_images = uploaded;
    let (added, unknown) = finish_prompt(&mut params, args).await?;
    run_job(state, params, args, ctx, added, unknown).await
}

async fn upscale_image(
    state: &Arc<AppState>,
    args: &Value,
    ctx: &CallContext,
) -> Result<Value, String> {
    let spec = arg_str(args, "image").ok_or("image is required")?;
    let src = inputs::load(spec).await?;
    let all = setups::list();
    // The model that made the image, when it has a local setup; else the
    // one asked for, else the most recent local one.
    let local: Vec<setups::Setup> = all
        .iter()
        .filter(|s| !setups::is_novelai(&s.params))
        .cloned()
        .collect();
    let made_with = src.metadata.get("model").map(String::as_str);
    let setup = match arg_str(args, "model") {
        Some(m) => setups::find(&local, Some(m), setups::Purpose::Generate)?,
        None => made_with
            .and_then(|m| setups::find(&local, Some(m), setups::Purpose::Generate).ok())
            .map_or_else(|| setups::find(&local, None, setups::Purpose::Generate), Ok)?,
    };
    let scale = args["scale"].as_f64().unwrap_or(2.0);
    if !(1.0..=4.0).contains(&scale) {
        return Err("scale must be between 1 and 4".into());
    }
    let (w, h) = (src.width(), src.height());
    if (f64::from(w.max(h)) * scale) > 8192.0 {
        return Err(format!("{w}x{h} at {scale}x is over 8192 px on a side."));
    }
    let mut params = setup.params.clone();
    params.mode = "img2img".into();
    params.refine_only = true;
    params.upscale_enabled = true;
    params.upscale_scale = scale;
    params.denoise = 1.0;
    if !args["denoise"].is_null() {
        params.upscale_denoise = arg_denoise(args, params.upscale_denoise)?;
    }
    if let Some(m) = arg_str(args, "upscale_model") {
        params.upscale_method = "model".into();
        params.upscale_model = Some(m.to_string());
    }
    params.width = w;
    params.height = h;
    params.batch_size = 1;
    params.seed = args["seed"].as_i64().filter(|s| *s >= 0).unwrap_or(-1);
    // The refine pass is guided by the image's own prompt when it has one.
    params.positive_prompt = arg_str(args, "prompt")
        .map(str::to_string)
        .or_else(|| src.metadata.get("positive_prompt").cloned())
        .unwrap_or_default();
    if let Some(neg) = src.metadata.get("negative_prompt") {
        params.negative_prompt = neg.clone();
    }
    params.input_image = Some(inputs::upload(state, &src).await?);
    params.seed = resolve_seed(&params);
    run_job(state, params, args, ctx, Vec::new(), Vec::new()).await
}

async fn create_comic(
    state: &Arc<AppState>,
    args: &Value,
    ctx: &CallContext,
) -> Result<Value, String> {
    let plan = comic::plan(args)?;
    let rx = comic::start(state.clone(), plan, HashMap::new());
    comic_answer(rx, args, ctx).await
}

async fn redo_comic_panels(
    state: &Arc<AppState>,
    args: &Value,
    ctx: &CallContext,
) -> Result<Value, String> {
    let (plan, kept) = comic::redo(args)?;
    let rx = comic::start(state.clone(), plan, kept);
    comic_answer(rx, args, ctx).await
}

async fn comic_answer(
    rx: tokio::sync::watch::Receiver<comic::ComicJob>,
    args: &Value,
    ctx: &CallContext,
) -> Result<Value, String> {
    let job = if args["wait"].as_bool() == Some(false) {
        rx.borrow().clone()
    } else {
        jobs::wait(rx, ctx.max_wait).await
    };
    comic_result(&job, args, ctx).await
}

async fn comic_result(
    job: &comic::ComicJob,
    args: &Value,
    ctx: &CallContext,
) -> Result<Value, String> {
    let hours = link_hours(args);
    let mut v = serde_json::to_value(job).unwrap_or_default();
    if !job.status.finished() {
        let done = job.panels.iter().filter(|p| p.image.is_some()).count();
        v["hint"] = json!(format!(
            "Still working ({done} of {} panels done). Call get_job with this job_id (wait_seconds up to 600).",
            job.panels.len()
        ));
    }
    if let Some(panels) = v["panels"].as_array_mut() {
        for p in panels.iter_mut() {
            if let Some(url) = p["image"]
                .as_str()
                .and_then(|f| link(ctx, links::Kind::Gallery, f, hours))
            {
                p["url"] = json!(url);
            }
        }
    }
    if let Some(url) = job
        .page
        .as_ref()
        .and_then(|p| link(ctx, links::Kind::Gallery, &p.filename, hours))
    {
        v["page"]["url"] = json!(url);
    }
    let mut content = vec![text(v)];
    if args["include_images"].as_bool().unwrap_or(true) && job.status == jobs::JobStatus::Done {
        if let Some(page) = &job.page {
            match preview(&page.filename, 1024).await {
                Ok((data, _, _)) => content.push(image_content(data)),
                Err(e) => log::warn!("[mcp] preview of {} failed: {e}", page.filename),
            }
        }
    }
    Ok(json!({
        "content": content,
        "isError": job.status == jobs::JobStatus::Failed,
    }))
}

async fn list_music_models(state: &Arc<AppState>) -> Result<Value, String> {
    let caps = crate::commands::music::capabilities(state)
        .await
        .map_err(|e| format!("Could not check music support (is ComfyUI running?): {e}"))?;
    let note = if !caps.missing_nodes.is_empty() {
        format!(
            "YuE2 nodes are missing from ComfyUI ({}). Update the managed runtime from MooshieUI's Music page.",
            caps.missing_nodes.join(", ")
        )
    } else if caps.checkpoints.is_empty() {
        "No YuE2 checkpoint is installed. Download one from MooshieUI's Music page.".into()
    } else {
        "generate_music can use any of these.".into()
    };
    Ok(ok(vec![text(json!({
        "category": "music",
        "available": caps.missing_nodes.is_empty() && !caps.checkpoints.is_empty(),
        "models": caps.checkpoints.iter().map(|m| json!({ "name": m })).collect::<Vec<_>>(),
        "note": note,
    }))]))
}

async fn generate_music(
    state: &Arc<AppState>,
    args: &Value,
    max_wait: Duration,
) -> Result<Value, String> {
    let req: music::MusicRequest =
        serde_json::from_value(args.clone()).map_err(|e| format!("Invalid arguments: {e}"))?;
    if req.style.trim().is_empty() {
        return Err("style is required".into());
    }
    let rx = music::start(state.clone(), req);
    if args["wait"].as_bool() == Some(false) {
        let job = rx.borrow().clone();
        return Ok(ok(vec![text(music_job_json(&job))]));
    }
    let job = jobs::wait(rx, max_wait).await;
    music_job_result(&job, args["include_audio"].as_bool().unwrap_or(false)).await
}

fn music_job_json(job: &MusicJob) -> Value {
    let mut v = serde_json::to_value(job).unwrap_or_default();
    if !job.status.finished() {
        v["hint"] = json!("Still working. Call get_job with this job_id (wait_seconds up to 600).");
    }
    v
}

async fn music_job_result(job: &MusicJob, include_audio: bool) -> Result<Value, String> {
    let mut content = vec![text(music_job_json(job))];
    if let (true, Some(file)) = (
        include_audio && job.status == jobs::JobStatus::Done,
        &job.file,
    ) {
        match tokio::fs::metadata(file).await {
            Ok(m) if m.len() <= MAX_INLINE_AUDIO => {
                let bytes = tokio::fs::read(file).await.map_err(|e| e.to_string())?;
                content.push(json!({
                    "type": "audio",
                    "mimeType": "audio/flac",
                    "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                }));
            }
            Ok(_) => content.push(json!({
                "type": "text",
                "text": "The song is over 16 MB, so it isn't embedded. Use the file path.",
            })),
            Err(e) => log::warn!("[mcp] reading {file} failed: {e}"),
        }
    }
    Ok(json!({
        "content": content,
        "isError": job.status == jobs::JobStatus::Failed,
    }))
}

fn job_json(job: &Job) -> Value {
    let mut v = serde_json::to_value(job).unwrap_or_default();
    if !job.status.finished() {
        v["hint"] = json!("Still working. Call get_job with this job_id (wait_seconds up to 600).");
    }
    v
}

/// How long links made for this call last.
fn link_hours(args: &Value) -> u64 {
    args["link_hours"]
        .as_u64()
        .unwrap_or(links::DEFAULT_HOURS)
        .clamp(1, links::MAX_HOURS)
}

/// A download link for a gallery or Eagle file, when one can be made.
fn link(ctx: &CallContext, kind: links::Kind, name: &str, hours: u64) -> Option<String> {
    links::base_url(ctx.proxied).and_then(|base| links::make(&base, kind, name, hours))
}

async fn job_result(job: &Job, args: &Value, ctx: &CallContext) -> Result<Value, String> {
    let include_images = args["include_images"].as_bool().unwrap_or(true);
    let mut v = job_json(job);
    let hours = link_hours(args);
    if let Some(images) = v["images"].as_array_mut() {
        for img in images.iter_mut() {
            if let Some(url) = img["filename"]
                .as_str()
                .and_then(|f| link(ctx, links::Kind::Gallery, f, hours))
            {
                img["url"] = json!(url);
            }
        }
    }
    let mut content = vec![text(v)];
    if include_images && job.status == jobs::JobStatus::Done {
        for img in &job.images {
            match preview(&img.filename, PREVIEW_SIZE).await {
                Ok((data, _, _)) => content.push(image_content(data)),
                Err(e) => log::warn!("[mcp] preview of {} failed: {e}", img.filename),
            }
        }
    }
    Ok(json!({
        "content": content,
        "isError": job.status == jobs::JobStatus::Failed,
    }))
}

async fn get_job(args: &Value, ctx: &CallContext) -> Result<Value, String> {
    let id = arg_str(args, "job_id").ok_or("job_id is required")?;
    let wait = Duration::from_secs(args["wait_seconds"].as_u64().unwrap_or(0)).min(ctx.max_wait);
    if comic::is_comic_job(id) {
        let rx = comic::get(id)
            .ok_or_else(|| format!("No job '{id}' (jobs are forgotten when MooshieUI restarts)"))?;
        let job = jobs::wait(rx, wait).await;
        return comic_result(&job, args, ctx).await;
    }
    if music::is_music_job(id) {
        let rx = music::get(id)
            .ok_or_else(|| format!("No job '{id}' (jobs are forgotten when MooshieUI restarts)"))?;
        let job = jobs::wait(rx, wait).await;
        return music_job_result(&job, args["include_audio"].as_bool().unwrap_or(false)).await;
    }
    let rx = jobs::get(id)
        .ok_or_else(|| format!("No job '{id}' (jobs are forgotten when MooshieUI restarts)"))?;
    let job = jobs::wait(rx, wait).await;
    job_result(&job, args, ctx).await
}

async fn cancel_generation(state: &Arc<AppState>, args: &Value) -> Result<Value, String> {
    let ids: Vec<String> = match arg_str(args, "job_id") {
        Some(id) => vec![id.to_string()],
        None => jobs::active()
            .into_iter()
            .map(|j| j.job_id)
            .chain(music::active().into_iter().map(|j| j.job_id))
            .chain(comic::active().into_iter().map(|j| j.job_id))
            .collect(),
    };
    let mut cancelled = Vec::new();
    for id in ids {
        let (job_id, status) = if comic::is_comic_job(&id) {
            let job = comic::cancel(state, &id).await?;
            (job.job_id, job.status)
        } else if music::is_music_job(&id) {
            let job = music::cancel(state, &id).await?;
            (job.job_id, job.status)
        } else {
            let job = jobs::cancel(state, &id).await?;
            (job.job_id, job.status)
        };
        cancelled.push(json!({ "job_id": job_id, "status": status }));
    }
    Ok(ok(vec![text(json!({ "jobs": cancelled }))]))
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max).collect::<String>())
    }
}

async fn list_library(args: &Value) -> Result<Value, String> {
    let kind = arg_str(args, "kind").unwrap_or("characters");
    let filter = arg_str(args, "filter").map(str::to_lowercase);
    let keep = |name: &str| {
        filter
            .as_deref()
            .is_none_or(|f| name.to_lowercase().contains(f))
    };
    if kind == "wildcards" {
        let table = tokio::task::spawn_blocking(expand::load_table)
            .await
            .map_err(|e| e.to_string())?;
        let items: Vec<Value> = table
            .iter()
            .filter(|(name, _)| keep(name))
            .take(500)
            .map(|(name, options)| {
                json!({
                    "token": format!("__{name}__"),
                    "options": options.len(),
                    "examples": options.iter().take(3).map(|o| clip(o, 60)).collect::<Vec<_>>(),
                })
            })
            .collect();
        return Ok(ok(vec![text(json!({
            "kind": "wildcards",
            "total": table.len(),
            "wildcards": items,
            "note": "Write a token in any prompt; it is replaced by one random option.",
        }))]));
    }
    let lib = library::Library::load().await;
    let value = match kind {
        "characters" => {
            let mut by_name: Vec<(String, Vec<&library::Character>)> = Vec::new();
            for c in &lib.characters {
                if !keep(&c.name) {
                    continue;
                }
                match by_name
                    .iter_mut()
                    .find(|(n, _)| n.eq_ignore_ascii_case(c.name.trim()))
                {
                    Some((_, cards)) => cards.push(c),
                    None => by_name.push((c.name.trim().to_string(), vec![c])),
                }
            }
            json!({
                "kind": "characters",
                "characters": by_name.iter().map(|(name, cards)| json!({
                    "name": name,
                    "architectures": cards.iter().map(|c| c.architecture.as_str()).collect::<Vec<_>>(),
                    "prompt": clip(&cards[0].prompt, 300),
                    "has_portrait": cards.iter().any(|c| c.reference.is_some() || c.thumbnail.is_some()),
                    "memory_files": cards.iter().flat_map(|c| c.memory.iter().map(|m| m.name.as_str())).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "note": "Pass names in generate_image's characters. get_character shows the full card, memory and portrait.",
            })
        }
        "styles" => json!({
            "kind": "styles",
            "styles": lib.styles.iter().filter(|s| keep(&s.name)).map(|s| json!({
                "name": s.name,
                "artists": library::style_fragment(&[s], false),
                "uc": s.negative,
                "active": lib.active_style_ids.contains(&s.id),
            })).collect::<Vec<_>>(),
            "note": "Pass names in generate_image's styles, or use_active_styles: true for the ones active in MooshieUI.",
        }),
        "chunks" => json!({
            "kind": "chunks",
            "chunks": lib.chunks.iter().filter(|c| keep(&c.name)).map(|c| {
                let mode = lib.active_chunks.iter().find(|(id, _)| id == &c.id).map(|(_, m)| match m {
                    library::ChunkMode::Prepend => "prepend",
                    library::ChunkMode::Append => "append",
                    library::ChunkMode::Random => "random",
                });
                json!({
                    "name": c.name,
                    "content": clip(&c.content, 300),
                    "uc": c.negative,
                    "active_mode": mode,
                })
            }).collect::<Vec<_>>(),
            "note": "Pass names (or { name, mode }) in generate_image's chunks. mode random picks one line.",
        }),
        other => {
            return Err(format!(
                "kind must be characters, styles, chunks or wildcards, not '{other}'"
            ))
        }
    };
    Ok(ok(vec![text(value)]))
}

/// Most memory text one get_character returns.
const MAX_MEMORY_CHARS: usize = 24_000;

/// An image content block from a `data:image/...;base64,` URL.
fn data_url_content(url: &str) -> Option<Value> {
    let rest = url.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    mime.starts_with("image/")
        .then(|| json!({ "type": "image", "mimeType": mime, "data": data }))
}

async fn get_character(args: &Value) -> Result<Value, String> {
    let name = arg_str(args, "name").ok_or("name is required")?;
    let lib = library::Library::load().await;
    let arch = arg_str(args, "architecture").unwrap_or(library::NOVELAI_ARCHITECTURE);
    let card = library::find_character(&lib, name, arch)?;
    let cards: Vec<&library::Character> = lib
        .characters
        .iter()
        .filter(|c| c.name.trim().eq_ignore_ascii_case(card.name.trim()))
        .collect();
    // Variants share their parent's memory.
    let memory_owner = card
        .parent_id
        .as_deref()
        .and_then(|id| lib.characters.iter().find(|c| c.id == id))
        .unwrap_or(card);
    let mut budget = MAX_MEMORY_CHARS;
    let memory: Vec<Value> = memory_owner
        .memory
        .iter()
        .map(|m| {
            let content = clip(&m.content, budget);
            budget = budget.saturating_sub(content.chars().count());
            json!({ "name": m.name, "content": content })
        })
        .collect();
    let mut content = vec![text(json!({
        "name": card.name,
        "cards": cards.iter().map(|c| json!({
            "architecture": c.architecture,
            "prompt": c.prompt,
            "uc": c.negative,
            "expression": c.expression,
        })).collect::<Vec<_>>(),
        "memory": memory,
    }))];
    if args["include_portrait"].as_bool().unwrap_or(true) {
        if let Some(img) = cards
            .iter()
            .find_map(|c| c.reference.as_deref().or(c.thumbnail.as_deref()))
            .and_then(data_url_content)
        {
            content.push(img);
        }
    }
    Ok(ok(content))
}

fn image_content(jpeg: Vec<u8>) -> Value {
    json!({
        "type": "image",
        "mimeType": "image/jpeg",
        "data": base64::engine::general_purpose::STANDARD.encode(jpeg),
    })
}

pub(super) fn gallery_dir() -> Result<PathBuf, String> {
    crate::config::gallery_dir().ok_or_else(|| "Cannot find gallery directory".to_string())
}

/// A gallery image file directly in `dir` (the owner's gallery), or an error.
pub(super) fn gallery_image_path(dir: &Path, filename: &str) -> Result<PathBuf, String> {
    crate::commands::api::validate_gallery_filename(filename)
        .map_err(|_| format!("Invalid filename '{filename}'"))?;
    if !is_image_file(filename) {
        return Err(format!("'{filename}' is not a gallery image"));
    }
    let path = dir.join(filename);
    if !path.is_file() {
        return Err(format!("No gallery image '{filename}'"));
    }
    Ok(path)
}

fn is_image_file(name: &str) -> bool {
    crate::commands::api::is_listable_gallery_file(name)
        && !name.to_ascii_lowercase().ends_with(".mp4")
}

/// A JPEG of `img` no larger than `max_size`, plus the original size.
fn jpeg_preview(img: image::DynamicImage, max_size: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let (w, h) = (img.width(), img.height());
    let thumb = if w.max(h) > max_size {
        img.thumbnail(max_size, max_size)
    } else {
        img
    };
    let rgb = image::DynamicImage::ImageRgb8(thumb.to_rgb8());
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode_image(&rgb)
        .map_err(|e| format!("JPEG encode failed: {e}"))?;
    Ok((out, w, h))
}

/// A JPEG preview of a gallery image, plus the original size.
async fn preview(filename: &str, max_size: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let path = gallery_image_path(&gallery_dir()?, filename)?;
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let img = crate::commands::api::decode_gallery_image(&bytes)?;
        jpeg_preview(img, max_size)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Metadata keys worth showing an agent, in the order shown.
const METADATA_KEYS: &[&str] = &[
    "positive_prompt",
    "negative_prompt",
    "model",
    "seed",
    "steps",
    "cfg",
    "sampler",
    "scheduler",
    "size",
    "loras",
    "denoise",
    "mode",
    "upscale_model",
    "upscale_scale",
    "date",
];

async fn get_image(args: &Value, ctx: &CallContext) -> Result<Value, String> {
    let filename = arg_str(args, "filename").ok_or("filename is required")?;
    let max_size = args["max_size"].as_u64().unwrap_or(1024).clamp(128, 2048) as u32;
    let (jpeg, width, height) = preview(filename, max_size).await?;
    let path = gallery_image_path(&gallery_dir()?, filename)?;
    let metadata = tokio::task::spawn_blocking(move || {
        std::fs::read(&path)
            .ok()
            .and_then(|b| crate::metadata::read_image_metadata(&b).ok().flatten())
    })
    .await
    .map_err(|e| e.to_string())?
    .unwrap_or_default();
    let settings: serde_json::Map<String, Value> = METADATA_KEYS
        .iter()
        .filter_map(|k| {
            metadata
                .get(*k)
                .filter(|v| !v.is_empty())
                .map(|v| (k.to_string(), json!(v)))
        })
        .collect();
    let marks = GalleryMarks::load().await;
    Ok(ok(vec![
        text(json!({
            "filename": filename,
            "width": width,
            "height": height,
            "url": link(ctx, links::Kind::Gallery, filename, link_hours(args)),
            "board": marks.board_of(filename),
            "favourite": marks.favourites.contains(filename),
            "rating": marks.ratings.get(filename),
            "settings": settings,
        })),
        image_content(jpeg),
    ]))
}

/// Files whose metadata was already read this session, so images without any
/// are not re-read on every search.
static CHECKED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Gallery images directly in `dir`, newest first.
fn gallery_images(dir: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, u64)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| is_image_file(&e.file_name().to_string_lossy()))
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            (e.path(), modified)
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    files
}

/// Read metadata from disk into the index for files whose row has no prompt
/// (images saved before the index stored prompts, or imported ones). Returns
/// how many of `paths` still lack one afterwards.
fn backfill(paths: &[PathBuf], budget: usize) -> usize {
    let rows = crate::gallery_index::rows_for(paths);
    let mut checked = CHECKED.lock().unwrap();
    let missing: Vec<&PathBuf> = paths
        .iter()
        .filter(|p| {
            rows.get(p.to_string_lossy().as_ref())
                .is_none_or(|r| r.prompt.is_none())
        })
        .filter(|p| !checked.contains(*p))
        .collect();
    let remaining = missing.len().saturating_sub(budget);
    for path in missing.into_iter().take(budget) {
        checked.insert(path.clone());
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let meta: Option<HashMap<String, String>> =
            crate::metadata::read_image_metadata(&bytes).ok().flatten();
        if meta.is_some() {
            crate::gallery_index::upsert(
                path,
                bytes.len() as u64,
                crate::metadata::detect_format(&bytes),
                meta.as_ref(),
            );
        }
    }
    remaining
}

/// Boards, favourites and ratings the owner gave gallery images, from their
/// synced prefs (keyed by gallery file name). Read only: the window's store
/// would overwrite a change made here.
#[derive(Debug, Default)]
struct GalleryMarks {
    boards: HashMap<String, String>,
    custom_boards: Vec<String>,
    favourites: HashSet<String>,
    ratings: HashMap<String, u64>,
}

impl GalleryMarks {
    fn from_value(v: Option<&Value>) -> Self {
        let Some(v) = v else {
            return Self::default();
        };
        let boards = v["boardAssignments"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, b)| b.as_str().map(|b| (k.clone(), b.to_string())))
                    .filter(|(_, b)| !b.is_empty() && b != "Unsorted")
                    .collect()
            })
            .unwrap_or_default();
        let custom_boards = v["customBoards"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|b| b.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let favourites = v["favourites"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(_, f)| f.as_bool() == Some(true))
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default();
        let ratings = v["ratings"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, r)| r.as_u64().filter(|r| *r > 0).map(|r| (k.clone(), r)))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            boards,
            custom_boards,
            favourites,
            ratings,
        }
    }

    async fn load() -> Self {
        let prefs = crate::user_prefs::load(crate::user_prefs::OWNER_KEY).await;
        Self::from_value(prefs.as_ref().and_then(|p| p.gallery_boards.as_ref()))
    }

    fn board_of(&self, filename: &str) -> Option<&str> {
        self.boards.get(filename).map(String::as_str)
    }

    /// Board names with how many images each holds, custom (empty) ones too.
    fn board_counts(&self) -> Vec<(String, usize)> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for board in self.boards.values() {
            match counts.iter_mut().find(|(b, _)| b == board) {
                Some((_, n)) => *n += 1,
                None => counts.push((board.clone(), 1)),
            }
        }
        for b in &self.custom_boards {
            if !counts.iter().any(|(c, _)| c == b) {
                counts.push((b.clone(), 0));
            }
        }
        counts.sort_by_key(|a| a.0.to_lowercase());
        counts
    }

    /// The board an agent named, matched case-insensitively.
    fn resolve_board(&self, name: &str) -> Result<String, String> {
        let wanted = name.trim();
        if wanted.eq_ignore_ascii_case("unsorted") {
            return Ok("Unsorted".into());
        }
        self.board_counts()
            .into_iter()
            .map(|(b, _)| b)
            .find(|b| b.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| {
                format!(
                    "No board called '{wanted}'. Boards: {}.",
                    self.board_counts()
                        .iter()
                        .map(|(b, _)| b.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    /// Whether an image passes the board / favourite / rating filters.
    fn keeps(&self, filename: &str, filter: &MarkFilter) -> bool {
        if let Some(board) = &filter.board {
            let own = self.board_of(filename).unwrap_or("Unsorted");
            if own != board {
                return false;
            }
        }
        if filter.favourites_only && !self.favourites.contains(filename) {
            return false;
        }
        if filter.min_rating > 0
            && self.ratings.get(filename).copied().unwrap_or(0) < filter.min_rating
        {
            return false;
        }
        true
    }
}

#[derive(Debug, Default)]
struct MarkFilter {
    board: Option<String>,
    favourites_only: bool,
    min_rating: u64,
}

impl MarkFilter {
    fn active(&self) -> bool {
        self.board.is_some() || self.favourites_only || self.min_rating > 0
    }
}

async fn list_boards() -> Result<Value, String> {
    let marks = GalleryMarks::load().await;
    Ok(ok(vec![text(json!({
        "boards": marks.board_counts().iter().map(|(b, n)| json!({ "name": b, "images": n })).collect::<Vec<_>>(),
        "favourites": marks.favourites.len(),
        "rated": marks.ratings.len(),
        "note": "Pass a board name to search_gallery's board. Images on no board are in Unsorted.",
    }))]))
}

async fn search_gallery(args: &Value, ctx: &CallContext) -> Result<Value, String> {
    let query = arg_str(args, "query").unwrap_or("").to_string();
    let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
    let marks = GalleryMarks::load().await;
    let filter = MarkFilter {
        board: arg_str(args, "board")
            .map(|b| marks.resolve_board(b))
            .transpose()?,
        favourites_only: args["favourites_only"].as_bool().unwrap_or(false),
        min_rating: args["min_rating"].as_u64().unwrap_or(0).min(5),
    };
    let dir = gallery_dir()?;
    let hours = link_hours(args);
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move || {
        let name_of = |p: &Path| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let files = gallery_images(&dir);
        let total = files.len();
        let annotate = |mut v: Value, filename: &str| {
            v["url"] = json!(link(&ctx, links::Kind::Gallery, filename, hours));
            if let Some(b) = marks.board_of(filename) {
                v["board"] = json!(b);
            }
            if marks.favourites.contains(filename) {
                v["favourite"] = json!(true);
            }
            if let Some(r) = marks.ratings.get(filename) {
                v["rating"] = json!(r);
            }
            v
        };
        if query.is_empty() {
            let newest: Vec<PathBuf> = files
                .into_iter()
                .map(|(p, _)| p)
                .filter(|p| marks.keeps(&name_of(p), &filter))
                .take(limit)
                .collect();
            backfill(&newest, newest.len());
            let rows = crate::gallery_index::rows_for(&newest);
            let images: Vec<Value> = newest
                .iter()
                .map(|p| {
                    let key = p.to_string_lossy();
                    let filename = name_of(p);
                    let v = match rows.get(key.as_ref()) {
                        Some(row) => json!(row),
                        None => json!({ "filename": filename }),
                    };
                    annotate(v, &filename)
                })
                .collect();
            return ok(vec![text(
                json!({ "total_images": total, "images": images }),
            )]);
        }
        let window: Vec<PathBuf> = files
            .into_iter()
            .take(SEARCH_WINDOW)
            .map(|(p, _)| p)
            .collect();
        let unindexed = backfill(&window, BACKFILL_PER_CALL);
        // Filters apply after the text match, so ask the index for more.
        let wanted = if filter.active() {
            (limit * 20).min(2000)
        } else {
            limit
        };
        let hits: Vec<Value> = crate::gallery_index::search(&dir, &query, wanted)
            .into_iter()
            .filter(|row| marks.keeps(&row.filename, &filter))
            .take(limit)
            .map(|row| {
                let filename = row.filename.clone();
                annotate(json!(row), &filename)
            })
            .collect();
        let mut result = json!({ "total_images": total, "query": query, "images": hits });
        if unindexed > 0 {
            result["note"] = json!(format!(
                "{unindexed} older images were not searched yet. Repeat the search to cover more."
            ));
        }
        ok(vec![text(result)])
    })
    .await
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Eagle
// ---------------------------------------------------------------------------

/// An Eagle folder by id or by name (`Art/Portraits` or just `Portraits`).
async fn eagle_folder_id(state: &Arc<AppState>, wanted: &str) -> Result<String, String> {
    let folders = crate::eagle::folders(&state.http_client)
        .await
        .map_err(|e| e.to_string())?;
    let w = wanted.trim();
    folders
        .iter()
        .find(|(id, _)| id == w)
        .or_else(|| folders.iter().find(|(_, n)| n.eq_ignore_ascii_case(w)))
        .or_else(|| {
            let hits: Vec<&(String, String)> = folders
                .iter()
                .filter(|(_, n)| {
                    n.rsplit('/')
                        .next()
                        .is_some_and(|last| last.eq_ignore_ascii_case(w))
                })
                .collect();
            (hits.len() == 1).then(|| hits[0])
        })
        .map(|(id, _)| id.clone())
        .ok_or_else(|| format!("No Eagle folder called '{w}'"))
}

async fn search_eagle(
    state: &Arc<AppState>,
    args: &Value,
    ctx: &CallContext,
) -> Result<Value, String> {
    let client = &state.http_client;
    if args["list_folders"].as_bool() == Some(true) {
        let folders = crate::eagle::folders(client)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(ok(vec![text(json!({
            "folders": folders.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>(),
        }))]));
    }
    let mut q = crate::eagle::ItemQuery {
        keyword: arg_str(args, "query").map(str::to_string),
        tags: args["tags"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        limit: args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize,
        offset: args["offset"].as_u64().unwrap_or(0) as usize,
        ..Default::default()
    };
    if let Some(folder) = arg_str(args, "folder") {
        q.folder_ids = vec![eagle_folder_id(state, folder).await?];
    }
    let items = crate::eagle::search_items(client, &q)
        .await
        .map_err(|e| e.to_string())?;
    let hours = link_hours(args);
    let images: Vec<Value> = items
        .iter()
        .map(|it| {
            let id = it["id"].as_str().unwrap_or("");
            json!({
                "id": id,
                "name": it["name"],
                "ext": it["ext"],
                "width": it["width"],
                "height": it["height"],
                "tags": it["tags"],
                "annotation": it["annotation"].as_str().map(|a| clip(a, 400)),
                "url": link(ctx, links::Kind::Eagle, id, hours),
            })
        })
        .collect();
    Ok(ok(vec![text(json!({
        "images": images,
        "note": "get_eagle_image shows one. The url downloads the original file.",
    }))]))
}

async fn get_eagle_image(
    state: &Arc<AppState>,
    args: &Value,
    ctx: &CallContext,
) -> Result<Value, String> {
    let id = arg_str(args, "id").ok_or("id is required")?.to_string();
    let max_size = args["max_size"].as_u64().unwrap_or(1024).clamp(128, 2048) as u32;
    let info = crate::eagle::item_info(&state.http_client, &id)
        .await
        .map_err(|e| e.to_string())?;
    let path = crate::eagle::item_file(&state.http_client, &id)
        .await
        .map_err(|e| e.to_string())?;
    let (jpeg, w, h) = tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let img = crate::commands::api::decode_gallery_image(&bytes)?;
        jpeg_preview(img, max_size)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(ok(vec![
        text(json!({
            "id": id,
            "name": info["name"],
            "width": w,
            "height": h,
            "tags": info["tags"],
            "annotation": info["annotation"],
            "url": link(ctx, links::Kind::Eagle, &id, link_hours(args)),
        })),
        image_content(jpeg),
    ]))
}

async fn send_to_eagle(state: &Arc<AppState>, args: &Value) -> Result<Value, String> {
    let filenames: Vec<String> = match &args["filenames"] {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    if filenames.is_empty() || filenames.len() > 100 {
        return Err("filenames needs 1 to 100 gallery file names".into());
    }
    let folder = match arg_str(args, "folder") {
        Some(f) => Some(eagle_folder_id(state, f).await?),
        None => state.config.read().await.eagle_folder_id.clone(),
    };
    let sent = crate::eagle::send_gallery_files(&state.http_client, &filenames, folder.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(ok(vec![text(json!({
        "sent": sent,
        "failed": filenames.len() - sent,
    }))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_marks_filter_by_board_favourite_and_rating() {
        let marks = GalleryMarks::from_value(Some(&json!({
            "boardAssignments": { "a.png": "Cats", "b.png": "Dogs", "c.png": "Unsorted" },
            "customBoards": ["Empty"],
            "favourites": { "a.png": true },
            "ratings": { "a.png": 5, "b.png": 2 },
        })));
        assert_eq!(
            marks.board_counts(),
            vec![("Cats".into(), 1), ("Dogs".into(), 1), ("Empty".into(), 0)]
        );
        assert_eq!(marks.resolve_board("cats").unwrap(), "Cats");
        assert!(marks.resolve_board("birds").is_err());
        let cats = MarkFilter {
            board: Some("Cats".into()),
            ..Default::default()
        };
        assert!(marks.keeps("a.png", &cats) && !marks.keeps("b.png", &cats));
        let unsorted = MarkFilter {
            board: Some("Unsorted".into()),
            ..Default::default()
        };
        assert!(marks.keeps("c.png", &unsorted) && marks.keeps("z.png", &unsorted));
        let fav = MarkFilter {
            favourites_only: true,
            ..Default::default()
        };
        assert!(marks.keeps("a.png", &fav) && !marks.keeps("b.png", &fav));
        let rated = MarkFilter {
            min_rating: 3,
            ..Default::default()
        };
        assert!(marks.keeps("a.png", &rated) && !marks.keeps("b.png", &rated));
    }
}
