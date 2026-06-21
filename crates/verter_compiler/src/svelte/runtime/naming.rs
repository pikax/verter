//! The component-function name derivation — the pinned official
//! `svelte@5.56.3` `get_component_name` + `Scope.generate` rule.

use super::SvelteRuntimeOptions;

/// Derive the component-function name, matching the pinned official
/// `svelte@5.56.3` derivation exactly (`get_component_name` then
/// `Scope.generate`).
///
/// Official `get_component_name(filename)`: split the filename on `/` or `\`,
/// take the basename, drop the FIRST `.svelte` occurrence, replace an `index`
/// stem with the parent directory name when a parent exists and is not `src`,
/// then UPPERCASE the first character. The result (or an explicit `name` option,
/// which skips the capitalize) is then passed through `Scope.generate`, which
/// replaces every non-`[A-Za-z0-9_$]` character with `_` and replaces a LEADING
/// digit with `_`. A missing filename defaults to `'(unknown)'`, which derives
/// `_unknown_`. An explicit `name` overrides the filename WITHOUT capitalizing.
pub(super) fn derive_component_name(opts: &SvelteRuntimeOptions) -> String {
    let preferred = match &opts.name {
        // An explicit `name` override is used verbatim (NOT capitalized), then
        // sanitized by `generate`.
        Some(name) => name.clone(),
        // Otherwise derive from the filename (default `'(unknown)'`).
        None => component_name_from_filename(opts.filename.as_deref().unwrap_or("(unknown)")),
    };
    generate_identifier(&preferred)
}

/// The official `get_component_name(filename)` — the filename-derived component
/// name BEFORE identifier sanitization.
///
/// The carrier extension is dropped EXTENSION-AGNOSTICALLY via `Path::file_stem`
/// (no hardcoded carrier-extension literal — the language-classification authority
/// owns extension matching). For every real carrier filename this equals the
/// official `basename.replace('.svelte', '')` (the stem of `App.svelte` is `App`,
/// of `foo.bar.svelte` is `foo.bar`, of `index.svelte` is `index`).
fn component_name_from_filename(filename: &str) -> String {
    // Split on `/` or `\`; the basename is the last segment, `last_dir` the one
    // before it.
    let parts: Vec<&str> = filename.split(['/', '\\']).collect();
    let basename = parts.last().copied().unwrap_or(filename);
    let last_dir = if parts.len() >= 2 {
        Some(parts[parts.len() - 2])
    } else {
        None
    };
    // The stem with its (single) extension dropped — extension-agnostic.
    let mut name = std::path::Path::new(basename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(basename)
        .to_string();
    // `index` → the parent directory name, unless the parent is `src`.
    if name == "index" {
        if let Some(dir) = last_dir {
            if dir != "src" {
                name = dir.to_string();
            }
        }
    }
    // Uppercase the first character.
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => name,
    }
}

/// The official `Scope.generate` identifier sanitization: replace every
/// non-`[A-Za-z0-9_$]` character with `_`, then replace a LEADING ASCII digit
/// with `_`.
fn generate_identifier(preferred: &str) -> String {
    let mut out: String = preferred
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // A LEADING digit is REPLACED (not prefixed) by `_`.
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        let mut chars = out.chars();
        chars.next();
        out = format!("_{}", chars.as_str());
    }
    out
}
