import fs from "fs";
import path from "path";

const dir = "src/lib/locales";
// Sort once so every section below reports in a deterministic, platform-independent
// order (readdirSync order is not guaranteed).
const files = fs.readdirSync(dir)
  .filter((f) => f.endsWith(".ts"))
  .sort();

function parseKeys(file) {
  const content = fs.readFileSync(path.join(dir, file), "utf8");
  const keys = new Map();
  for (const m of content.matchAll(/"([^"]+)":\s*"((?:\\.|[^"\\])*)"/g)) {
    keys.set(m[1], m[2]);
  }
  return keys;
}

// Placeholder names inside a value, e.g. "Hello {name}" -> Set{ "name" }.
function placeholders(value) {
  const set = new Set();
  for (const m of value.matchAll(/\{([^}]+)\}/g)) set.add(m[1]);
  return set;
}

function sameSet(a, b) {
  if (a.size !== b.size) return false;
  for (const x of a) if (!b.has(x)) return false;
  return true;
}

const all = Object.fromEntries(files.map((f) => [f, parseKeys(f)]));
const en = all["en.ts"];

if (!en) {
  console.error(`ERROR: ${dir}/en.ts not found. It is the source of truth for i18n parity.`);
  process.exit(1);
}

let failed = false;

console.log("Key counts:");
for (const f of files) console.log(`  ${f}: ${all[f].size}`);

console.log("\nParity vs en.ts:");
for (const f of files) {
  if (f === "en.ts") continue;
  const missing = [...en.keys()].filter((k) => !all[f].has(k));
  const extra = [...all[f].keys()].filter((k) => !en.has(k));

  // Placeholder mismatches for keys present in both files.
  const placeholderMismatches = [];
  for (const [k, enVal] of en) {
    if (!all[f].has(k)) continue;
    const enPh = placeholders(enVal);
    const locPh = placeholders(all[f].get(k));
    if (!sameSet(enPh, locPh)) {
      placeholderMismatches.push(
        `${k} (en: {${[...enPh].join(", ")}} vs ${f.replace(".ts", "")}: {${[...locPh].join(", ")}})`,
      );
    }
  }

  if (missing.length || extra.length || placeholderMismatches.length) {
    failed = true;
    console.log(
      `  ${f}: missing=${missing.length} extra=${extra.length} placeholder_mismatch=${placeholderMismatches.length}`,
    );
    if (missing.length) console.log("    missing:", missing.slice(0, 12).join(", "));
    if (extra.length) console.log("    extra:", extra.slice(0, 12).join(", "));
    if (placeholderMismatches.length)
      console.log("    placeholders:", placeholderMismatches.slice(0, 12).join("; "));
  }
}

// Informational only: keys whose translation is byte-identical to English.
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

console.log("\nUntranslated (identical to en.ts, informational):");
for (const [f, keys] of Object.entries(untranslated).sort()) {
  console.log(`  ${f}: ${keys.length}`);
}

if (failed) {
  console.error(
    "\ni18n parity FAILED. Every key and {placeholder} in en.ts must exist in all locale files. See CONTRIBUTING.md (i18n).",
  );
  process.exit(1);
}
console.log("\ni18n parity OK.");
