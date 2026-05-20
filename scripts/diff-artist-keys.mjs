import fs from "fs";

function keysIn(file) {
  const text = fs.readFileSync(file, "utf8");
  return new Set([...text.matchAll(/"(artist_gallery\.[^"]+)":/g)].map((m) => m[1]));
}

const en = keysIn("src/lib/locales/en.ts");
const de = keysIn("src/lib/locales/de.ts");
const missing = [...en].filter((k) => !de.has(k));
console.log("missing in de:", missing.length);
for (const k of missing) console.log(k);
