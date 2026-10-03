//! Conversation history for the prompt enhancer's persistent sessions.
//!
//! The frontend owns the transcript and sends it with every request; this
//! module turns it into each wire format. Only complete user-then-assistant
//! exchanges survive `sanitize`, so a stale or hand-edited client cannot send
//! an orphaned turn that an API would reject.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

/// Keep only complete, non-empty user then assistant pairs, in order.
pub fn sanitize(history: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut out = Vec::with_capacity(history.len());
    let mut i = 0;
    while i + 1 < history.len() {
        let (asked, answered) = (&history[i], &history[i + 1]);
        if asked.role == Role::User
            && answered.role == Role::Assistant
            && !asked.content.trim().is_empty()
            && !answered.content.trim().is_empty()
        {
            out.push(asked.clone());
            out.push(answered.clone());
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

/// Browser-mode handlers receive raw JSON; anything unreadable means no history.
pub fn history_from_args(value: &Value) -> Vec<ChatMessage> {
    serde_json::from_value::<Vec<ChatMessage>>(value.clone())
        .map(|h| sanitize(&h))
        .unwrap_or_default()
}

fn turns(history: &[ChatMessage]) -> impl Iterator<Item = Value> + '_ {
    history
        .iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
}

/// OpenAI-compatible and llama-server: system, the session, then the new turn.
pub fn openai_messages(system: &str, history: &[ChatMessage], user_content: Value) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": system })];
    messages.extend(turns(history));
    messages.push(json!({ "role": "user", "content": user_content }));
    Value::Array(messages)
}

/// Anthropic carries the system prompt in its own field, so only the turns.
pub fn anthropic_messages(history: &[ChatMessage], user_content: Value) -> Value {
    let mut messages: Vec<Value> = turns(history).collect();
    messages.push(json!({ "role": "user", "content": user_content }));
    Value::Array(messages)
}

/// A tag upsampler (DanTagGen) is a completion model, not a chat model: earlier
/// turns would be read as more tags to continue, so it never gets any.
pub fn for_purpose<'a>(purpose: &str, history: &'a [ChatMessage]) -> &'a [ChatMessage] {
    if purpose == "tag_upsampler" {
        &[]
    } else {
        history
    }
}

/// Told only when earlier turns are actually sent. Plain enhance is a one-click
/// rewrite of whatever is in the box, and users change subject freely, so the
/// session is context for references, not material to carry over.
pub fn with_session_clause(system: &str, history: &[ChatMessage]) -> String {
    if history.is_empty() {
        return system.to_string();
    }
    format!(
        "{system}\n\nEarlier messages are previous requests in this session and your answers to them. \
Work only on the newest message. Use earlier turns only when it refers to them, and never carry \
characters, tags or settings over from them on your own."
    )
}

/// The companions take one text turn, so earlier exchanges are written into it.
pub fn single_turn_text(history: &[ChatMessage], user: &str) -> String {
    if history.is_empty() {
        return user.to_string();
    }
    let mut text = String::from("Previous conversation in this session, oldest first:\n\n");
    for m in history {
        let who = match m.role {
            Role::User => "User",
            Role::Assistant => "You",
        };
        text.push_str(&format!("[{who}]\n{}\n\n", m.content.trim()));
    }
    text.push_str("[New request]\n");
    text.push_str(user);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(s: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: s.into(),
        }
    }
    fn assistant(s: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: s.into(),
        }
    }

    #[test]
    fn sanitize_keeps_complete_pairs_in_order() {
        let h = vec![user("a"), assistant("b"), user("c"), assistant("d")];
        assert_eq!(sanitize(&h), h);
    }

    #[test]
    fn sanitize_drops_orphans_empties_and_misordered_turns() {
        let h = vec![
            assistant("orphan answer"),
            user("a"),
            assistant("b"),
            user("question with no answer"),
            user("c"),
            assistant("   "),
            user("e"),
            assistant("f"),
        ];
        assert_eq!(
            sanitize(&h),
            vec![user("a"), assistant("b"), user("e"), assistant("f")]
        );
    }

    #[test]
    fn history_from_args_tolerates_missing_and_malformed_values() {
        assert!(history_from_args(&Value::Null).is_empty());
        assert!(history_from_args(&json!("nope")).is_empty());
        assert!(history_from_args(&json!([{ "role": "robot", "content": "x" }])).is_empty());
        assert_eq!(
            history_from_args(&json!([
                { "role": "user", "content": "a" },
                { "role": "assistant", "content": "b" }
            ])),
            vec![user("a"), assistant("b")]
        );
    }

    #[test]
    fn openai_messages_put_system_first_and_new_turn_last() {
        let v = openai_messages("sys", &[user("a"), assistant("b")], json!("new"));
        assert_eq!(
            v,
            json!([
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "a" },
                { "role": "assistant", "content": "b" },
                { "role": "user", "content": "new" }
            ])
        );
    }

    #[test]
    fn empty_history_matches_the_old_single_turn_payload() {
        assert_eq!(
            openai_messages("sys", &[], json!("new")),
            json!([
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "new" }
            ])
        );
        assert_eq!(
            anthropic_messages(&[], json!("new")),
            json!([{ "role": "user", "content": "new" }])
        );
        assert_eq!(single_turn_text(&[], "new"), "new");
    }

    #[test]
    fn anthropic_messages_leave_the_system_prompt_out() {
        let v = anthropic_messages(&[user("a"), assistant("b")], json!("new"));
        assert_eq!(
            v,
            json!([
                { "role": "user", "content": "a" },
                { "role": "assistant", "content": "b" },
                { "role": "user", "content": "new" }
            ])
        );
    }

    #[test]
    fn tag_upsamplers_get_no_history() {
        let h = vec![user("a"), assistant("b")];
        assert!(for_purpose("tag_upsampler", &h).is_empty());
        assert_eq!(for_purpose("natural_language", &h), &h[..]);
    }

    #[test]
    fn session_clause_appears_only_with_history() {
        assert_eq!(with_session_clause("sys", &[]), "sys");
        let s = with_session_clause("sys", &[user("a"), assistant("b")]);
        assert!(s.starts_with("sys\n\n"));
        assert!(s.contains("Work only on the newest message"));
        assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'));
    }

    #[test]
    fn single_turn_text_writes_the_session_before_the_request() {
        let t = single_turn_text(
            &[user("make it night"), assistant("BASE: night")],
            "add rain",
        );
        assert!(t.starts_with("Previous conversation in this session, oldest first:"));
        let night = t.find("make it night").unwrap();
        let answer = t.find("BASE: night").unwrap();
        let new = t.find("[New request]\nadd rain").unwrap();
        assert!(night < answer && answer < new);
        assert!(t.ends_with("add rain"));
    }
}
