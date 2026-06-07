//! Discriminating, offline (no-tsgo) self-tests for the hover-extraction grammar.

use super::*;

/// The EXACT hover the pinned tsgo 7.0.0-dev.20260526.1 returns for the design's
/// canonical example (verified empirically — see the §4 spike). The nested
/// member `;`s inside the object body must NOT terminate the RHS.
#[test]
fn extracts_canonical_object_body_with_nested_semicolons() {
    let hover = "```typescript\ntype __oracle_probe__0 = {\n    id: number;\n    label: string;\n    tag?: \"a\" | \"b\";\n}\n```\n";
    let rhs = extract_probe_rhs(hover, "__oracle_probe__0").unwrap();
    assert_eq!(
        rhs,
        "{\n    id: number;\n    label: string;\n    tag?: \"a\" | \"b\";\n}"
    );
    // The member `;`s are inside `{}` (depth 1) — only a depth-0 `;` (or
    // end-of-block) terminates, so the whole balanced body is captured.
    assert!(rhs.contains("label: string;"));
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
    // A trailing `;` after a simple RHS terminates; trailing prose is dropped.
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
    // A leading prose line + a NON-probe code block, then the probe block.
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
fn arrow_function_type_does_not_underflow_or_misterminate() {
    // `=>` carries a `>` with no matching `<`; depth must not underflow and the
    // RHS must capture the whole function type.
    let hover = "```typescript\ntype __oracle_probe__0 = (p0: number) => string\n```";
    assert_eq!(
        extract_probe_rhs(hover, "__oracle_probe__0").unwrap(),
        "(p0: number) => string"
    );
}
