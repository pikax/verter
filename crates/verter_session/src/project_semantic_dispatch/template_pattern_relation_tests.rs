//! The checker's template-literal relation: a string literal or template
//! literal type below a template literal pattern fits when each hole's
//! slice — the target texts matched leftmost
//! (`inferFromLiteralPartsToTemplateLiteral`) — fits its hole
//! (`isValidTypeForTemplateLiteralPlaceholder`): any string for `string`, a
//! finite JavaScript number spelling for `number`, one bigint token for
//! `bigint`, the unchanged text for a string mapping; a template source's
//! lone-hole slice relates by its hole. `string` is below no pattern.
//!
//! Every expected answer is TypeScript 7.0.2's, read off the TS2322 message
//! of `declare const p: [<probes>]; export const s: null = p;` under each
//! `strictNullChecks` × `noImplicitAny` setting; the four settings agree.

use super::checker_probe_lane_tests::mismatches;

/// Measured on TypeScript 7.0.2: every row's `… ? 1 : 2` is the recorded
/// answer.
#[test]
fn a_template_pattern_accepts_the_slices_its_holes_admit() {
    let failures = mismatches(
        "",
        &[
            ("\"x1\" extends `x${string}` ? 1 : 2", "1"),
            ("\"x\" extends `x${string}` ? 1 : 2", "1"),
            ("\"y1\" extends `x${string}` ? 1 : 2", "2"),
            ("\"12\" extends `${number}` ? 1 : 2", "1"),
            ("\"1e3\" extends `${number}` ? 1 : 2", "1"),
            ("\" 1\" extends `${number}` ? 1 : 2", "1"),
            ("\"1a\" extends `${number}` ? 1 : 2", "2"),
            ("\"10\" extends `${bigint}` ? 1 : 2", "1"),
            ("\"10n\" extends `${bigint}` ? 1 : 2", "2"),
            ("string extends `x${string}` ? 1 : 2", "2"),
            ("`xy${string}` extends `x${string}` ? 1 : 2", "1"),
            ("`x${string}` extends `xy${string}` ? 1 : 2", "2"),
            ("`x${number}` extends `x${string}` ? 1 : 2", "1"),
            ("`x${string}` extends `x${number}` ? 1 : 2", "2"),
            ("`a${string}b` extends `a${string}` ? 1 : 2", "1"),
            ("`a${string}` extends `${string}a` ? 1 : 2", "2"),
            ("\"a-b\" extends `${string}-${string}` ? 1 : 2", "1"),
            ("\"ab\" extends `${string}-${string}` ? 1 : 2", "2"),
            ("\"x1\" extends `x${number}` ? 1 : 2", "1"),
            ("\"xtrue\" extends `x${boolean}` ? 1 : 2", "1"),
            ("\"xnull\" extends `x${null}` ? 1 : 2", "1"),
            ("\"xAB\" extends `x${Uppercase<string>}` ? 1 : 2", "1"),
            ("\"xAb\" extends `x${Uppercase<string>}` ? 1 : 2", "2"),
            ("\"a1b2\" extends `a${number}b${number}` ? 1 : 2", "1"),
            ("\"a1bb2\" extends `a${number}b${number}` ? 1 : 2", "2"),
            ("\"abc\" extends `${string}${string}` ? 1 : 2", "1"),
            ("\"12\" extends `${number}${number}` ? 1 : 2", "1"),
            ("\"1\" extends `${number}${number}` ? 1 : 2", "2"),
            ("`${number}` extends `${bigint}` ? 1 : 2", "2"),
            ("`${bigint}` extends `${number}` ? 1 : 2", "2"),
            ("`a${number}` extends `a${number}${string}` ? 1 : 2", "1"),
            ("`a-${number}` extends `a${string}-${number}` ? 1 : 2", "1"),
            ("`${number}px` extends `${number}${string}` ? 1 : 2", "1"),
            ("[\"x1\" | \"x2\"] extends [`x${string}`] ? 1 : 2", "1"),
            ("`x${string}` extends `x${string}` | number ? 1 : 2", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `keyof` over a template-pattern index signature holds the pattern: it
/// and the union of its key types are each below the other.
///
/// Measured on TypeScript 7.0.2 over `type T5 = { [k: `x${string}`]: 1;
/// [k: number]: 2 }`: `[keyof T5] extends [number | `x${string}`] ? 1 : 2`
/// and the reverse are `1`.
#[test]
fn a_keyof_over_a_pattern_index_signature_holds_the_pattern() {
    let failures = mismatches(
        "type T5 = { [k: `x${string}`]: 1; [k: number]: 2 };\n",
        &[
            ("[keyof T5] extends [number | `x${string}`] ? 1 : 2", "1"),
            ("[number | `x${string}`] extends [keyof T5] ? 1 : 2", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A string mapping over `string` relates by the mapping as a conditional's
/// operand: a literal is below `Uppercase<string>` when upper-casing leaves
/// it unchanged, and the mapping is below `string`.
///
/// Measured on TypeScript 7.0.2: `"AB" extends Uppercase<string> ? 1 : 2`
/// is `1`, `"Ab" extends Uppercase<string> ? 1 : 2` is `2`, and
/// `` `${Uppercase<string>}` extends string ? 1 : 2 `` is `1`.
#[test]
#[ignore = "a string mapping over string relates by the mapping as a conditional operand"]
fn a_string_mapping_over_string_relates_by_the_mapping() {
    let failures = mismatches(
        "",
        &[
            ("\"AB\" extends Uppercase<string> ? 1 : 2", "1"),
            ("\"Ab\" extends Uppercase<string> ? 1 : 2", "2"),
            ("`${Uppercase<string>}` extends string ? 1 : 2", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
