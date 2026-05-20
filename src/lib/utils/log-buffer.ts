// Bounded frontend console ring buffer for diagnostic export.
//
// Intercepts console.{log,info,warn,error,debug} plus uncaught errors and
// unhandled promise rejections, storing the last N formatted lines in
// memory. The buffer is included in exported diagnostic logs so bug reports
// contain frontend state even when the user has no dev-tools access (Tauri
// app mode on Windows/macOS doesn't expose them in release builds).

const MAX_LINES = 1000;
const buffer: string[] = [];

function formatArg(value: unknown): string {
  if (typeof value === "string") return value;
  if (value instanceof Error) {
    return value.stack ?? `${value.name}: ${value.message}`;
  }
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function appendLine(level: string, args: unknown[]): void {
  try {
    const ts = new Date().toISOString();
    const body = args.map(formatArg).join(" ");
    const line = `[${ts}] ${level.toUpperCase().padEnd(5)} - ${body}`;
    buffer.push(line);
    while (buffer.length > MAX_LINES) buffer.shift();
  } catch {
    // Never let logging crash the caller
  }
}

/** Snapshot of the frontend console ring buffer. */
export function getLogSnapshot(): string[] {
  return buffer.slice();
}

/**
 * Install console and global-error interceptors. Safe to call once at app
 * startup; subsequent calls are no-ops.
 */
let installed = false;
export function installConsoleInterceptor(): void {
  if (installed) return;
  installed = true;

  const levels = ["log", "info", "warn", "error", "debug"] as const;
  for (const level of levels) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      appendLine(level, args);
      original(...args);
    };
  }

  window.addEventListener("error", (event) => {
    appendLine("error", [
      `${event.message} at ${event.filename ?? "?"}:${event.lineno ?? 0}:${event.colno ?? 0}`,
      event.error,
    ]);
  });

  window.addEventListener("unhandledrejection", (event) => {
    appendLine("error", ["unhandledrejection:", event.reason]);
  });
}
