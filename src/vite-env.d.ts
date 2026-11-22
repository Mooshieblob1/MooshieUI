/// <reference types="svelte" />
/// <reference types="vite/client" />

// Injected by Vite's `define` at build time (see vite.config.ts).
declare const __APP_VERSION__: string;

declare module "virtual:changelog-releases" {
  import type { ChangelogRelease } from "./lib/utils/changelog.js";
  const releases: ChangelogRelease[];
  export default releases;
}
