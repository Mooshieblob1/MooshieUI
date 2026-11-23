//! Translate prompt weight syntax between ComfyUI/A1111 and NovelAI.
//!
//! The prompt box holds whatever the user typed or imported, in either syntax,
//! and nothing rewrites it there. Translation happens only at the boundary
//! where a backend could not read it:
//!
//! - [`to_novelai`] runs on the way out for every NovelAI request. NovelAI does
//!   not understand `(tag:1.1)`: it reads the parentheses as literal characters
//!   and the weight is silently lost, so a prompt that looked weighted in the
//!   UI would generate unweighted and the user would pay Anlas for it. Text
//!   already in NovelAI syntax passes through byte for byte, which is why the
//!   frontend sends a NovelAI prompt as typed instead of converting it to
//!   ComfyUI syntax first: that round trip split `1.2::a, b::` into one span
//!   per tag, rounded weights and dropped the sign of `-1::hat::`.
//! - [`from_novelai`] runs when a NovelAI request's prompt is handed to
//!   ComfyUI instead: the local refine pass, and the face-detail tag
//!   extraction, both written against ComfyUI syntax. It mirrors the
//!   frontend's `translateNaiWeightSyntax`, which does the same job for a
//!   local generation.
//!
//! It lives in Rust rather than next to the frontend translator because a
//! silent bug here costs real money.
//!
//! # What is converted
//!
//! - `(tag:1.1)` becomes `1.1::tag::`
//! - Nested weights are flattened by multiplication, since NovelAI has no
//!   well-defined nesting of `::`: `((tag:1.05):1.05)` becomes `1.1::tag::`
//! - A group weight distributes over the comma-separated tags inside it:
//!   `(a, b:1.2)` becomes `1.2::a::, 1.2::b::`
//! - A weight of 1.0 is dropped rather than written out
//! - A weight of exactly 1.05 or 0.95 goes back to `{tag}` or `[tag]`. That is
//!   what [`from_novelai`] and the frontend write for NovelAI's own braces, so
//!   a prompt that went through either still reaches NovelAI as
//!   `{{{year 2025}}}` instead of as `1.16::year 2025::`. Nesting composes,
//!   since the inner group is already `{...}` when the outer one is rewritten.
//! - A numeric span whose text ends in a digit gets a comma before the closing
//!   `::`. NovelAI reads `2025::` as the opener of a weight of 2025, so
//!   `1.3::year 2025::, tag` would weight everything after it by 2025;
//!   `1.3::year 2025,::, tag` does not.
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
        let content = &out[group.content.clone()];
        let replacement = match emphasis_brackets(content, group.weight) {
            Some((open, close)) => format!("{open}{}{close}", content.trim()),
            None => distribute(content, group.weight),
        };
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

/// The NovelAI bracket a `(content:weight)` group came from, if any.
///
/// The frontend translates `{tag}` to `(tag:1.05)` and `[tag]` to
/// `(tag:0.95)`, one layer at a time. Writing those back as brackets keeps the
/// prompt the way the user typed it, and avoids the numeric form entirely for
/// the common case. Content that already holds a `::` span is left to
/// [`distribute`], which folds the weights together, because a bracket around a
/// numeric span is not something NovelAI documents.
fn emphasis_brackets(content: &str, weight: f64) -> Option<(char, char)> {
    if content.trim().is_empty() || content.contains("::") {
        return None;
    }
    if (weight - 1.05).abs() < 1e-9 {
        Some(('{', '}'))
    } else if (weight - 0.95).abs() < 1e-9 {
        Some(('[', ']'))
    } else {
        None
    }
}

/// Apply `weight` to every comma-separated tag in `content`.
///
/// NovelAI weights a single run of text, so a group covering several tags has
/// to be expanded into one weighted run per tag. Splitting on commas is exactly
/// the boundary the weight was meant to cover.
fn distribute(content: &str, weight: f64) -> String {
    // Trimmed first: padding the user left inside the parentheses would
    // otherwise survive as a doubled space next to the surrounding comma.
    // Spacing *between* the segments is left alone. A trailing comma is the
    // guard `apply_weight` writes after a tag ending in a digit; it comes back
    // in on a reimported prompt and would otherwise grow by one comma a trip.
    content
        .trim()
        .trim_end_matches(',')
        .trim_end()
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
    // NovelAI takes digits right before `::` as a new weight, so the span for
    // `year 2025` would never close. A comma between them ends the tag first.
    let guard = if trimmed.ends_with(|c: char| c.is_ascii_digit()) {
        ","
    } else {
        ""
    };
    format!("{lead}{}::{trimmed}{guard}::{trail}", format_weight(weight))
}

/// Rewrite a prompt from NovelAI weight syntax into ComfyUI syntax.
///
/// The inverse of [`to_novelai`], for the places a NovelAI request's prompt is
/// read by something written against ComfyUI syntax: the local refine pass,
/// whose `CLIPTextEncode` would take `1.1::tag::` literally, and the
/// face-detail tag extraction. A prompt imported from a NovelAI image does not
/// come through here; it is restored into the panel as NovelAI wrote it.
///
/// This mirrors the frontend's `translateNaiWeightSyntax`, which does the same
/// job for a local generation:
///
/// - `1.1::tag::` becomes `(tag:1.10)`, and `-1::tag::` becomes `(tag:-1.00)`
/// - `{tag}` becomes `(tag:1.05)` and `[tag]` becomes `(tag:0.95)`, innermost
///   first, so `{{tag}}` becomes `((tag:1.05):1.05)`
/// - An escaped bracket is left alone, since it is a literal character
///
/// Linear in the prompt's length, so a prompt built to be slow cannot pin a
/// worker.
pub fn from_novelai(prompt: &str) -> String {
    let out = expand_novelai_weights(prompt);
    let out = rewrite_brackets(&out, '{', '}', "1.05");
    rewrite_brackets(&out, '[', ']', "0.95")
}

/// Rewrite every `weight::text::` run as `(text:weight)`.
///
/// The weight is the digit run immediately before the opening `::`, with its
/// minus sign when it has one (see [`is_weight_sign`]), and the run ends at the
/// next `::`. A run whose text holds a further colon is left alone: that is not
/// one flat weighted span, and guessing at it would corrupt the prompt rather
/// than fail visibly.
fn expand_novelai_weights(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    // Everything before `cursor` has already been copied or rewritten.
    let mut cursor = 0usize;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] != b':' || bytes[i + 1] != b':' {
            i += 1;
            continue;
        }
        let mut num_start = i;
        while num_start > cursor {
            let c = bytes[num_start - 1];
            if c.is_ascii_digit() || c == b'.' {
                num_start -= 1;
            } else {
                break;
            }
        }
        if num_start < i && is_weight_sign(bytes, num_start, cursor) {
            num_start -= 1;
        }
        let weight = text[num_start..i].parse::<f64>().ok();
        let close = text[i + 2..].find("::").map(|rel| i + 2 + rel);
        if let (Some(weight), Some(close)) = (weight, close) {
            let inner = &text[i + 2..close];
            if !inner.contains(':') && !inner.trim().is_empty() {
                out.push_str(&text[cursor..num_start]);
                out.push_str(&format!("({}:{:.2})", inner.trim(), weight));
                cursor = close + 2;
                i = cursor;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Is the byte before `num_start` the minus sign of the weight starting there?
///
/// NovelAI's negative weights (`-1::hat::`) put the sign directly before the
/// number. A hyphen that ends a word (`sci-1::x::` is not something anyone
/// writes, but `tag-2::` could be) is part of that word, so the sign only
/// counts at the start of the text or after a separator.
fn is_weight_sign(bytes: &[u8], num_start: usize, cursor: usize) -> bool {
    if num_start <= cursor || bytes[num_start - 1] != b'-' {
        return false;
    }
    let sign = num_start - 1;
    sign == 0 || {
        let before = bytes[sign - 1];
        before.is_ascii_whitespace() || matches!(before, b',' | b'(' | b'[' | b'{' | b':')
    }
}

/// Rewrite every unescaped `open ... close` pair as `(inner:weight)`, in one
/// pass.
///
/// A closing bracket pairs with the nearest unescaped opening bracket before it
/// that is still unpaired, so nesting resolves innermost first. Two cases stay
/// literal:
///
/// - A closing bracket with no unpaired opening bracket before it.
/// - A closing bracket whose partner would enclose only whitespace, as in `{}`.
///   That opening bracket stays unpaired. If a later closing bracket takes it,
///   this one is retried against the next opening bracket out, which no longer
///   encloses only whitespace.
///
/// The second rule is a quirk kept from the original fixed-point loop, which
/// rewrote one innermost pair per pass and rescanned the whole prompt each
/// time. The tests hold that loop as an oracle and check this against it.
fn rewrite_brackets(text: &str, open: char, close: char, weight: &str) -> String {
    /// An unpaired opening bracket, and the whitespace-only close waiting on
    /// it, if one is.
    struct Open {
        at: usize,
        blank_close: Option<usize>,
    }

    let mut unpaired: Vec<Open> = Vec::new();
    // Byte offsets of the brackets to rewrite, `true` for an opening one.
    let mut rewrites: Vec<(usize, bool)> = Vec::new();
    // The last non-whitespace character: a close right after its partner,
    // with only whitespace between, is the empty-pair case above.
    let mut last_solid: Option<usize> = None;
    // Length of the backslash run ending just before the current character.
    let mut backslashes = 0usize;

    for (i, ch) in text.char_indices() {
        let escaped = backslashes % 2 == 1;
        backslashes = if ch == '\\' { backslashes + 1 } else { 0 };
        if !escaped && ch == open {
            unpaired.push(Open {
                at: i,
                blank_close: None,
            });
        } else if !escaped && ch == close {
            match unpaired.last_mut() {
                None => {}
                Some(top) if last_solid == Some(top.at) => top.blank_close = Some(i),
                Some(_) => {
                    // Pairing an opening bracket frees the blank close that was
                    // waiting on it, which pairs with the next one out, and so on.
                    let mut closing = Some(i);
                    while let Some(close_at) = closing {
                        let Some(partner) = unpaired.pop() else { break };
                        rewrites.push((partner.at, true));
                        rewrites.push((close_at, false));
                        closing = partner.blank_close;
                    }
                }
            }
        }
        if !ch.is_whitespace() {
            last_solid = Some(i);
        }
    }

    if rewrites.is_empty() {
        return text.to_string();
    }
    rewrites.sort_unstable_by_key(|&(at, _)| at);
    let mut out = String::with_capacity(text.len() + rewrites.len() / 2 * (weight.len() + 1));
    let mut cursor = 0;
    for (at, is_open) in rewrites {
        out.push_str(&text[cursor..at]);
        if is_open {
            out.push('(');
        } else {
            out.push(':');
            out.push_str(weight);
            out.push(')');
        }
        // Both brackets are ASCII, so one byte.
        cursor = at + 1;
    }
    out.push_str(&text[cursor..]);
    out
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
        // NovelAI has no defined nesting of `::`, so 1.2 * 1.1 is written out.
        assert_eq!(to_novelai("((tag:1.2):1.1)"), "1.32::tag::");
    }

    #[test]
    fn brace_emphasis_from_the_frontend_goes_back_to_braces() {
        // `{{tag}}` arrives as `((tag:1.05):1.05)`. Flattening it would turn
        // the braces the user typed into `1.1::tag::`.
        assert_eq!(to_novelai("(tag:1.05)"), "{tag}");
        assert_eq!(to_novelai("((tag:1.05):1.05)"), "{{tag}}");
        assert_eq!(to_novelai("(tag:0.95)"), "[tag]");
        assert_eq!(to_novelai("((tag:0.95):0.95)"), "[[tag]]");
    }

    #[test]
    fn braces_around_a_tag_with_a_number_are_kept() {
        // The reported case: `{{{year 2025}}}, tag` became
        // `1.16::year 2025::, tag`, and NovelAI read `2025::` as a weight of
        // 2025 for everything after it.
        assert_eq!(
            to_novelai("(((year 2025:1.05):1.05):1.05), tag"),
            "{{{year 2025}}}, tag"
        );
    }

    #[test]
    fn braces_around_a_group_keep_the_group_whole() {
        assert_eq!(to_novelai("(a, b:1.05), c"), "{a, b}, c");
    }

    #[test]
    fn mixed_braces_and_brackets_nest_as_typed() {
        // `{[tag]}` and `[{tag}]` are both 0.9975 in NovelAI; keep the shape.
        assert_eq!(to_novelai("((tag:0.95):1.05)"), "{[tag]}");
        assert_eq!(to_novelai("((tag:1.05):0.95)"), "[{tag}]");
    }

    #[test]
    fn a_numeric_weight_around_braces_wraps_them() {
        assert_eq!(to_novelai("((tag:1.05):1.2)"), "1.2::{tag}::");
    }

    #[test]
    fn braces_around_a_numeric_span_fold_into_it() {
        // A bracket around a `::` span is not documented NovelAI syntax.
        assert_eq!(to_novelai("((tag:1.2):1.05)"), "1.26::tag::");
    }

    #[test]
    fn a_span_ending_in_a_digit_is_closed_safely() {
        // `1.16::year 2025::` reads as `1.16::year` followed by an unclosed
        // weight of 2025. The comma ends the tag before the closing `::`.
        assert_eq!(
            to_novelai("(year 2025:1.16), tag"),
            "1.16::year 2025,::, tag"
        );
        assert_eq!(to_novelai("(as109:-1)"), "-1::as109,::");
    }

    #[test]
    fn a_guarded_span_survives_a_trip_through_novelai_metadata() {
        // NovelAI writes the guarded prompt into the PNG; restoring and
        // regenerating must not add a comma each time.
        let sent = "1.16::year 2025,::, tag";
        assert_eq!(to_novelai(&from_novelai(sent)), sent);
        let braces = "{{{year 2025}}}, tag";
        assert_eq!(to_novelai(&from_novelai(braces)), braces);
    }

    #[test]
    fn a_span_with_a_number_inside_is_unchanged() {
        assert_eq!(
            to_novelai("(2girls, solo:1.2)"),
            "1.2::2girls::, 1.2::solo::"
        );
    }

    #[test]
    fn a_guarded_span_is_left_alone_on_a_second_pass() {
        let once = to_novelai("(year 2025:1.16), tag");
        assert_eq!(to_novelai(&once), once);
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
    #[test]
    fn from_novelai_expands_a_weighted_run() {
        assert_eq!(from_novelai("1.1::masterpiece::"), "(masterpiece:1.10)");
    }

    #[test]
    fn from_novelai_expands_runs_in_place() {
        assert_eq!(from_novelai("a, 1.2::b::, c"), "a, (b:1.20), c");
    }

    #[test]
    fn from_novelai_trims_the_weighted_text() {
        assert_eq!(from_novelai("1.2:: b ::"), "(b:1.20)");
    }

    #[test]
    fn from_novelai_handles_a_weight_below_one() {
        assert_eq!(from_novelai("0.9::blurry::"), "(blurry:0.90)");
    }

    #[test]
    fn from_novelai_keeps_the_sign_of_a_negative_weight() {
        assert_eq!(from_novelai("-1::hat::"), "(hat:-1.00)");
        assert_eq!(
            from_novelai("1girl, -0.5::coat::, rain"),
            "1girl, (coat:-0.50), rain"
        );
        assert_eq!(from_novelai("{-1::hat::}"), "((hat:-1.00):1.05)");
    }

    #[test]
    fn from_novelai_reads_a_hyphen_that_ends_a_word_as_part_of_the_word() {
        assert_eq!(from_novelai("type-2::tag::"), "type-(tag:2.00)");
    }

    #[test]
    fn a_negative_span_survives_both_ways() {
        // Typed NovelAI syntax goes out untouched, and comes back negative for
        // the local pass rather than as `-(hat:1.00)`.
        assert_eq!(to_novelai("-1::hat::"), "-1::hat::");
        assert_eq!(to_novelai(&from_novelai("-1::hat::")), "-1::hat::");
    }

    #[test]
    fn novelai_syntax_goes_out_exactly_as_typed() {
        // What the frontend now sends for a NovelAI prompt: the user's own
        // text. None of it may be split per tag, rounded or re-bracketed.
        for prompt in [
            "1.2::rain, night::, 1girl",
            "1.05::tag::",
            "1.234::tag::",
            "-1::hat::, 0.6::as109, tianliang duohe fangdongye::",
            "{{{year 2025}}}, [[blurry]]",
            "1.3::year 2025,::, tag",
            "2.1::transparent background::",
            "1girl, rain,\nText:\nMumei",
        ] {
            assert_eq!(to_novelai(prompt), prompt);
        }
    }

    #[test]
    fn from_novelai_leaves_a_run_with_no_weight_alone() {
        assert_eq!(from_novelai("::tag::"), "::tag::");
    }

    #[test]
    fn from_novelai_leaves_an_empty_run_alone() {
        assert_eq!(from_novelai("1.2::::"), "1.2::::");
    }

    #[test]
    fn from_novelai_converts_curly_emphasis() {
        assert_eq!(from_novelai("{tag}"), "(tag:1.05)");
    }

    #[test]
    fn from_novelai_converts_square_de_emphasis() {
        assert_eq!(from_novelai("[tag]"), "(tag:0.95)");
    }

    #[test]
    fn from_novelai_nests_repeated_emphasis_innermost_first() {
        assert_eq!(from_novelai("{{tag}}"), "((tag:1.05):1.05)");
    }

    #[test]
    fn from_novelai_converts_each_bracket_group_in_a_list() {
        assert_eq!(from_novelai("{a}, b, [c]"), "(a:1.05), b, (c:0.95)");
    }

    #[test]
    fn from_novelai_leaves_escaped_brackets_as_literals() {
        assert_eq!(from_novelai(r"tag_\[1\]"), r"tag_\[1\]");
    }

    #[test]
    fn from_novelai_leaves_an_empty_bracket_pair_alone() {
        assert_eq!(from_novelai("a, {}, b"), "a, {}, b");
    }

    #[test]
    fn from_novelai_leaves_a_danbooru_parenthesis_tag_alone() {
        assert_eq!(
            from_novelai("hatsune_miku_(vocaloid)"),
            "hatsune_miku_(vocaloid)"
        );
    }

    #[test]
    fn from_novelai_handles_multibyte_text() {
        assert_eq!(
            from_novelai("1.2::\u{3053}\u{3093}::"),
            "(\u{3053}\u{3093}:1.20)"
        );
    }

    #[test]
    fn from_novelai_round_trips_a_comfyui_weight() {
        assert_eq!(from_novelai(&to_novelai("(tag:1.2)")), "(tag:1.20)");
    }
}

/// The single-pass bracket rewrite checked against the fixed-point loop it
/// replaced, which rescanned the whole prompt once per bracket pair.
#[cfg(test)]
mod linear_bracket_tests {
    use super::*;

    /// The original `from_novelai`, kept verbatim as the oracle.
    fn oracle_from_novelai(prompt: &str) -> String {
        let mut out = expand_novelai_weights(prompt);
        while let Some(next) = oracle_rewrite_innermost_bracket(&out, b'{', b'}', "1.05") {
            out = next;
        }
        while let Some(next) = oracle_rewrite_innermost_bracket(&out, b'[', b']', "0.95") {
            out = next;
        }
        out
    }

    fn oracle_rewrite_innermost_bracket(
        text: &str,
        open: u8,
        close: u8,
        weight: &str,
    ) -> Option<String> {
        let bytes = text.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] != close || is_escaped(bytes, i) {
                continue;
            }
            let mut start = i;
            let found = loop {
                if start == 0 {
                    break None;
                }
                start -= 1;
                if bytes[start] == open && !is_escaped(bytes, start) {
                    break Some(start);
                }
            };
            let Some(start) = found else { continue };
            let inner = &text[start + 1..i];
            if inner.trim().is_empty() {
                continue;
            }
            let mut out = String::with_capacity(text.len() + 8);
            out.push_str(&text[..start]);
            out.push_str(&format!("({inner}:{weight})"));
            out.push_str(&text[i + 1..]);
            return Some(out);
        }
        None
    }

    fn assert_matches_oracle(prompt: &str) {
        assert_eq!(
            from_novelai(prompt),
            oracle_from_novelai(prompt),
            "diverged on {prompt:?}"
        );
    }

    #[test]
    fn tricky_prompts_match_the_old_loop() {
        for prompt in [
            "",
            "{tag}",
            "{{tag}}",
            "{{{{tag}}}}",
            "[[tag]]",
            "{[tag]}",
            "[{tag}]",
            "{[tag}]",
            "[{tag]}",
            "{a}, b, [c]",
            "{a}}",
            "{{a}",
            "}{a}{",
            "}}{{",
            "{}",
            "{ }",
            "a, {}, b",
            "{}}",
            "{{}}",
            "{ {}}",
            "{ } a}",
            "{{ } a}",
            "{{ }} a}}",
            "{{{ } } a}",
            "[ ] {}",
            "{\u{3000}}",
            "{\n}",
            "{\u{3000}a}",
            r"\{a}",
            r"\\{a}",
            r"\\\{a}",
            r"{a\}",
            r"{a\\}",
            r"{a\}}",
            r"tag_\[1\]",
            r"{\}}",
            r"\\",
            "1.2::{a}::",
            "{1.2::a::}",
            "1.2::a::, {b}, [0.9::c::]",
            "{hatsune_miku_(vocaloid)}",
            "{\u{3053}\u{3093}}, [\u{3053}]",
            "{{a}, {b}}, [[c], [d]]",
        ] {
            assert_matches_oracle(prompt);
        }
    }

    #[test]
    fn random_prompts_match_the_old_loop() {
        // A fixed xorshift stream, so a failure reproduces.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let alphabet = [
            "{", "}", "[", "]", "\\", " ", "a", ":", "::", "1", ".", "(", ")", ",", "\u{3000}",
            "\u{e9}",
        ];
        for _ in 0..40_000 {
            let len = (next() % 14) as usize;
            let prompt: String = (0..len)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                .collect();
            assert_matches_oracle(&prompt);
        }
    }

    #[test]
    fn a_huge_prompt_converts_in_linear_time() {
        // The old loop took seconds on 40k pairs and minutes on a megabyte.
        let flat = "{a}".repeat(200_000);
        let out = from_novelai(&flat);
        assert_eq!(out.len(), "(a:1.05)".len() * 200_000);
        assert!(out.starts_with("(a:1.05)(a:1.05)"));

        let depth = 200_000;
        let nested = format!("{}a{}", "[".repeat(depth), "]".repeat(depth));
        let out = from_novelai(&nested);
        assert!(out.starts_with("((("));
        assert!(out.ends_with(":0.95):0.95)"));
        assert_eq!(out.matches(":0.95)").count(), depth);
    }
}
