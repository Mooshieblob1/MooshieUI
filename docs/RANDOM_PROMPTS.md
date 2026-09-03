# Random Prompt Syntax

MooshieUI supports Dynamic-Prompts-compatible alternation syntax in both the
positive and negative prompt boxes.  Each time you generate, the expander
resolves every alternation block before the prompt reaches the ComfyUI
workflow.  The gallery stores the resolved prompt, so regenerating from
metadata always reproduces the exact same image.

## Basic alternation

Pick one option at random:

    {red|green|blue} car

Every generate click rolls fresh, deterministically seeded from your seed
value so the same seed always picks the same option.

## Picking multiple options

Pick exactly N distinct options, joined with ", ":

    {2$$fluffy|sleek|shiny|matte} texture

Pick a random count in a range:

    {1-3$$bokeh|depth of field|lens flare}

Use a custom separator:

    {2$$ and $$watercolor|oil paint|pencil sketch}

## Weights

Make one option more likely with N:: prefix (default weight is 1):

    {3::masterpiece|1::rough sketch}

Here "masterpiece" is three times as likely as "rough sketch".

## Nesting

Inner blocks are expanded first:

    {a sunny {morning|afternoon}|a rainy evening}

This can produce: "a sunny morning", "a sunny afternoon", or "a rainy evening".

## Literal escapes

Use backslash to include a brace or pipe literally:

    \{not random\}   -> {not random}
    {a\|b|c}        -> picks from "a|b" or "c"

Note: escapes are only processed when alternation syntax is present in the
same prompt string.

## Braces without alternation

NovelAI uses `{tag}` for emphasis and `{tag:1.5}` for weighted emphasis.
Any `{...}` block that contains no top-level `|` is left completely untouched,
so existing NovelAI prompts and prompt scheduling tags work unchanged.

## UI indicator

When the prompt box contains alternation syntax, a small dice badge appears
next to the prompt header.  Hover over it to see an example roll.  Click it
to see a different roll.  The prompt box always shows the template; only the
submitted (resolved) prompt changes each generation.

## Implementation notes

- Expander: `src/lib/utils/randomPrompt.ts`
- Called from `toParams()` in `src/lib/stores/generation.svelte.ts`, right
  after inline preset resolution and before scheduling / regional tag parsing.
- PRNG: splitmix32 seeded from `(userSeed >>> 0)`.  When seed is -1 (random),
  a fresh random integer is chosen for this submission.
- Negative prompt uses `rngSeed + 1` to ensure a different stream from the
  positive prompt.
- The resolved (expanded) prompt is what the workflow and gallery metadata
  receive; the template text stays in the UI.
