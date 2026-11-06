//! Error bodies that must never reach the screen as they are.
//!
//! Most services the app calls sit behind Cloudflare or another proxy, and
//! when that front layer refuses or fails a request it answers with its own
//! HTML page (a 403 block, a challenge, a 502). Quoting that body in an error
//! message prints a wall of markup where a sentence belongs, so every error
//! path that quotes a response body goes through here first.

/// Characters of a web page kept for the log, enough for a Cloudflare ray ID.
const LOGGED_PAGE_CHARS: usize = 300;

/// Characters of a plain error body kept in a `detail_suffix`.
const DETAIL_CHARS: usize = 300;

/// Whether an error body is markup (an HTML or XML page) rather than an API
/// answer. JSON and plain-text errors never start with `<`.
pub fn is_web_page(body: &str) -> bool {
    body.trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with('<')
}

/// What to say instead of a web page, naming the service that answered.
pub fn web_page_message(service: &str, status: u16) -> String {
    match status {
        403 => format!(
            "{service} turned the request away (Cloudflare or a firewall). Try again in a minute."
        ),
        429 => format!("{service} is rate limiting requests. Try again shortly."),
        _ => format!("{service} is not responding right now (HTTP {status}). Try again shortly."),
    }
}

/// An error body ready to show: as it came, unless it is a web page, which
/// becomes [`web_page_message`] (the start of the page goes to the log).
pub fn readable_body(service: &str, status: u16, body: &str) -> String {
    if is_web_page(body) {
        log_page(service, status, body);
        web_page_message(service, status)
    } else {
        body.to_string()
    }
}

/// The `": detail"` tail for messages shaped like `"X returned 403{detail}"`:
/// the start of a plain body, a short sentence for a web page, or nothing for
/// an empty body.
pub fn detail_suffix(status: u16, body: &str) -> String {
    if is_web_page(body) {
        log_page("A server", status, body);
        return if status == 403 {
            ": Cloudflare or a firewall turned the request away. Try again in a minute.".to_string()
        } else {
            ": the server sent an error page instead of an answer. Try again shortly.".to_string()
        };
    }
    let detail: String = body.trim().chars().take(DETAIL_CHARS).collect();
    if detail.is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    }
}

fn log_page(service: &str, status: u16, body: &str) {
    log::warn!(
        "{service} answered HTTP {status} with a web page: {}",
        body.chars().take(LOGGED_PAGE_CHARS).collect::<String>()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The start of the page that showed up under the NovelAI Anlas readout.
    const CLOUDFLARE_PAGE: &str = "<!DOCTYPE html>\n<!--[if lt IE 7]> <html class=\"no-js ie6 oldie\" lang=\"en-US\"> <![endif]-->";

    #[test]
    fn pages_are_recognised_and_api_answers_are_not() {
        assert!(is_web_page(CLOUDFLARE_PAGE));
        assert!(is_web_page(
            "\u{feff}  <html><body>502 Bad Gateway</body></html>"
        ));
        assert!(is_web_page("<?xml version=\"1.0\"?><Error/>"));
        assert!(!is_web_page(r#"{"error":"nope"}"#));
        assert!(!is_web_page("Internal Server Error"));
        assert!(!is_web_page(""));
    }

    #[test]
    fn a_page_becomes_one_sentence_naming_the_service() {
        let m = readable_body("CivitAI", 403, CLOUDFLARE_PAGE);
        assert!(!m.contains('<'), "{m}");
        assert!(m.starts_with("CivitAI turned the request away"), "{m}");
        assert_eq!(
            readable_body("ComfyUI", 502, "<html></html>"),
            "ComfyUI is not responding right now (HTTP 502). Try again shortly."
        );
    }

    #[test]
    fn plain_bodies_pass_through() {
        let json = r#"{"error":"prompt_outputs_failed_validation","node_errors":{}}"#;
        assert_eq!(readable_body("ComfyUI", 400, json), json);
    }

    #[test]
    fn detail_suffix_quotes_text_and_never_a_page() {
        assert_eq!(
            detail_suffix(400, "  model not found \n"),
            ": model not found"
        );
        assert_eq!(detail_suffix(500, ""), "");
        let page = detail_suffix(403, CLOUDFLARE_PAGE);
        assert!(!page.contains('<') && page.contains("Cloudflare"), "{page}");
        assert!(!detail_suffix(503, "<html>down</html>").contains('<'));
        let long = "x".repeat(1000);
        assert_eq!(detail_suffix(500, &long).len(), 2 + 300);
    }
}
