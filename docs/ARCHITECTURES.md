# Adding a model architecture

New model families are declared in one place, the architecture registry in
`src-tauri/src/architectures/`. The rest of the app (detection, encoder and VAE
pairing, generation checks, custom node installs, the frontend's labels,
presets, Model Hub entries and auto-downloads) reads it, so a new family does
not need another arm in every per-family `match` or TypeScript table.

Families that predate the registry (`MODEL_FAMILIES` in
`src/lib/utils/modelFamily.ts`, mirrored by `LEGACY_FAMILIES` in
`commands/api.rs`) keep their hand-written code. A registered family is always
consulted first. To move one into the registry, give it a spec, delete it from
both lists and remove its old arms.

## Steps

1. Create `src-tauri/src/architectures/<name>.rs` with
   `pub static SPEC: ArchitectureSpec = ArchitectureSpec { .. };`.
2. Add `mod <name>;` and `&<name>::SPEC` to `ALL` in `architectures/mod.rs`.
3. Run `cargo test architectures`. The `registered_architectures_are_well_formed`
   test checks unique family ids, full-SHA pins and https downloads.

No TypeScript change is needed for the registry-driven behaviour below. UI that
is specific to the model (a custom panel, an option card) is still written by
hand.

## What a spec declares

| Field | Used for |
| --- | --- |
| `families` | One entry per variant: id, label, filename / CivitAI `baseModel` detection (`Detect`), `turbo`, sampling defaults, Model Hub entries, optional encoder / VAE override |
| `split_only` | Never a single-file checkpoint: always loaded split (with a `loader` hook, a checkpoint load is refused) |
| `prompt_style` | Tags or natural language (prompt tools, saved characters, Enhance) |
| `tensor_architecture` | Key from structural detection (`infer_*` in `commands/api.rs`) that means this architecture |
| `text_encoder` | `CLIPLoader` type, filename markers, `strict`, download |
| `vae` | Preferred basenames, markers, `strict`, download |
| `model_slots` | Further files (second encoder, second expert, stage B...). Picked or downloaded by the model panel, sent as `GenerationParams.architecture_files[key]` |
| `custom_nodes` | Third-party packs, installed at their pinned commit on ComfyUI start, checked before generating |
| `core_nodes`, `min_comfyui` | Core node classes the graph needs; a missing one asks the user to update ComfyUI |
| `int8_fast_model_type` | `OTUNetLoaderW8A8` model type, or `None` to refuse INT8-Fast |
| `modes` | Supported modes (others are refused), Image Edit reference count, files a mode loads by fixed name, per-mode sampling, optional dedicated builder |
| `negative_prompt` | `false` hides the negative prompt box |
| `resolution_multiple` | Size multiple the model needs (served to the frontend; round in the builder too) |
| `controlnet` | `false` refuses requests with ControlNet on (no ControlNet fits the model) |

## Hooks

All optional, all plain functions:

- `loader: fn(&mut Map<String, Value>, u32, &GenerationParams) -> LoaderOutput`
  replaces `UNETLoader` + `CLIPLoader` + `VAELoader` for split models
  (several encoders, a custom loader node). The LoRA chain runs on its outputs.
- `text_encode: fn(&GenerationParams, &Value, &str) -> Value` returns the node
  that replaces each `CLIPTextEncode` (clip link, prompt text). It runs on the
  finished graph, so hires fix, face fix and segment detail are covered too.
  Output 0 must be the conditioning.
- `ModeSpec::build: fn(&GenerationParams, i64) -> WorkflowResult` builds a
  fully dedicated graph for one mode. The generic injections and
  `finish_workflow` (upscale, face fix, segment detail, save) still run on it;
  set `refiner_model_source` when the refiners need another model.

## Bundled loader nodes

A family whose model or encoder core ComfyUI cannot load, and for which no
maintained third-party pack fits, can ship its own loader in
`comfyui-nodes/<name>_support/` instead of a `NodePack`. Deploy it in
`comfyui::nodes::ensure_mooshie_nodes`, list its class in
`REQUIRED_MOOSHIE_NODE_CLASSES`, mirror it in `comfyui-nodes/install.sh`,
`scripts/comfyui-compat/smoke_test.py` and `deploy/Dockerfile`, and emit it
from a `loader` hook. `architectures/kolors.rs` is the example.

## MCP

MCP reuses the owner's last request per model, so `architecture_files` rides
along with the rest of the model setup. A new per-image input still has to be
cleared in `mcp::setups::snapshot` (see `docs/MCP.md`).
