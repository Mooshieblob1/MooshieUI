## What's New in v0.4.0

### Image Interrogator
- **New feature**: Analyze any image to extract tags (character, artist, general, copyright, rating) using the WD EVA02 Large v3 ONNX model
- Interrogate from the prompt area (drag & drop, file browse, or Ctrl+V paste from clipboard)
- Interrogate from gallery images and session images via right-click context menu or lightbox button
- Configurable confidence thresholds for general and character tags in Settings > Interrogator
- Model auto-downloads on first use with progress indicator
- One-click "Apply to prompt" to inject detected tags directly into your positive prompt

### Context Menus
- Right-click context menu on gallery images (grid and list view) and session panel images
- Quick access to: Get Image Tags, Img2Img, Inpaint, Upscale, Save As, Copy, Delete

### Lightbox Improvements
- Compact icon-only action buttons with tooltip labels
- Buttons grouped by purpose: Generation | Reuse | Export | Delete
- New Interrogate and Remix buttons added to the lightbox toolbar

### Release Notes Rendering
- About page now renders release notes as proper formatted markdown (headings, bold, links, lists, tables, code blocks)
- Powered by the `marked` library for GitHub Flavored Markdown

### LoRA Gallery
- LoRA cards now scale based on bottom panel height instead of fixed width
- Cards maintain a constant 3:4 aspect ratio, becoming smaller or larger as you resize the panel

### Under the Hood
- Added `ort` (ONNX Runtime) and `csv` crates for inference on the Rust backend
- New `InterrogatorState` manages model lifecycle and caching
- New Tauri commands: `interrogate_image`, `interrogate_image_path`, `interrogate_gallery_image`, `interrogate_clipboard`
- Added `clipboard-manager:allow-read-image` capability for clipboard interrogation
- `InterrogatorError` variant added to `AppError` enum
