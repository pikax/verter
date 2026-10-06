//! Search the supplied known files in extension and index order.

use std::collections::HashMap;
use verter_session_query::resolution::{normalize_known_file_id, resolve_known_dependency_base};

#[must_use]
pub fn resolve_known_dependency_id(
    owner_id: &str,
    specifier: &str,
    known_index: &HashMap<String, String>,
    extensions: &[String],
) -> Option<String> {
    let resolved_base = resolve_known_dependency_base(owner_id, specifier)?;
    if let Some(match_id) = known_index.get(&normalize_known_file_id(&resolved_base)) {
        return Some(match_id.clone());
    }

    let mut seen = std::collections::HashSet::new();
    for extension in extensions {
        if extension.is_empty() {
            continue;
        }
        let with_extension = format!("{resolved_base}{extension}");
        if seen.insert(with_extension.clone()) {
            if let Some(match_id) = known_index.get(&normalize_known_file_id(&with_extension)) {
                return Some(match_id.clone());
            }
        }
        let with_index = format!("{}/index{extension}", resolved_base.trim_end_matches('/'));
        if seen.insert(with_index.clone()) {
            if let Some(match_id) = known_index.get(&normalize_known_file_id(&with_index)) {
                return Some(match_id.clone());
            }
        }
    }
    None
}
