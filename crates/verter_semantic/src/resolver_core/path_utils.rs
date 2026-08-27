//! Dependency-neutral path and carrier helpers used by module resolution.

use std::collections::HashMap;

#[cfg(test)]
thread_local! {
    pub(crate) static NORMALIZE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The reserved carrier API virtual-file suffix.
pub const CARRIER_API_VIRTUAL_SUFFIX: &str = ".verter.ts";

/// The module-specifier spelling used to reach the carrier API virtual file.
pub const CARRIER_API_MODULE_SPECIFIER_SUFFIX: &str = ".verter.js";

#[must_use]
pub fn carrier_ide_provider_path(source_id: &str, is_jsx: bool) -> String {
    let ext = if is_jsx { ".jsx" } else { ".tsx" };
    format!("{source_id}{ext}")
}

#[must_use]
pub fn carrier_api_provider_path(source_id: &str) -> String {
    format!("{source_id}{CARRIER_API_VIRTUAL_SUFFIX}")
}

#[must_use]
pub fn carrier_source_extensions() -> Vec<String> {
    verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .iter()
        .map(|ext| (*ext).to_string())
        .collect()
}

#[must_use]
pub fn path_is_carrier(path: &str) -> bool {
    verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .iter()
        .any(|ext| {
            let needle = format!(".{ext}");
            path.len() > needle.len() && path.ends_with(&needle)
        })
}

#[must_use]
pub fn strip_carrier_extension(path: &str) -> &str {
    let registry = verter_language::LanguageRegistry::global();
    let mut extensions = registry.carrier_extensions();
    extensions.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    for ext in extensions {
        let needle = format!(".{ext}");
        if path.len() > needle.len() {
            if let Some(stem) = path.strip_suffix(&needle) {
                return stem;
            }
        }
    }
    path
}

#[must_use]
pub fn normalize_canonical_id(value: &str) -> String {
    #[cfg(test)]
    NORMALIZE_CALLS.with(|calls| calls.set(calls.get() + 1));
    verter_audit::attribute_n!(NormalizeCanonicalId, value.len());
    verter_span::path::canonicalize_path(value)
}

#[must_use]
pub fn collapse_path(value: &str) -> String {
    verter_audit::attribute_n!(CollapsePath, value.len());
    let normalized = normalize_canonical_id(value);

    if let Some(after) = normalized.strip_prefix("//") {
        let mut segs = after.split('/').filter(|s| !s.is_empty());
        let mut root = String::from("//");
        if let Some(host) = segs.next() {
            root.push_str(host);
        }
        if let Some(share) = segs.next() {
            root.push('/');
            root.push_str(share);
        }
        let mut parts: Vec<&str> = Vec::new();
        for part in segs {
            match part {
                "." => {}
                ".." => {
                    parts.pop();
                }
                part => parts.push(part),
            }
        }
        return if parts.is_empty() {
            root
        } else {
            format!("{root}/{}", parts.join("/"))
        };
    }

    let (prefix, rest) = if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        (normalized[..2].to_string(), normalized[2..].to_string())
    } else {
        (String::new(), normalized.clone())
    };

    let absolute = rest.starts_with('/');
    let mut parts = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if let Some(last) = parts.last() {
                    if *last != ".." {
                        parts.pop();
                    } else if !absolute {
                        parts.push("..");
                    }
                } else if !absolute {
                    parts.push("..");
                }
            }
            part => parts.push(part),
        }
    }

    let mut result = String::new();
    if !prefix.is_empty() {
        result.push_str(&prefix);
    }
    if absolute {
        result.push('/');
    }
    result.push_str(&parts.join("/"));

    if result.is_empty() {
        if absolute {
            "/".to_string()
        } else {
            ".".to_string()
        }
    } else if result.len() == 2 && result.as_bytes()[1] == b':' {
        format!("{result}/")
    } else {
        result
    }
}

#[must_use]
pub fn join_paths(base: &str, path: &str) -> String {
    if path.is_empty() {
        return normalize_canonical_id(base);
    }
    if is_absolute_specifier(path) {
        return collapse_path(path);
    }

    let normalized_base = normalize_canonical_id(base)
        .trim_end_matches('/')
        .to_string();
    let normalized_path = normalize_canonical_id(path);
    collapse_path(&format!(
        "{}/{}",
        normalized_base,
        normalized_path
            .trim_start_matches("./")
            .trim_start_matches('/')
    ))
}

#[must_use]
pub fn parent_dir(path: &str) -> String {
    let normalized = normalize_canonical_id(path);
    normalized
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

#[must_use]
pub fn is_relative_specifier(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with(".\\")
        || specifier.starts_with("..\\")
}

#[must_use]
pub fn is_absolute_specifier(specifier: &str) -> bool {
    specifier.starts_with('/')
        || specifier.starts_with('\\')
        || specifier.as_bytes().get(1) == Some(&b':')
}

#[must_use]
pub fn build_known_file_index(known_ids: &[String]) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for known_id in known_ids {
        index
            .entry(normalize_known_file_id(known_id))
            .or_insert_with(|| known_id.clone());
    }
    index
}

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

#[must_use]
pub fn resolve_known_dependency_base(owner_id: &str, specifier: &str) -> Option<String> {
    if is_relative_specifier(specifier) {
        return Some(join_paths(&parent_dir(owner_id), specifier));
    }
    if is_absolute_specifier(specifier) {
        return Some(collapse_path(specifier));
    }
    None
}

#[must_use]
pub fn normalize_known_file_id(file_id: &str) -> String {
    collapse_path(file_id)
}
