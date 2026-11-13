import assert from "node:assert/strict";
import test from "node:test";
import { externalLinkTarget } from "../../src/lib/utils/externalLink.ts";

const APP = "http://tauri.localhost";

function link(href: string, target = "", attrs: string[] = []) {
  return { href, target, hasAttribute: (name: string) => attrs.includes(name) };
}

test("new-tab links open in the OS browser", () => {
  const url = "https://civitai.com/models/123";
  assert.equal(externalLinkTarget(link(url, "_blank"), APP), url);
  assert.equal(externalLinkTarget(link("https://civitai.red/models/9", "_blank"), APP), "https://civitai.red/models/9");
});

test("web links without a target still leave the app window alone", () => {
  assert.equal(externalLinkTarget(link("https://huggingface.co/x"), APP), "https://huggingface.co/x");
  assert.equal(externalLinkTarget(link("mailto:blob@example.com"), APP), "mailto:blob@example.com");
});

test("in-app, custom-scheme and download links stay with the webview", () => {
  assert.equal(externalLinkTarget(link(`${APP}/settings`), APP), null);
  assert.equal(externalLinkTarget(link("gallery://localhost/a.png", "_blank"), APP), null);
  assert.equal(externalLinkTarget(link("javascript:void(0)", "_blank"), APP), null);
  assert.equal(externalLinkTarget(link("https://example.com/f.png", "_blank", ["download"]), APP), null);
  assert.equal(externalLinkTarget(link("", "_blank"), APP), null);
});
