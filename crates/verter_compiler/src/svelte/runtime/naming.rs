//! The component-function name derivation — the pinned official
//! `svelte@5.56.3` `get_component_name` + `Scope.generate` rule.

use rustc_hash::FxHashSet;

use super::SvelteRuntimeOptions;

/// The pinned official `svelte@5.56.3` reserved-word set (`RESERVED_WORDS`,
/// `src/utils.js`) — `Scope.generate` suffixes a `_N` counter until the name is
/// none of these. A generated component-function name equal to a reserved word is
/// invalid JS, so `var` / `class` / `await` / … deconflict to `var_1` / ….
const RESERVED_WORDS: &[&str] = &[
    "arguments",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Derive the component-function name, matching the pinned official
/// `svelte@5.56.3` derivation exactly (`get_component_name` then
/// `module.scope.generate`).
///
/// Official `get_component_name(filename)`: split the filename on `/` or `\`,
/// take the basename, drop the FIRST `.svelte` occurrence, replace an `index`
/// stem with the parent directory name when a parent exists and is not `src`,
/// then UPPERCASE the first character. The result (or an explicit `name` option,
/// which skips the capitalize) is then passed through `Scope.generate`, which
/// replaces every non-`[A-Za-z0-9_$]` UTF-16 code unit with `_`, replaces a LEADING
/// digit with `_`, and then suffixes a `_N` counter until the name collides with
/// none of: a reserved word, a declared user binding, or a referenced identifier
/// (`conflicts` — the union of the scope graph's declared names and every free
/// identifier referenced ANYWHERE in the component: template expressions AND the
/// instance / module scripts, matching svelte's `module.scope.generate` check against
/// `references` ∪ `declarations` ∪ `conflicts` over the module scope).
/// A missing filename defaults to `'(unknown)'`, which derives `_unknown_`. An
/// explicit `name` overrides the filename WITHOUT capitalizing.
pub(super) fn derive_component_name(
    opts: &SvelteRuntimeOptions,
    conflicts: &FxHashSet<String>,
) -> String {
    let preferred = match &opts.name {
        // An explicit `name` override is used verbatim (NOT capitalized), then
        // sanitized by `generate`.
        Some(name) => name.clone(),
        // Otherwise derive from the filename (default `'(unknown)'`).
        None => component_name_from_filename(opts.filename.as_deref().unwrap_or("(unknown)")),
    };
    generate_identifier(&preferred, conflicts)
}

/// The official `get_component_name(filename)` — the filename-derived component
/// name BEFORE identifier sanitization.
///
/// Faithful to `svelte@5.56.3` `get_component_name`: from the basename, drop the
/// FIRST `.svelte` literal occurrence (`basename.replace('.svelte', '')` — a JS
/// string-pattern `replace` hits the first match only), NOT the last file extension.
/// So `App.svelte` → `App`, `foo.bar.svelte` → `foo.bar`, `index.svelte` → `index`,
/// and `Widget.svelte.test.svelte` → `Widget.test.svelte` (the interior dots later
/// sanitize to `_`). A non-`.svelte` carrier keeps its dots (only the literal
/// `.svelte` is stripped). This is NOT `Path::file_stem` (which drops the LAST
/// extension: `Widget.svelte.test.svelte` → `Widget.svelte.test`, a divergence).
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
    // Drop the FIRST carrier-suffix occurrence from the basename (the official JS
    // strips the first match of the carrier suffix), NOT the last file extension. The
    // carrier suffix is sourced from the language authority — the canonical `svelte`
    // token the registry builds the carrier row from — rather than a hardcoded
    // extension literal (the single-language-classifier rule owns extension matching);
    // this is a plain find-and-splice of the first occurrence, not post-hoc
    // string-munging of built codegen output.
    let carrier_suffix = format!(
        ".{}",
        verter_language::FrameworkAdapterId::svelte().as_str()
    );
    let mut name = match basename.find(&carrier_suffix) {
        Some(pos) => {
            let mut stripped = String::with_capacity(basename.len() - carrier_suffix.len());
            stripped.push_str(&basename[..pos]);
            stripped.push_str(&basename[pos + carrier_suffix.len()..]);
            stripped
        }
        None => basename.to_string(),
    };
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

/// The official `Scope.generate`: sanitize (replace every non-`[A-Za-z0-9_$]`
/// UTF-16 code unit with `_`, then replace a LEADING ASCII digit with `_`), then
/// suffix a `_N` counter until the name is none of a reserved word, a declared user
/// binding, or a referenced identifier (`conflicts`). Matches svelte's
/// `preferred.replace(…).replace(…)` + the
/// `while (references|declarations|conflicts|is_reserved) name = ${preferred}_${n++}`
/// loop. The sanitize runs per UTF-16 CODE UNIT (svelte's regex operates on the
/// JS UTF-16 string, not Unicode scalars): a non-ASCII BMP char is ONE `_`, and an
/// astral char (a surrogate PAIR) is TWO `_` (`"💩"` → `"__"`). An empty sanitized
/// name has no conflict, so it stays empty (an anonymous
/// `export default function ($$anchor)`), exactly as svelte's `generate('')`.
fn generate_identifier(preferred: &str, conflicts: &FxHashSet<String>) -> String {
    let sanitized: String = preferred
        .encode_utf16()
        .map(|unit| {
            // A valid identifier char is ASCII ([A-Za-z0-9_$]) — a single UTF-16
            // unit. Every other unit (a non-ASCII BMP char, or EITHER half of an
            // astral surrogate pair) maps to one `_`.
            if unit < 128 {
                let c = unit as u8 as char;
                if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                    c
                } else {
                    '_'
                }
            } else {
                '_'
            }
        })
        .collect();
    // A LEADING digit is REPLACED (not prefixed) by `_`.
    let sanitized = if sanitized.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        let mut chars = sanitized.chars();
        chars.next();
        format!("_{}", chars.as_str())
    } else {
        sanitized
    };
    // Deconflict: the sanitized name wins unless it is a reserved word, a declared
    // user binding, or a referenced identifier, in which case `${sanitized}_${n}` is
    // tried for n = 1, 2, ….
    let is_conflict = |name: &str| RESERVED_WORDS.contains(&name) || conflicts.contains(name);
    if !is_conflict(&sanitized) {
        return sanitized;
    }
    let mut n = 1u32;
    loop {
        let candidate = format!("{sanitized}_{n}");
        if !is_conflict(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{derive_component_name, svelte_hash};
    use crate::svelte::runtime::SvelteRuntimeOptions;
    use rustc_hash::FxHashSet;

    /// Derive the filename-fallback component name (no explicit `name`, no conflicts).
    fn from_filename(filename: &str) -> String {
        let opts = SvelteRuntimeOptions {
            filename: Some(filename.to_string()),
            ..Default::default()
        };
        derive_component_name(&opts, &FxHashSet::default())
    }

    #[test]
    fn filename_fallback_matches_official_get_component_name() {
        // svelte@5.56.3 `get_component_name` = `basename.replace('.svelte', '')` — JS
        // string-pattern replace drops the FIRST `.svelte` occurrence, NOT the last
        // file extension (`Path::file_stem`). Then capitalize the first char and pass
        // through `Scope.generate` (which sanitizes non-identifier chars to `_`).
        assert_eq!(from_filename("App.svelte"), "App");
        // A multi-dot stem: `foo.bar` → capitalize `Foo.bar` → sanitize → `Foo_bar`.
        assert_eq!(from_filename("foo.bar.svelte"), "Foo_bar");
        // The DISCRIMINATOR: a `.svelte.test.svelte` carrier drops only the
        // FIRST `.svelte` (→ `Widget.test.svelte` → sanitize → `Widget_test_svelte`),
        // NOT the last extension (`file_stem` → `Widget.svelte.test` →
        // `Widget_svelte_test`, the pre-fix divergence).
        assert_eq!(
            from_filename("Widget.svelte.test.svelte"),
            "Widget_test_svelte"
        );
        // NEGATIVE: it is NOT the file_stem-derived (last-extension) spelling.
        assert_ne!(
            from_filename("Widget.svelte.test.svelte"),
            "Widget_svelte_test"
        );
        // `index` resolves to the parent directory name unless the parent is `src`.
        assert_eq!(from_filename("foo/index.svelte"), "Foo");
        assert_eq!(from_filename("src/index.svelte"), "Index");
        // A non-`.svelte` carrier keeps its extension dots (only the `.svelte` literal
        // is stripped, per `.replace('.svelte', '')`), which then sanitize to `_`.
        assert_eq!(from_filename("App.svg"), "App_svg");
    }

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
