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

/// The official `svelte@5.56.3` `hash(str)` (`src/compiler/utils.js`) — the djb2-XOR string
/// hash a `<svelte:head>`'s `$.head('<hash>', …)` scope key is built from (applied to the
/// compile `filename`). Byte-for-byte faithful: strip carriage returns, seed `5381`, fold each
/// UTF-16 code unit in REVERSE order (`((h << 5) - h) ^ code`, 32-bit signed wrapping), then
/// emit the UNSIGNED 32-bit result (`>>> 0`) in base36. The topology comparator treats the
/// emitted hash literal as STRUCTURAL, so it must match the official over the same filename.
pub(super) fn svelte_hash(input: &str) -> String {
    // `str.replace(/\r/g, '')` — carriage returns never enter the fold.
    let stripped: String = input.chars().filter(|&c| c != '\r').collect();
    // JS bitwise arithmetic is 32-bit signed (`ToInt32`): `i32` + `wrapping_*` reproduces the
    // `(h << 5) - h` then `^ code` chain exactly (the intermediate double subtraction is
    // re-narrowed to int32 by the following `^`, which equals `wrapping_sub` mod 2^32).
    let units: Vec<u16> = stripped.encode_utf16().collect();
    let mut hash: i32 = 5381;
    for &unit in units.iter().rev() {
        hash = hash.wrapping_shl(5).wrapping_sub(hash) ^ i32::from(unit);
    }
    // `(hash >>> 0).toString(36)` — reinterpret as unsigned, base36 (lowercase, `0` for zero).
    to_base36(hash as u32)
}

/// The unsigned base36 spelling (`0-9a-z`) of `n` — the official `Number.prototype.toString(36)`
/// (`'0'` for zero, no sign, lowercase digits).
fn to_base36(mut n: u32) -> String {
    if n == 0 {
        return "0".to_string();
    }
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    // Every byte is an ASCII digit, so the conversion is infallible.
    String::from_utf8(buf).expect("base36 digits are ASCII")
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

#[cfg(test)]
mod tests {
    use super::svelte_hash;

    #[test]
    fn svelte_hash_matches_official_over_head_fixture_filenames() {
        // Byte-exact against the pinned `svelte@5.56.3` `hash(filename)` (the values the
        // committed `special/svelte_head_*` goldens' `$.head('<hash>', …)` carry). A drift here
        // is a structural conformance failure (the topology comparator signs the literal).
        assert_eq!(svelte_hash("special/svelte_head_html.svelte"), "1tufvvq");
        assert_eq!(
            svelte_hash("special/svelte_head_static_title.svelte"),
            "63bkss"
        );
        assert_eq!(
            svelte_hash("special/svelte_head_prop_title.svelte"),
            "16e2757"
        );
        assert_eq!(
            svelte_hash("special/svelte_head_state_title.svelte"),
            "1523ehv"
        );
        assert_eq!(
            svelte_hash("special/svelte_head_title_meta.svelte"),
            "75ty8d"
        );
        assert_eq!(svelte_hash("special/svelte_head_meta.svelte"), "w8zktq");
        assert_eq!(
            svelte_hash("special/svelte_head_body_sibling.svelte"),
            "rlmige"
        );
        // Edge: an empty string hashes the seed `5381` (`>>> 0`) in base36.
        assert_eq!(svelte_hash(""), "45h");
        // NEGATIVE: distinct filenames hash distinctly (not a constant).
        assert_ne!(svelte_hash("a.svelte"), svelte_hash("b.svelte"));
    }
}
