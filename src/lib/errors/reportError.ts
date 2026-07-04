import type { FriendlyError, ReportPayload, ReportSink } from "./types.js";
import { appVersion, appMode, platformInfo } from "../utils/platformInfo.js";
import { openExternalUrl } from "../utils/openExternal.js";
import { exportLogsContent, getConfig } from "../utils/api.js";

export const GITHUB_REPO = "Mooshieblob1/MooshieUI";
const MAX_LOG_TAIL = 200_000; // ~200 KB cap for the proxy payload

/** Assemble a structured report from a resolved error. */
export async function buildReportPayload(
  error: FriendlyError,
  userNote?: string,
  includeLogs = false,
): Promise<ReportPayload> {
  const { os, arch } = platformInfo();
  let logsTail: string | undefined;
  if (includeLogs) {
    try {
      const logs = await exportLogsContent();
      logsTail = logs.length > MAX_LOG_TAIL ? logs.slice(-MAX_LOG_TAIL) : logs;
    } catch {
      logsTail = undefined;
    }
  }
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
    try {
      const logs = await exportLogsContent();
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
