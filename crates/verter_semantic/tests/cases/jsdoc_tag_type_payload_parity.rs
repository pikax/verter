//! Parity test: `lower_ts_type` (direct OXC AST visit) vs.
//! `parse_jsdoc_tag_type_payload` (JSDoc-private wrap-and-lower helper).
//!
//! Both code paths must produce equivalent `TypeExpr` for every fixture
//! in the curated corpus. The helper lives in
//! `verter_semantic::analysis::jsdoc` (post-W5.2 rename); it is the
//! single permitted text-input boundary for the typed-IR resolver
//! pipeline. Every other producer-side caller lowers from a
//! `TSType<'_>` AST node via `verter_type_expr_oxc::lower_ts_type`
//! directly.
//!
//! Path A: parse `type __T = INPUT;` via OXC, walk to the
//! `TSTypeAliasDeclaration` named `__T`, read its `.type_annotation`,
//! and call `lower_ts_type(&type_annotation, &wrapped_source)`.
//!
//! Path B: call `parse_jsdoc_tag_type_payload(INPUT, None)` — the
//! JSDoc-private wrap-and-lower helper exposed by `verter_semantic`.
//!
//! Both go through the canonical OXC parser; neither bypasses the
//! lowering routines. A divergence indicates a real bug in the
//! lowering layer or in the helper, not a test artefact — the corpus
//! is the contract.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload;
use verter_type_expr::TypeExpr;
use verter_type_expr_oxc::lower_ts_type;

/// Curated corpus exercising every `TSType` variant the resolver hits.
///
/// Fixture-only by W0.7 design — there is no OXC `TSType` generator in
/// the workspace and writing one is out of scope. Each fixture covers a
/// distinct shape; the categories below total exactly **76** entries.
const PARITY_CORPUS: &[&str] = &[
    // ---- Primitives (12) ----
    "string",
    "number",
    "boolean",
    "symbol",
    "bigint",
    "any",
    "unknown",
    "void",
    "never",
    "null",
    "undefined",
    "object",
    // ---- Literal types (8) ----
    // Exercises the negative-number / bigint / boolean paths the hand
    // parser historically broke on.
    r#""hello""#,
    "42",
    "-1",
    "2.5",
    "true",
    "false",
    "0n",
    "100n",
    // ---- Refs (6) ----
    "MyType",
    "Promise<string>",
    "Map<string, number>",
    "Foo.Bar.Baz",
    "Array<string>",
    "ReadonlyArray<number>",
    // ---- Utility types (9) ----
    "Partial<MyType>",
    "Required<MyType>",
    "Pick<User, 'id'>",
    "Pick<User, 'id' | 'name'>",
    "Omit<User, 'password'>",
    "Record<'a' | 'b', number>",
    "Awaited<Promise<string>>",
    "Extract<typeof y, R>",
    "ReturnType<typeof createConfig>",
    // ---- Operators (11) ----
    "keyof T",
    "keyof { a: string; b: number }",
    "typeof myVar",
    "typeof module.exports",
    "T['key']",
    "T[number]",
    "T extends string ? true : false",
    "T extends Array<infer U> ? U : never",
    "{ [K in keyof T]: T[K] }",
    "{ [K in keyof T]?: T[K] }",
    "{ -readonly [K in keyof T]: T[K] }",
    // ---- Composites (25) ----
    "string | number",
    "string | number | boolean",
    "string | null",
    r#""red" | "blue" | "green""#,
    "A & B",
    "Base & { extra: boolean }",
    "string[]",
    "string[][]",
    "[string, number]",
    "[string, number?]",
    "[string, ...number[]]",
    "[name: string, age: number]",
    "readonly [string, number]",
    "{ name: string; age: number }",
    "{ name?: string }",
    "{ readonly id: number }",
    "{ [key: string]: number }",
    "{ greet(name: string): void }",
    "{ (x: number): string }",
    "(x: string) => number",
    "() => void",
    "(x?: string) => void",
    "(...args: string[]) => void",
    "<T>(x: T) => T",
    "<T extends Base = string>(value: T) => T",
    // ---- Template literals (2) ----
    "`btn-${string}`",
    "`${number}px`",
    // ---- Parenthesized (1) ----
    "(string | number)",
    // ---- Compound (1) — discriminated union ----
    "{ type: 'a'; value: string } | { type: 'b'; count: number }",
    // ---- Vue-specific patterns (1) ----
    // `PropType<T>` inside `as PropType<T>` — at the parity-test layer
    // both paths produce the same shallow `Ref { name: "PropType" }`
    // tree. The alias-Pick discriminator that previously appeared here
    // is moved to W1.1's projector-level fixture set.
    "PropType<{ a: string; b: number }>",
];

/// Path A — parse `type __T = INPUT;` via OXC, find the alias's
/// `type_annotation`, lower via `lower_ts_type`.
///
/// The allocator is dropped at the end of the call, but `TypeExpr` is
/// fully owned (no borrows into the arena) so the lowered value
/// outlives the parser.
fn lower_via_ast(input: &str) -> TypeExpr {
    let wrapper = format!("type __T = {input};");
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, &wrapper, source_type).parse();

    for stmt in &ret.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            if alias.id.name == "__T" {
                return lower_ts_type(&alias.type_annotation, &wrapper);
            }
        }
    }

    panic!(
        "Path A: failed to find `type __T = ...;` in parsed wrapper for fixture {input:?}; \
         parser errors: {:?}",
        ret.errors,
    );
}

/// Path B — call the JSDoc-private wrap-and-lower helper.
fn lower_via_jsdoc_helper(input: &str) -> TypeExpr {
    parse_jsdoc_tag_type_payload(input, None)
}

#[test]
fn lower_ts_type_and_parse_jsdoc_tag_type_payload_agree_on_corpus() {
    // Sanity: cross-check the corpus length against the §6 W0.7
    // categorical breakdown so a mis-edit can't silently shrink the
    // gate. 12 + 8 + 6 + 9 + 11 + 25 + 2 + 1 + 1 + 1 = 76.
    assert_eq!(
        PARITY_CORPUS.len(),
        76,
        "PARITY_CORPUS must contain exactly 76 fixtures (per W0.7 §6); got {}",
        PARITY_CORPUS.len(),
    );

    let mut divergences: Vec<(String, String, String)> = Vec::new();

    for fixture in PARITY_CORPUS {
        // This test characterizes STRUCTURAL parity between the two lowering
        // entry points (the W0.7 intent: `lower_ts_type` and
        // `parse_jsdoc_tag_type_payload` agree on the lowered TYPE). The two
        // paths legitimately differ on span COORDINATES — path A keeps its
        // private `type __T = …` wrapper's offsets, while the JSDoc helper, when
        // called without a source position (`None`), CLEARS spans as honest
        // absence. Normalise spans on both sides so the comparison is over the
        // type tree, not buffer-relative provenance.
        let mut a = lower_via_ast(fixture);
        let mut b = lower_via_jsdoc_helper(fixture);
        a.clear_spans();
        b.clear_spans();
        if a != b {
            divergences.push(((*fixture).to_string(), format!("{a:#?}"), format!("{b:#?}")));
        }
    }

    assert!(
        divergences.is_empty(),
        "parity divergences across {} fixtures (out of {}):\n{:#?}",
        divergences.len(),
        PARITY_CORPUS.len(),
        divergences,
    );
}
