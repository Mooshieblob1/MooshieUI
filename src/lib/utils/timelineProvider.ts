/**
 * Registration seam between the video timeline store and the generation store.
 *
 * `generation.toParams()` has to embed the compiled H3 Director timeline JSON,
 * but the hub `generation` store must never import a feature store (see
 * CLAUDE.md) and `videoTimeline` legitimately reads `generation.videoVariant`
 * to derive `reference_mode`. This module breaks the cycle exactly the way
 * `syncTrigger.ts` breaks the store -> prefsSync one: the feature store
 * registers a callback on load, the hub calls through it.
 *
 * `videoTimeline` is imported by `prefsSync`, which `App.svelte` imports, so
 * the compiler is always registered before the first generation.
 */

/**
 * Everything the Director node needs from the timeline. `data` is the
 * `timeline_data` JSON; the two flags are node *widgets* rather than keys
 * inside that JSON, so they have to travel alongside it.
 */
export interface CompiledTimeline {
  data: string;
  useCustomMotion: boolean;
  useCustomAudio: boolean;
}

/** Compiles the current timeline into the node's inputs. */
export type TimelineCompiler = (globalPrompt: string) => CompiledTimeline | null;

let _compile: TimelineCompiler | null = null;

/** Called by the video timeline store on module load. */
export function registerTimelineCompiler(compile: TimelineCompiler): void {
  _compile = compile;
}

/**
 * Returns the compiled Director inputs, or `null` when the timeline is
 * disabled, empty, or the store has not loaded yet (image-only sessions never
 * import it). A `null` return means "fall back to the native H3 template".
 *
 * @param globalPrompt the fully composed positive prompt, used as the node's
 *   `global_prompt` fallback (its socket is `force_input`, so the value has to
 *   travel inside `timeline_data`).
 */
export function compileTimeline(globalPrompt: string): CompiledTimeline | null {
  return _compile?.(globalPrompt) ?? null;
}

/**
 * Reports whether the timeline will drive the graph, i.e. whether
 * `compileTimeline()` would return a payload for the current state.
 */
export type TimelineActivityReader = () => boolean;

let _isActive: TimelineActivityReader | null = null;

/** Called by the video timeline store on module load. */
export function registerTimelineActivity(read: TimelineActivityReader): void {
  _isActive = read;
}

/**
 * Whether the Director node is about to take over the graph. `generation`
 * uses this to relax the ref2va "needs a reference image" rule: the timeline
 * supplies its own references (shot stills and cast photos), so the settings
 * panel's nine slots stop being mandatory once it is driving.
 *
 * Reading this inside a `$derived`/`get` stays reactive because the registered
 * reader synchronously touches the timeline store's `$state` fields.
 */
export function isTimelineActive(): boolean {
  return _isActive?.() ?? false;
}
