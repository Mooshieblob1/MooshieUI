//! Where an instance owner's API key comes from: the field itself, or an
//! environment variable it names.
//!
//! Any owner key field (CivitAI, NovelAI, ElevenLabs, fal.ai, Segmind, the
//! external LLM) can hold `env:NAME` instead of the key. The key is then read
//! from MooshieUI's own environment each time it is used, so it never touches
//! `config.json`. Only the owner may do this: a named account or a moderator
//! naming a variable would make the server hand the owner's environment to a
//! provider of their choosing, so those paths refuse it (see
//! [`crate::user_secrets`] and the webserver's role checks).
//!
//! Compiles in both the desktop and the server build.

use serde::Serialize;

/// Prefix that marks a stored value as a variable name rather than a key.
pub const ENV_PREFIX: &str = "env:";

/// Longest variable name accepted. Real ones are far shorter.
const MAX_NAME_LEN: usize = 128;

/// MooshieUI's own settings (`MOOSHIEUI_SECRET_KEY`, `MOOSHIEUI_ADMIN_PASS`,
/// ...) are never a provider key, so naming one is refused rather than sent
/// to a third party as a bearer token.
const RESERVED_PREFIX: &str = "MOOSHIEUI_";

/// The variable a stored value names, if it is a reference at all.
pub fn env_name(stored: &str) -> Option<&str> {
    stored.trim().strip_prefix(ENV_PREFIX).map(str::trim)
}

/// Whether `stored` names a variable rather than holding a key.
pub fn is_env_ref(stored: &str) -> bool {
    env_name(stored).is_some()
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= MAX_NAME_LEN
        && (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.to_ascii_uppercase().starts_with(RESERVED_PREFIX)
}

/// Refuse a reference that could never resolve, so the owner hears about a
/// typo when saving rather than as a missing key later. A plain key passes.
pub fn check(stored: &str) -> Result<(), String> {
    match env_name(stored) {
        Some(name) if !valid_name(name) => Err(format!(
            "\"{name}\" is not a usable environment variable name. Use letters, digits and \
             underscores, starting with a letter, and not a MOOSHIEUI_ setting."
        )),
        _ => Ok(()),
    }
}

/// The key a stored owner value stands for: the value itself, or the named
/// variable's value. `None` when nothing usable is there.
pub fn resolve(stored: Option<&str>) -> Option<String> {
    resolve_with(stored, |name| std::env::var(name).ok())
}

fn resolve_with(
    stored: Option<&str>,
    lookup: impl FnOnce(&str) -> Option<String>,
) -> Option<String> {
    let stored = stored?.trim();
    if stored.is_empty() {
        return None;
    }
    let value = match env_name(stored) {
        Some(name) if valid_name(name) => lookup(name)?,
        Some(_) => return None,
        None => stored.to_string(),
    };
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// What the settings UI shows for a field that names a variable. Carries the
/// name, never the value.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnvKeyRef {
    pub name: String,
    /// Whether the variable holds a value in MooshieUI's environment.
    pub set: bool,
}

pub fn env_ref(stored: Option<&str>) -> Option<EnvKeyRef> {
    let name = env_name(stored?)?;
    Some(EnvKeyRef {
        name: name.to_string(),
        set: resolve(stored).is_some(),
    })
}

/// Which of the owner's key fields name a variable, for the settings UI.
/// `available` is false for anyone but the owner, who then gets no names and
/// no option to enter one.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct KeyEnvStatus {
    pub available: bool,
    pub keys: std::collections::BTreeMap<&'static str, EnvKeyRef>,
}

pub fn owner_status(config: &crate::config::AppConfig) -> KeyEnvStatus {
    let fields: [(&'static str, Option<&str>); 6] = [
        ("civitai", config.civitai_api_key.as_deref()),
        ("novelai", config.novelai_api_key.as_deref()),
        ("elevenlabs", config.elevenlabs_api_key.as_deref()),
        ("fal", config.fal_api_key.as_deref()),
        ("segmind", config.segmind_api_key.as_deref()),
        ("llm", Some(config.llm_external_api_key.as_str())),
    ];
    KeyEnvStatus {
        available: true,
        keys: fields
            .into_iter()
            .filter_map(|(field, stored)| Some((field, env_ref(stored)?)))
            .collect(),
    }
}

/// The error to show when a reference does not resolve, naming the variable,
/// or `None` when the field is not a reference.
pub fn missing_message(stored: Option<&str>, provider: &str) -> Option<String> {
    let name = env_name(stored?)?;
    Some(format!(
        "The {provider} key is set to come from the environment variable {name}, but MooshieUI \
         does not see a value for it. Set it, then restart MooshieUI."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str) -> Option<String> {
        match name {
            "FAL_KEY" => Some(" fal-from-env \n".into()),
            "EMPTY" => Some("  ".into()),
            _ => None,
        }
    }

    #[test]
    fn a_plain_key_is_used_as_is() {
        assert_eq!(
            resolve_with(Some("  sk-plain "), |_| panic!("no lookup")),
            Some("sk-plain".into())
        );
        assert_eq!(resolve_with(Some("  "), |_| panic!("no lookup")), None);
        assert_eq!(resolve_with(None, |_| panic!("no lookup")), None);
    }

    #[test]
    fn a_reference_reads_the_variable() {
        assert_eq!(
            resolve_with(Some("env:FAL_KEY"), lookup),
            Some("fal-from-env".into())
        );
        assert_eq!(
            resolve_with(Some(" env: FAL_KEY "), lookup),
            Some("fal-from-env".into())
        );
        assert_eq!(resolve_with(Some("env:MISSING"), lookup), None);
        assert_eq!(resolve_with(Some("env:EMPTY"), lookup), None);
    }

    #[test]
    fn bad_and_reserved_names_never_resolve() {
        for stored in [
            "env:",
            "env:1ABC",
            "env:A-B",
            "env:MOOSHIEUI_SECRET_KEY",
            "env:mooshieui_admin_pass",
        ] {
            assert_eq!(
                resolve_with(Some(stored), |_| Some("leaked".into())),
                None,
                "{stored}"
            );
            assert!(check(stored).is_err(), "{stored}");
        }
        assert!(check("env:_PRIVATE_KEY2").is_ok());
        assert!(check("sk-anything").is_ok());
    }

    #[test]
    fn the_status_carries_the_name_only() {
        assert_eq!(env_ref(Some("sk-plain")), None);
        let r = env_ref(Some("env:SURELY_NOT_SET_IN_TESTS_9F2")).unwrap();
        assert_eq!(r.name, "SURELY_NOT_SET_IN_TESTS_9F2");
        assert!(!r.set);
        assert!(missing_message(Some("env:X_KEY"), "fal.ai")
            .unwrap()
            .contains("X_KEY"));
        assert_eq!(missing_message(Some("sk"), "fal.ai"), None);
    }

    #[test]
    fn owner_status_lists_only_references() {
        let config = crate::config::AppConfig {
            civitai_api_key: Some("civitai-plain".into()),
            fal_api_key: Some("env:SURELY_NOT_SET_IN_TESTS_FAL".into()),
            llm_external_api_key: "env:SURELY_NOT_SET_IN_TESTS_LLM".into(),
            ..Default::default()
        };
        let status = owner_status(&config);
        assert!(status.available);
        assert_eq!(
            status.keys.keys().copied().collect::<Vec<_>>(),
            vec!["fal", "llm"]
        );
        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("civitai-plain"));
    }
}
