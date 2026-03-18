use std::collections::HashMap;
use std::io::Cursor;

/// Embed A1111-compatible metadata into PNG image bytes.
/// Stores params in the "parameters" tEXt chunk, which is the standard
/// used by AUTOMATIC1111, ComfyUI, InvokeAI, and other SD tools.
pub fn embed_png_metadata(
    image_bytes: &[u8],
    params: &HashMap<String, String>,
) -> Result<Vec<u8>, String> {
    // Build the A1111-format parameters string
    let parameters_text = format_a1111_params(params);

    // Decode the source PNG
    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let mut reader = decoder.read_info().map_err(|e| format!("PNG decode error: {}", e))?;

    let info = reader.info().clone();
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let output_info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame read error: {}", e))?;
    buf.truncate(output_info.buffer_size());

    // Re-encode with metadata
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, info.width, info.height);
        encoder.set_color(info.color_type);
        encoder.set_depth(info.bit_depth);
        if let Some(srgb) = info.srgb {
            encoder.set_source_srgb(srgb);
        }

        // Add the parameters text chunk
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

/// Read A1111-compatible metadata from PNG bytes.
/// Returns the raw "parameters" text chunk content and parsed key-value pairs.
pub fn read_png_metadata(image_bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let reader = decoder.read_info().map_err(|e| format!("PNG decode error: {}", e))?;

    let info = reader.info();

    // Look for the "parameters" text chunk
    for chunk in &info.uncompressed_latin1_text {
        if chunk.keyword == "parameters" {
            let text = &chunk.text;
            return Ok(Some(parse_a1111_params(text)));
        }
    }

    Ok(None)
}

/// Format generation parameters into A1111-compatible string.
/// Format:
/// ```
/// positive prompt
/// Negative prompt: negative prompt
/// Steps: 20, Sampler: euler_cfg_pp, Scheduler: sgm_uniform, CFG scale: 1.4, Seed: 12345, Size: 1024x1024, Model: SIH-1.5, VAE: sdxl_vae
/// ```
fn format_a1111_params(params: &HashMap<String, String>) -> String {
    let positive = params.get("positive_prompt").cloned().unwrap_or_default();
    let negative = params.get("negative_prompt").cloned().unwrap_or_default();

    let mut settings = Vec::new();
    let setting_keys = [
        ("steps", "Steps"),
        ("sampler", "Sampler"),
        ("scheduler", "Scheduler"),
        ("cfg", "CFG scale"),
        ("seed", "Seed"),
        ("size", "Size"),
        ("model", "Model"),
        ("vae", "VAE"),
        ("denoise", "Denoising strength"),
        ("mode", "Generation mode"),
        ("loras", "LoRAs"),
        ("upscale_model", "Upscale model"),
        ("upscale_scale", "Upscale scale"),
        ("upscale_denoise", "Upscale denoise"),
    ];

    for (key, label) in setting_keys {
        if let Some(value) = params.get(key) {
            if !value.is_empty() {
                settings.push(format!("{}: {}", label, value));
            }
        }
    }

    let mut result = positive;
    if !negative.is_empty() {
        result.push_str(&format!("\nNegative prompt: {}", negative));
    }
    if !settings.is_empty() {
        result.push('\n');
        result.push_str(&settings.join(", "));
    }

    result
}

/// Parse A1111-format parameters string back into key-value pairs.
fn parse_a1111_params(text: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let lines: Vec<&str> = text.lines().collect();

    if lines.is_empty() {
        return params;
    }

    // Find the "Negative prompt:" line and the settings line
    let mut positive_lines = Vec::new();
    let mut negative_lines = Vec::new();
    let mut settings_line = None;
    let mut in_negative = false;

    for line in &lines {
        if line.starts_with("Negative prompt: ") {
            in_negative = true;
            negative_lines.push(line.trim_start_matches("Negative prompt: "));
        } else if !in_negative && settings_line.is_none() {
            // Check if this line looks like a settings line (starts with "Steps:" etc.)
            if line.starts_with("Steps:") || line.starts_with("Sampler:") || line.starts_with("CFG") {
                settings_line = Some(*line);
            } else {
                positive_lines.push(*line);
            }
        } else if in_negative {
            // Check if this is the settings line
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

    // Parse settings line: "Steps: 20, Sampler: euler, CFG scale: 1.4, ..."
    if let Some(settings) = settings_line {
        // Split by ", " but handle values that might contain commas (like LoRAs)
        let mut current_key = String::new();
        let mut current_value = String::new();

        for part in settings.split(", ") {
            if let Some(colon_pos) = part.find(": ") {
                // Save previous key-value
                if !current_key.is_empty() {
                    store_setting(&mut params, &current_key, &current_value);
                }
                current_key = part[..colon_pos].to_string();
                current_value = part[colon_pos + 2..].to_string();
            } else if !current_key.is_empty() {
                // Continuation of previous value
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
