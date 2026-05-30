#!/usr/bin/env node
/**
 * Work around Tauri patching linuxdeploy (dd seek=8) breaking AppImage bundling
 * on rolling distros like Arch. Installs a tiny ELF wrapper that survives the
 * patch and execs the real AppImage at `<cache>/linuxdeploy-<arch>.AppImage.real`.
 */
import { spawn } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import { access, chmod, copyFile, mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const NODE_ARCH_TO_LINUXDEPLOY = {
  x64: "x86_64",
  arm64: "aarch64",
};

const LINUXDEPLOY_BASE =
  "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy";

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: "inherit", ...options });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} exited with code ${code ?? "unknown"}`));
    });
  });
}

const LINUX_ENV = {
  APPIMAGELAUNCHER_DISABLE: "1",
  APPIMAGE_EXTRACT_AND_RUN: "1",
};

function runCapture(command, args, options = {}) {
  const { env: extraEnv, ...spawnOptions } = options;
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      stdio: ["ignore", "pipe", "pipe"],
      ...spawnOptions,
      env: { ...process.env, ...LINUX_ENV, ...extraEnv },
    });
    let stdout = "";
    let stderr = "";
    child.stdout?.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr?.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code: code ?? 1, stdout, stderr }));
  });
}

async function exists(path) {
  try {
    await access(path, fsConstants.F_OK);
    return true;
  } catch {
    return false;
  }
}

async function isTinyElf(path) {
  try {
    const header = await readFile(path, { flag: "r" });
    if (header.length < 4 || header[0] !== 0x7f || header[1] !== 0x45) return false;
    const { size } = await stat(path);
    return size < 512 * 1024;
  } catch {
    return false;
  }
}

async function linuxdeployRealLooksValid(path) {
  try {
    const { size } = await stat(path);
    if (size < 1_000_000) return false;
    const { stdout, stderr } = await runCapture(path, ["--help"], {});
    return `${stdout}\n${stderr}`.includes("linuxdeploy");
  } catch {
    return false;
  }
}

async function downloadLinuxdeploy(dest) {
  const url = `${LINUXDEPLOY_BASE}/linuxdeploy-${dest.arch}.AppImage`;
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`Failed to download linuxdeploy (${response.status}) from ${url}`);
  }
  const buffer = Buffer.from(await response.arrayBuffer());
  await writeFile(dest.path, buffer, { mode: 0o755 });
}

async function compileWrapper(outputPath) {
  const sourcePath = join(__dirname, "linuxdeploy-wrapper.c");
  const compilers = ["cc", "gcc"];
  let lastError = null;
  for (const compiler of compilers) {
    try {
      await run(compiler, ["-O2", "-o", outputPath, sourcePath]);
      await chmod(outputPath, 0o755);
      return;
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError ?? new Error("No C compiler available to build linuxdeploy wrapper");
}

async function wrapperWorks(path) {
  const { stdout, stderr } = await runCapture(path, ["--help"], {});
  const output = `${stdout}\n${stderr}`;
  return output.includes("linuxdeploy");
}

async function installWrapper(wrapperPath, realPath) {
  const tempPath = join(tmpdir(), `linuxdeploy-wrapper-${process.pid}`);
  await compileWrapper(tempPath);
  await copyFile(tempPath, wrapperPath);
  await chmod(wrapperPath, 0o755);
  if (!(await wrapperWorks(wrapperPath))) {
    throw new Error("linuxdeploy wrapper installed but failed to launch the real AppImage");
  }
  if (!(await exists(realPath))) {
    throw new Error(`linuxdeploy real binary missing at ${realPath}`);
  }
}

export async function ensureLinuxdeploy() {
  if (process.platform !== "linux") return;

  const deployArch = NODE_ARCH_TO_LINUXDEPLOY[process.arch];
  if (!deployArch) {
    console.warn(`[mooshie] skipping linuxdeploy fix: unsupported arch ${process.arch}`);
    return;
  }

  const cacheDir = join(homedir(), ".cache", "tauri");
  const wrapperPath = join(cacheDir, `linuxdeploy-${deployArch}.AppImage`);
  const realPath = `${wrapperPath}.real`;

  await mkdir(cacheDir, { recursive: true });

  const realOk = (await exists(realPath)) && (await linuxdeployRealLooksValid(realPath));
  if (!realOk) {
    console.info(`[mooshie] downloading linuxdeploy for ${deployArch}…`);
    await downloadLinuxdeploy({ path: realPath, arch: deployArch });
    await chmod(realPath, 0o755);
  }

  const wrapperOk = (await exists(wrapperPath)) && (await isTinyElf(wrapperPath));
  if (!wrapperOk) {
    console.info("[mooshie] installing linuxdeploy wrapper (Tauri AppImage bundler fix)…");
    await installWrapper(wrapperPath, realPath);
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  ensureLinuxdeploy().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  });
}
