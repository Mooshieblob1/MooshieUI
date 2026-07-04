/** A raw backend/frontend error resolved into user-facing guidance. */
export interface FriendlyError {
  /** Catalog id, or "unknown" for the generic fallback. */
  code: string;
  title: string;
  what: string;
  why: string;
  /** Ordered steps the user can try. */
  fixes: string[];
  /** Whether a "Report this error" action is offered. */
  reportable: boolean;
  /** Original error text, always preserved for the details block and reports. */
  raw: string;
}

/** Structured payload sent to a ReportSink. */
export interface ReportPayload {
  errorCode: string;
  rawMessage: string;
  appVersion: string;
  os: string;
  arch: string;
  mode: "desktop" | "browser";
  timestamp: string;
  userNote?: string;
  /** Tail of exportLogsContent(); used by the proxy sink, omitted for URL sink. */
  logsTail?: string;
}

/** Destination for a report. Phase A: prefilled URL. Phase B: NUC proxy. */
export interface ReportSink {
  submit(payload: ReportPayload): Promise<{ issueUrl?: string }>;
}
