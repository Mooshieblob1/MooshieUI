#!/usr/bin/env node
/**
 * Work around Tauri patching linuxdeploy (dd seek=8) breaking AppImage bundling
 * on rolling distros like Arch. Installs a tiny ELF wrapper that survives the
 * patch and execs the real AppImage at `<cache>/linuxdeploy-<arch>.AppImage.real`.
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { constants as fsConstants } from "node:fs";
import {
  access,
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const NODE_ARCH_TO_LINUXDEPLOY = {
  x64: "x86_64",
  arm64: "aarch64",
};

// The same linuxdeploy build Tauri's bundler downloads (tauri-apps/binary-releases
// release "linuxdeploy": linuxdeploy 1-alpha, git 659c9db, built 2024-07-26).
// That release tag is reused for updates, so each asset is pinned by SHA-256 and
// a replaced asset is refused instead of executed. Update URL and hashes together.
const LINUXDEPLOY_BASE =
  "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy";
const LINUXDEPLOY_SHA256 = {
  x86_64: "e762bea85c8eb0d4b3508d46e5c1f037f717d0f9303ae3b4aafc8b04991fa1ef",
  aarch64: "b12b5cc57bd0921e1f98d73f58aa364503bc1a27f54b7a69fd2870bce7fa2f55",
};

function sha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

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

/** Only the pinned build counts as installed; anything else is never executed. */
async function linuxdeployRealIsPinned(path, arch) {
  try {
    return sha256(await readFile(path)) === LINUXDEPLOY_SHA256[arch];
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
  const digest = sha256(buffer);
  if (digest !== LINUXDEPLOY_SHA256[dest.arch]) {
    throw new Error(
      `linuxdeploy from ${url} has SHA-256 ${digest}, expected ${LINUXDEPLOY_SHA256[dest.arch]}; refusing to use it`,
    );
  }
  // Write beside the target and rename, so a failed write never leaves a
  // partial binary at the path that is trusted afterwards.
  const partial = `${dest.path}.download-${process.pid}`;
  await writeFile(partial, buffer, { mode: 0o755 });
  await rename(partial, dest.path);
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
  // A private (0700) directory: a predictable /tmp name could be pre-created or
  // swapped by another local user between compiling and copying.
  const buildDir = await mkdtemp(join(tmpdir(), "linuxdeploy-wrapper-"));
  try {
    const tempPath = join(buildDir, "linuxdeploy-wrapper");
    await compileWrapper(tempPath);
    await copyFile(tempPath, wrapperPath);
  } finally {
    await rm(buildDir, { recursive: true, force: true });
  }
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

  const realOk = await linuxdeployRealIsPinned(realPath, deployArch);
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
