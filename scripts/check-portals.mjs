import fs from "fs";
import path from "path";

// Every full-screen overlay (`fixed inset-0`) must carry `use:portal`, so it
// renders on document.body and covers the whole app instead of the side or
// bottom panel it was opened from (see src/lib/utils/portal.ts).
//
// An overlay that is deliberately not portaled, such as the click-catcher
// behind an anchored dropdown, opts out with a `<!-- portal-exempt: reason -->`
// comment just above its opening tag.

const root = "src";

function svelteFiles(dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...svelteFiles(full));
    else if (entry.name.endsWith(".svelte")) out.push(full);
  }
  return out.sort();
}

// End index of the opening tag that starts at `start`, skipping `>` inside
// quoted attribute values and `{...}` expressions.
function tagEnd(src, start) {
  let quote = null;
  let depth = 0;
  for (let i = start + 1; i < src.length; i++) {
    const c = src[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if (depth > 0) {
      if (c === "{") depth++;
      else if (c === "}") depth--;
      else if (c === '"' || c === "'" || c === "`") {
        // String inside an expression: skip to its end.
        const close = src.indexOf(c, i + 1);
        if (close === -1) return -1;
        i = close;
      }
    } else if (c === '"' || c === "'") quote = c;
    else if (c === "{") depth++;
    else if (c === ">") return i;
  }
  return -1;
}

function classTokens(tag) {
  const m = tag.match(/\sclass=(?:"([^"]*)"|'([^']*)')/);
  if (!m) return new Set();
  return new Set((m[1] ?? m[2]).split(/\s+/));
}

// True when the comments directly above the tag (only whitespace between)
// include a `portal-exempt:` marker.
function exempted(before) {
  let rest = before;
  for (;;) {
    rest = rest.trimEnd();
    if (!rest.endsWith("-->")) return false;
    const open = rest.lastIndexOf("<!--");
    if (open === -1) return false;
    if (/^<!--\s*portal-exempt:/.test(rest.slice(open))) return true;
    rest = rest.slice(0, open);
  }
}

function lineOf(src, index) {
  return src.slice(0, index).split("\n").length;
}

const violations = [];

for (const file of svelteFiles(root)) {
  const src = fs.readFileSync(file, "utf8");
  // Markup only: drop <script> and <style> bodies, keeping offsets intact.
  const markup = src.replace(/<(script|style)\b[\s\S]*?<\/\1>/g, (m) => m.replace(/[^\n]/g, " "));
  for (const m of markup.matchAll(/<([a-z][\w-]*)\b/g)) {
    const end = tagEnd(markup, m.index);
    if (end === -1) continue;
    const tag = markup.slice(m.index, end + 1);
    const classes = classTokens(tag);
    if (!classes.has("fixed") || !classes.has("inset-0")) continue;
    if (/\suse:portal\b/.test(tag)) continue;
    if (exempted(src.slice(0, m.index))) continue;
    violations.push(`${file}:${lineOf(markup, m.index)}`);
  }
}

if (violations.length > 0) {
  console.error("Full-screen overlays (fixed inset-0) without use:portal:");
  for (const v of violations) console.error(`  ${v}`);
  console.error(
    "\nAdd `use:portal` (src/lib/utils/portal.ts) so the overlay covers the whole app," +
      "\nor mark a deliberate exception with <!-- portal-exempt: reason --> above the tag.",
  );
  process.exit(1);
}

console.log("Portal check passed: every fixed inset-0 overlay is portaled.");
