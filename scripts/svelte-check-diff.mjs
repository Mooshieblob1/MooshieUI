import fs from "fs";

// svelte-check --output machine emits lines like:
//   1783564417942 ERROR "src\\lib\\Foo.svelte" 12:3 "message text"
//   1783564417942 WARNING "src/lib/Bar.svelte" 4:9 "... a11y_... https://svelte.dev/e/a11y_x"
//   1783564417942 COMPLETED 340 FILES 39 ERRORS 90 WARNINGS 32 FILES_WITH_PROBLEMS
const LINE_RE =
  /^\d+\s+(ERROR|WARNING)\s+"((?:[^"\\]|\\.)*)"\s+(\d+):(\d+)\s+"((?:[^"\\]|\\.)*)"/;

function normPath(raw) {
  // Machine output escapes backslashes on Windows: "src\\lib\\x" -> src/lib/x
  return raw.replace(/\\\\/g, "\\").replace(/\\/g, "/");
}

function unescapeMsg(raw) {
  return raw.replace(/\\n/g, " ").replace(/\\"/g, '"').replace(/\\\\/g, "\\").trim();
}

function isA11y(msg) {
  return /a11y[_-]/.test(msg) || /svelte\.dev\/e\/a11y/.test(msg);
}

// changedSet: Set<string> of normalized paths, or null = treat all as in-scope.
export function analyze(machineText, changedSet) {
  const errors = [];
  const a11y = [];
  const otherWarnings = [];
  for (const line of machineText.split(/\r?\n/)) {
    const m = LINE_RE.exec(line);
    if (!m) continue;
    const level = m[1];
    const file = normPath(m[2]);
    const item = { file, line: Number(m[3]), col: Number(m[4]), msg: unescapeMsg(m[5]) };
    if (changedSet && !changedSet.has(file)) continue;
    if (level === "ERROR") errors.push(item);
    else if (isA11y(item.msg) && file.endsWith(".svelte")) a11y.push(item);
    else otherWarnings.push(item);
  }
  return { errors, a11y, otherWarnings };
}

function readChanged(file) {
  const set = new Set();
  for (const raw of fs.readFileSync(file, "utf8").split(/\r?\n/)) {
    const t = raw.trim().replace(/\\/g, "/");
    if (t) set.add(t);
  }
  return set;
}

function a11yMarkdown(a11y) {
  if (!a11y.length) {
    return "### a11y advisory\n\nNo a11y warnings in the files this PR changed. Nice.\n";
  }
  const rows = a11y
    .map((w) => `| \`${w.file}:${w.line}\` | ${w.msg.replace(/\|/g, "\\|")} |`)
    .join("\n");
  return (
    "### a11y advisory (non-blocking)\n\n" +
    "These accessibility warnings are in files this PR changed. They do not block " +
    "the merge, but please fix what you can. See CONTRIBUTING.md (a11y).\n\n" +
    "| Location | Warning |\n|---|---|\n" +
    rows +
    "\n"
  );
}

function report(result, blockOnErrors) {
  const { errors, a11y } = result;
  if (errors.length) {
    console.log(`\nBlocking type errors in changed files (${errors.length}):`);
    for (const e of errors) console.log(`  ${e.file}:${e.line}:${e.col}  ${e.msg}`);
  } else {
    console.log("\nNo blocking type errors in changed files.");
  }
  console.log(`\na11y warnings in changed .svelte files (${a11y.length}, advisory):`);
  for (const w of a11y) console.log(`  ${w.file}:${w.line}:${w.col}  ${w.msg}`);
  if (blockOnErrors && errors.length) {
    console.error(
      `\nFAILED: ${errors.length} svelte-check error(s) in files this PR changed. See CONTRIBUTING.md.`,
    );
    return 1;
  }
  return 0;
}

// ---- CLI ----
const args = process.argv.slice(2);
function flag(name) {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
}

if (args.includes("--self-test")) {
  const sample = [
    '1 ERROR "src/lib/Changed.svelte" 10:5 "Type X not assignable"',
    '1 ERROR "src/lib/Untouched.svelte" 20:1 "baseline error"',
    '1 WARNING "src/lib/Changed.svelte" 12:9 "click events must have key events https://svelte.dev/e/a11y_click_events_have_key_events"',
    '1 WARNING "src/lib/Untouched.svelte" 4:2 "a11y_label_has_associated_control"',
    '1 WARNING "src/lib/Changed.svelte" 1:1 "state_referenced_locally (not a11y)"',
    "1 COMPLETED 2 FILES 2 ERRORS 3 WARNINGS 2 FILES_WITH_PROBLEMS",
  ].join("\n");
  const changed = new Set(["src/lib/Changed.svelte"]);
  const r = analyze(sample, changed);
  const checks = [
    [r.errors.length === 1, `errors scoped to changed: got ${r.errors.length}, want 1`],
    [r.errors[0]?.file === "src/lib/Changed.svelte", "error is from the changed file"],
    [r.a11y.length === 1, `a11y scoped to changed .svelte: got ${r.a11y.length}, want 1`],
    [r.otherWarnings.length === 1, `non-a11y warning kept separate: got ${r.otherWarnings.length}, want 1`],
  ];
  // When changedSet is null, everything is in scope.
  const rAll = analyze(sample, null);
  checks.push([rAll.errors.length === 2, `null scope keeps all errors: got ${rAll.errors.length}, want 2`]);
  // Windows-style escaped path normalizes and matches a forward-slash changed set.
  const win = '1 ERROR "src\\\\lib\\\\Win.svelte" 1:1 "x"';
  const rWin = analyze(win, new Set(["src/lib/Win.svelte"]));
  checks.push([rWin.errors.length === 1, `windows path normalized: got ${rWin.errors.length}, want 1`]);

  let ok = true;
  for (const [pass, label] of checks) {
    console.log(`${pass ? "PASS" : "FAIL"}: ${label}`);
    if (!pass) ok = false;
  }
  process.exit(ok ? 0 : 1);
}

const machineFile = flag("--machine");
const changedFile = flag("--changed");
const summaryOut = flag("--summary");

let machineText = "";
if (machineFile) {
  machineText = fs.readFileSync(machineFile, "utf8");
} else {
  // Informational/local mode with no machine file: nothing to do without input.
  console.log("Usage: node scripts/svelte-check-diff.mjs --machine <file> [--changed <file>] [--summary <out.md>]");
  console.log("       node scripts/svelte-check-diff.mjs --self-test");
  process.exit(0);
}

const changedSet = changedFile ? readChanged(changedFile) : null;
const result = analyze(machineText, changedSet);
if (summaryOut) fs.writeFileSync(summaryOut, a11yMarkdown(result.a11y));
const code = report(result, Boolean(changedSet));
process.exit(code);
