# Anima camera controls

In Generate, select an Anima model and expand **Anima Camera** below the negative
prompt. In combined view it stays beneath the shared prompt area on either tab.
Enable it to add camera tags to the positive prompt of each generation. It supports
txt2img, img2img, and inpainting.

Choose a direction, elevation, and framing preset, or enter numeric values.
The coloured handles in the 3D guide adjust horizontal angle, elevation, and
zoom; scrolling over the guide also adjusts zoom. **Dutch angle** adds a tilted
composition tag. The guide shows camera placement, not a generated-image preview.
If WebGL is unavailable, the numeric controls and presets still work.

**Extra Prompts** contains lens/perspective, focus/depth of field, motion,
composition, illustration style, and lighting presets. Each group accepts one
preset or custom English prompt text, with an optional weight. Extreme viewpoints
replace the ordinary direction and elevation tags while retaining framing and
Dutch angle. Their orientation is described by the prompt, not the 3D guide.

The read-only preview shows the fragment added to the positive prompt. The
defaults and weight calculation match
[Comfyui-Anima_camera_angle-Aitha v1.0.1](https://github.com/AithaEzio/Comfyui-Anima_camera_angle-Aitha/releases/tag/v1.0.1),
including its strong direction weighting. Prompt tags guide the model; they do
not guarantee exact geometry. Imported code and runtime notices are included in
[`public/licenses/anima-camera-LICENSE.txt`](../public/licenses/anima-camera-LICENSE.txt).

No custom node, extra model weights, LoRA, or separate installation is required.
Camera controls work alongside Anima Style Reference, including SnapRef.
The existing Qwen Image Edit camera picker uses its own settings.

Camera settings are saved with generation tabs and profiles. **Reset camera**
resets these settings without changing your written prompt. Importing an image's
positive prompt disables the camera toggle, since the imported text already
contains its camera tags; it does not infer slider settings from those tags.
Explicit prompt overrides supplied by other tools bypass the camera fragment.
Camera settings remain frontend-only; no backend parameters or MCP schema
fields are introduced. MCP snapshots clear the saved positive prompt and use the
assistant-supplied prompt, so MCP requests do not inherit these camera tags.

## Validation

Run `npm run check`. The Guardrails workflow also runs
`scripts/tests/animaCamera.test.ts`, with 180 expected prompts exported from
the pinned package's Python implementation. Cases cover all illustration
presets, direction/elevation/framing boundaries, Dutch angle, custom text,
weights, and extreme viewpoints.

For UI testing, verify the panel starts disabled and collapsed, settings survive
reload and tab/profile switching, numeric controls and the guide stay synchronized,
and closing/reopening the panel releases and recreates the guide. Test without
WebGL, on a narrow touch layout, and with combined positive/negative prompt tabs.
Check that repeated generation leaves the written prompt unchanged, imported
prompts do not acquire another camera fragment, and other model families do not
send Anima camera tags.
