// Official Antigravity PreToolUse hook. Consume input without storing it and
// reject every tool, including built-in tools that need no ACP permission.
process.stdin.resume();
process.stdin.on('end', () => {
  process.stdout.write(JSON.stringify({
    allowTool: false,
    denyReason: 'Tools are disabled for Prompt Assistant. Reply using only the supplied input.'
  }));
});
