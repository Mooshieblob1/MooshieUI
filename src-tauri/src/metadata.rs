use std::collections::HashMap;
use std::io::Cursor;

/// Embed SwarmUI-compatible JSON metadata into PNG image bytes.
/// Stores the JSON string in the "parameters" tEXt chunk.
pub fn embed_png_metadata(
    image_bytes: &[u8],
    params: &HashMap<String, String>,
) -> Result<Vec<u8>, String> {
    let parameters_text = format_swarmui_json(params);

    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let mut reader = decoder.read_info().map_err(|e| format!("PNG decode error: {}", e))?;

    let info = reader.info().clone();
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let output_info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame read error: {}", e))?;
    buf.truncate(output_info.buffer_size());

    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, info.width, info.height);
        encoder.set_color(info.color_type);
        encoder.set_depth(info.bit_depth);
        if let Some(srgb) = info.srgb {
            encoder.set_source_srgb(srgb);
        }

        encoder
            .add_text_chunk("parameters".to_string(), parameters_text)
            .map_err(|e| format!("Failed to add text chunk: {}", e))?;

        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG encode error: {}", e))?;
        writer
            .write_image_data(&buf)
            .map_err(|e| format!("PNG write error: {}", e))?;
    }

    Ok(output)
}

/// Read metadata from PNG bytes.
/// Tries SwarmUI JSON format first, falls back to legacy A1111 format.
pub fn read_png_metadata(image_bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let reader = decoder.read_info().map_err(|e| format!("PNG decode error: {}", e))?;

    let info = reader.info();

    for chunk in &info.uncompressed_latin1_text {
        if chunk.keyword == "parameters" {
            let text = chunk.text.trim();
            // Try SwarmUI JSON first
            if text.starts_with('{') {
                if let Some(parsed) = parse_swarmui_json(text) {
                    return Ok(Some(parsed));
                }
            }
            // Fall back to legacy A1111 format
            return Ok(Some(parse_a1111_params(text)));
        }
    }

    Ok(None)
}

/// Build SwarmUI-compatible JSON from the flat metadata map.
/// The metadata map keys use our internal names; we translate to SwarmUI param IDs.
fn format_swarmui_json(params: &HashMap<String, String>) -> String {
    let mut image_params = serde_json::Map::new();

    // Map internal keys to SwarmUI-style param IDs
    let mappings: &[(&str, &str)] = &[
        ("positive_prompt", "prompt"),
        ("negative_prompt", "negativeprompt"),
        ("model", "model"),
        ("vae", "vae"),
        ("seed", "seed"),
        ("steps", "steps"),
        ("cfg", "cfgscale"),
        ("sampler", "sampler"),
        ("scheduler", "scheduler"),
        ("denoise", "denoise"),
        ("mode", "generationmode"),
        ("loras", "loras"),
    ];

    for &(internal, swarm) in mappings {
        if let Some(value) = params.get(internal) {
            if !value.is_empty() {
                image_params.insert(swarm.to_string(), serde_json::Value::String(value.clone()));
            }
        }
    }

    // Width and height from "size" field (e.g. "1024x1024")
    if let Some(size) = params.get("size") {
        if let Some((w, h)) = size.split_once('x') {
            if let (Ok(width), Ok(height)) = (w.parse::<u32>(), h.parse::<u32>()) {
                image_params.insert("width".to_string(), serde_json::json!(width));
                image_params.insert("height".to_string(), serde_json::json!(height));
            }
        }
    }

    // Upscale params
    if let Some(v) = params.get("upscale_model") {
        if !v.is_empty() {
            image_params.insert("upscalemodel".to_string(), serde_json::Value::String(v.clone()));
        }
    }
    if let Some(v) = params.get("upscale_scale") {
        if !v.is_empty() {
            image_params.insert("upscalescale".to_string(), serde_json::Value::String(v.clone()));
        }
    }
    if let Some(v) = params.get("upscale_denoise") {
        if !v.is_empty() {
            image_params.insert("upscaledenoise".to_string(), serde_json::Value::String(v.clone()));
        }
    }

    // Build extra data from reserved keys
    let mut extra_data = serde_json::Map::new();
    let extra_keys = ["date", "generation_time"];
    for &key in &extra_keys {
        if let Some(value) = params.get(key) {
            if !value.is_empty() {
                extra_data.insert(key.to_string(), serde_json::Value::String(value.clone()));
            }
        }
    }

    // Build root object
    let mut root = serde_json::Map::new();
    root.insert("sui_image_params".to_string(), serde_json::Value::Object(image_params));
    root.insert("sui_extra_data".to_string(), serde_json::Value::Object(extra_data));

    serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string())
}

/// Parse SwarmUI JSON format back into our flat key-value map.
fn parse_swarmui_json(text: &str) -> Option<HashMap<String, String>> {
    let root: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = root.as_object()?;

    let mut params = HashMap::new();

    if let Some(image_params) = obj.get("sui_image_params").and_then(|v| v.as_object()) {
        // Map SwarmUI param IDs back to our internal keys
        let reverse_mappings: &[(&str, &str)] = &[
            ("prompt", "positive_prompt"),
            ("negativeprompt", "negative_prompt"),
            ("model", "model"),
            ("vae", "vae"),
            ("seed", "seed"),
            ("steps", "steps"),
            ("cfgscale", "cfg"),
            ("sampler", "sampler"),
            ("scheduler", "scheduler"),
            ("denoise", "denoise"),
            ("generationmode", "mode"),
            ("loras", "loras"),
            ("upscalemodel", "upscale_model"),
            ("upscalescale", "upscale_scale"),
            ("upscaledenoise", "upscale_denoise"),
        ];

        for &(swarm, internal) in reverse_mappings {
            if let Some(value) = image_params.get(swarm) {
                let s = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if !s.is_empty() {
                    params.insert(internal.to_string(), s);
                }
            }
        }

        // Reconstruct "size" from width/height
        if let (Some(w), Some(h)) = (image_params.get("width"), image_params.get("height")) {
            let ws = match w { serde_json::Value::Number(n) => n.to_string(), serde_json::Value::String(s) => s.clone(), _ => String::new() };
            let hs = match h { serde_json::Value::Number(n) => n.to_string(), serde_json::Value::String(s) => s.clone(), _ => String::new() };
            if !ws.is_empty() && !hs.is_empty() {
                params.insert("size".to_string(), format!("{}x{}", ws, hs));
            }
        }
    }

    if let Some(extra) = obj.get("sui_extra_data").and_then(|v| v.as_object()) {
        if let Some(date) = extra.get("date").and_then(|v| v.as_str()) {
            params.insert("date".to_string(), date.to_string());
        }
        if let Some(gen_time) = extra.get("generation_time").and_then(|v| v.as_str()) {
            params.insert("generation_time".to_string(), gen_time.to_string());
        }
    }

    if params.is_empty() {
        None
    } else {
        Some(params)
    }
}

/// Parse legacy A1111-format parameters string back into key-value pairs.
fn parse_a1111_params(text: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let lines: Vec<&str> = text.lines().collect();

    if lines.is_empty() {
        return params;
    }

    let mut positive_lines = Vec::new();
    let mut negative_lines = Vec::new();
    let mut settings_line = None;
    let mut in_negative = false;

    for line in &lines {
        if line.starts_with("Negative prompt: ") {
            in_negative = true;
            negative_lines.push(line.trim_start_matches("Negative prompt: "));
        } else if !in_negative && settings_line.is_none() {
            if line.starts_with("Steps:") || line.starts_with("Sampler:") || line.starts_with("CFG") {
                settings_line = Some(*line);
            } else {
                positive_lines.push(*line);
            }
        } else if in_negative {
            if line.starts_with("Steps:") || line.starts_with("Sampler:") || line.starts_with("CFG") {
                settings_line = Some(*line);
                in_negative = false;
            } else {
                negative_lines.push(*line);
            }
        }
    }

    params.insert("positive_prompt".to_string(), positive_lines.join("\n"));
    if !negative_lines.is_empty() {
        params.insert("negative_prompt".to_string(), negative_lines.join("\n"));
    }

    if let Some(settings) = settings_line {
        let mut current_key = String::new();
        let mut current_value = String::new();

        for part in settings.split(", ") {
            if let Some(colon_pos) = part.find(": ") {
                if !current_key.is_empty() {
                    store_setting(&mut params, &current_key, &current_value);
                }
                current_key = part[..colon_pos].to_string();
                current_value = part[colon_pos + 2..].to_string();
            } else if !current_key.is_empty() {
                current_value.push_str(", ");
                current_value.push_str(part);
            }
        }
        if !current_key.is_empty() {
            store_setting(&mut params, &current_key, &current_value);
        }
    }

    params
}

fn store_setting(params: &mut HashMap<String, String>, key: &str, value: &str) {
    let normalized_key = match key {
        "Steps" => "steps",
        "Sampler" => "sampler",
        "Scheduler" => "scheduler",
        "CFG scale" => "cfg",
        "Seed" => "seed",
        "Size" => "size",
        "Model" => "model",
        "VAE" => "vae",
        "Denoising strength" => "denoise",
        "Generation mode" => "mode",
        "LoRAs" => "loras",
        "Upscale model" => "upscale_model",
        "Upscale scale" => "upscale_scale",
        "Upscale denoise" => "upscale_denoise",
        other => other,
    };
    params.insert(normalized_key.to_string(), value.to_string());
}
