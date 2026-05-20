import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(fileURLToPath(import.meta.url));
const translations = JSON.parse(
  fs.readFileSync(path.join(root, "i18n-external-comfy.json"), "utf8"),
);

const localeFiles = {
  de: "de.ts",
  fr: "fr.ts",
  ja: "ja.ts",
  ko: "ko.ts",
  zh: "zh.ts",
  "zh-tw": "zh-tw.ts",
  pt: "pt.ts",
  ru: "ru.ts",
  it: "it.ts",
  es: "es.ts",
};

for (const [locale, filename] of Object.entries(localeFiles)) {
  const filePath = path.join(root, "..", "src", "lib", "locales", filename);
  let content = fs.readFileSync(filePath, "utf8");
  const strings = translations[locale];
  if (!strings) continue;

  for (const [key, value] of Object.entries(strings)) {
    const escaped = value.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    const pattern = new RegExp(
      `("${key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}":\\s*")([^"]*)(")`,
    );
    if (!pattern.test(content)) {
      console.warn(`[${locale}] missing key: ${key}`);
      continue;
    }
    content = content.replace(pattern, `$1${escaped}$3`);
  }

  fs.writeFileSync(filePath, content);
  console.log(`Updated ${filename}`);
}
