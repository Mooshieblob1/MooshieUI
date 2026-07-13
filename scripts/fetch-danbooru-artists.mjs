#!/usr/bin/env node
/**
 * Fetch Danbooru artist tags and write src/lib/assets/danbooru-artists.json.
 *
 * One-off, maintainer-run script (NOT part of the build or CI). It supplements
 * the top-10k danbooru-tags.json, which carries only ~269 artist tags, so that
 * artist autocomplete works on non-Anima (SDXL/Illustrious/Pony/Flux) models.
 * See issue #465.
 *
 * Usage:  node scripts/fetch-danbooru-artists.mjs
 * Requires Node 18+ (built-in fetch). Commits the resulting JSON to the repo;
 * the app lazy-imports it as a separate chunk (no main-bundle growth).
 *
 * Danbooru API: https://danbooru.donmai.us/wiki_pages/help:api
 */

import { writeFileSync } from "node:fs";

const MIN_POSTS = 100; // artist tags with fewer posts are dropped (tunes file size)
const PAGE_LIMIT = 1000; // Danbooru max rows per page
const DELAY_MS = 600; // polite delay between pages
const OUTPUT = "src/lib/assets/danbooru-artists.json";
const USER_AGENT = "MooshieUI-asset-generator/1.0 (github.com/Mooshieblob1/MooshieUI)";

async function fetchPage(page) {
  const url = new URL("https://danbooru.donmai.us/tags.json");
  url.searchParams.set("search[category]", "1"); // 1 = artist
  url.searchParams.set("search[order]", "count");
  url.searchParams.set("limit", String(PAGE_LIMIT));
  url.searchParams.set("page", String(page));
  const resp = await fetch(url.toString(), { headers: { "User-Agent": USER_AGENT } });
  if (!resp.ok) throw new Error(`HTTP ${resp.status} ${resp.statusText} on page ${page}`);
  return resp.json();
}

async function main() {
  const results = [];
  let page = 1;
  let done = false;

  while (!done) {
    const tags = await fetchPage(page);
    if (!Array.isArray(tags) || tags.length === 0) break;

    for (const t of tags) {
      if (typeof t.post_count !== "number" || t.post_count < MIN_POSTS) {
        done = true;
        break;
      }
      // Match the TagEntry shape used by danbooru-tags.json: { n, c, p }.
      // The tags endpoint carries no aliases, so none are included.
      results.push({ n: t.name, c: 1, p: t.post_count });
    }

    const last = tags[tags.length - 1];
    console.log(
      `Page ${page}: fetched ${tags.length}, kept ${results.length} total (last post_count=${last?.post_count})`,
    );

    // Danbooru caps numeric pagination at 1000 pages; we stop well before that.
    if (page >= 1000) {
      console.warn("Reached page 1000 pagination cap; stopping.");
      break;
    }

    page += 1;
    await new Promise((r) => setTimeout(r, DELAY_MS));
  }

  // Compact JSON (no pretty-print) keeps the lazy chunk small.
  writeFileSync(OUTPUT, JSON.stringify(results), "utf-8");
  console.log(`\nDone: ${results.length} artist tags written to ${OUTPUT}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
