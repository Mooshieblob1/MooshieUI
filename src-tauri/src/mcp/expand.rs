//! Prompt expansion for MCP requests: `__name__` file wildcards and `{a|b}`
//! alternation.
//!
//! The Generate page expands both in the frontend (`utils/wildcards.ts`,
//! `utils/randomPrompt.ts`) before a request reaches Rust, so an assistant's
//! prompt would otherwise arrive with `__hair__` still in it. This is a port
//! of those two files, seeded the same way, so a fixed seed repeats its picks.
//! In-order wildcards (the editor's "in order" switch) keep their counters in
//! the browser, so here every name picks at random.

use std::collections::BTreeMap;

use regex::Regex;
use serde_json::Value;

/// Lowercased wildcard name to its options. Ordered so `*` globs are stable.
pub type Table = BTreeMap<String, Vec<String>>;

/// splitmix32, the generator both frontend expanders use.
struct SplitMix32(u32);

impl SplitMix32 {
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9);
        let mut z = self.0;
        z = (z ^ (z >> 16)).wrapping_mul(0x85eb_ca6b);
        z = (z ^ (z >> 13)).wrapping_mul(0xc2b2_ae35);
        z ^ (z >> 16)
    }

    fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4_294_967_296.0
    }
}

// ---------------------------------------------------------------------------
// {a|b} alternation
// ---------------------------------------------------------------------------

fn has_top_level_pipe(s: &[char]) -> bool {
    let mut depth = 0usize;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            '\\' if i + 1 < s.len() => i += 1,
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

fn split_top_level_pipes(s: &[char]) -> Vec<Vec<char>> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            '\\' if i + 1 < s.len() => i += 1,
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                parts.push(s[start..i].to_vec());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(s[start..].to_vec());
    parts
}

fn find_top_level_dollar_dollar(s: &[char]) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = 0;
    while i + 1 < s.len() {
        match s[i] {
            '\\' => i += 1,
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => return None,
            '$' if depth == 0 && s[i + 1] == '$' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

struct CountSpec {
    min: usize,
    max: usize,
    sep: String,
    options: Vec<char>,
}

/// `N$$`, `N-M$$` and an optional `sep$$` at the start of a group.
fn parse_count_spec(inner: &[char]) -> Option<CountSpec> {
    let text: String = inner.iter().collect();
    static RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"(?s)^(\d+)(?:-(\d+))?\$\$(.*)$").unwrap());
    let caps = RE.captures(&text)?;
    let min: usize = caps[1].parse().ok()?;
    let max: usize = caps
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(min);
    let after: Vec<char> = caps[3].chars().collect();
    let (sep, options) = match find_top_level_dollar_dollar(&after) {
        Some(pos) => (after[..pos].iter().collect(), after[pos + 2..].to_vec()),
        None => (", ".to_string(), after),
    };
    Some(CountSpec {
        min,
        max: max.max(min),
        sep,
        options,
    })
}

struct WeightedOption {
    text: Vec<char>,
    weight: f64,
}

fn parse_options(parts: Vec<Vec<char>>) -> Vec<WeightedOption> {
    static RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"(?s)^(\d+(?:\.\d+)?)::(.*)$").unwrap());
    parts
        .into_iter()
        .map(|p| {
            let s: String = p.iter().collect();
            match RE.captures(&s) {
                Some(c) => WeightedOption {
                    text: c[2].chars().collect(),
                    weight: c[1].parse().unwrap_or(1.0),
                },
                None => WeightedOption {
                    text: p,
                    weight: 1.0,
                },
            }
        })
        .collect()
}

fn weighted_pick(weights: &[f64], rng: &mut SplitMix32) -> usize {
    let total: f64 = weights.iter().map(|w| w.max(0.0)).sum();
    if total <= 0.0 {
        return 0;
    }
    let mut r = rng.next_f64() * total;
    for (i, w) in weights.iter().enumerate() {
        r -= w.max(0.0);
        if r <= 0.0 {
            return i;
        }
    }
    weights.len() - 1
}

fn process_alternation(inner: &[char], rng: &mut SplitMix32) -> String {
    let (mut count, sep, options) = match parse_count_spec(inner) {
        Some(spec) => {
            let count = if spec.min < spec.max {
                spec.min + (rng.next_u32() as usize) % (spec.max - spec.min + 1)
            } else {
                spec.min
            };
            (count, spec.sep, spec.options)
        }
        None => (1, ", ".to_string(), inner.to_vec()),
    };
    let options = parse_options(split_top_level_pipes(&options));
    if options.is_empty() {
        return String::new();
    }
    count = count.clamp(1, options.len());
    let mut pool: Vec<usize> = (0..options.len()).collect();
    let mut picks = Vec::with_capacity(count);
    for _ in 0..count {
        let weights: Vec<f64> = pool.iter().map(|&i| options[i].weight).collect();
        let local = weighted_pick(&weights, rng);
        picks.push(pool.remove(local));
    }
    picks
        .into_iter()
        .map(|i| expand_inner(&options[i].text, rng))
        .collect::<Vec<_>>()
        .join(&sep)
}

fn expand_inner(text: &[char], rng: &mut SplitMix32) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        if text[i] == '\\' && i + 1 < text.len() {
            out.push(text[i + 1]);
            i += 2;
            continue;
        }
        if text[i] == '{' {
            let mut depth = 1;
            let mut j = i + 1;
            while j < text.len() && depth > 0 {
                if text[j] == '\\' && j + 1 < text.len() {
                    j += 2;
                    continue;
                }
                if text[j] == '{' {
                    depth += 1;
                } else if text[j] == '}' {
                    depth -= 1;
                }
                j += 1;
            }
            let end = if depth == 0 { j - 1 } else { j };
            let inner = &text[i + 1..end];
            if has_top_level_pipe(inner) {
                out.push_str(&process_alternation(inner, rng));
            } else {
                // NovelAI emphasis ({tag}) and the like pass through.
                out.push('{');
                out.push_str(&expand_inner(inner, rng));
                if depth == 0 {
                    out.push('}');
                }
            }
            i = j;
            continue;
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

/// Whether `text` has a `{...}` group with a top-level `|`.
pub fn has_random_syntax(text: &str) -> bool {
    if !text.contains('{') || !text.contains('|') {
        return false;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            i += 2;
            continue;
        }
        if chars[i] == '{' {
            let mut depth = 1;
            let mut j = i + 1;
            while j < chars.len() && depth > 0 {
                if chars[j] == '\\' && j + 1 < chars.len() {
                    j += 2;
                    continue;
                }
                if chars[j] == '{' {
                    depth += 1;
                } else if chars[j] == '}' {
                    depth -= 1;
                }
                j += 1;
            }
            let end = if depth == 0 { j - 1 } else { j };
            if has_top_level_pipe(&chars[i + 1..end]) {
                return true;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    false
}

/// Expand every `{a|b|c}` group, seeded. Groups without a top-level `|` are
/// left as written.
pub fn expand_random(text: &str, seed: u32) -> String {
    if !text.contains('{') || !text.contains('|') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    expand_inner(&chars, &mut SplitMix32(seed))
}

// ---------------------------------------------------------------------------
// Wildcard files
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Tree {
    Leaf(String),
    List(Vec<Tree>),
    Map(Vec<(String, Tree)>),
}

pub fn normalize_name(name: &str) -> String {
    let n = name.trim().replace('\\', "/");
    let mut collapsed = String::with_capacity(n.len());
    for c in n.chars() {
        if c == '/' && collapsed.ends_with('/') {
            continue;
        }
        collapsed.push(c);
    }
    collapsed.trim_matches('/').to_lowercase()
}

fn strip_extension(path: &str) -> &str {
    let lower = path.to_ascii_lowercase();
    for ext in [".txt", ".yaml", ".yml", ".json"] {
        if lower.ends_with(ext) {
            return &path[..path.len() - ext.len()];
        }
    }
    path
}

fn leaves(node: &Tree, out: &mut Vec<String>) {
    match node {
        Tree::Leaf(s) => {
            if !s.trim().is_empty() {
                out.push(s.trim().to_string());
            }
        }
        Tree::List(items) => items.iter().for_each(|c| leaves(c, out)),
        Tree::Map(entries) => entries.iter().for_each(|(_, c)| leaves(c, out)),
    }
}

fn add_tree(table: &mut Table, prefix: &str, node: &Tree, overwrite: bool) {
    let name = normalize_name(prefix);
    if !name.is_empty() && (overwrite || !table.contains_key(&name)) {
        let mut options = Vec::new();
        leaves(node, &mut options);
        if !options.is_empty() {
            table.insert(name, options);
        }
    }
    if let Tree::Map(entries) = node {
        for (key, child) in entries {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}/{key}")
            };
            add_tree(table, &path, child, overwrite);
        }
    }
}

fn json_tree(value: &Value) -> Option<Tree> {
    match value {
        Value::String(s) => Some(Tree::Leaf(s.clone())),
        Value::Number(n) => Some(Tree::Leaf(n.to_string())),
        Value::Bool(b) => Some(Tree::Leaf(b.to_string())),
        Value::Array(items) => Some(Tree::List(items.iter().filter_map(json_tree).collect())),
        Value::Object(map) => Some(Tree::Map(
            map.iter()
                .filter_map(|(k, v)| json_tree(v).map(|t| (k.clone(), t)))
                .collect(),
        )),
        Value::Null => None,
    }
}

fn parse_file(path: &str, content: &str) -> Option<Tree> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".txt") {
        return Some(Tree::List(
            content
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| Tree::Leaf(l.to_string()))
                .collect(),
        ));
    }
    if lower.ends_with(".json") {
        return serde_json::from_str::<Value>(content)
            .ok()
            .and_then(|v| json_tree(&v));
    }
    if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        return parse_simple_yaml(content);
    }
    None
}

/// Build the lookup table from `(relative path, content)` pairs, the way
/// `parseWildcardFiles` does: a file's path names it, map keys name their
/// subtrees, and bare keys never replace a name a file already took.
pub fn parse_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Table {
    let mut table = Table::new();
    let mut bare: Vec<(String, Tree)> = Vec::new();
    for (path, content) in files {
        let Some(tree) = parse_file(path, content) else {
            continue;
        };
        add_tree(&mut table, strip_extension(path), &tree, true);
        if let Tree::Map(entries) = &tree {
            bare.extend(entries.iter().cloned());
        }
    }
    for (key, tree) in &bare {
        add_tree(&mut table, key, tree, false);
    }
    table
}

/// The wildcards folder as a table. Empty when it can't be read.
pub fn load_table() -> Table {
    match crate::commands::wildcards::list() {
        Ok(listing) => parse_files(
            listing
                .files
                .iter()
                .map(|f| (f.path.as_str(), f.content.as_str())),
        ),
        Err(e) => {
            log::warn!("[mcp] could not read wildcards: {e}");
            Table::new()
        }
    }
}

// A small YAML reader, the same subset as the frontend's `parseSimpleYaml`:
// nested maps, `- item` lists, inline `[a, b]` lists, quotes and comments.

struct YamlLine {
    indent: usize,
    text: String,
}

fn strip_yaml_comment(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut quote: Option<char> = None;
    for i in 0..chars.len() {
        let c = chars[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' && (i == 0 || chars[i - 1].is_whitespace()) => {
                return chars[..i].iter().collect();
            }
            None => {}
        }
    }
    line.to_string()
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    let quoted = v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')));
    if quoted {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

fn split_inline_list(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in body.chars() {
        match quote {
            Some(q) => {
                current.push(c);
                if c == q {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                current.push(c);
            }
            None if c == ',' => items.push(std::mem::take(&mut current)),
            None => current.push(c),
        }
    }
    items.push(current);
    items
        .iter()
        .map(|i| unquote(i))
        .filter(|i| !i.is_empty())
        .collect()
}

fn scalar(value: &str) -> Tree {
    let v = value.trim();
    if v.starts_with('[') && v.ends_with(']') && v.len() >= 2 {
        return Tree::List(
            split_inline_list(&v[1..v.len() - 1])
                .into_iter()
                .map(Tree::Leaf)
                .collect(),
        );
    }
    Tree::Leaf(unquote(v))
}

fn key_colon(text: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut quote: Option<char> = None;
    for (n, &(i, c)) in chars.iter().enumerate() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ':' && (n + 1 == chars.len() || chars[n + 1].1 == ' ') => {
                return Some(i);
            }
            None => {}
        }
    }
    None
}

fn is_list_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

fn parse_yaml_block(lines: &[YamlLine], pos: &mut usize, indent: usize) -> Option<Tree> {
    if *pos >= lines.len() {
        return None;
    }
    if is_list_item(&lines[*pos].text) {
        let mut list = Vec::new();
        while *pos < lines.len() && lines[*pos].indent == indent && is_list_item(&lines[*pos].text)
        {
            let item = lines[*pos].text[1..].trim().to_string();
            *pos += 1;
            if !item.is_empty() {
                list.push(scalar(&item));
            } else if *pos < lines.len() && lines[*pos].indent > indent {
                let child_indent = lines[*pos].indent;
                if let Some(child) = parse_yaml_block(lines, pos, child_indent) {
                    list.push(child);
                }
            }
        }
        return Some(Tree::List(list));
    }
    let mut map = Vec::new();
    while *pos < lines.len() && lines[*pos].indent == indent {
        let text = lines[*pos].text.clone();
        *pos += 1;
        let Some(colon) = key_colon(&text) else {
            continue;
        };
        let key = unquote(&text[..colon]);
        let rest = text[colon + 1..].trim();
        if !rest.is_empty() && rest != "|" && rest != ">" {
            map.push((key, scalar(rest)));
        } else if *pos < lines.len()
            && (lines[*pos].indent > indent
                || (lines[*pos].indent == indent && is_list_item(&lines[*pos].text)))
        {
            let child_indent = lines[*pos].indent;
            if let Some(child) = parse_yaml_block(lines, pos, child_indent) {
                map.push((key, child));
            }
        }
    }
    Some(Tree::Map(map))
}

fn parse_simple_yaml(content: &str) -> Option<Tree> {
    let mut lines = Vec::new();
    for raw in content.lines() {
        if raw.trim() == "---" {
            continue;
        }
        let no_comment = strip_yaml_comment(&raw.replace('\t', "  "));
        let no_comment = no_comment.trim_end();
        if no_comment.trim().is_empty() {
            continue;
        }
        let indent = no_comment.len() - no_comment.trim_start().len();
        lines.push(YamlLine {
            indent,
            text: no_comment.trim().to_string(),
        });
    }
    if lines.is_empty() {
        return None;
    }
    let mut pos = 0;
    let first = lines[0].indent;
    parse_yaml_block(&lines, &mut pos, first)
}

// ---------------------------------------------------------------------------
// Wildcard expansion
// ---------------------------------------------------------------------------

static TOKEN_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"__([A-Za-z0-9*][A-Za-z0-9_\-./* ]*?)__").unwrap());

/// Options for a name, with `*` matching any run of characters.
fn lookup<'a>(table: &'a Table, name: &str) -> Option<Vec<&'a String>> {
    if !name.contains('*') {
        return table.get(name).map(|v| v.iter().collect());
    }
    let pattern = format!(
        "^{}$",
        name.split('*')
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join(".*")
    );
    let re = Regex::new(&pattern).ok()?;
    let options: Vec<&String> = table
        .iter()
        .filter(|(k, _)| re.is_match(k))
        .flat_map(|(_, v)| v.iter())
        .collect();
    (!options.is_empty()).then_some(options)
}

const MAX_DEPTH: usize = 8;

fn expand_wildcards_once(
    text: &str,
    table: &Table,
    rng: &mut SplitMix32,
    depth: usize,
    unknown: &mut Vec<String>,
) -> String {
    if depth > MAX_DEPTH || !text.contains("__") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for caps in TOKEN_RE.captures_iter(text) {
        let whole = caps.get(0).unwrap();
        out.push_str(&text[last..whole.start()]);
        last = whole.end();
        let name = normalize_name(&caps[1]);
        match lookup(table, &name) {
            Some(values) if !values.is_empty() => {
                let index = (rng.next_f64() * values.len() as f64) as usize % values.len();
                let pick = values[index].clone();
                out.push_str(&expand_wildcards_once(
                    &pick,
                    table,
                    rng,
                    depth + 1,
                    unknown,
                ));
            }
            _ => {
                if !unknown.contains(&name) {
                    unknown.push(name);
                }
                out.push_str(whole.as_str());
            }
        }
    }
    out.push_str(&text[last..]);
    out
}

fn expand_wildcards(text: &str, table: &Table, seed: u32, unknown: &mut Vec<String>) -> String {
    if !text.contains("__") {
        return text.to_string();
    }
    // A different stream from the {a|b} expander, as in the frontend.
    let mut rng = SplitMix32(seed ^ 0x5bd1_e995);
    expand_wildcards_once(text, table, &mut rng, 0, unknown)
}

/// Expand wildcards, then alternation, then once more each (an option may
/// hold the other), the way `toParams` does. `salt` separates the positive
/// and negative streams. Names that matched no file are added to `unknown`.
pub fn expand_prompt(
    text: &str,
    table: &Table,
    seed: u32,
    salt: u32,
    unknown: &mut Vec<String>,
) -> String {
    let s = seed.wrapping_add(salt);
    let mut out = expand_wildcards(text, table, s, unknown);
    if has_random_syntax(&out) {
        out = expand_random(&out, s);
    }
    if out.contains("__") {
        let s2 = s.wrapping_add(7);
        out = expand_wildcards(&out, table, s2, unknown);
        if has_random_syntax(&out) {
            out = expand_random(&out, s2);
        }
    }
    out
}

/// Whether a prompt needs expanding at all.
pub fn needs_expansion(text: &str) -> bool {
    TOKEN_RE.is_match(text) || has_random_syntax(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        parse_files([
            ("colors.txt", "# comment\nred\n\nblue\n"),
            ("hair/styles.txt", "ponytail\ntwintails"),
            (
                "pack.yaml",
                "outfits:\n  casual:\n    - hoodie\n    - jeans\n  formal: [suit, \"evening gown\"]\nmoods:\n  - happy # smile\n  - __colors__ eyes\n",
            ),
            ("extra.json", r#"{"weather": ["rain", "snow"]}"#),
        ])
    }

    #[test]
    fn text_files_skip_comments_and_blank_lines() {
        assert_eq!(table()["colors"], vec!["red", "blue"]);
        assert_eq!(table()["hair/styles"], vec!["ponytail", "twintails"]);
    }

    #[test]
    fn yaml_and_json_keys_become_names() {
        let t = table();
        assert_eq!(t["pack/outfits/casual"], vec!["hoodie", "jeans"]);
        assert_eq!(t["outfits/formal"], vec!["suit", "evening gown"]);
        assert_eq!(t["outfits"].len(), 4);
        assert_eq!(t["moods"], vec!["happy", "__colors__ eyes"]);
        assert_eq!(t["weather"], vec!["rain", "snow"]);
    }

    #[test]
    fn wildcards_expand_nested_and_report_unknown_names() {
        let t = table();
        let mut unknown = Vec::new();
        for seed in 0..50 {
            let out = expand_prompt("1girl, __moods__, __nope__", &t, seed, 0, &mut unknown);
            assert!(!out.contains("__moods__"));
            assert!(out.contains("__nope__"));
            assert!(
                out.contains("happy") || out.contains("red eyes") || out.contains("blue eyes"),
                "{out}"
            );
        }
        assert_eq!(unknown, vec!["nope"]);
    }

    #[test]
    fn globs_pick_across_names() {
        let t = table();
        let mut unknown = Vec::new();
        let out = expand_prompt("__hair/*__", &t, 3, 0, &mut unknown);
        assert!(out == "ponytail" || out == "twintails");
    }

    #[test]
    fn a_fixed_seed_repeats_its_picks() {
        let t = table();
        let mut u = Vec::new();
        let a = expand_prompt("__colors__, {cat|dog|fox}", &t, 42, 0, &mut u);
        let b = expand_prompt("__colors__, {cat|dog|fox}", &t, 42, 0, &mut u);
        assert_eq!(a, b);
    }

    #[test]
    fn alternation_matches_the_frontend_syntax() {
        for seed in 0..30 {
            let out = expand_random("{a|b|c}", seed);
            assert!(["a", "b", "c"].contains(&out.as_str()));
            let two = expand_random("{2$$ and $$x|y|z}", seed);
            assert_eq!(two.matches(" and ").count(), 1, "{two}");
            let weighted = expand_random("{0::never|1::always}", seed);
            assert_eq!(weighted, "always");
            let nested = expand_random("{a|{b|c}}", seed);
            assert!(["a", "b", "c"].contains(&nested.as_str()));
        }
    }

    #[test]
    fn emphasis_braces_and_escapes_pass_through() {
        assert_eq!(
            expand_random("{masterpiece}, 1girl", 1),
            "{masterpiece}, 1girl"
        );
        assert!(!has_random_syntax("{{best quality}}, \\{a\\|b\\}"));
        assert_eq!(expand_random("x \\{a\\|b\\} {c|c}", 1), "x {a|b} c");
        assert!(has_random_syntax("{a|b}"));
        assert!(!needs_expansion("plain, tags, 1.2::weighted::"));
        assert!(needs_expansion("__colors__"));
    }

    #[test]
    fn splitmix_matches_the_javascript_generator() {
        // First outputs of the frontend's makeSplitmix32(0).
        let mut r = SplitMix32(0);
        assert_eq!(r.next_u32(), 2_462_723_854);
        assert_eq!(r.next_u32(), 1_020_716_019);
    }
}
