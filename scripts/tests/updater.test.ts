import assert from "node:assert/strict";
import test from "node:test";
import {
  AUTO_UPDATE_STORAGE_KEY,
  isGenerationIdle,
  loadAutoUpdateMode,
  parseAutoUpdateMode,
  saveAutoUpdateMode,
  shouldAutoDownload,
  shouldAutoInstallWhenIdle,
} from "../../src/lib/utils/autoUpdate.ts";

function makeMemoryStorage(initial: Record<string, string> = {}) {
  const data = new Map<string, string>(Object.entries(initial));
  return {
    getItem(key: string) {
      return data.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      data.set(key, value);
    },
  };
}

test("parseAutoUpdateMode defaults invalid values to off", () => {
  assert.equal(parseAutoUpdateMode("off"), "off");
  assert.equal(parseAutoUpdateMode("download"), "download");
  assert.equal(parseAutoUpdateMode("auto"), "auto");
  assert.equal(parseAutoUpdateMode(null), "off");
  assert.equal(parseAutoUpdateMode(undefined), "off");
  assert.equal(parseAutoUpdateMode("invalid"), "off");
});

test("loadAutoUpdateMode and saveAutoUpdateMode persist under expected key", () => {
  const storage = makeMemoryStorage();
  assert.equal(loadAutoUpdateMode(storage), "off");

  saveAutoUpdateMode("download", storage);
  assert.equal(storage.getItem(AUTO_UPDATE_STORAGE_KEY), "download");
  assert.equal(loadAutoUpdateMode(storage), "download");

  saveAutoUpdateMode("auto", storage);
  assert.equal(loadAutoUpdateMode(storage), "auto");

  // Unknown persisted value falls back to off
  storage.setItem(AUTO_UPDATE_STORAGE_KEY, "bogus");
  assert.equal(loadAutoUpdateMode(storage), "off");
});

test("shouldAutoDownload respects mode and managed package installs", () => {
  assert.equal(shouldAutoDownload("off", ""), false);
  assert.equal(shouldAutoDownload("download", ""), true);
  assert.equal(shouldAutoDownload("auto", ""), true);

  // Never auto-download on apt/dnf/zypper/pacman managed installs
  assert.equal(shouldAutoDownload("download", "sudo apt update && sudo apt upgrade"), false);
  assert.equal(shouldAutoDownload("auto", "yay -Syu mooshieui-bin"), false);
});

test("isGenerationIdle and shouldAutoInstallWhenIdle guard active generations and cancellations", () => {
  assert.equal(isGenerationIdle(0, false), true);
  assert.equal(isGenerationIdle(1, false), false);
  assert.equal(isGenerationIdle(0, true), false);

  assert.equal(shouldAutoInstallWhenIdle("auto", "", true, false), true);
  assert.equal(shouldAutoInstallWhenIdle("download", "", true, false), false);
  assert.equal(shouldAutoInstallWhenIdle("off", "", true, false), false);
  assert.equal(shouldAutoInstallWhenIdle("auto", "sudo dnf upgrade --refresh mooshieui", true, false), false);
  assert.equal(shouldAutoInstallWhenIdle("auto", "", false, false), false);
  assert.equal(shouldAutoInstallWhenIdle("auto", "", true, true), false);
});
