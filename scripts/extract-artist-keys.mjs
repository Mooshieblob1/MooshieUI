import fs from "fs";

const en = fs.readFileSync("src/lib/locales/en.ts", "utf8");
const keys = [...en.matchAll(/"(artist_gallery\.[^"]+)":/g)].map((m) => m[1]);
console.log(JSON.stringify({ count: keys.length, keys }, null, 2));
