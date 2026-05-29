#!/usr/bin/env node
import { spawn } from "node:child_process";

const args = process.argv.slice(2);

/** npm, pnpm, or yarn — matches the package manager that invoked this script. */
function packageManager() {
  const userAgent = process.env.npm_config_user_agent ?? "";
  if (userAgent.startsWith("pnpm")) return "pnpm";
  if (userAgent.startsWith("yarn")) return "yarn";
  return "npm";
}

function run(command, commandArgs) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, commandArgs, {
      stdio: "inherit",
      shell: process.platform === "win32",
    });

    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} exited with code ${code ?? "unknown"}`));
    });
  });
}

async function runBuild(pm) {
  if (pm === "npm") {
    await run("npm", ["run", "build"]);
  } else {
    await run(pm, ["build"]);
  }
}

async function runTauri(pm, tauriArgs) {
  await run(pm, ["exec", "tauri", ...tauriArgs]);
}

async function main() {
  const pm = packageManager();
  const firstArg = args[0]?.toLowerCase();
  if (firstArg === "dev") {
    await runBuild(pm);
  }
  await runTauri(pm, args);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
