import fs from "fs";
import path from "path";

const dir = "src/lib/locales";
const files = fs.readdirSync(dir).filter((f) => f.endsWith(".ts"));

function parseKeys(file) {
  const content = fs.readFileSync(path.join(dir, file), "utf8");
  const keys = new Map();
  for (const m of content.matchAll(/"([^"]+)":\s*"((?:\\.|[^"\\])*)"/g)) {
    keys.set(m[1], m[2]);
  }
  return keys;
}

const all = Object.fromEntries(files.map((f) => [f, parseKeys(f)]));
const en = all["en.ts"];

console.log("Key counts:");
for (const f of files.sort()) console.log(`  ${f}: ${all[f].size}`);

console.log("\nParity vs en.ts:");
for (const f of files) {
  if (f === "en.ts") continue;
  const missing = [...en.keys()].filter((k) => !all[f].has(k));
  const extra = [...all[f].keys()].filter((k) => !en.has(k));
  if (missing.length || extra.length) {
    console.log(`  ${f}: missing=${missing.length} extra=${extra.length}`);
    if (missing.length) console.log("    missing sample:", missing.slice(0, 8).join(", "));
    if (extra.length) console.log("    extra sample:", extra.slice(0, 8).join(", "));
  }
}

// Find keys in non-en files that still match English exactly (likely untranslated)
const untranslated = {};
for (const f of files) {
  if (f === "en.ts") continue;
  const same = [];
  for (const [k, v] of all[f]) {
    const enVal = en.get(k);
    if (enVal && v === enVal && /[A-Za-z]{4,}/.test(v)) same.push(k);
  }
  if (same.length) untranslated[f] = same;
}

console.log("\nUntranslated (identical to en.ts):");
for (const [f, keys] of Object.entries(untranslated).sort()) {
  console.log(`  ${f}: ${keys.length}`);
  for (const k of keys.slice(0, 30)) console.log(`    ${k}: ${en.get(k)}`);
  if (keys.length > 30) console.log(`    ... +${keys.length - 30} more`);
}