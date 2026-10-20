//! A conditional type's `infer` declaration read outside the scope its
//! `extends` clause gives it — the false branch, or another declaration's
//! constraint in the same pattern — is the checker's TS2304, whose error
//! type reads as `any`; a name an outer declaration binds reads that
//! declaration.
//!
//! Every expected answer is TypeScript 7.0.2's, the same in all four
//! `strictNullChecks` × `noImplicitAny` settings: TS2304 for `U` in `I`, `W`
//! and `P` and for `A` in `C`, none for `O`; `I<2>` prints as `U`, the
//! error type.

use super::checker_probe_lane_tests::with_probe;
use super::differential_harness_tests::{Matrix, Read};
use verter_type_engine::semantic_query::{
    CheckerDiagnosticCode, ProjectionMode, ProjectionReductionContext, QueryError, SemanticNodeData,
};

const SOURCE: &str = r##"
type I<T> = T extends [infer U] ? 1 : U;
type W<T> = T extends [infer U] ? U : [U];
type C<T> = T extends [infer A, infer B extends A] ? B : 0;
type O<T, U> = T extends [infer U] ? 1 : U;
type P<T> = T extends [infer U] ? 1 : U<string>;
"##;

/// The checker diagnostics of the recoveries `probe` evaluates to.
fn recovery_codes(probe: &str) -> Vec<CheckerDiagnosticCode> {
    with_probe(SOURCE, probe, |dispatch, node| {
        let value = dispatch
            .normalize_node_for_structural_fact_demand(
                node,
                ProjectionReductionContext::published(ProjectionMode::Expanded),
            )
            .into_complete_node()
            .expect("the probe evaluates completely");
        let mut codes = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut stack = vec![value];
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            let Some(data) = dispatch.graph().node_data(node) else {
                continue;
            };
            if let SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. }) =
                data.as_ref()
            {
                codes.push(diagnostic.code);
            }
            data.for_each_retained_child(|child| stack.push(child));
        }
        codes
    })
}

/// An `infer` name the false branch reads, with nothing else binding it, is
/// TS2304 — whole (`I<2>`), nested (`W<3>` is `[U]`) and applied (`P<2>`
/// reads `U<string>`) — while the true
/// branch, an outer type parameter of the same name, and the pattern's own
/// matches read their declarations.
#[test]
fn an_infer_declaration_read_outside_its_scope_is_ts2304() {
    assert_eq!(
        recovery_codes("I<2>"),
        vec![CheckerDiagnosticCode::CannotFindName]
    );
    assert_eq!(
        recovery_codes("W<3>"),
        vec![CheckerDiagnosticCode::CannotFindName]
    );
    assert_eq!(
        recovery_codes("P<2>"),
        vec![CheckerDiagnosticCode::CannotFindName]
    );
    assert_eq!(recovery_codes(r#"O<3, "outer">"#), Vec::new());
    let failures = Matrix::new(SOURCE).same(&[
        (Read::Type(r#"I<[2]>"#), r#"1"#),
        (Read::Type(r#"W<[2]>"#), r#"2"#),
        (Read::Type(r#"C<[1, 2]>"#), r#"2"#),
        (Read::Type(r#"O<3, "outer">"#), r#""outer""#),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
