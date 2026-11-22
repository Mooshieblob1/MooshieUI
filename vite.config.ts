import fs from "node:fs";
import path from "node:path";
import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { parseChangelogReleases } from "./src/lib/utils/changelog.ts";

const host = process.env.TAURI_DEV_HOST;

const VIRTUAL_CHANGELOG_ID = "virtual:changelog-releases";
const RESOLVED_VIRTUAL_CHANGELOG_ID = "\0" + VIRTUAL_CHANGELOG_ID;

function changelogReleasesPlugin(): Plugin {
  const changelogPath = path.resolve(import.meta.dirname, "CHANGELOG.md");
  return {
    name: "mooshieui-changelog-releases",
    resolveId(id) {
      if (id === VIRTUAL_CHANGELOG_ID) return RESOLVED_VIRTUAL_CHANGELOG_ID;
    },
    load(id) {
      if (id !== RESOLVED_VIRTUAL_CHANGELOG_ID) return;
      this.addWatchFile(changelogPath);
      const source = fs.existsSync(changelogPath)
        ? fs.readFileSync(changelogPath, "utf8")
        : "";
      const releases = parseChangelogReleases(source, 15);
      return `export default ${JSON.stringify(releases)};`;
    },
  };
}

export default defineConfig({
  plugins: [svelte(), tailwindcss(), changelogReleasesPlugin()],
  define: {
    __APP_VERSION__: JSON.stringify(process.env.npm_package_version),
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    // Cargo builds into src-tauri/target while Vite is walking the tree, and
    // on Windows a watcher on a DLL the linker still holds throws EBUSY and
    // kills the dev server. Nothing under src-tauri is frontend input.
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
