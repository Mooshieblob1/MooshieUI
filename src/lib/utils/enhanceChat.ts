/**
 * Enhance chat: the pure half.
 *
 * Enhance is a conversation with the prompt assistant. Every answer is still a
 * prompt in the target model's own format (V5 fields, tags, H3), but the model
 * may also talk to the user: a short note on what it changed, or, when the
 * user only asked a question, an answer with no prompt at all. The talking
 * part rides in one `<reply>` block, which is split off before the prompt is
 * parsed, validated or repaired, so none of that machinery ever sees prose it
 * would mangle.
 *
 * Leaf util: no stores, no IPC, no Svelte, so it runs under plain Node.
 * `CHAT_CLAUSE` and `splitReply` are mirrored in Rust
 * (`grounding::CHAT_RULES` / `grounding::split_reply`) for the plain Enhance,
 * whose system prompt and repair live there. Change them together.
 */

/** Added to the end of every Enhance system prompt. */
export const CHAT_CLAUSE = `CHAT
You are talking with the user in a chat panel beside their prompt box. They can read everything you write and answer you, and the app applies your prompt for them when they choose to.
- When the newest message asks for a prompt or a change to one, answer with the prompt exactly as the output format above says. You may put one short <reply>...</reply> block before it: a sentence or two on what you did or a question back, in the user's language. Never put prompt text inside the reply.
- When the newest message only asks a question, wants advice or an opinion, or talks about something without asking for a change, answer in a <reply>...</reply> block alone and write no prompt, even where the text above says to always return one.
- Everything outside the reply block is the prompt and is applied as it stands, so nothing else may go outside it: no greeting, no explanation, no markdown.`;

const OPEN = /<reply>/i;
const CLOSE = /<\/reply>/i;

export interface SplitReply {
  /** What the model said to the user, trimmed. Empty when it said nothing. */
  reply: string;
  /** Everything else: the prompt, in the target format. Empty for a reply-only answer. */
  prompt: string;
}

/**
 * Pull the `<reply>` blocks out of an answer.
 *
 * Every block is taken, wherever it sits, and joined: a model that writes its
 * note after the prompt instead of before is still understood. A block left
 * open runs to the first blank line, which is where a model that forgot the
 * closing tag almost always started the prompt; with no blank line after it,
 * the whole rest of the answer is the reply.
 */
export function splitReply(raw: string): SplitReply {
  let rest = (raw ?? "").replace(/\r\n/g, "\n");
  const replies: string[] = [];
  for (;;) {
    const open = rest.match(OPEN);
    if (!open || open.index === undefined) break;
    const start = open.index;
    const after = rest.slice(start + open[0].length);
    const close = after.match(CLOSE);
    let body: string;
    let tail: string;
    if (close && close.index !== undefined) {
      body = after.slice(0, close.index);
      tail = after.slice(close.index + close[0].length);
    } else {
      const blank = after.search(/\n[ \t]*\n/);
      body = blank === -1 ? after : after.slice(0, blank);
      tail = blank === -1 ? "" : after.slice(blank);
    }
    if (body.trim()) replies.push(body.trim());
    rest = rest.slice(0, start) + tail;
  }
  // A stray closing tag with no opening one is noise, not prompt text.
  rest = rest.replace(/<\/reply>/gi, "");
  return { reply: replies.join("\n\n"), prompt: rest.trim() };
}

/**
 * One assistant turn as the session stores it: the reply block first, then
 * the prompt. Stored this way, earlier turns show the model the same shape it
 * is asked to answer in.
 */
export function joinReply(reply: string, prompt: string): string {
  const r = reply.trim();
  const p = prompt.trim();
  if (!r) return p;
  return p ? `<reply>${r}</reply>\n${p}` : `<reply>${r}</reply>`;
}
