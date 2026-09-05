import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
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
