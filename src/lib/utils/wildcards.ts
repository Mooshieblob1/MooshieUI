// File wildcards: `__name__` in a prompt picks one line from a wildcard file.
//
// Files live in `<app data>/wildcards/` (see src-tauri/src/commands/wildcards.rs)
// and follow the Dynamic Prompts conventions, so community packs drop in as is:
//
//   colors.txt           one option per line, `#` lines are comments  -> __colors__
//   hair/styles.txt      folders become path segments                 -> __hair/styles__
//   pack.yaml / .json    nested keys of lists                         -> __key/sub__ (and __pack/key/sub__)
//
// A map key on its own (`__key__`) picks from every option under it, and `*`
// matches any run of characters (`__hair/*__`). An option may hold more
// wildcards or `{a|b}` alternation; both expand in later passes.
//
// Pure functions only: the store (stores/wildcards.svelte.ts) owns the files
// and the in-order counters, and toParams calls `expandWildcards`.

export interface WildcardSource {
  path: string;
  content: string;
}

/** Lowercased wildcard name to its options. */
export type WildcardTable = Map<string, string[]>;

export function normalizeWildcardName(name: string): string {
  return name.trim().replace(/\\/g, "/").replace(/\/+/g, "/").replace(/^\/|\/$/g, "").toLowerCase();
}

function stripExtension(path: string): string {
  return path.replace(/\.(txt|ya?ml|json)$/i, "");
}

function parseTextLines(content: string): string[] {
  return content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"));
}

type Tree = string | Tree[] | { [key: string]: Tree };

/** Every string under a node, depth first. */
function leaves(node: Tree, out: string[] = []): string[] {
  if (typeof node === "string") {
    if (node.trim()) out.push(node.trim());
  } else if (Array.isArray(node)) {
    for (const child of node) leaves(child, out);
  } else if (node && typeof node === "object") {
    for (const child of Object.values(node)) leaves(child, out);
  }
  return out;
}

function addTree(table: WildcardTable, prefix: string, node: Tree, overwrite: boolean) {
  const name = normalizeWildcardName(prefix);
  if (name && (overwrite || !table.has(name))) {
    const options = leaves(node);
    if (options.length > 0) table.set(name, options);
  }
  if (node && typeof node === "object" && !Array.isArray(node)) {
    for (const [key, child] of Object.entries(node)) {
      addTree(table, prefix ? `${prefix}/${key}` : key, child, overwrite);
    }
  }
}

function toTree(value: unknown): Tree | null {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (Array.isArray(value)) return value.map(toTree).filter((v): v is Tree => v !== null);
  if (value && typeof value === "object") {
    const out: { [key: string]: Tree } = {};
    for (const [key, child] of Object.entries(value)) {
      const tree = toTree(child);
      if (tree !== null) out[key] = tree;
    }
    return out;
  }
  return null;
}

function parseFileTree(file: WildcardSource): Tree | null {
  if (/\.txt$/i.test(file.path)) return parseTextLines(file.content);
  if (/\.json$/i.test(file.path)) {
    try {
      return toTree(JSON.parse(file.content));
    } catch {
      return null;
    }
  }
  if (/\.ya?ml$/i.test(file.path)) return toTree(parseSimpleYaml(file.content));
  return null;
}

/**
 * The names a file is best addressed by, with their option counts: its path
 * for a list, or its key paths for a YAML/JSON map (the Dynamic Prompts way).
 */
export function wildcardNamesForFile(file: WildcardSource): Array<{ name: string; count: number }> {
  const tree = parseFileTree(file);
  if (tree === null) return [];
  const table: WildcardTable = new Map();
  if (tree && typeof tree === "object" && !Array.isArray(tree)) {
    for (const [key, child] of Object.entries(tree)) addTree(table, key, child, true);
  } else {
    addTree(table, stripExtension(file.path), tree, true);
  }
  return [...table.entries()]
    .map(([name, options]) => ({ name, count: options.length }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** Build the lookup table from the files in the wildcards folder. */
export function parseWildcardFiles(files: readonly WildcardSource[]): WildcardTable {
  const table: WildcardTable = new Map();
  // Keys without the file name are registered after every file-path name, and
  // never over one, so a file always wins over a same-named key elsewhere.
  const bareKeys: Array<{ key: string; tree: Tree }> = [];
  for (const file of files) {
    const base = stripExtension(file.path);
    const tree = parseFileTree(file);
    if (tree === null) continue;
    addTree(table, base, tree, true);
    if (tree && typeof tree === "object" && !Array.isArray(tree)) {
      for (const [key, child] of Object.entries(tree)) bareKeys.push({ key, tree: child });
    }
  }
  for (const { key, tree } of bareKeys) addTree(table, key, tree, false);
  return table;
}

// ---------------------------------------------------------------------------
// A small YAML reader for wildcard packs: nested maps, `- item` lists, inline
// `[a, b]` lists, quoted strings and `#` comments. Anchors, block scalars and
// multi-document files are out of scope; a line it cannot read is skipped.
// ---------------------------------------------------------------------------

interface YamlLine {
  indent: number;
  text: string;
}

function stripYamlComment(line: string): string {
  let quote: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") {
      quote = c;
    } else if (c === "#" && (i === 0 || /\s/.test(line[i - 1]))) {
      return line.slice(0, i);
    }
  }
  return line;
}

function unquote(value: string): string {
  const v = value.trim();
  if (v.length >= 2 && ((v[0] === '"' && v.endsWith('"')) || (v[0] === "'" && v.endsWith("'")))) {
    return v.slice(1, -1);
  }
  return v;
}

function splitInlineList(body: string): string[] {
  const items: string[] = [];
  let current = "";
  let quote: string | null = null;
  for (const c of body) {
    if (quote) {
      current += c;
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") {
      quote = c;
      current += c;
    } else if (c === ",") {
      items.push(current);
      current = "";
    } else {
      current += c;
    }
  }
  items.push(current);
  return items.map(unquote).filter((item) => item.length > 0);
}

function scalar(value: string): unknown {
  const v = value.trim();
  if (v.startsWith("[") && v.endsWith("]")) return splitInlineList(v.slice(1, -1));
  return unquote(v);
}

/** Index of the `:` that ends a map key, or -1. */
function keyColon(text: string): number {
  let quote: string | null = null;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") {
      quote = c;
    } else if (c === ":" && (i === text.length - 1 || text[i + 1] === " ")) {
      return i;
    }
  }
  return -1;
}

export function parseSimpleYaml(content: string): unknown {
  const lines: YamlLine[] = [];
  for (const raw of content.split(/\r?\n/)) {
    if (raw.trim() === "---") continue;
    const noComment = stripYamlComment(raw.replace(/\t/g, "  ")).replace(/\s+$/, "");
    if (!noComment.trim()) continue;
    lines.push({ indent: noComment.length - noComment.trimStart().length, text: noComment.trim() });
  }
  let pos = 0;

  function parseBlock(indent: number): unknown {
    if (pos >= lines.length) return null;
    if (lines[pos].text.startsWith("- ") || lines[pos].text === "-") {
      const list: unknown[] = [];
      while (pos < lines.length && lines[pos].indent === indent && (lines[pos].text.startsWith("- ") || lines[pos].text === "-")) {
        const item = lines[pos].text.slice(1).trim();
        pos++;
        if (item) {
          list.push(scalar(item));
        } else if (pos < lines.length && lines[pos].indent > indent) {
          list.push(parseBlock(lines[pos].indent));
        }
      }
      return list;
    }
    const map: Record<string, unknown> = {};
    while (pos < lines.length && lines[pos].indent === indent) {
      const { text } = lines[pos];
      const colon = keyColon(text);
      pos++;
      if (colon === -1) continue;
      const key = unquote(text.slice(0, colon));
      const rest = text.slice(colon + 1).trim();
      if (rest && rest !== "|" && rest !== ">") {
        map[key] = scalar(rest);
      } else if (pos < lines.length && (lines[pos].indent > indent || (lines[pos].indent === indent && lines[pos].text.startsWith("- ")))) {
        map[key] = parseBlock(lines[pos].indent);
      } else {
        map[key] = null;
      }
    }
    return map;
  }

  if (lines.length === 0) return null;
  const result = parseBlock(lines[0].indent);
  // Lines indented less than the first one cannot belong anywhere; ignore them.
  return result;
}

// ---------------------------------------------------------------------------
// Expansion
// ---------------------------------------------------------------------------

/** `__name__`, where a name is letters, digits, `_ - . / *` and inner spaces. */
const TOKEN_RE = /__([A-Za-z0-9*][A-Za-z0-9_\-./* ]*?)__/g;

export function findWildcardTokens(text: string): Array<{ start: number; end: number; name: string }> {
  const out: Array<{ start: number; end: number; name: string }> = [];
  if (!text.includes("__")) return out;
  TOKEN_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = TOKEN_RE.exec(text)) !== null) {
    out.push({ start: match.index, end: match.index + match[0].length, name: normalizeWildcardName(match[1]) });
  }
  return out;
}

export function hasWildcardTokens(text: string): boolean {
  return findWildcardTokens(text).length > 0;
}

/** Options for a name, with `*` globbing across every name in the table. */
export function lookupWildcard(table: WildcardTable, name: string): string[] | null {
  if (!name.includes("*")) return table.get(name) ?? null;
  const re = new RegExp(
    `^${name.split("*").map((part) => part.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*")}$`,
  );
  const options: string[] = [];
  for (const [key, values] of table) {
    if (re.test(key)) options.push(...values);
  }
  return options.length > 0 ? options : null;
}

/** splitmix32: small, fast, and seeded, so a fixed seed repeats its picks. */
export function seededRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x9e3779b9) >>> 0;
    let z = state;
    z = Math.imul(z ^ (z >>> 16), 0x85ebca6b) >>> 0;
    z = Math.imul(z ^ (z >>> 13), 0xc2b2ae35) >>> 0;
    z = (z ^ (z >>> 16)) >>> 0;
    return z / 0x100000000;
  };
}

export interface ExpandOptions {
  random: () => number;
  /** Names that step through their options in order instead of at random. */
  ordered?: ReadonlySet<string>;
  /** Next index per in-order name. */
  counters?: Readonly<Record<string, number>>;
  /** Nested wildcards resolved per call; deeper ones are left as written. */
  maxDepth?: number;
}

export interface ExpandResult {
  text: string;
  /** Names that were not found, so the UI can say so. */
  unknown: string[];
  /** In-order names that were used, so their counters can advance once sent. */
  orderedUsed: string[];
}

export function expandWildcards(text: string, table: WildcardTable, options: ExpandOptions): ExpandResult {
  const unknown = new Set<string>();
  const orderedUsed = new Set<string>();
  const maxDepth = options.maxDepth ?? 8;

  const expandOnce = (input: string, depth: number): string => {
    if (depth > maxDepth || !input.includes("__")) return input;
    TOKEN_RE.lastIndex = 0;
    return input.replace(TOKEN_RE, (whole, rawName: string) => {
      const name = normalizeWildcardName(rawName);
      const values = lookupWildcard(table, name);
      if (!values || values.length === 0) {
        unknown.add(name);
        return whole;
      }
      let pick: string;
      if (options.ordered?.has(name)) {
        const index = (options.counters?.[name] ?? 0) % values.length;
        pick = values[index];
        orderedUsed.add(name);
      } else {
        pick = values[Math.floor(options.random() * values.length) % values.length];
      }
      return expandOnce(pick, depth + 1);
    });
  };

  return { text: expandOnce(text, 0), unknown: [...unknown], orderedUsed: [...orderedUsed] };
}
