import type { FriendlyError, ReportPayload, ReportSink } from "./types.js";
import { appVersion, appMode, platformInfo } from "../utils/platformInfo.js";
import { openExternalUrl } from "../utils/openExternal.js";
import { exportLogsContent, getConfig } from "../utils/api.js";
import { getLogSnapshot } from "../utils/log-buffer.js";

export const GITHUB_REPO = "Mooshieblob1/MooshieUI";

/**
 * Ceiling for the server-side log build. In browser/hosted mode `exportLogsContent()`
 * is a REST round-trip that can stall (slow diagnostic build, expired auth, dropped
 * connection); without a cap a hung request would silently cost us the whole log.
 */
const LOG_CAPTURE_TIMEOUT_MS = 8000;

/** Reject `p` if it has not settled within `ms`. Does not cancel the underlying work. */
function withTimeout<T>(p: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`log capture timed out after ${ms}ms`)),
      ms,
    );
    p.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}

function errReason(err: unknown): string {
  if (err instanceof Error) return err.message;
  const s = String(err);
  return s === "[object Object]" ? "unknown error" : s;
}

/**
 * Build a fallback log from what we already hold client-side when the server-built
 * log is unavailable. The frontend ring buffer captures console output, uncaught
 * errors, and unhandled rejections, so it is genuinely useful on its own. If even
 * that is empty, return an explicit marker so a report can never again arrive with
 * no log and no explanation (the failure that produced issue #454).
 */
function fallbackLog(why: string): string {
  const { os, arch } = platformInfo();
  const snapshot = getLogSnapshot();
  const header = [
    `(diagnostics: server log capture failed: ${why})`,
    `app ${appVersion()} | mode ${appMode()} | ${os}/${arch}`,
    "--- frontend log buffer follows ---",
  ].join("\n");
  if (snapshot.length > 0) return `${header}\n${snapshot.join("\n")}`;
  return `(log capture failed: ${why}; frontend log buffer empty)`;
}

/**
 * Capture a diagnostics log for a report, degrading gracefully so a report ALWAYS
 * carries a log or an explicit reason it is missing. Never throws.
 *
 * Order: full server-built log (time-bounded) -> frontend ring buffer -> marker.
 */
async function captureDiagnosticLog(): Promise<string> {
  try {
    // Send the whole log — the proxy attaches the full text (chunked into issue
    // comments past GitHub's body limit) so nothing useful is lost.
    const content = await withTimeout(exportLogsContent(), LOG_CAPTURE_TIMEOUT_MS);
    if (content && content.trim()) return content;
    return fallbackLog("server returned an empty log");
  } catch (err) {
    return fallbackLog(errReason(err));
  }
}

/** Assemble a structured report from a resolved error. */
export async function buildReportPayload(
  error: FriendlyError,
  userNote?: string,
  includeLogs = false,
): Promise<ReportPayload> {
  const { os, arch } = platformInfo();
  const logsTail = includeLogs ? await captureDiagnosticLog() : undefined;
  return {
    errorCode: error.code,
    rawMessage: error.raw,
    appVersion: appVersion(),
    os,
    arch,
    mode: appMode(),
    timestamp: new Date().toISOString(),
    userNote: userNote?.trim() || undefined,
    logsTail,
  };
}

/** Build a Markdown issue body. Plain ASCII, no em dashes. */
function issueBody(p: ReportPayload): string {
  const lines = [
    "### What happened",
    p.userNote || "(no description provided)",
    "",
    "### Error",
    "```",
    p.rawMessage || "(empty)",
    "```",
    "",
    "### Environment",
    `- App version: ${p.appVersion}`,
    `- OS: ${p.os}`,
    `- Arch: ${p.arch}`,
    `- Mode: ${p.mode}`,
    `- Error code: ${p.errorCode}`,
    `- When: ${p.timestamp}`,
    "",
    "### Diagnostics",
    "Paste your diagnostics log below (it was copied to your clipboard):",
    "",
    "```",
    "",
    "```",
  ];
  return lines.join("\n");
}

/** Phase A: open a prefilled GitHub New Issue page and copy diagnostics to clipboard. */
class PrefilledIssueSink implements ReportSink {
  async submit(payload: ReportPayload): Promise<{ issueUrl?: string }> {
    // Copy full diagnostics to clipboard since URL length is capped near 8 KB.
    // captureDiagnosticLog never throws and always yields a log or an explicit
    // reason, so the user has something useful to paste even if the build failed.
    try {
      const logs = await captureDiagnosticLog();
      await globalThis.navigator?.clipboard?.writeText(logs);
    } catch {
      // Clipboard may be unavailable; the issue still opens.
    }
    const title = `[in-app] ${payload.errorCode}: ${payload.rawMessage.slice(0, 80)}`;
    const params = new URLSearchParams({
      title,
      body: issueBody(payload),
      labels: "bug,in-app-report",
    });
    const url = `https://github.com/${GITHUB_REPO}/issues/new?${params.toString()}`;
    await openExternalUrl(url);
    return { issueUrl: url };
  }
}

/** Phase B: POST to the NUC proxy, which creates the issue and returns its URL. */
class ProxySink implements ReportSink {
  constructor(private endpoint: string) {}
  async submit(payload: ReportPayload): Promise<{ issueUrl?: string }> {
    const resp = await fetch(this.endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Mooshie-App": "1" },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) throw new Error(`Report endpoint returned ${resp.status}`);
    const data = await resp.json().catch(() => ({}));
    return { issueUrl: data.issueUrl };
  }
}

/** Choose the active sink from config. Proxy when report_endpoint is set. */
async function activeSink(): Promise<{ sink: ReportSink; usesLogs: boolean }> {
  try {
    const cfg = await getConfig();
    const endpoint = cfg.report_endpoint;
    if (endpoint) return { sink: new ProxySink(endpoint), usesLogs: true };
  } catch {
    // Fall through to the prefilled sink.
  }
  return { sink: new PrefilledIssueSink(), usesLogs: false };
}

/** Report a resolved error via the active sink, falling back to a prefilled issue. */
export async function reportError(
  error: FriendlyError,
  userNote?: string,
): Promise<{ issueUrl?: string }> {
  const { sink, usesLogs } = await activeSink();
  const payload = await buildReportPayload(error, userNote, usesLogs);
  try {
    return await sink.submit(payload);
  } catch (err) {
    if (sink instanceof ProxySink) {
      // Proxy unreachable or failed; fall back to a prefilled GitHub issue so the
      // report is never lost. The prefilled sink does not use logsTail.
      const fallback = new PrefilledIssueSink();
      const fallbackPayload = await buildReportPayload(error, userNote, false);
      return fallback.submit(fallbackPayload);
    }
    throw err;
  }
}
