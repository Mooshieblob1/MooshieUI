#!/usr/bin/env node
import { spawn } from "node:child_process";

const args = process.argv.slice(2);

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

async function main() {
  const firstArg = args[0]?.toLowerCase();
  if (firstArg === "dev") {
    await run("pnpm", ["build"]);
  }
  await run("pnpm", ["exec", "tauri", ...args]);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
