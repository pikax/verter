//! Small syntactic identifier / literal-scanning helpers shared across the
//! Svelte IDE projector continuation modules (`mod`, `bind`, `special`).
//!
//! These are pure, allocation-free predicates over an identifier / expression
//! slice — they decide whether a directive local / tag name / `bind:this` lvalue
//! is safe to emit as a bare identifier or interpolate into a synthesised type
//! query, so the projection never produces invalid-identifier residue. Extracted
//! from `projector/mod.rs` for file size; re-exported through `mod` so the
//! sibling continuation modules reach them via `use super::*`.

/// Advance `chars` past the body of a string / template / char literal opened
/// by `quote`, honouring backslash escapes. Used by the function-binding
/// top-level-comma scanner so a comma inside a literal is not mistaken for the
/// `get, set` separator. (A template literal's `${…}` interpolation is not
/// descended — a top-level comma cannot legally appear there at the binding
/// expression's depth-0, so skipping the whole literal body is conservative and
/// correct for this heuristic.)
pub(super) fn skip_string_literal(chars: &mut std::iter::Peekable<std::str::Chars>, quote: char) {
    while let Some(c) = chars.next() {
        if c == '\\' {
            // Skip the escaped character.
            chars.next();
        } else if c == quote {
            return;
        }
    }
}

/// Whether `name` is a valid JS binding identifier — used to decide whether a
/// shorthand `style:color` / a `transition:`/`animate:` local can be projected
/// as a bare identifier reference. Conservative ASCII rule: a leading
/// `A-Za-z_$`, then `A-Za-z0-9_$`. A name failing this (empty, hyphenated, …)
/// is NOT emitted as an identifier (the directive is removed — no invalid
/// identifier residue in the projected TSX).
pub(super) fn is_valid_binding_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Whether `name` is a bare tag identifier safe to interpolate raw into a
/// `__VerterHostEl<"…">` string literal (no `"`, no newline, no backslash).
/// Used as a defensive guard at the `host_element_hint` interpolation site
/// (NIT-1) — the parser only classifies a bare tag as `Intrinsic`, so this holds
/// today; the guard hardens against a future producer change.
pub(super) fn is_bare_tag_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c != '"' && c != '\\' && c != '\n' && c != '\r' && c != '<' && c != '>')
}

/// Whether `name` is a valid component reference identifier to interpolate into
/// `InstanceType<typeof Name>` / `InstanceType<typeof Name>["$props"][…]`. A
/// component tag is PascalCase OR a dotted/namespaced member access
/// (`ns.Widget`) — both are valid `typeof` operands. A name with any other
/// character (a quote, a `<`, whitespace) is NOT emitted (the host falls back to
/// `Element`).
pub(super) fn is_valid_component_reference(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // Each dotted segment must be a valid identifier.
    name.split('.').all(is_valid_binding_identifier)
}

/// Whether `expr` is a TYPE-QUERY-SAFE lvalue — a bare identifier or a dotted
/// member chain (`el`, `refs.first`) — so `typeof expr` is a valid TS type
/// query. An element-access (`refs[i]`), a call, or any other expression is NOT
/// safe (`typeof refs[i]` parses `i` as a type), and the `bind:this` projection
/// routes those through the read-bearing invariant form instead. Whitespace is
/// trimmed first (a `{ el }`-style padded expression slice).
pub(super) fn is_type_query_safe_lvalue(expr: &str) -> bool {
    let trimmed = expr.trim();
    !trimmed.is_empty() && trimmed.split('.').all(is_valid_binding_identifier)
}
