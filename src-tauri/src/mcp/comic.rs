//! Comic pages over MCP: the Comic page builder's layouts, prompt rules and
//! page assembly, so an assistant can lay out a page, write every panel and
//! get the finished page back.
//!
//! Mirrors `src/lib/utils/comicLayout.ts` (templates, reading order, panel
//! sizes, gutters, prompt assembly, lettering) and the panel loop and page
//! assembly in `src/lib/utils/comicRunner.ts`. Change them together.
//!
//! Each panel is an ordinary MCP job (`jobs::start`), so it runs on the
//! owner's saved setup, lands in the gallery and, on NovelAI, stays inside the
//! free Opus window. The page is assembled here and saved to the gallery too.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::watch;

use super::jobs::{self, JobStatus, SavedImage, Store, Tracked};
use super::{library, setups};
use crate::comfyui::types::GenerationParams;
use crate::novelai::face_pass::FREE_PIXELS;
use crate::novelai::params::{NovelAiCharacter, NovelAiCoord};
use crate::state::AppState;

pub const MAX_PANELS: usize = 9;
/// NovelAI takes six character prompts at most, so a one-image page has six panels at most.
pub const ONE_IMAGE_MAX_PANELS: usize = 6;
const MIN_RATIO: f64 = 0.12;
const MAX_GUTTER: u32 = 80;
const DEFAULT_GUTTER: u32 = 24;
/// The longest a single panel may take before the page gives up on it.
const PANEL_WAIT: Duration = Duration::from_secs(31 * 60);

// ---------------------------------------------------------------------------
// Layouts
// ---------------------------------------------------------------------------

/// A layout is a binary split tree, as on the Comic page. A leaf is a panel; a
/// split divides its box at `ratio`, side by side (`row`) or stacked.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Panel(usize),
    Split {
        row: bool,
        ratio: f64,
        a: Box<Node>,
        b: Box<Node>,
    },
}

fn p() -> Node {
    Node::Panel(0)
}

fn row(ratio: f64, a: Node, b: Node) -> Node {
    Node::Split {
        row: true,
        ratio,
        a: Box::new(a),
        b: Box::new(b),
    }
}

fn col(ratio: f64, a: Node, b: Node) -> Node {
    Node::Split {
        row: false,
        ratio,
        a: Box::new(a),
        b: Box::new(b),
    }
}

/// Number the leaves 0, 1, 2... in tree order.
fn numbered(node: Node) -> Node {
    fn walk(n: Node, next: &mut usize) -> Node {
        match n {
            Node::Panel(_) => {
                let id = *next;
                *next += 1;
                Node::Panel(id)
            }
            Node::Split { row, ratio, a, b } => {
                let a = walk(*a, next);
                let b = walk(*b, next);
                Node::Split {
                    row,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }
            }
        }
    }
    walk(node, &mut 0)
}

/// Template ids and their panel counts, the same as the Comic page's.
pub const TEMPLATES: &[(&str, usize)] = &[
    ("full", 1),
    ("two_rows", 2),
    ("two_columns", 2),
    ("three_rows", 3),
    ("one_over_two", 3),
    ("two_over_one", 3),
    ("tall_left_two_right", 3),
    ("four_rows", 4),
    ("grid_2x2", 4),
    ("one_two_one", 4),
    ("one_one_two", 4),
    ("two_one_two", 5),
    ("one_two_two", 5),
    ("grid_2x3", 6),
    ("two_one_three", 6),
];

pub fn template(id: &str) -> Option<Node> {
    const T: f64 = 1.0 / 3.0;
    let shape = match id {
        "full" => p(),
        "two_rows" => col(0.5, p(), p()),
        "two_columns" => row(0.5, p(), p()),
        "three_rows" => col(T, p(), col(0.5, p(), p())),
        "one_over_two" => col(0.5, p(), row(0.5, p(), p())),
        "two_over_one" => col(0.5, row(0.5, p(), p()), p()),
        "tall_left_two_right" => row(0.55, p(), col(0.5, p(), p())),
        "four_rows" => col(0.25, p(), col(T, p(), col(0.5, p(), p()))),
        "grid_2x2" => col(0.5, row(0.5, p(), p()), row(0.5, p(), p())),
        "one_two_one" => col(0.3, p(), col(0.55, row(0.5, p(), p()), p())),
        "one_one_two" => col(0.3, p(), col(0.45, p(), row(0.5, p(), p()))),
        "two_one_two" => col(0.32, row(0.6, p(), p()), col(0.5, p(), row(0.4, p(), p()))),
        "one_two_two" => col(0.3, p(), col(0.5, row(0.5, p(), p()), row(0.6, p(), p()))),
        "grid_2x3" => col(
            T,
            row(0.5, p(), p()),
            col(0.5, row(0.5, p(), p()), row(0.5, p(), p())),
        ),
        "two_one_three" => col(
            0.3,
            row(0.6, p(), p()),
            col(0.45, p(), row(T, p(), row(0.5, p(), p()))),
        ),
        _ => return None,
    };
    Some(numbered(shape))
}

/// The template the Comic page would pick for `count` panels, or rows for
/// counts no template has.
fn default_layout(count: usize) -> (String, Node) {
    let id = match count {
        1 => "full",
        2 => "two_rows",
        3 => "one_over_two",
        4 => "grid_2x2",
        5 => "two_one_two",
        6 => "grid_2x3",
        _ => "",
    };
    if let Some(node) = template(id) {
        return (id.to_string(), node);
    }
    // 7: 1 + 2 + 2 + 2, 8: 2 + 2 + 2 + 2, 9: 3 + 3 + 3.
    let rows: Vec<usize> = match count {
        7 => vec![1, 2, 2, 2],
        8 => vec![2, 2, 2, 2],
        _ => vec![3, 3, 3],
    };
    let specs = rows
        .iter()
        .map(|n| RowIn {
            panels: Some(*n),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    ("rows".into(), rows_layout(&specs).expect("valid rows"))
}

/// One row of a custom layout: its share of the page height and its panels'
/// shares of its width.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RowIn {
    pub height: Option<f64>,
    pub panels: Option<usize>,
    pub widths: Option<Vec<f64>>,
}

/// Weighted pieces chained into splits: the first takes its share, the rest
/// share what is left.
fn chain(mut items: Vec<(f64, Node)>, is_row: bool) -> Node {
    if items.len() == 1 {
        return items.remove(0).1;
    }
    let total: f64 = items.iter().map(|(w, _)| *w).sum();
    let (w, first) = items.remove(0);
    let ratio = (w / total).clamp(MIN_RATIO, 1.0 - MIN_RATIO);
    let rest = chain(items, is_row);
    Node::Split {
        row: is_row,
        ratio,
        a: Box::new(first),
        b: Box::new(rest),
    }
}

/// A layout from rows, top to bottom.
pub fn rows_layout(rows: &[RowIn]) -> Result<Node, String> {
    if rows.is_empty() {
        return Err("rows needs at least one row".into());
    }
    let weight = |w: Option<f64>| -> Result<f64, String> {
        match w {
            None => Ok(1.0),
            Some(v) if v.is_finite() && v > 0.0 => Ok(v),
            Some(_) => Err("row heights and panel widths must be above 0".into()),
        }
    };
    let mut items = Vec::new();
    for r in rows {
        let widths = match (&r.widths, r.panels) {
            (Some(w), _) if !w.is_empty() => w.clone(),
            (_, Some(n)) if n >= 1 => vec![1.0; n],
            (_, Some(_)) => return Err("a row needs at least one panel".into()),
            _ => vec![1.0],
        };
        let mut cells = Vec::new();
        for w in widths {
            cells.push((weight(Some(w))?, p()));
        }
        items.push((weight(r.height)?, chain(cells, true)));
    }
    Ok(numbered(chain(items, false)))
}

/// A box in page units: 0..1 on both axes, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

fn split_rect(r: Rect, is_row: bool, ratio: f64) -> (Rect, Rect) {
    if is_row {
        let wa = r.w * ratio;
        (
            Rect { w: wa, ..r },
            Rect {
                x: r.x + wa,
                w: r.w - wa,
                ..r
            },
        )
    } else {
        let ha = r.h * ratio;
        (
            Rect { h: ha, ..r },
            Rect {
                y: r.y + ha,
                h: r.h - ha,
                ..r
            },
        )
    }
}

/// Every panel's box, by leaf id.
pub fn rects(node: &Node) -> HashMap<usize, Rect> {
    fn walk(n: &Node, r: Rect, out: &mut HashMap<usize, Rect>) {
        match n {
            Node::Panel(id) => {
                out.insert(*id, r);
            }
            Node::Split { row, ratio, a, b } => {
                let (ra, rb) = split_rect(r, *row, *ratio);
                walk(a, ra, out);
                walk(b, rb, out);
            }
        }
    }
    let mut out = HashMap::new();
    walk(
        node,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
        &mut out,
    );
    out
}

/// Reading order: top to bottom, and within a band left to right, or right to
/// left for manga. Two panels share a band when their tops are within 4% of
/// the page. An insertion sort, because the band rule is not a total order.
pub fn reading_order(rects: &HashMap<usize, Rect>, right_to_left: bool) -> Vec<usize> {
    let before = |p: &Rect, q: &Rect| -> bool {
        if (p.y - q.y).abs() > 0.04 {
            return p.y < q.y;
        }
        if right_to_left {
            p.x + p.w > q.x + q.w
        } else {
            p.x < q.x
        }
    };
    let mut ids: Vec<usize> = rects.keys().copied().collect();
    ids.sort_unstable();
    let mut out: Vec<usize> = Vec::with_capacity(ids.len());
    for id in ids {
        let r = rects[&id];
        let at = out
            .iter()
            .position(|o| before(&r, &rects[o]))
            .unwrap_or(out.len());
        out.insert(at, id);
    }
    out
}

pub fn page_aspect(shape: &str) -> f64 {
    match shape {
        "landscape" => std::f64::consts::SQRT_2,
        "square" => 1.0,
        _ => 1.0 / std::f64::consts::SQRT_2,
    }
}

/// Output page size in pixels: the long edge is 2048.
pub fn page_size(shape: &str) -> (u32, u32) {
    let aspect = page_aspect(shape);
    let long = 2048.0;
    if aspect >= 1.0 {
        (long as u32, (long / aspect).round() as u32)
    } else {
        ((long * aspect).round() as u32, long as u32)
    }
}

fn snap64(v: f64) -> u32 {
    ((v / 64.0).round() * 64.0).max(256.0) as u32
}

/// Shrink the longer side 64 px at a time until `w * h` fits `cap`.
fn fit_pixels(mut w: u32, mut h: u32, cap: u64) -> (u32, u32) {
    while u64::from(w) * u64::from(h) > cap && (w > 256 || h > 256) {
        if w >= h {
            w -= 64;
        } else {
            h -= 64;
        }
    }
    (w, h)
}

/// Size to generate a panel at: its on-page aspect (clamped to 1:3..3:1, the
/// page crops the rest), the setup's pixel count, multiples of 64. `cap`
/// keeps NovelAI inside its free window.
pub fn generation_size(rect: Rect, page: (u32, u32), budget: u64, cap: Option<u64>) -> (u32, u32) {
    let raw = (rect.w * f64::from(page.0)) / (rect.h * f64::from(page.1)).max(1.0);
    let aspect = raw.clamp(1.0 / 3.0, 3.0);
    let pixels = budget.max(256 * 256) as f64;
    let (w, h) = (
        snap64((pixels * aspect).sqrt()),
        snap64((pixels / aspect).sqrt()),
    );
    match cap {
        Some(cap) => fit_pixels(w, h, cap),
        None => (w, h),
    }
}

/// A panel's box on the output page, inset for gutters, in whole pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PxRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub fn pixel_rect(rect: Rect, page: (u32, u32), gutter: u32) -> PxRect {
    let (pw, ph) = (f64::from(page.0), f64::from(page.1));
    let half = f64::from(gutter) / 2.0;
    let edge = |at_edge: bool| if at_edge { half } else { 0.0 };
    let x0 = rect.x * pw + half + edge(rect.x <= 1e-6);
    let y0 = rect.y * ph + half + edge(rect.y <= 1e-6);
    let x1 = (rect.x + rect.w) * pw - half - edge(rect.x + rect.w >= 1.0 - 1e-6);
    let y1 = (rect.y + rect.h) * ph - half - edge(rect.y + rect.h >= 1.0 - 1e-6);
    PxRect {
        x: x0.round().max(0.0) as u32,
        y: y0.round().max(0.0) as u32,
        w: (x1 - x0).round().max(1.0) as u32,
        h: (y1 - y0).round().max(1.0) as u32,
    }
}

/// Source crop (x, y, w, h) that fills `target` from an image of `src` size.
pub fn cover_crop(src: (u32, u32), target: (u32, u32)) -> (f64, f64, f64, f64) {
    let (sw, sh) = (f64::from(src.0), f64::from(src.1));
    let (tw, th) = (f64::from(target.0), f64::from(target.1));
    let scale = (tw / sw).max(th / sh);
    let (w, h) = (tw / scale, th / scale);
    ((sw - w) / 2.0, (sh - h) / 2.0, w, h)
}

/// NovelAI character position for a one-image panel: its centre, rounded to
/// two places and kept off the edges.
pub fn one_image_centre(r: Rect) -> (f64, f64) {
    let clamp = |v: f64| ((v * 100.0).round() / 100.0).clamp(0.1, 0.9);
    (clamp(r.x + r.w / 2.0), clamp(r.y + r.h / 2.0))
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

pub fn join_tags(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|p| {
            p.trim()
                .trim_matches(|c: char| c == ',' || c.is_whitespace())
                .to_string()
        })
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn tag_list(text: &str) -> Vec<String> {
    text.split([',', '\n'])
        .map(|t| {
            t.trim()
                .trim_start_matches(['(', '[', '{'])
                .trim_end_matches([')', ']', '}'])
                .replace('_', " ")
                .to_lowercase()
                .trim()
                .to_string()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// Bubbles from a list of strings or one string with a bubble per line.
pub fn dialogue_lines(value: &Value) -> Result<Vec<String>, String> {
    let lines = |s: &str| -> Vec<String> {
        s.lines()
            .map(|l| l.trim().trim_matches('"').trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    };
    match value {
        Value::Null => Ok(Vec::new()),
        Value::String(s) => Ok(lines(s)),
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                out.extend(lines(
                    item.as_str()
                        .ok_or("dialogue must be strings, one per speech bubble")?,
                ));
            }
            Ok(out)
        }
        _ => Err("dialogue must be strings, one per speech bubble".into()),
    }
}

pub fn lettering_language(lines: &[String]) -> &'static str {
    let has = |lo: u32, hi: u32| {
        lines
            .iter()
            .flat_map(|l| l.chars())
            .any(|c| (lo..=hi).contains(&(c as u32)))
    };
    if has(0x3040, 0x30ff) {
        "japanese text"
    } else if has(0xac00, 0xd7af) {
        "korean text"
    } else if has(0x4e00, 0x9fff) {
        "chinese text"
    } else {
        "english text"
    }
}

/// Tags lettering needs that `prompt` lacks: a bubble and the text tag for
/// the language it is written in.
pub fn lettering_tags(prompt: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let tags = tag_list(prompt);
    let mut out = Vec::new();
    if !tags.iter().any(|t| t.ends_with("bubble")) {
        out.push("speech bubble");
    }
    if !tags.iter().any(|t| t.ends_with(" text")) {
        out.push(lettering_language(lines));
    }
    out.join(", ")
}

/// The Text: block NovelAI letters: the words only, a blank line between
/// bubbles. Always the last thing in a prompt.
pub fn text_block(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("\n\nText:\n{}", lines.join("\n\n"))
    }
}

/// A prompt's tag line and the prose after its first blank line.
pub fn split_tag_line(prompt: &str) -> (String, String) {
    let lines: Vec<&str> = prompt.trim().lines().collect();
    match lines.iter().position(|l| l.trim().is_empty()) {
        Some(i) => (
            lines[..i].join("\n").trim().to_string(),
            lines[i..].join("\n").trim().to_string(),
        ),
        None => (prompt.trim().to_string(), String::new()),
    }
}

/// Take a Text: block off a prompt: the prompt without it, and its bubbles.
pub fn take_text_block(prompt: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = prompt.lines().collect();
    match lines.iter().position(|l| l.trim() == "Text:") {
        Some(i) => (
            lines[..i].join("\n").trim().to_string(),
            lines[i + 1..]
                .iter()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
        ),
        None => (prompt.trim().to_string(), Vec::new()),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CastIn {
    pub name: String,
    pub tags: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PanelIn {
    pub prompt: String,
    pub dialogue: Value,
    /// Page cast names in this panel.
    pub cast: Vec<String>,
    /// Saved MooshieUI characters in this panel.
    pub characters: Vec<String>,
    /// Replaces the page setting for this panel.
    pub setting: String,
    pub negative_prompt: Option<String>,
}

/// A panel as it will be drawn.
#[derive(Debug, Clone)]
pub struct Panel {
    pub prompt: String,
    pub dialogue: Vec<String>,
    pub cast: Vec<usize>,
    pub characters: Vec<String>,
    pub setting: String,
    pub negative_prompt: Option<String>,
}

/// The page fields every panel prompt draws on.
#[derive(Debug, Clone, Default)]
pub struct PageText {
    pub style: String,
    pub setting: String,
    pub cast: Vec<CastIn>,
}

impl Panel {
    fn setting<'a>(&'a self, page: &'a PageText) -> &'a str {
        if self.setting.trim().is_empty() {
            page.setting.trim()
        } else {
            self.setting.trim()
        }
    }

    fn cast_tags<'a>(&self, page: &'a PageText) -> Vec<&'a str> {
        self.cast
            .iter()
            .filter_map(|i| page.cast.get(*i))
            .map(|c| c.tags.as_str())
            .collect()
    }

    /// The panel on its own, without its Text: block (that goes on last,
    /// after saved styles and chunks are added). The cast and setting join
    /// the tag line, before any prose. Lettering tags only with `lettering`.
    pub fn prompt(&self, page: &PageText, lettering: bool) -> String {
        let (tags, prose) = split_tag_line(&self.prompt);
        let extra = if lettering {
            lettering_tags(&self.prompt, &self.dialogue)
        } else {
            String::new()
        };
        let mut parts = vec![page.style.as_str(), tags.as_str()];
        let cast = self.cast_tags(page);
        parts.extend(cast.iter().copied());
        parts.push(self.setting(page));
        parts.push(&extra);
        let line = join_tags(&parts);
        [line, prose]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Character prompt for this panel on a one-image NovelAI page.
    pub fn one_image_prompt(&self, number: usize, page: &PageText) -> String {
        let label = format!("panel {number}");
        let mut parts = vec![label.as_str(), self.prompt.as_str()];
        let cast = self.cast_tags(page);
        parts.extend(cast.iter().copied());
        parts.push(self.setting.trim());
        join_tags(&parts)
    }
}

/// Base prompt for a one-image NovelAI page, without its Text: block.
pub fn one_image_base(page: &PageText, panels: usize, dialogue: &[String]) -> String {
    let count = if panels > 1 {
        format!("{panels} panels, multiple views")
    } else {
        String::new()
    };
    let base = join_tags(&[&page.style, "comic", &count, &page.setting]);
    let tags = lettering_tags(&base, dialogue);
    join_tags(&[&base, &tags])
}

// ---------------------------------------------------------------------------
// The request
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct SpecIn {
    layout: Option<String>,
    rows: Option<Vec<RowIn>>,
    panels: Vec<PanelIn>,
    cast: Vec<CastIn>,
    style: String,
    setting: String,
    page_shape: Option<String>,
    gutter: Option<u32>,
    right_to_left: bool,
    mode: Option<String>,
    model: Option<String>,
    negative_prompt: Option<String>,
    seed: Option<i64>,
    steps: Option<u32>,
    cfg: Option<f64>,
    sampler: Option<String>,
    scheduler: Option<String>,
}

/// Everything a comic job needs, checked.
#[derive(Debug, Clone)]
pub struct Plan {
    pub layout_name: String,
    pub layout: Node,
    /// Leaf ids in reading order; panel `i` is drawn in `order[i]`.
    pub order: Vec<usize>,
    pub panels: Vec<Panel>,
    pub page: PageText,
    pub shape: String,
    pub gutter: u32,
    pub one_image: bool,
    pub setup: GenerationParams,
    pub negative_prompt: Option<String>,
    pub seed: Option<i64>,
    pub steps: Option<u32>,
    pub cfg: Option<f64>,
    pub sampler: Option<String>,
    pub scheduler: Option<String>,
    /// `styles`, `chunks` and `use_active_styles` from the call.
    pub additions: Value,
}

impl Plan {
    pub fn nai(&self) -> bool {
        setups::is_novelai(&self.setup)
    }

    fn rect(&self, i: usize) -> Rect {
        rects(&self.layout)[&self.order[i]]
    }

    /// The pixels the owner's setup generates at, held to NovelAI's free window.
    fn budget(&self) -> (u64, Option<u64>) {
        let pixels = u64::from(self.setup.width) * u64::from(self.setup.height);
        if self.nai() {
            (pixels.min(FREE_PIXELS), Some(FREE_PIXELS))
        } else {
            (pixels, None)
        }
    }
}

fn nai_model(params: &GenerationParams) -> Option<&'static crate::novelai::models::NovelAiModel> {
    let nai = params.novelai.as_ref()?;
    let id = if crate::novelai::models::is_novelai_model(&params.checkpoint) {
        params.checkpoint.clone()
    } else {
        nai.model.clone()
    };
    crate::novelai::models::find(&id)
}

pub fn plan(args: &Value) -> Result<Plan, String> {
    let spec: SpecIn =
        serde_json::from_value(args.clone()).map_err(|e| format!("Invalid arguments: {e}"))?;
    let count = spec.panels.len();
    if count == 0 {
        return Err("panels is required: one entry per panel, in reading order".into());
    }
    if count > MAX_PANELS {
        return Err(format!("A page takes {MAX_PANELS} panels at most."));
    }
    let (layout_name, layout) = match (&spec.layout, &spec.rows) {
        (Some(_), Some(_)) => return Err("Give layout or rows, not both.".into()),
        (Some(id), None) => {
            let node = template(id.trim()).ok_or_else(|| {
                format!(
                    "Unknown layout '{id}'. Layouts: {}",
                    TEMPLATES
                        .iter()
                        .map(|(t, n)| format!("{t} ({n})"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            (id.trim().to_string(), node)
        }
        (None, Some(rows)) => ("rows".into(), rows_layout(rows)?),
        (None, None) => default_layout(count),
    };
    let leaves = rects(&layout).len();
    if leaves != count {
        return Err(format!(
            "The layout has {leaves} panels but panels has {count} entries. Give one entry per panel."
        ));
    }
    let shape = match spec.page_shape.as_deref().map(str::trim) {
        None | Some("") | Some("portrait") => "portrait",
        Some("square") => "square",
        Some("landscape") => "landscape",
        Some(other) => {
            return Err(format!(
                "page_shape must be portrait, square or landscape, not '{other}'"
            ))
        }
    };
    let one_image = match spec.mode.as_deref().map(str::trim) {
        None | Some("") | Some("panels") => false,
        Some("one_image") => true,
        Some(other) => return Err(format!("mode must be panels or one_image, not '{other}'")),
    };
    let gutter = spec.gutter.unwrap_or(DEFAULT_GUTTER);
    if gutter > MAX_GUTTER {
        return Err(format!("gutter is {MAX_GUTTER} px at most"));
    }
    let cast = spec
        .cast
        .iter()
        .map(|c| CastIn {
            name: c.name.trim().to_string(),
            tags: c.tags.trim().to_string(),
        })
        .collect::<Vec<_>>();
    let mut panels = Vec::with_capacity(count);
    for (i, raw) in spec.panels.iter().enumerate() {
        let (prompt, mut dialogue) = take_text_block(&raw.prompt);
        dialogue.extend(dialogue_lines(&raw.dialogue)?);
        if prompt.is_empty() {
            return Err(format!("Panel {} has no prompt.", i + 1));
        }
        if prompt.chars().count() > 6000 {
            return Err(format!("Panel {}'s prompt is too long.", i + 1));
        }
        let mut members = Vec::new();
        for name in &raw.cast {
            let wanted = name.trim().to_lowercase();
            let at = cast
                .iter()
                .position(|c| c.name.to_lowercase() == wanted)
                .ok_or_else(|| {
                    format!(
                        "Panel {} names '{name}', who is not in cast. Add them to cast with their appearance tags.",
                        i + 1
                    )
                })?;
            members.push(at);
        }
        panels.push(Panel {
            prompt,
            dialogue,
            cast: members,
            characters: raw.characters.clone(),
            setting: raw.setting.trim().to_string(),
            negative_prompt: raw.negative_prompt.clone(),
        });
    }
    let setup = setups::find(
        &setups::list(),
        spec.model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty()),
        setups::Purpose::Generate,
    )?;
    if one_image {
        let model = nai_model(&setup.params)
            .filter(|m| m.v4_prompt)
            .ok_or("mode one_image draws the whole page as one NovelAI image, so it needs a NovelAI V4 or later model.")?;
        let max = model.max_characters.min(ONE_IMAGE_MAX_PANELS);
        if count > max {
            return Err(format!("mode one_image takes {max} panels at most."));
        }
    }
    let rects = rects(&layout);
    let order = reading_order(&rects, spec.right_to_left);
    Ok(Plan {
        layout_name,
        layout,
        order,
        panels,
        page: PageText {
            style: spec.style.trim().to_string(),
            setting: spec.setting.trim().to_string(),
            cast,
        },
        shape: shape.to_string(),
        gutter,
        one_image,
        setup: setup.params,
        negative_prompt: spec.negative_prompt,
        seed: spec.seed.filter(|s| *s >= 0),
        steps: spec.steps,
        cfg: spec.cfg,
        sampler: spec.sampler,
        scheduler: spec.scheduler,
        additions: json!({
            "styles": args.get("styles").cloned().unwrap_or(Value::Null),
            "chunks": args.get("chunks").cloned().unwrap_or(Value::Null),
            "use_active_styles": args.get("use_active_styles").cloned().unwrap_or(Value::Null),
        }),
    })
}

/// The overrides for one request of the page.
fn overrides(
    plan: &Plan,
    prompt: String,
    negative: Option<String>,
    size: (u32, u32),
    seed_offset: i64,
) -> setups::Overrides {
    setups::Overrides {
        prompt,
        negative_prompt: negative.or_else(|| plan.negative_prompt.clone()),
        width: Some(size.0),
        height: Some(size.1),
        steps: plan.steps,
        cfg: plan.cfg,
        seed: plan.seed.map(|s| s + seed_offset),
        batch_size: Some(1),
        sampler: plan.sampler.clone(),
        scheduler: plan.scheduler.clone(),
        loras: None,
        transparent_background: None,
        ..Default::default()
    }
}

/// The request for panel `i`, generated on its own.
async fn panel_params(plan: &Plan, i: usize) -> Result<GenerationParams, String> {
    let panel = &plan.panels[i];
    let nai = plan.nai();
    let (budget, cap) = plan.budget();
    let size = generation_size(plan.rect(i), page_size(&plan.shape), budget, cap);
    let o = overrides(
        plan,
        panel.prompt(&plan.page, nai),
        panel.negative_prompt.clone(),
        size,
        i as i64,
    );
    let mut params = setups::apply(&plan.setup, &o)?;
    let mut args = plan.additions.clone();
    args["characters"] = json!(panel.characters);
    super::tools::finish_prompt(&mut params, &args).await?;
    // NovelAI letters the dialogue; other models get none.
    if nai {
        params
            .positive_prompt
            .push_str(&text_block(&panel.dialogue));
    }
    Ok(params)
}

/// The request for a whole page drawn as one NovelAI image, each panel a
/// character prompt placed at the panel's centre.
async fn one_image_params(plan: &Plan) -> Result<GenerationParams, String> {
    let dialogue: Vec<String> = plan
        .panels
        .iter()
        .flat_map(|p| p.dialogue.clone())
        .collect();
    let aspect = page_aspect(&plan.shape);
    let (budget, cap) = plan.budget();
    let pixels = budget as f64;
    let (w, h) = fit_pixels(
        snap64((pixels * aspect).sqrt()),
        snap64((pixels / aspect).sqrt()),
        cap.unwrap_or(FREE_PIXELS),
    );
    let o = overrides(
        plan,
        one_image_base(&plan.page, plan.panels.len(), &dialogue),
        None,
        (w, h),
        0,
    );
    let mut params = setups::apply(&plan.setup, &o)?;
    let per_box_uc = nai_model(&params).is_some_and(|m| m.character_negatives);
    let needs_library = plan.panels.iter().any(|p| !p.characters.is_empty());
    let lib = if needs_library {
        Some(library::Library::load().await)
    } else {
        None
    };
    let arch = library::architecture_of(&params);
    let mut boxes = Vec::new();
    let mut extra_uc = Vec::new();
    for (i, panel) in plan.panels.iter().enumerate() {
        let mut prompt = panel.one_image_prompt(i + 1, &plan.page);
        let mut uc = panel.negative_prompt.clone().unwrap_or_default();
        if let Some(lib) = &lib {
            for name in &panel.characters {
                let c = library::find_character(lib, name, &arch)?;
                prompt = join_tags(&[&prompt, &c.prompt]);
                let neg = c.negative.lines().collect::<Vec<_>>().join(", ");
                uc = join_tags(&[&uc, &neg]);
            }
        }
        if !per_box_uc && !uc.is_empty() {
            extra_uc.push(uc.clone());
            uc.clear();
        }
        let (x, y) = one_image_centre(plan.rect(i));
        boxes.push(NovelAiCharacter {
            prompt,
            negative_prompt: uc,
            center: NovelAiCoord { x, y },
            enabled: true,
        });
    }
    if !extra_uc.is_empty() {
        let mut parts = vec![params.negative_prompt.as_str()];
        parts.extend(extra_uc.iter().map(String::as_str));
        params.negative_prompt = join_tags(&parts);
    }
    let block = params
        .novelai
        .as_mut()
        .ok_or("mode one_image needs a NovelAI model")?;
    block.characters = boxes;
    block.use_coords = true;
    super::tools::finish_prompt(&mut params, &plan.additions).await?;
    params.positive_prompt.push_str(&text_block(&dialogue));
    Ok(params)
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PanelState {
    pub panel: usize,
    pub status: JobStatus,
    /// Gallery filename of the panel image.
    pub image: Option<String>,
    pub seed: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicJob {
    pub job_id: String,
    pub status: JobStatus,
    pub mode: String,
    pub model: String,
    pub layout: String,
    pub page_shape: String,
    pub panels: Vec<PanelState>,
    /// The assembled page in the gallery.
    pub page: Option<SavedImage>,
    pub error: Option<String>,
    /// The panel job running now, for cancelling.
    #[serde(skip)]
    pub current_job: Option<String>,
    #[serde(skip)]
    pub plan: Option<Arc<Plan>>,
}

impl Tracked for ComicJob {
    fn finished(&self) -> bool {
        self.status.finished()
    }
}

static COMICS: Store<ComicJob> = Store::new();

pub fn is_comic_job(job_id: &str) -> bool {
    job_id.starts_with("comic-")
}

pub fn get(job_id: &str) -> Option<watch::Receiver<ComicJob>> {
    COMICS.get(job_id)
}

pub fn active() -> Vec<ComicJob> {
    COMICS.active()
}

pub fn recent(n: usize) -> Vec<ComicJob> {
    COMICS.recent(n)
}

/// Start a comic job. `kept` holds gallery images to reuse, by panel index.
pub fn start(
    state: Arc<AppState>,
    plan: Plan,
    kept: HashMap<usize, String>,
) -> watch::Receiver<ComicJob> {
    let plan = Arc::new(plan);
    let job = ComicJob {
        job_id: format!("comic-{}", uuid::Uuid::new_v4().simple()),
        status: JobStatus::Queued,
        mode: if plan.one_image {
            "one_image"
        } else {
            "panels"
        }
        .into(),
        model: setups::model_key(&plan.setup),
        layout: plan.layout_name.clone(),
        page_shape: plan.shape.clone(),
        panels: (0..plan.panels.len())
            .map(|i| {
                let image = kept.get(&i).cloned();
                PanelState {
                    panel: i + 1,
                    status: if image.is_some() {
                        JobStatus::Done
                    } else {
                        JobStatus::Queued
                    },
                    image,
                    seed: None,
                    error: None,
                }
            })
            .collect(),
        page: None,
        error: None,
        current_job: None,
        plan: Some(Arc::clone(&plan)),
    };
    let tx = COMICS.insert(job.job_id.clone(), job);
    let rx = tx.subscribe();
    tokio::spawn(async move {
        if let Err(e) = run(&state, &tx, &plan).await {
            tx.send_if_modified(|j| {
                if j.status.finished() {
                    return false;
                }
                j.status = JobStatus::Failed;
                j.error = Some(e.clone());
                true
            });
            log::warn!("[mcp] comic {} failed: {e}", tx.borrow().job_id);
        }
    });
    rx
}

fn cancelled(tx: &watch::Sender<ComicJob>) -> bool {
    tx.borrow().status == JobStatus::Cancelled
}

/// Run one request as an MCP job and wait for it.
async fn run_one(
    state: &Arc<AppState>,
    tx: &watch::Sender<ComicJob>,
    params: GenerationParams,
) -> jobs::Job {
    let rx = jobs::start(Arc::clone(state), params);
    let id = rx.borrow().job_id.clone();
    tx.send_modify(|j| j.current_job = Some(id.clone()));
    // Cancelled between submitting and here: stop the panel too.
    if cancelled(tx) {
        let _ = jobs::cancel(state, &id).await;
    }
    let job = jobs::wait(rx, PANEL_WAIT).await;
    tx.send_modify(|j| j.current_job = None);
    job
}

async fn run(
    state: &Arc<AppState>,
    tx: &watch::Sender<ComicJob>,
    plan: &Plan,
) -> Result<(), String> {
    tx.send_modify(|j| {
        if j.status == JobStatus::Queued {
            j.status = JobStatus::Running;
        }
    });
    if plan.one_image {
        let params = one_image_params(plan).await?;
        let job = run_one(state, tx, params).await;
        if cancelled(tx) {
            return Ok(());
        }
        let image = match (job.status, job.images.first()) {
            (JobStatus::Done, Some(img)) => img.clone(),
            _ => return Err(job.error.unwrap_or_else(|| "The page didn't finish".into())),
        };
        tx.send_modify(|j| {
            for p in &mut j.panels {
                p.status = JobStatus::Done;
                p.seed = Some(job.seed.clone());
            }
            j.page = Some(image);
            j.status = JobStatus::Done;
        });
        return Ok(());
    }

    for i in 0..plan.panels.len() {
        if cancelled(tx) {
            return Ok(());
        }
        if tx.borrow().panels[i].image.is_some() {
            continue;
        }
        tx.send_modify(|j| j.panels[i].status = JobStatus::Running);
        let params = match panel_params(plan, i).await {
            Ok(p) => p,
            Err(e) => {
                tx.send_modify(|j| {
                    j.panels[i].status = JobStatus::Failed;
                    j.panels[i].error = Some(e);
                });
                continue;
            }
        };
        let job = run_one(state, tx, params).await;
        tx.send_modify(|j| {
            let p = &mut j.panels[i];
            p.seed = Some(job.seed.clone()).filter(|s| !s.is_empty());
            match (job.status, job.images.first()) {
                (JobStatus::Done, Some(img)) => {
                    p.status = JobStatus::Done;
                    p.image = Some(img.filename.clone());
                }
                (JobStatus::Cancelled, _) => p.status = JobStatus::Cancelled,
                _ => {
                    p.status = JobStatus::Failed;
                    p.error = Some(
                        job.error
                            .clone()
                            .unwrap_or_else(|| "The panel didn't finish".into()),
                    );
                }
            }
        });
    }
    if cancelled(tx) {
        return Ok(());
    }

    let images: HashMap<usize, String> = tx
        .borrow()
        .panels
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.image.clone().map(|f| (plan.order[i], f)))
        .collect();
    if images.is_empty() {
        return Err("No panel finished, so there is no page.".into());
    }
    let job_id = tx.borrow().job_id.clone();
    let page = save_page(state, plan, &images, &job_id).await?;
    tx.send_modify(|j| {
        j.page = Some(page);
        j.status = JobStatus::Done;
    });
    Ok(())
}

/// Draw the finished panels onto one page: white paper, gutters, a black
/// border round every panel, each image cropped to fill its box. Panels with
/// no image stay blank.
pub fn assemble(
    layout: &Node,
    shape: &str,
    gutter: u32,
    images: &HashMap<usize, image::DynamicImage>,
) -> image::RgbImage {
    let (pw, ph) = page_size(shape);
    let mut page = image::RgbImage::from_pixel(pw, ph, image::Rgb([255, 255, 255]));
    let border = ((pw.max(ph) as f64) / 600.0).round().max(2.0) as u32;
    let mut ids: Vec<(usize, Rect)> = rects(layout).into_iter().collect();
    ids.sort_by_key(|(id, _)| *id);
    for (id, rect) in ids {
        let b = pixel_rect(rect, (pw, ph), gutter);
        if let Some(img) = images.get(&id) {
            let (cx, cy, cw, ch) = cover_crop((img.width(), img.height()), (b.w, b.h));
            let crop = img.crop_imm(
                cx.round().max(0.0) as u32,
                cy.round().max(0.0) as u32,
                (cw.round() as u32).max(1),
                (ch.round() as u32).max(1),
            );
            let fitted = crop
                .resize_exact(b.w, b.h, image::imageops::FilterType::Lanczos3)
                .to_rgb8();
            image::imageops::replace(&mut page, &fitted, i64::from(b.x), i64::from(b.y));
        }
        let black = image::Rgb([0, 0, 0]);
        for y in b.y..(b.y + b.h).min(ph) {
            for x in b.x..(b.x + b.w).min(pw) {
                let edge = x < b.x + border
                    || y < b.y + border
                    || x + border >= b.x + b.w
                    || y + border >= b.y + b.h;
                if edge {
                    page.put_pixel(x, y, black);
                }
            }
        }
    }
    page
}

async fn save_page(
    state: &Arc<AppState>,
    plan: &Plan,
    images: &HashMap<usize, String>,
    job_id: &str,
) -> Result<SavedImage, String> {
    let dir = super::tools::gallery_dir()?;
    let paths: Vec<(usize, PathBuf)> = images
        .iter()
        .map(|(id, f)| super::tools::gallery_image_path(&dir, f).map(|p| (*id, p)))
        .collect::<Result<_, _>>()?;
    let layout = plan.layout.clone();
    let shape = plan.shape.clone();
    let gutter = plan.gutter;
    let png = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let mut decoded = HashMap::new();
        for (id, path) in paths {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            decoded.insert(id, crate::commands::api::decode_gallery_image(&bytes)?);
        }
        let page = assemble(&layout, &shape, gutter, &decoded);
        let mut out = std::io::Cursor::new(Vec::new());
        page.write_to(&mut out, image::ImageFormat::Png)
            .map_err(|e| format!("Could not encode the page: {e}"))?;
        Ok(out.into_inner())
    })
    .await
    .map_err(|e| e.to_string())??;
    let (width, height) = page_size(&plan.shape);
    let summary = plan
        .panels
        .iter()
        .enumerate()
        .map(|(i, p)| format!("Panel {}: {}", i + 1, p.prompt.lines().next().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n");
    let mut metadata = HashMap::new();
    metadata.insert(
        "positive_prompt".to_string(),
        join_tags(&[&plan.page.style, &plan.page.setting]),
    );
    metadata.insert("comic_panels".to_string(), summary);
    metadata.insert("size".to_string(), format!("{width}x{height}"));
    metadata.insert("mooshie_source".to_string(), "mcp".to_string());
    let template = state.config.read().await.output_filename_template.clone();
    let dir2 = dir.clone();
    let id = job_id.to_string();
    let filename = tokio::task::spawn_blocking(move || {
        crate::webserver::save_to_gallery_in_dir(
            &dir2,
            &png,
            "comic_page.png",
            &id,
            Some("txt2img"),
            Some(&metadata),
            Some("both"),
            template.as_deref(),
            crate::watermark::owner_choice(),
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    jobs::announce_saved(state, &filename, "txt2img").await;
    Ok(SavedImage {
        filename,
        width: Some(width),
        height: Some(height),
    })
}

/// Cancel a comic: the panel running now is stopped and no later panel starts.
pub async fn cancel(state: &Arc<AppState>, job_id: &str) -> Result<ComicJob, String> {
    let tx = COMICS
        .sender(job_id)
        .ok_or_else(|| format!("No job '{job_id}'"))?;
    if tx.borrow().status.finished() {
        return Ok(tx.borrow().clone());
    }
    tx.send_modify(|j| {
        j.status = JobStatus::Cancelled;
        j.error = Some("Cancelled".into());
    });
    let current = tx.borrow().current_job.clone();
    if let Some(id) = current {
        jobs::cancel(state, &id).await?;
    }
    let job = tx.borrow().clone();
    Ok(job)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct PanelChange {
    panel: usize,
    prompt: Option<String>,
    dialogue: Option<Value>,
    negative_prompt: Option<String>,
    setting: Option<String>,
}

/// A new comic job from a finished one: the listed panels are generated again
/// (with any changes), the rest keep their images, and the page is rebuilt.
pub fn redo(args: &Value) -> Result<(Plan, HashMap<usize, String>), String> {
    let id = args["job_id"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("job_id is required: the comic job to redo panels of")?;
    let rx = get(id).ok_or_else(|| {
        format!("No comic job '{id}' (jobs are forgotten when MooshieUI restarts)")
    })?;
    let old = rx.borrow().clone();
    if !old.status.finished() {
        return Err("That comic is still being made. Wait for it, or cancel it first.".into());
    }
    let mut plan = (*old.plan.ok_or("That job has no page to redo")?).clone();
    if plan.one_image {
        return Err("A one_image page is a single image: call create_comic again instead.".into());
    }
    let count = plan.panels.len();
    let mut redo: Vec<usize> = match &args["panels"] {
        Value::Array(items) => items
            .iter()
            .map(|v| v.as_u64().map(|n| n as usize))
            .collect::<Option<Vec<_>>>()
            .ok_or("panels must be panel numbers")?,
        Value::Null => Vec::new(),
        _ => return Err("panels must be a list of panel numbers".into()),
    };
    let changes: Vec<PanelChange> = match args.get("changes") {
        None | Some(Value::Null) => Vec::new(),
        Some(v) => {
            serde_json::from_value(v.clone()).map_err(|e| format!("Invalid changes: {e}"))?
        }
    };
    for c in &changes {
        if c.panel == 0 || c.panel > count {
            return Err(format!(
                "There is no panel {} (the page has {count}).",
                c.panel
            ));
        }
        let panel = &mut plan.panels[c.panel - 1];
        if let Some(prompt) = &c.prompt {
            let (text, lines) = take_text_block(prompt);
            if text.is_empty() {
                return Err(format!("Panel {} needs a prompt.", c.panel));
            }
            panel.prompt = text;
            if !lines.is_empty() {
                panel.dialogue = lines;
            }
        }
        if let Some(d) = &c.dialogue {
            panel.dialogue = dialogue_lines(d)?;
        }
        if let Some(n) = &c.negative_prompt {
            panel.negative_prompt = Some(n.clone());
        }
        if let Some(s) = &c.setting {
            panel.setting = s.trim().to_string();
        }
        redo.push(c.panel);
    }
    if redo.is_empty() {
        return Err("Name the panels to make again in panels, or change them in changes.".into());
    }
    if let Some(n) = redo.iter().find(|n| **n == 0 || **n > count) {
        return Err(format!("There is no panel {n} (the page has {count})."));
    }
    // A fixed seed would only redraw the same panel.
    plan.seed = args["seed"].as_i64().filter(|s| *s >= 0);
    let kept = old
        .panels
        .iter()
        .enumerate()
        .filter(|(i, _)| !redo.contains(&(i + 1)))
        .filter_map(|(i, p)| p.image.clone().map(|f| (i, f)))
        .collect();
    Ok((plan, kept))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_cover_the_page_with_the_panels_they_name() {
        for (id, n) in TEMPLATES {
            let node = template(id).unwrap();
            let r = rects(&node);
            assert_eq!(r.len(), *n, "{id}");
            let area: f64 = r.values().map(|r| r.w * r.h).sum();
            assert!((area - 1.0).abs() < 1e-9, "{id}");
        }
        assert!(template("nope").is_none());
    }

    #[test]
    fn reading_order_goes_by_band_then_side() {
        let node = template("tall_left_two_right").unwrap();
        let r = rects(&node);
        // The tall panel and the top right one share a band; the tall one is left.
        assert_eq!(reading_order(&r, false), vec![0, 1, 2]);
        assert_eq!(reading_order(&r, true), vec![1, 0, 2]);
        let grid = rects(&template("grid_2x2").unwrap());
        assert_eq!(reading_order(&grid, false), vec![0, 1, 2, 3]);
        assert_eq!(reading_order(&grid, true), vec![1, 0, 3, 2]);
    }

    #[test]
    fn rows_build_the_layout_described() {
        let rows = vec![
            RowIn {
                height: Some(2.0),
                ..Default::default()
            },
            RowIn {
                widths: Some(vec![1.0, 3.0]),
                ..Default::default()
            },
        ];
        let node = rows_layout(&rows).unwrap();
        let r = rects(&node);
        assert_eq!(r.len(), 3);
        assert!((r[&0].h - 2.0 / 3.0).abs() < 1e-9);
        assert!((r[&1].w - 0.25).abs() < 1e-9);
        assert!((r[&2].x - 0.25).abs() < 1e-9);
        assert!(rows_layout(&[]).is_err());
        for n in 1..=MAX_PANELS {
            assert_eq!(rects(&default_layout(n).1).len(), n, "{n}");
        }
    }

    #[test]
    fn sizes_follow_the_panel_and_stay_free_on_novelai() {
        let page = page_size("portrait");
        assert_eq!(page, (1448, 2048));
        let wide = Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 0.25,
        };
        let (w, h) = generation_size(wide, page, 832 * 1216, None);
        assert!(w > h && w % 64 == 0 && h % 64 == 0);
        let (w, h) = generation_size(wide, page, 1536 * 1536, Some(FREE_PIXELS));
        assert!(u64::from(w) * u64::from(h) <= FREE_PIXELS);
        let b = pixel_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 0.5,
                h: 1.0,
            },
            (1000, 1000),
            20,
        );
        assert_eq!(
            b,
            PxRect {
                x: 20,
                y: 20,
                w: 470,
                h: 960
            }
        );
        let (x, y, cw, ch) = cover_crop((1000, 500), (100, 100));
        assert_eq!((x, y, cw, ch), (250.0, 0.0, 500.0, 500.0));
    }

    #[test]
    fn panel_prompts_put_looks_on_the_tag_line_and_lettering_last() {
        let page = PageText {
            style: "manga".into(),
            setting: "classroom".into(),
            cast: vec![CastIn {
                name: "Aki".into(),
                tags: "1girl, long black hair".into(),
            }],
        };
        let panel = Panel {
            prompt: "close-up, smile\n\nAki asks \"Ready?\"".into(),
            dialogue: vec!["Ready?".into()],
            cast: vec![0],
            characters: vec![],
            setting: String::new(),
            negative_prompt: None,
        };
        assert_eq!(
            panel.prompt(&page, true),
            "manga, close-up, smile, 1girl, long black hair, classroom, speech bubble, english text\n\nAki asks \"Ready?\""
        );
        assert_eq!(
            panel.prompt(&page, false),
            "manga, close-up, smile, 1girl, long black hair, classroom\n\nAki asks \"Ready?\""
        );
        assert_eq!(text_block(&panel.dialogue), "\n\nText:\nReady?");
        assert_eq!(
            panel.one_image_prompt(2, &page),
            "panel 2, close-up, smile\n\nAki asks \"Ready?\", 1girl, long black hair"
        );
        assert_eq!(
            one_image_base(&page, 3, &["やった".to_string()]),
            "manga, comic, 3 panels, multiple views, classroom, speech bubble, japanese text"
        );
        let (prompt, lines) = take_text_block("1girl, smile\n\nText:\nHi!\n\nBye.");
        assert_eq!(prompt, "1girl, smile");
        assert_eq!(lines, vec!["Hi!", "Bye."]);
        assert_eq!(
            dialogue_lines(&json!(["\"Wait!\"", "No.\nNever."])).unwrap(),
            vec!["Wait!", "No.", "Never."]
        );
        assert!(dialogue_lines(&json!(3)).is_err());
    }

    #[test]
    fn assembly_draws_borders_and_fills_panels() {
        let node = template("two_columns").unwrap();
        let mut images = HashMap::new();
        images.insert(
            0,
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                64,
                64,
                image::Rgb([200, 0, 0]),
            )),
        );
        let page = assemble(&node, "square", 24, &images);
        assert_eq!(page.dimensions(), (2048, 2048));
        // Gutter is white, the border black, the filled panel red, the empty one white.
        assert_eq!(page.get_pixel(5, 5).0, [255, 255, 255]);
        assert_eq!(page.get_pixel(24, 500).0, [0, 0, 0]);
        assert_eq!(page.get_pixel(500, 1000).0, [200, 0, 0]);
        assert_eq!(page.get_pixel(1500, 1000).0, [255, 255, 255]);
    }
}
