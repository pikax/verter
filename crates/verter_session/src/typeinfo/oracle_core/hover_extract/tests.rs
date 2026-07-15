//! Discriminating, offline (no-tsgo) self-tests for the hover-extraction grammar.

use super::*;

/// The EXACT hover the pinned tsgo 7.0.0-dev.20260526.1 returns for the design's
/// canonical example (verified empirically). The nested member `;`s inside the
/// object body must NOT terminate the RHS.
#[test]
fn extracts_canonical_object_body_with_nested_semicolons() {
    let hover = "```typescript\ntype __oracle_probe__0 = {\n    id: number;\n    label: string;\n    tag?: \"a\" | \"b\";\n}\n```\n";
    let rhs = extract_probe_rhs(hover, "__oracle_probe__0").unwrap();
    assert_eq!(
        rhs,
        "{\n    id: number;\n    label: string;\n    tag?: \"a\" | \"b\";\n}"
    );
    // The member `;`s are inside `{}` — the whole balanced body is captured.
    assert!(rhs.contains("label: string;"));
}

/// The EXACT shape the adopted LSP driver's hover (empty client caps,
/// `tsgo/ipc.rs` `capabilities: {}`) delivers — the BARE
/// `type <probe> = <body>` with NO markdown fence. The whole-text plaintext shape
/// must extract it.
#[test]
fn extracts_bare_unfenced_hover_from_plaintext_caps_driver() {
    let hover = "type __oracle_probe__0 = {\n    id: number;\n    label: string;\n    tag?: \"a\" | \"b\";\n}";
    let rhs = extract_probe_rhs(hover, "__oracle_probe__0").unwrap();
    assert_eq!(
        rhs,
        "{\n    id: number;\n    label: string;\n    tag?: \"a\" | \"b\";\n}"
    );
}

#[test]
fn fallback_does_not_fire_when_any_fence_present() {
    // A non-ts fenced block IS present, so the prose `type ...` is NOT picked up
    // by the whole-text fallback (the fallback fires only with NO fence at all).
    let hover = "type __oracle_probe__0 = never (in prose)\n\n```json\n{}\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn extracts_bare_alias_name_shallow() {
    // Shallow mode: tsgo prints the alias name (no trailing `;` in hover).
    let hover = "```typescript\ntype __oracle_probe__0 = ComposedProps\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap(),
        "ComposedProps"
    );
}

#[test]
fn depth_zero_semicolon_terminates() {
    // A trailing `;` after a simple RHS is allowed; the RHS excludes it.
    let hover = "```typescript\ntype __oracle_probe__1 = string | number;\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__1").unwrap(),
        "string | number"
    );
}

#[test]
fn semicolon_inside_string_literal_does_not_terminate() {
    let hover = "```typescript\ntype __oracle_probe__0 = \"a;b\" | \"c\"\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap(),
        "\"a;b\" | \"c\""
    );
}

#[test]
fn picks_the_block_naming_the_probe_ignoring_prose_and_other_blocks() {
    // A leading prose line + a NON-probe code block, then the probe block. Each
    // fenced body is itself an exact alias; the wrong-name block is skipped and
    // the probe block extracts.
    let hover = "Some documentation about the symbol.\n\n```typescript\ntype Unrelated = number\n```\n\n```typescript\ntype __oracle_probe__2 = boolean\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__2").unwrap(),
        "boolean"
    );
}

#[test]
fn ignores_inline_and_non_typescript_fences() {
    // An inline `code` span and a non-ts fenced block must be ignored.
    let hover = "Inline `type __oracle_probe__0 = never` prose.\n\n```json\n{\"type\": \"__oracle_probe__0\"}\n```";
    // No FENCED typescript block names the probe → NoProbeBlock.
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn boundary_match_does_not_confuse_a_longer_name() {
    // `__oracle_probe__1` must NOT match when only `__oracle_probe__10` is present.
    let hover = "```typescript\ntype __oracle_probe__10 = number\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__1").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
    // and the exact name extracts.
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__10").unwrap(),
        "number"
    );
}

#[test]
fn unclosed_fence_fails() {
    let hover = "```typescript\ntype __oracle_probe__0 = {\n    id: number;";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::UnclosedFence
    );
}

#[test]
fn no_probe_block_when_header_absent() {
    let hover = "```typescript\nconst x: number\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn arrow_function_type_is_captured_whole() {
    // A function-type RHS captures the whole function type (the OXC parse balances
    // the `=>` and parameter list).
    let hover = "```typescript\ntype __oracle_probe__0 = (p0: number) => string\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap(),
        "(p0: number) => string"
    );
}

// ===========================================================================
// Negatives REJECTED by the strict whole-hover probe-alias grammar (§Q2
// "hover-extraction grammar"). MOST are discriminating against the prior loose
// `type <probe> = …` substring scan: that scan would have WRONGLY ACCEPTED them
// (header-in-prose, `export`/`declare` modifiers, type parameters, trailing
// declarations, …), so reverting the strict parse back to the substring scan
// makes each of those FAIL. The exception is `plaintext_wrong_probe_name_is_rejected`:
// the old scan already keyed on the exact probe-name substring, so it would have
// rejected a wrong name too — that case is useful correctness coverage, NOT a
// proof of discrimination against the old scan.
// ===========================================================================

#[test]
fn plaintext_header_embedded_in_prose_is_rejected() {
    // A bare probe header EMBEDDED in surrounding prose (no fence) is NOT an exact
    // whole-hover alias — the loose scan would have found the header and returned
    // `{ id: number }`.
    let hover =
        "The resolved type of the symbol is type __oracle_probe__0 = { id: number } as printed.";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn plaintext_export_modifier_is_rejected() {
    // `export type …` is an `ExportNamedDeclaration`, not a bare alias → REJECT.
    let hover = "export type __oracle_probe__0 = { id: number }";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn plaintext_declare_modifier_is_rejected() {
    // `declare type …` carries the `declare` modifier → REJECT.
    let hover = "declare type __oracle_probe__0 = { id: number }";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn plaintext_generic_alias_header_is_rejected() {
    // A parameterized alias header `type P<T> = …` is out of grammar (exact,
    // non-generic alias only) → REJECT.
    let hover = "type __oracle_probe__0<T> = { value: T }";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn plaintext_trailing_extra_declaration_is_rejected() {
    // A trailing extra declaration after the alias → REJECT (the whole hover must
    // be EXACTLY one alias).
    let hover = "type __oracle_probe__0 = { id: number }\ntype Other = string";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn plaintext_trailing_non_comment_text_is_rejected() {
    // Trailing non-comment prose after the alias → REJECT.
    let hover = "type __oracle_probe__0 = number\nthis is some trailing prose";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn plaintext_wrong_probe_name_is_rejected() {
    // A clean alias declaring the WRONG name → REJECT.
    let hover = "type __oracle_probe__7 = number";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn plaintext_truncated_unbalanced_is_rejected() {
    // A truncated / unbalanced plaintext alias (no closing brace) does not parse
    // to a clean single alias → REJECT.
    let hover = "type __oracle_probe__0 = { id: number; label: string";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn plaintext_leading_comment_is_tolerated() {
    // A leading line/block comment / JSDoc before the alias is allowed; the alias
    // still extracts.
    let hover = "/** The symbol's resolved type. */\ntype __oracle_probe__0 = { id: number }";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap(),
        "{ id: number }"
    );
}

#[test]
fn fenced_block_with_surrounding_code_is_rejected() {
    // The SAME exact-alias standard applies to a fenced TS block: arbitrary
    // surrounding code inside the fence (an extra declaration) → REJECT. The loose
    // scan would have returned `{ id: number }` from the embedded header.
    let hover = "```typescript\nconst sentinel = 1;\ntype __oracle_probe__0 = { id: number }\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}

#[test]
fn fenced_export_modifier_is_rejected() {
    // An `export` modifier inside the fenced block is rejected (same standard).
    let hover = "```typescript\nexport type __oracle_probe__0 = { id: number }\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap_err(),
        HoverExtractError::NoProbeBlock
    );
}
