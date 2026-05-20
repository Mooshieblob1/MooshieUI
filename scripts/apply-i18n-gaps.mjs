/**
 * Apply scripts/i18n-gap-translations.json to locale TS files.
 * Patches existing keys or appends missing keys before `export default`.
 */
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, "..");
const inputFile = process.argv[2] ?? "i18n-gap-translations.json";
const translations = JSON.parse(
  fs.readFileSync(path.join(__dirname, inputFile), "utf8"),
);

function escapeValue(v) {
  return String(v).replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

function setKeyInContent(content, key, value) {
  const escaped = escapeValue(value);
  const keyRe = key.replace(/\./g, "\\.");
  const re = new RegExp(
    `("${keyRe}":)\\s*(?:"(?:[^"\\\\]|\\\\.)*"|'(?:[^'\\\\]|\\\\.)*')`,
    "m",
  );
  if (re.test(content)) {
    return content.replace(re, `$1 "${escaped}"`);
  }
  const insertLine = `  "${key}": "${escaped}",\n`;
  if (content.includes(insertLine.trim())) return content;
  // Insert before closing `};` of locale object (not before export default).
  const closeMatch = content.match(/\n\};\s*\nexport default /);
  if (closeMatch) {
    const idx = closeMatch.index;
    return content.slice(0, idx) + `\n${insertLine}` + content.slice(idx);
  }
  return content.replace(/(\n)(export default \w+;)/, `${insertLine}$1$2`);
}

for (const loc of Object.keys(translations)) {
  const file = path.join(root, "src/lib/locales", `${loc}.ts`);
  let content = fs.readFileSync(file, "utf8");
  for (const [key, value] of Object.entries(translations[loc])) {
    content = setKeyInContent(content, key, value);
  }
  fs.writeFileSync(file, content);
  console.log(`patched ${loc}.ts (${Object.keys(translations[loc]).length} keys) from ${inputFile}`);
}
