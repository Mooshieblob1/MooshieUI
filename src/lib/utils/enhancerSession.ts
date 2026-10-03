/**
 * Persistent prompt enhancer sessions: the pure half.
 *
 * Each enhancer flow keeps one conversation that is resent with every request
 * until the user clears it. This file owns the shapes, the token budget and
 * the stored-data parsing; `stores/enhancerSessions.svelte.ts` owns the state.
 * Leaf util: it must not import any store, IPC or Svelte module, which is also
 * what lets it run under plain Node for checking.
 */

export type EnhancerFlow = "nai" | "enhance" | "h3" | "compose";
export const ENHANCER_FLOWS: readonly EnhancerFlow[] = ["nai", "enhance", "h3", "compose"];

export interface SessionTurn {
  /** Exactly the user text that was sent, with images replaced by a marker. */
  user: string;
  /** The final successful answer. */
  assistant: string;
  /** Epoch ms. */
  at: number;
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

/** Past this, the oldest turn is deleted from storage, not just left unsent. */
export const MAX_SESSION_TURNS = 50;
/** History allowance for API and companion backends, below their real windows to bound cost. */
export const REMOTE_CONTEXT_TOKENS = 24_000;
/** Must match `LOCAL_CONTEXT_TOKENS` in src-tauri/src/prompt_assistant/server.rs (the `-c` value). */
export const LOCAL_CONTEXT_TOKENS = 16_384;
/** Plain enhance and Compose build their system prompt in Rust, so the frontend reserves this much for it. */
export const RUST_SYSTEM_RESERVE_TOKENS = 3_000;

/**
 * Conservative for both scripts: ASCII averages closer to 4 characters per
 * token, while Japanese, Chinese and Korean run near one token per character,
 * so counting them like ASCII would undershoot by two to three times.
 */
export function estimateTokens(text: string): number {
  let ascii = 0;
  let other = 0;
  for (const ch of text) {
    if (ch.charCodeAt(0) < 128) ascii++;
    else other++;
  }
  return Math.ceil(ascii / 3) + other;
}

/**
 * Appended to a system prompt only when earlier turns are actually sent.
 * Mirrors `with_session_clause` in src-tauri/src/prompt_assistant/history.rs.
 */
export function withSessionClause(system: string, sentTurns: number): string {
  if (sentTurns <= 0) return system;
  return `${system}

Earlier messages are previous requests in this session and your answers to them. Work only on the newest message. Use earlier turns only when it refers to them, and never carry characters, tags or settings over from them on your own.`;
}

export function imageMarker(count: number, kind: "reference" | "first-frame"): string {
  if (count <= 0) return "";
  if (kind === "first-frame") return "[a first-frame image was attached]";
  return count === 1
    ? "[1 reference image was attached]"
    : `[${count} reference images were attached]`;
}

export function withImageMarker(
  text: string,
  count: number,
  kind: "reference" | "first-frame",
): string {
  const marker = imageMarker(count, kind);
  if (!marker) return text;
  return text.trim() ? `${text}\n${marker}` : marker;
}

export interface HistoryBudget {
  contextTokens: number;
  systemTokens: number;
  userText: string;
  maxOutputTokens: number;
}

export interface BuiltHistory {
  messages: ChatMessage[];
  sentTurns: number;
  droppedTurns: number;
}

/**
 * The newest whole turns that fit, oldest first. Stops at the first turn that
 * does not fit, so the history sent is always one unbroken recent stretch.
 * The new request itself is never trimmed: when it alone overruns the budget,
 * it goes out with no history, exactly as before sessions existed.
 */
export function buildHistory(turns: SessionTurn[], budget: HistoryBudget): BuiltHistory {
  const available = Math.floor(
    (budget.contextTokens -
      budget.systemTokens -
      estimateTokens(budget.userText) -
      budget.maxOutputTokens) *
      0.9,
  );
  const kept: SessionTurn[] = [];
  let used = 0;
  for (let i = turns.length - 1; i >= 0; i--) {
    const cost = estimateTokens(turns[i].user) + estimateTokens(turns[i].assistant);
    if (used + cost > available) break;
    used += cost;
    kept.unshift(turns[i]);
  }
  const messages = kept.flatMap((t): ChatMessage[] => [
    { role: "user", content: t.user },
    { role: "assistant", content: t.assistant },
  ]);
  return { messages, sentTurns: kept.length, droppedTurns: turns.length - kept.length };
}

export function appendTurn(turns: SessionTurn[], turn: SessionTurn): SessionTurn[] {
  return [...turns, turn].slice(-MAX_SESSION_TURNS);
}

export function emptySessions(): Record<EnhancerFlow, SessionTurn[]> {
  return { nai: [], enhance: [], h3: [], compose: [] };
}

function isTurn(value: unknown): value is SessionTurn {
  const t = value as SessionTurn | null;
  return (
    !!t &&
    typeof t.user === "string" &&
    typeof t.assistant === "string" &&
    typeof t.at === "number"
  );
}

/** Whatever is stored, the result is a full, well-formed set of sessions. */
export function parseStoredSessions(raw: string | null): Record<EnhancerFlow, SessionTurn[]> {
  const sessions = emptySessions();
  if (!raw) return sessions;
  let data: unknown;
  try {
    data = JSON.parse(raw);
  } catch {
    return sessions;
  }
  if (!data || typeof data !== "object") return sessions;
  for (const flow of ENHANCER_FLOWS) {
    const list = (data as Record<string, unknown>)[flow];
    if (Array.isArray(list)) sessions[flow] = list.filter(isTurn).slice(-MAX_SESSION_TURNS);
  }
  return sessions;
}
