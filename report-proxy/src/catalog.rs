include!("catalog_data.rs");

#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub title: &'static str,
    pub what: &'static str,
    pub why: &'static str,
    pub fixes: &'static str,
}

/// Look up the human-readable copy for an error code, if known.
pub fn lookup(error_code: &str) -> Option<CatalogEntry> {
    catalog_lookup(error_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_code_is_found() {
        let e = lookup("out_of_memory").expect("out_of_memory should exist");
        assert_eq!(e.title, "Ran out of memory");
        assert!(e.fixes.contains("||"), "fixes are || separated");
    }

    #[test]
    fn unknown_code_is_none() {
        assert!(lookup("no_such_code_xyz").is_none());
    }
}
