//! Rewrite ComfyUI/A1111 prompt weight syntax into NovelAI weight syntax.
//!
//! The frontend translates *into* ComfyUI syntax (`1.1::tag::` becomes
//! `(tag:1.1)`) because ComfyUI is the default backend. NovelAI does not
//! understand `(tag:1.1)`: it reads the parentheses as literal characters and
//! the weight is silently lost, so a prompt that looked weighted in the UI
//! generates unweighted and the user pays Anlas for it.
//!
//! This module runs on the way out, only for NovelAI requests. It lives in Rust
//! rather than next to the frontend translator because the repo has no frontend
//! test framework and a silent bug here costs real money.
//!
//! # What is converted
//!
//! - `(tag:1.1)` becomes `1.1::tag::`
//! - Nested weights are flattened by multiplication, since NovelAI has no
//!   well-defined nesting of `::`: `((tag:1.05):1.05)` becomes `1.1::tag::`
//! - A group weight distributes over the comma-separated tags inside it:
//!   `(a, b:1.2)` becomes `1.2::a::, 1.2::b::`
//! - A weight of 1.0 is dropped rather than written out
//!
//! # What is deliberately left alone
//!
//! - **Bare parentheses.** `hatsune_miku_(vocaloid)` is a Danbooru tag, not
//!   emphasis. The frontend translator does not treat `(tag)` as a weight
//!   either, so neither does this.
//! - **`{tag}` and `[tag]`.** NovelAI understands both natively.
//! - **Prompts already in NovelAI syntax.** The conversion is idempotent.

/// Rewrite a prompt from ComfyUI/A1111 weight syntax into NovelAI syntax.
pub fn to_novelai(prompt: &str) -> String {
    let mut out = prompt.to_string();
    // Innermost-first, so by the time a group is rewritten its own content
    // holds no parentheses and any inner weight is already a `::` form that
    // `distribute` can fold into the outer one.
    loop {
        let Some(group) = find_innermost_weighted(&out) else {
            return out;
        };
        let replacement = distribute(&out[group.content.clone()], group.weight);
        out.replace_range(group.span, &replacement);
    }
}

/// An innermost `(content:weight)` group located in a prompt.
struct WeightedGroup {
    /// Byte range of the whole `(...)`, including both parentheses.
    span: std::ops::Range<usize>,
    /// Byte range of the text between `(` and the weight's `:`.
    content: std::ops::Range<usize>,
    weight: f64,
}

/// Find the first innermost `(content:weight)` group.
///
/// "Innermost" means `content` contains no parentheses of its own, which is
/// what makes the outer loop in [`to_novelai`] terminate: every pass removes
/// one pair.
fn find_innermost_weighted(text: &str) -> Option<WeightedGroup> {
    let bytes = text.as_bytes();
    let mut open: Option<usize> = None;

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' if !is_escaped(bytes, i) => open = Some(i),
            b')' if !is_escaped(bytes, i) => {
                let Some(start) = open.take() else { continue };
                let inner = &text[start + 1..i];
                // The weight is whatever follows the last colon. Splitting on
                // the last one keeps `(1girl: standing:1.2)` working.
                let Some(colon) = inner.rfind(':') else {
                    continue;
                };
                let weight: f64 = match inner[colon + 1..].trim().parse() {
                    Ok(w) => w,
                    // No trailing number, so this is an ordinary parenthesised
                    // phrase or a Danbooru tag. Leave it be.
                    Err(_) => continue,
                };
                let content_start = start + 1;
                return Some(WeightedGroup {
                    span: start..i + 1,
                    content: content_start..content_start + colon,
                    weight,
                });
            }
            _ => {}
        }
    }
    None
}

/// Is the byte at `i` preceded by an odd number of backslashes?
fn is_escaped(bytes: &[u8], i: usize) -> bool {
    let mut slashes = 0;
    let mut j = i;
    while j > 0 && bytes[j - 1] == b'\\' {
        slashes += 1;
        j -= 1;
    }
    slashes % 2 == 1
}

/// Apply `weight` to every comma-separated tag in `content`.
///
/// NovelAI weights a single run of text, so a group covering several tags has
/// to be expanded into one weighted run per tag. Splitting on commas is exactly
/// the boundary the weight was meant to cover.
fn distribute(content: &str, weight: f64) -> String {
    // Trimmed first: padding the user left inside the parentheses would
    // otherwise survive as a doubled space next to the surrounding comma.
    // Spacing *between* the segments is left alone.
    content
        .trim()
        .split(',')
        .map(|segment| apply_weight(segment, weight))
        .collect::<Vec<_>>()
        .join(",")
}

/// Wrap one segment in NovelAI weight syntax, preserving its surrounding space.
fn apply_weight(segment: &str, weight: f64) -> String {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return segment.to_string();
    }
    let lead = &segment[..segment.len() - segment.trim_start().len()];
    let trail = &segment[segment.trim_end().len()..];

    // An inner group has already been rewritten into `w::text::`. Fold the two
    // weights together rather than nesting, which NovelAI does not define.
    let (weight, trimmed) = match split_novelai_weight(trimmed) {
        Some((inner_weight, inner_text)) => (weight * inner_weight, inner_text),
        None if trimmed.contains("::") => {
            // Partly rewritten text that is not a single weighted run, e.g.
            // `a 1.2::b:: c`. Wrapping it would emit syntax NovelAI cannot
            // parse and fail the whole paid request, so the outer weight is
            // dropped instead.
            return segment.to_string();
        }
        None => (weight, trimmed),
    };

    if (weight - 1.0).abs() < f64::EPSILON {
        return format!("{lead}{trimmed}{trail}");
    }
    format!("{lead}{}::{trimmed}::{trail}", format_weight(weight))
}

/// Split an exact `weight::text::` run into its parts.
fn split_novelai_weight(text: &str) -> Option<(f64, &str)> {
    let rest = text.strip_suffix("::")?;
    let (weight, inner) = rest.split_once("::")?;
    // A second `::` means this is not one flat weighted run.
    if inner.contains("::") {
        return None;
    }
    Some((weight.trim().parse().ok()?, inner))
}

/// Two decimal places, without the trailing zeros NovelAI never writes.
fn format_weight(weight: f64) -> String {
    let s = format!("{weight:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weighted_tag_becomes_novelai_syntax() {
        assert_eq!(to_novelai("(masterpiece:1.3)"), "1.3::masterpiece::");
    }

    #[test]
    fn a_weighted_tag_keeps_its_neighbours() {
        assert_eq!(
            to_novelai("1girl, (masterpiece:1.3), solo"),
            "1girl, 1.3::masterpiece::, solo"
        );
    }

    #[test]
    fn a_group_weight_distributes_over_its_tags() {
        // NovelAI weights one run of text, so a multi-tag group has to expand.
        assert_eq!(to_novelai("(a, b:1.2)"), "1.2::a::, 1.2::b::");
    }

    #[test]
    fn nested_weights_are_flattened_by_multiplication() {
        // NovelAI has no defined nesting of `::`, so 1.05 * 1.05 is written out.
        assert_eq!(to_novelai("((tag:1.05):1.05)"), "1.1::tag::");
    }

    #[test]
    fn a_nested_group_folds_into_the_outer_weight() {
        assert_eq!(to_novelai("(a, (b:1.2):1.1)"), "1.1::a::, 1.32::b::");
    }

    #[test]
    fn a_weight_of_one_is_dropped() {
        // `1::tag::` is legal but noisy, and the user did not type it.
        assert_eq!(to_novelai("(tag:1.0)"), "tag");
    }

    #[test]
    fn a_negative_weight_survives() {
        assert_eq!(to_novelai("(tag:-1.5)"), "-1.5::tag::");
    }

    #[test]
    fn bare_parentheses_are_left_alone() {
        // Danbooru tags are full of these and none of them mean emphasis.
        let tag = "hatsune_miku_(vocaloid), 1girl";
        assert_eq!(to_novelai(tag), tag);
    }

    #[test]
    fn escaped_parentheses_are_left_alone() {
        let tag = r"hatsune_miku_\(vocaloid\)";
        assert_eq!(to_novelai(tag), tag);
    }

    #[test]
    fn novelai_braces_are_left_alone() {
        // NovelAI reads these natively; rewriting them would only add noise.
        let prompt = "{tag}, [other]";
        assert_eq!(to_novelai(prompt), prompt);
    }

    #[test]
    fn an_already_converted_prompt_is_unchanged() {
        // The conversion runs on the way out and must never double-apply.
        let prompt = "1.3::masterpiece::, 1girl";
        assert_eq!(to_novelai(prompt), prompt);
    }

    #[test]
    fn a_colon_inside_the_tag_survives() {
        assert_eq!(
            to_novelai("(1girl: standing:1.2)"),
            "1.2::1girl: standing::"
        );
    }

    #[test]
    fn a_group_with_no_weight_is_left_alone() {
        let prompt = "(a, b), c";
        assert_eq!(to_novelai(prompt), prompt);
    }

    #[test]
    fn mixed_partly_weighted_text_drops_the_outer_weight_rather_than_break() {
        // Wrapping this would emit `1.1::a 1.2::b:: c::`, which NovelAI cannot
        // parse. Losing the outer weight beats failing the whole paid request.
        assert_eq!(to_novelai("(a (b:1.2) c:1.1)"), "a 1.2::b:: c");
    }

    #[test]
    fn an_empty_prompt_stays_empty() {
        assert_eq!(to_novelai(""), "");
    }

    #[test]
    fn whitespace_around_a_tag_is_preserved() {
        assert_eq!(to_novelai("a, ( b :1.2), c"), "a, 1.2::b::, c");
    }

    /// The artist style store builds its fragment as `(tag:weight)`, so the
    /// shapes it can emit are worth pinning: a style reaching NovelAI in
    /// A1111 syntax would be charged for and generate wrong.
    #[test]
    fn a_style_fragment_is_rewritten() {
        assert_eq!(to_novelai("(artist_tag:1)"), "artist_tag");
        assert_eq!(to_novelai("(artist_tag:1.2)"), "1.2::artist_tag::");
        assert_eq!(to_novelai("(a:1), (b:0.8)"), "a, 0.8::b::");
    }

    #[test]
    fn a_style_fragment_keeps_its_escaped_parentheses() {
        // The style store escapes Danbooru parentheses before weighting.
        let src = "(hoshino_\\(artist\\):1.2)";
        assert_eq!(to_novelai(src), r"1.2::hoshino_\(artist\)::");
    }
}
