<script lang="ts">
  import { catalogIds } from "../../errors/errorCatalog.js";
  import { resolveError } from "../../errors/resolveError.js";
  import ErrorCard from "./ErrorCard.svelte";

  // A representative raw message per id, chosen to match that id's matcher.
  const SAMPLES: Record<string, string> = {
    comfyui_not_running: "Failed to connect to ComfyUI: not running",
    connection_failed: "Connection failed: os error 111",
    websocket_dropped: "WebSocket error: connection closed",
    api_error_5xx: "API error (503): Service Unavailable",
    download_404: "HTTP error: 404 Not Found",
    disk_full: "No space left on device",
    checksum_mismatch: "sha256 mismatch for downloaded file",
    civitai_auth: "CivitAI download failed: 401 unauthorized (api key)",
    hf_page_url: "This looks like a huggingface.co /blob/ page url",
    model_not_found: "Model not found: sd_xl_base.safetensors",
    download_network: "HTTP error: request timed out",
    comfyui_launch_failed: "Failed to start ComfyUI: spawn error",
    python_env_broken: "python: No module named torch",
    attention_backend_install: "flash-attn install failed",
    out_of_memory: "CUDA out of memory",
    unsupported_gpu: "No CUDA device detected",
    missing_node: "Unknown node: ImpactWildcardProcessor",
    invalid_workflow: "Invalid workflow: malformed graph",
    generation_interrupted: "Generation was interrupted",
    io_permission: "IO error: permission denied",
    serialization: "Serialization error: expected value at line 1",
  };

  const ids = [...catalogIds(), "unknown"];
  function sampleFor(id: string): string {
    if (id === "unknown") return "some totally unrecognized failure text";
    return SAMPLES[id] ?? id;
  }
</script>

<div class="fixed inset-0 z-[60] overflow-auto bg-neutral-950 p-6">
  <h2 class="mb-4 text-lg font-semibold text-neutral-100">Error gallery ({ids.length})</h2>
  <div class="grid gap-4 md:grid-cols-2">
    {#each ids as id}
      <div>
        <p class="mb-1 text-xs text-neutral-500">{id}</p>
        <ErrorCard error={resolveError(sampleFor(id))} />
      </div>
    {/each}
  </div>
</div>
