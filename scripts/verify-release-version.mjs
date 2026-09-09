import fs from 'node:fs';
const tag = process.argv[2];
if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag ?? '')) {
  throw new Error('An explicit release tag is required');
}
const versions = [
  JSON.parse(fs.readFileSync('package.json', 'utf8')).version,
  JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8')).version,
  fs.readFileSync('src-tauri/Cargo.toml', 'utf8').match(/^version = "([^"]+)"/m)?.[1]
];
if (!versions.every(version => version === tag.slice(1))) {
  throw new Error(`Release ${tag} does not match package/Tauri/Cargo versions: ${versions.join(', ')}`);
}
console.log(`All release versions match ${tag}`);
