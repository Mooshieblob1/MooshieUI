import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";
import ts from "typescript";

function compile(source) {
  return ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
}

const availability = { exports: {} };
vm.runInNewContext(compile(await readFile("src/lib/utils/modelAvailability.ts", "utf8")), availability);
const { resolveAvailableModel, localOnlyModels } = availability.exports;

assert.equal(resolveAvailableModel("model.safetensors", [], "old.safetensors"), undefined);
assert.equal(resolveAvailableModel("model.safetensors", ["model.safetensors"], "old.safetensors"), "model.safetensors");
assert.equal(resolveAvailableModel("model.safetensors", ["subfolder/model.safetensors"]), "subfolder/model.safetensors");
assert.equal(resolveAvailableModel("subfolder/model.safetensors", ["subfolder\\model.safetensors"]), "subfolder\\model.safetensors");
assert.equal(resolveAvailableModel("model.safetensors", ["a/model.safetensors", "b/model.safetensors"]), undefined);
assert.equal(resolveAvailableModel("model.safetensors", ["a/model.safetensors", "b/model.safetensors"], "b/model.safetensors"), "b/model.safetensors");
assert.equal(resolveAvailableModel("model.safetensors", ["renamed.safetensors"], "renamed.safetensors"), "renamed.safetensors");
assert.deepEqual(Array.from(localOnlyModels(["local.safetensors"], ["server.safetensors"])), ["local.safetensors"]);

// Run the real store with synthetic API/disk responses. No Svelte component or
// ComfyUI process is needed to check the inventory boundary and refresh races.
let apiModels = ["server.safetensors"];
let fail = false;
let hold;
let config = { server_mode: "remote", server_url: "https://server-a.invalid", comfyui_path: "local-comfyui", extra_model_paths: null };
const api = {
  getConfig: async () => ({ ...config }),
  getModels: async (category) => {
    if (fail) throw new Error("Synthetic connection failure");
    const result = category === "checkpoints" ? [...apiModels] : [];
    if (hold) await hold;
    return result;
  },
  getSamplers: async () => ({ samplers: ["euler"], schedulers: ["normal"] }),
  getEmbeddings: async () => [],
  listModelFiles: async (category) => category === "checkpoints" ? [{ filename: "local.safetensors" }] : [],
};
const context = {
  exports: {}, $state: (value) => value,
  console: { log() {}, error() {} },
  require: (name) => name.includes("modelAvailability") ? availability.exports : api,
};
vm.runInNewContext(compile(await readFile("src/lib/stores/models.svelte.ts", "utf8")), context);
const store = context.exports.models;
assert.equal(await store.refresh(), true);
assert.deepEqual(Array.from(store.checkpoints), ["server.safetensors"]);
assert.deepEqual(Array.from(store.localOnly.checkpoints), ["local.safetensors"]);
assert.equal(store.remote, true);
const firstScope = store.cacheScope;

fail = true;
assert.equal(await store.refresh(), false);
assert.deepEqual(Array.from(store.checkpoints), []);
assert.equal(store.loading, false);
fail = false;

let release;
hold = new Promise((resolve) => { release = resolve; });
const oldRefresh = store.refresh();
await new Promise((resolve) => setImmediate(resolve));
hold = undefined;
config = { ...config, server_url: "https://server-b.invalid" };
apiModels = ["new-server.safetensors"];
assert.equal(await store.refresh(), true);
release();
assert.equal(await oldRefresh, false);
assert.deepEqual(Array.from(store.checkpoints), ["new-server.safetensors"]);
assert.notEqual(store.cacheScope, firstScope);
console.log("Model availability checks passed: stale cache, server subfolders, ambiguity, local-only files, failed refresh, and server-switch races.");
