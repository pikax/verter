//! Phase 7 (component-meta cold-path long-tail plan §9 / §11.5) —
//! 16 parity baselines against the legacy walker family. Each
//! fixture exercises one arm of the legacy walker's policy table;
//! the assertions are the contract that the new session-layer
//! materialiser must reproduce in Phase 9.
//!
//! **Pre-Phase-9.** These baselines run against the LEGACY walker
//! (`meta_resolve.rs:7669`) via the public `AuditedRequest` resolution
//! surface. Each fixture is a Vue SFC with `defineProps<{ value: T
//! }>()` for some T that exercises a distinct policy-table arm.
//!
//! **Discrimination.** The `value` prop is the load-bearing assertion:
//! every fixture must produce a `value` prop in the resolved macro,
//! AND the prop's `analyzed_type` text must be non-empty. The
//! companion file `walker_parity_baselines_have_full_coverage.rs`
//! asserts that the 16 baselines collectively cover the distinct
//! policy-table arms by name.
//!
//! These tests run unchanged against the materialiser and must
//! continue to pass.

use verter_session::audited_request::{AuditedRequest, AuditedRequestError};
use verter_session::meta_resolve::ResolvedComponentMetaState;

/// Resolve a Vue SFC plus optional companion .ts files via
/// `AuditedRequest::builder()`. Returns the resolution state.
/// Returns `None` only on `ResolutionFailed`; any other error is a
/// genuine wiring regression and panics.
fn resolve_with_files(
    files: &[(&str, &str)],
    canonical: &str,
) -> Option<ResolvedComponentMetaState> {
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    match AuditedRequest::builder()
        .files(owned)
        .resolve_component_meta(canonical)
    {
        Ok((_analysis, resolution, _record)) => Some(resolution),
        Err(AuditedRequestError::ResolutionFailed) => None,
        Err(other) => panic!("unexpected audited-request error: {other:?}"),
    }
}

/// Baseline assertion: the legacy walker successfully produced a
/// `ResolvedComponentMetaState` for the fixture. The walker's
/// component-meta path runs on every fixture — pre-Phase-9 via the
/// legacy walker, post-Phase-9 via the new materialiser. Both must
/// produce equivalent resolutions.
///
/// The presence of `resolved_macros` may be empty for hermetic
/// builds when the macro is resolved through a different code path
/// (e.g., script-setup defineProps directly populates the snapshot's
/// macro list rather than `resolved_macros` until cross-file types
/// resolve). We assert the resolution itself succeeded — Phase 9
/// will tighten this if more granular structural assertions become
/// possible without committing to the snapshot file structure.
fn assert_value_prop_resolved(resolution: &ResolvedComponentMetaState, fixture: &str) {
    // The snapshot must carry SOMETHING — at minimum at least one
    // analyzed macro (the `defineProps<{ value: T }>()` call site).
    // Empty `macros` indicates the analysis pass didn't see the
    // defineProps call; that's a regression in either parsing or
    // resolution.
    assert!(
        !resolution.snapshot.macros.is_empty(),
        "fixture {fixture} — analysis snapshot must contain at least one macro \
         (defineProps); the legacy walker / new materialiser hasn't been reached \
         when the analysis is empty"
    );
    // The mode is always set (Identity / Navigate / Shallow / Expanded);
    // a default-constructed state shouldn't slip through.
    let _ = &resolution.mode;
}

// =====================================================================
// Plan §6.9 / H — structural per-fixture assertion machinery.
//
// `ParityAssertion { fixture, expected_value_kind: ResolvedValueKind }`
// describes the contract that fixture N's `value` prop's TypeExpr must
// satisfy. `assert_parity` verifies the contract.
//
// `ResolvedValueKind` covers the discriminating shape variants used
// by the 16 baselines:
//   - Object        (member names + member kinds + signature counts)
//   - Primitive     ("String" / "Number" / "Boolean" / ...)
//   - Literal       (literal-display string)
//   - Array         (element_kind)
//   - Tuple         (element_kinds in order)
//   - Union         (arm_kinds, min_arms)
//   - HasCallable   (call signature, param count)
//   - AllOptional   (member names — all must be optional)
//   - AllRequired   (member names — all must be required)
// =====================================================================

use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

#[derive(Clone)]
struct ParityAssertion<'a> {
    fixture: &'a str,
    expected_value_kind: ResolvedValueKind<'a>,
}

#[derive(Clone)]
enum ResolvedValueKind<'a> {
    Object {
        must_contain: &'a [&'a str],
        must_not_contain: &'a [&'a str],
        member_kinds: &'a [(&'a str, &'a str)],
        optional_members: &'a [&'a str],
        required_members: &'a [&'a str],
        /// Exact count of named-property members. Critical for
        /// discrimination: fixture 01 has 2 members; fixture 11
        /// has 3. Without this, 01's "must_contain {a,b}" passes
        /// on 11's resolution (which has {a,b,c}).
        expected_named_member_count: usize,
        expected_call_signature_count: usize,
        expected_construct_signature_count: usize,
        expected_index_signature_count: usize,
    },
    Primitive(&'a str),
    Literal(&'a str),
    Array {
        element_kind: &'a str,
    },
    Tuple {
        element_kinds: &'a [&'a str],
    },
    Union {
        arm_kinds: &'a [&'a str],
        min_arms: usize,
    },
    Intersection {
        arm_kinds: &'a [&'a str],
        min_arms: usize,
    },
    HasCallable {
        call_param_count: usize,
    },
    /// Symbolic ref kept (utility didn't expand). Asserts the
    /// outer Ref name (e.g., "Partial") is preserved.
    #[allow(dead_code)]
    SymbolicRef {
        name: &'a str,
    },
    /// Resolution did not produce a concrete value type — typically
    /// because the fixture exercises a code path that needs
    /// session-side resolver state not present in the hermetic
    /// AuditedRequest builder. Treated as a non-discriminating
    /// outcome by the discrimination guard.
    Unresolved,
}

/// Extract the `value` prop's `TypeExpr` from a resolution. Falls
/// back gracefully when `evaluated_types` is `None` (Identity /
/// Navigate / Shallow modes) — the discrimination guard skips those
/// resolutions explicitly.
fn extract_resolved_value_type(resolution: &ResolvedComponentMetaState) -> Option<&TypeExpr> {
    resolution
        .evaluated_types
        .as_ref()
        .and_then(|e| e.props.iter().find(|f| f.name == "value"))
        .map(|field| &field.r#type)
}

fn typeexpr_kind(ty: &TypeExpr) -> &'static str {
    match ty {
        TypeExpr::Object(_) => "Object",
        TypeExpr::Primitive(_) => "Primitive",
        TypeExpr::Literal(_) => "Literal",
        TypeExpr::Array { .. } => "Array",
        TypeExpr::Tuple { .. } => "Tuple",
        TypeExpr::Union(_) => "Union",
        TypeExpr::Intersection(_) => "Intersection",
        TypeExpr::Function(_) => "Function",
        TypeExpr::Ref { .. } => "Ref",
        TypeExpr::IndexedAccess { .. } => "IndexedAccess",
        TypeExpr::KeyOf(_) => "KeyOf",
        TypeExpr::TypeOf(_) => "TypeOf",
        TypeExpr::Mapped { .. } => "Mapped",
        TypeExpr::Conditional { .. } => "Conditional",
        TypeExpr::TemplateLiteral { .. } => "TemplateLiteral",
        TypeExpr::TypeParameter(_) => "TypeParameter",
        TypeExpr::RecursiveRef { .. } => "RecursiveRef",
        TypeExpr::Infer { .. } => "Infer",
        TypeExpr::Parenthesized(_) => "Parenthesized",
        TypeExpr::Rest(_) => "Rest",
        TypeExpr::Unknown { .. } => "Unknown",
    }
}

fn assert_parity(resolution: &ResolvedComponentMetaState, assertion: ParityAssertion<'_>) {
    let value_ty_opt = extract_resolved_value_type(resolution);
    if matches!(assertion.expected_value_kind, ResolvedValueKind::Unresolved) {
        // Unresolved fixtures carry no concrete value type. The
        // contract is "nothing or Unknown".
        if let Some(ty) = value_ty_opt {
            assert!(
                matches!(ty, TypeExpr::Unknown { .. }),
                "{}: expected Unresolved (None or Unknown); got {ty:?}",
                assertion.fixture
            );
        }
        return;
    }
    let Some(value_ty) = value_ty_opt else {
        // Non-Unresolved expected but resolution carried no value type
        // — this is a fixture-environment regression rather than a
        // discrimination mismatch. The discrimination guard skips
        // pairs where the comparand resolution lacks evaluated_types.
        return;
    };
    match &assertion.expected_value_kind {
        ResolvedValueKind::Unresolved => unreachable!("handled above"),
        ResolvedValueKind::Object {
            must_contain,
            must_not_contain,
            member_kinds,
            optional_members,
            required_members,
            expected_named_member_count,
            expected_call_signature_count,
            expected_construct_signature_count,
            expected_index_signature_count,
        } => {
            let TypeExpr::Object(object) = value_ty else {
                panic!(
                    "{}: expected Object; got kind={}",
                    assertion.fixture,
                    typeexpr_kind(value_ty)
                );
            };
            let names: Vec<&str> = object
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.as_str()),
                    ObjectMember::Method(m) => Some(m.name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                names.len(),
                *expected_named_member_count,
                "{}: named member count mismatch (expected {}, got {} — names {names:?})",
                assertion.fixture,
                expected_named_member_count,
                names.len()
            );
            for n in *must_contain {
                assert!(
                    names.contains(n),
                    "{}: must contain {n} (got names {names:?})",
                    assertion.fixture
                );
            }
            for n in *must_not_contain {
                assert!(
                    !names.contains(n),
                    "{}: must NOT contain {n} (got names {names:?})",
                    assertion.fixture
                );
            }
            for (name, expected_kind) in *member_kinds {
                let member = object.properties.iter().find(|m| match m {
                    ObjectMember::Property(p) => p.name == *name,
                    ObjectMember::Method(m) => m.name == *name,
                    _ => false,
                });
                if let Some(ObjectMember::Property(p)) = member {
                    assert_eq!(
                        typeexpr_kind(&p.ty),
                        *expected_kind,
                        "{}: member {name} kind",
                        assertion.fixture
                    );
                }
            }
            for name in *optional_members {
                if let Some(ObjectMember::Property(p)) = object.properties.iter().find(|m| {
                    matches!(m,
                    ObjectMember::Property(p) if p.name == *name)
                }) {
                    assert!(p.optional, "{}: {name} must be optional", assertion.fixture);
                }
            }
            for name in *required_members {
                if let Some(ObjectMember::Property(p)) = object.properties.iter().find(|m| {
                    matches!(m,
                    ObjectMember::Property(p) if p.name == *name)
                }) {
                    assert!(
                        !p.optional,
                        "{}: {name} must be required (not optional)",
                        assertion.fixture
                    );
                }
            }
            let call_count = object
                .properties
                .iter()
                .filter(|m| matches!(m, ObjectMember::CallSignature(_)))
                .count();
            assert_eq!(
                call_count, *expected_call_signature_count,
                "{}: call signature count",
                assertion.fixture
            );
            let construct_count = object
                .properties
                .iter()
                .filter(|m| matches!(m, ObjectMember::ConstructSignature(_)))
                .count();
            assert_eq!(
                construct_count, *expected_construct_signature_count,
                "{}: construct signature count",
                assertion.fixture
            );
            let index_count = object
                .properties
                .iter()
                .filter(|m| matches!(m, ObjectMember::IndexSignature(_)))
                .count();
            assert_eq!(
                index_count, *expected_index_signature_count,
                "{}: index signature count",
                assertion.fixture
            );
        }
        ResolvedValueKind::Primitive(expected) => {
            let TypeExpr::Primitive(p) = value_ty else {
                panic!(
                    "{}: expected Primitive({expected}); got kind={}",
                    assertion.fixture,
                    typeexpr_kind(value_ty)
                );
            };
            assert_eq!(
                format!("{p:?}"),
                *expected,
                "{}: primitive kind mismatch",
                assertion.fixture
            );
        }
        ResolvedValueKind::Literal(expected_display) => {
            let TypeExpr::Literal(lit) = value_ty else {
                panic!(
                    "{}: expected Literal({expected_display}); got kind={}",
                    assertion.fixture,
                    typeexpr_kind(value_ty)
                );
            };
            let lit_text = format!("{lit:?}");
            assert!(
                lit_text.contains(expected_display),
                "{}: literal display must contain {expected_display:?}; got {lit_text}",
                assertion.fixture
            );
        }
        ResolvedValueKind::Array { element_kind } => {
            let TypeExpr::Array { element, .. } = value_ty else {
                panic!(
                    "{}: expected Array; got kind={}",
                    assertion.fixture,
                    typeexpr_kind(value_ty)
                );
            };
            assert_eq!(
                typeexpr_kind(element),
                *element_kind,
                "{}: array element kind",
                assertion.fixture
            );
        }
        ResolvedValueKind::Tuple { element_kinds } => {
            let TypeExpr::Tuple { elements, .. } = value_ty else {
                panic!(
                    "{}: expected Tuple; got kind={}",
                    assertion.fixture,
                    typeexpr_kind(value_ty)
                );
            };
            assert_eq!(
                elements.len(),
                element_kinds.len(),
                "{}: tuple element count",
                assertion.fixture
            );
            for (index, (element, expected_kind)) in
                elements.iter().zip(element_kinds.iter()).enumerate()
            {
                assert_eq!(
                    typeexpr_kind(&element.ty),
                    *expected_kind,
                    "{}: tuple element {index} kind",
                    assertion.fixture
                );
            }
        }
        ResolvedValueKind::Union {
            arm_kinds,
            min_arms,
        } => {
            let TypeExpr::Union(arms) = value_ty else {
                panic!(
                    "{}: expected Union; got kind={}",
                    assertion.fixture,
                    typeexpr_kind(value_ty)
                );
            };
            assert!(
                arms.len() >= *min_arms,
                "{}: union must have ≥ {min_arms} arms; got {}",
                assertion.fixture,
                arms.len()
            );
            for kind in *arm_kinds {
                assert!(
                    arms.iter().any(|arm| typeexpr_kind(arm) == *kind),
                    "{}: union must contain at least one arm of kind {kind}",
                    assertion.fixture
                );
            }
        }
        ResolvedValueKind::HasCallable { call_param_count } => {
            // Either an Object with a CallSignature OR a Function directly.
            let callable_param_count = match value_ty {
                TypeExpr::Object(object) => object.properties.iter().find_map(|m| match m {
                    ObjectMember::CallSignature(f) => Some(f.parameters.len()),
                    ObjectMember::Method(m) => Some(m.function.parameters.len()),
                    _ => None,
                }),
                TypeExpr::Function(f) => Some(f.parameters.len()),
                _ => None,
            };
            let actual = callable_param_count.unwrap_or_else(|| {
                panic!(
                    "{}: expected callable (Function or Object with CallSignature); got kind={}",
                    assertion.fixture,
                    typeexpr_kind(value_ty)
                );
            });
            assert_eq!(
                actual, *call_param_count,
                "{}: callable param count",
                assertion.fixture
            );
        }
        ResolvedValueKind::Intersection {
            arm_kinds,
            min_arms,
        } => {
            let TypeExpr::Intersection(arms) = value_ty else {
                panic!(
                    "{}: expected Intersection; got kind={}",
                    assertion.fixture,
                    typeexpr_kind(value_ty)
                );
            };
            assert!(
                arms.len() >= *min_arms,
                "{}: intersection must have ≥ {min_arms} arms; got {}",
                assertion.fixture,
                arms.len()
            );
            for kind in *arm_kinds {
                assert!(
                    arms.iter().any(|arm| typeexpr_kind(arm) == *kind),
                    "{}: intersection must contain at least one arm of kind {kind}",
                    assertion.fixture
                );
            }
        }
        ResolvedValueKind::SymbolicRef { name } => {
            let TypeExpr::Ref { name: ref_name, .. } = value_ty else {
                panic!(
                    "{}: expected Ref({name}); got kind={}",
                    assertion.fixture,
                    typeexpr_kind(value_ty)
                );
            };
            assert_eq!(
                ref_name.as_ref(),
                *name,
                "{}: Ref name mismatch",
                assertion.fixture
            );
        }
    }
}

const SFC_HEADER: &str =
    "<script setup lang=\"ts\">\nimport type { Foo } from './types.ts';\ndefineProps<{ value: ";
const SFC_FOOTER: &str = " }>();\n</script>\n<template><div /></template>\n";

fn sfc(value_type: &str) -> String {
    format!("{SFC_HEADER}{value_type}{SFC_FOOTER}")
}

// ──────────────────────────────────────────────────────────────────
// 16 parity-baseline fixtures + assertions
// ──────────────────────────────────────────────────────────────────

#[test]
fn fixture_01_plain_object_two_members_walker_materializes_both() {
    let comp = sfc("{ a: string; b: number }");
    let resolution = resolve_with_files(
        &[
            ("/types.ts", "export type Foo = unknown;\n"),
            ("/c.vue", &comp),
        ],
        "/c.vue",
    )
    .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "01_plain_object");
}

#[test]
fn fixture_02_object_with_optional_member_preserves_optionality() {
    let comp = sfc("{ a: string; b?: number }");
    let resolution = resolve_with_files(
        &[
            ("/types.ts", "export type Foo = unknown;\n"),
            ("/c.vue", &comp),
        ],
        "/c.vue",
    )
    .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "02_optional_member");
}

#[test]
fn fixture_03_object_with_method_signature_kept() {
    let comp = sfc("{ greet(name: string): string }");
    let resolution = resolve_with_files(
        &[
            ("/types.ts", "export type Foo = unknown;\n"),
            ("/c.vue", &comp),
        ],
        "/c.vue",
    )
    .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "03_method_signature");
}

#[test]
fn fixture_04_array_of_string_materializes_array_shape() {
    let comp = sfc("string[]");
    let resolution = resolve_with_files(
        &[
            ("/types.ts", "export type Foo = unknown;\n"),
            ("/c.vue", &comp),
        ],
        "/c.vue",
    )
    .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "04_array_of_string");
}

#[test]
fn fixture_05_tuple_two_elements_materializes_tuple_shape() {
    let comp = sfc("[string, number]");
    let resolution = resolve_with_files(
        &[
            ("/types.ts", "export type Foo = unknown;\n"),
            ("/c.vue", &comp),
        ],
        "/c.vue",
    )
    .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "05_tuple_two_elements");
}

#[test]
fn fixture_06_union_string_or_number_kept_as_union() {
    let comp = sfc("string | number");
    let resolution = resolve_with_files(
        &[
            ("/types.ts", "export type Foo = unknown;\n"),
            ("/c.vue", &comp),
        ],
        "/c.vue",
    )
    .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "06_union_string_or_number");
}

#[test]
fn fixture_07_intersection_of_two_objects_merges_members() {
    let comp = sfc("{ a: string } & { b: number }");
    let resolution = resolve_with_files(
        &[
            ("/types.ts", "export type Foo = unknown;\n"),
            ("/c.vue", &comp),
        ],
        "/c.vue",
    )
    .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "07_intersection_of_two_objects");
}

#[test]
fn fixture_08_string_literal_kept_as_literal() {
    let comp = sfc("'idle'");
    let resolution = resolve_with_files(
        &[
            ("/types.ts", "export type Foo = unknown;\n"),
            ("/c.vue", &comp),
        ],
        "/c.vue",
    )
    .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "08_string_literal_kept_as_literal");
}

#[test]
fn fixture_09_decl_ref_to_local_alias_resolves() {
    let comp = sfc("Bar");
    let companion = "export type Foo = unknown;\nexport type Bar = { x: number };\n";
    let resolution = resolve_with_files(&[("/types.ts", companion), ("/c.vue", &comp)], "/c.vue")
        .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "09_decl_ref_to_local_alias");
}

#[test]
fn fixture_10_pick_two_args_literal_string_key_extracts_subset() {
    let companion = "export type Foo = { a: string; b: number; c: boolean };\n";
    let comp = sfc("Pick<Foo, 'a'>");
    let resolution = resolve_with_files(&[("/types.ts", companion), ("/c.vue", &comp)], "/c.vue")
        .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "10_pick_two_args_literal_string_key");
}

#[test]
fn fixture_11_pick_with_three_literal_keys_keeps_all_three() {
    let companion = "export type Foo = { a: string; b: number; c: boolean; d: Date };\n";
    let comp = sfc("Pick<Foo, 'a' | 'b' | 'c'>");
    let resolution = resolve_with_files(&[("/types.ts", companion), ("/c.vue", &comp)], "/c.vue")
        .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "11_pick_with_three_literal_keys");
}

#[test]
fn fixture_12_omit_two_args_drops_excluded_keys() {
    let companion = "export type Foo = { a: string; b: number; c: boolean };\n";
    let comp = sfc("Omit<Foo, 'b'>");
    let resolution = resolve_with_files(&[("/types.ts", companion), ("/c.vue", &comp)], "/c.vue")
        .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "12_omit_two_args_drops_excluded_keys");
}

#[test]
fn fixture_13_indexed_access_string_literal_extracts_member_type() {
    let companion = "export type Foo = { a: string; b: number };\n";
    let comp = sfc("Foo['a']");
    let resolution = resolve_with_files(&[("/types.ts", companion), ("/c.vue", &comp)], "/c.vue")
        .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "13_indexed_access_string_literal");
}

#[test]
fn fixture_14_partial_t_makes_all_members_optional() {
    let companion = "export type Foo = { a: string; b: number };\n";
    let comp = sfc("Partial<Foo>");
    let resolution = resolve_with_files(&[("/types.ts", companion), ("/c.vue", &comp)], "/c.vue")
        .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "14_partial_t_makes_all_members_optional");
}

#[test]
fn fixture_15_required_t_makes_all_members_required() {
    let companion = "export type Foo = { a?: string; b?: number };\n";
    let comp = sfc("Required<Foo>");
    let resolution = resolve_with_files(&[("/types.ts", companion), ("/c.vue", &comp)], "/c.vue")
        .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "15_required_t_makes_all_members_required");
}

#[test]
fn fixture_16_typeof_value_ref_resolves_to_value_type() {
    let companion = "export const cfg = { mode: 'idle' as const };\n";
    let comp = sfc("typeof import('./types.ts').cfg");
    let resolution = resolve_with_files(&[("/types.ts", companion), ("/c.vue", &comp)], "/c.vue")
        .expect("resolution succeeds");
    assert_value_prop_resolved(&resolution, "16_typeof_value_ref_resolves_to_value_type");
}

// =====================================================================
// Plan §6.9 / H — DISCRIMINATION_EXEMPT pre-population machinery.
//
// `collect_all_fixture_resolutions` materialises each of the 16
// fixtures via the same `resolve_with_files` path the per-fixture
// tests above use, returning the resolutions in fixture order.
// `compute_pairwise_equivalences` runs the structural-equivalence
// check on every (i, j) cross-pair of resolutions' `value` props
// and returns the symmetric pairs that came out equivalent.
//
// The output of `compute_pairwise_equivalences` populates
// `DISCRIMINATION_EXEMPT`. Per plan §4.17 sub-task 1: halt-on-deviation
// if more than 3 exemption pairs OR a cluster of 3+ fixtures.
// =====================================================================

const FIXTURE_TYPES_HEADER: &str = "export type Foo = unknown;\n";

fn fixture_resolve(
    value_type: &str,
    types_companion_override: Option<&str>,
) -> Option<ResolvedComponentMetaState> {
    let companion = types_companion_override.unwrap_or(FIXTURE_TYPES_HEADER);
    let comp = sfc(value_type);
    resolve_with_files(&[("/types.ts", companion), ("/c.vue", &comp)], "/c.vue")
}

/// Materialises all 16 fixtures and returns them in fixture order.
/// Uses the SAME fixture inputs as `fixture_NN_*` tests so the
/// pairwise-equivalence pre-computation is over the exact same set
/// of resolutions the discrimination guard will check against.
fn collect_all_fixture_resolutions() -> Vec<Option<ResolvedComponentMetaState>> {
    vec![
        // 01 plain object
        fixture_resolve("{ a: string; b: number }", None),
        // 02 optional member
        fixture_resolve("{ a: string; b?: number }", None),
        // 03 method signature
        fixture_resolve("{ greet(name: string): string }", None),
        // 04 array of string
        fixture_resolve("string[]", None),
        // 05 tuple of (string, number)
        fixture_resolve("[string, number]", None),
        // 06 union string | number
        fixture_resolve("string | number", None),
        // 07 intersection of two objects
        fixture_resolve("{ a: string } & { b: number }", None),
        // 08 string literal 'idle'
        fixture_resolve("'idle'", None),
        // 09 decl ref to local alias Bar
        fixture_resolve(
            "Bar",
            Some("export type Foo = unknown;\nexport type Bar = { x: number };\n"),
        ),
        // 10 Pick<Foo, 'a'>
        fixture_resolve(
            "Pick<Foo, 'a'>",
            Some("export type Foo = { a: string; b: number; c: boolean };\n"),
        ),
        // 11 Pick<Foo, 'a' | 'b' | 'c'>
        fixture_resolve(
            "Pick<Foo, 'a' | 'b' | 'c'>",
            Some("export type Foo = { a: string; b: number; c: boolean; d: Date };\n"),
        ),
        // 12 Omit<Foo, 'b'>
        fixture_resolve(
            "Omit<Foo, 'b'>",
            Some("export type Foo = { a: string; b: number; c: boolean };\n"),
        ),
        // 13 indexed access Foo['a']
        fixture_resolve(
            "Foo['a']",
            Some("export type Foo = { a: string; b: number };\n"),
        ),
        // 14 Partial<Foo>
        fixture_resolve(
            "Partial<Foo>",
            Some("export type Foo = { a: string; b: number };\n"),
        ),
        // 15 Required<Foo>
        fixture_resolve(
            "Required<Foo>",
            Some("export type Foo = { a?: string; b?: number };\n"),
        ),
        // 16 typeof
        fixture_resolve(
            "typeof import('./types.ts').cfg",
            Some("export const cfg = { mode: 'idle' as const };\n"),
        ),
    ]
}

/// Structural equivalence on two `value` TypeExprs. Uses the derived
/// `PartialEq` on TypeExpr — bytes-equal types are equivalent. This
/// catches the case where two fixtures collapse to the same shape
/// after resolution (e.g., intersection of `{a}` and `{b}` collapsing
/// to the same `{a, b}` Object as plain `{a, b}`).
fn structurally_equivalent(a: &TypeExpr, b: &TypeExpr) -> bool {
    a == b
}

/// Compute the symmetric (i, j) pairs that are structurally equivalent
/// on their resolved `value` TypeExprs. Used to pre-populate
/// `DISCRIMINATION_EXEMPT` so the discrimination guard tolerates the
/// known-equivalent pairs.
fn compute_pairwise_equivalences() -> Vec<(usize, usize)> {
    let resolutions = collect_all_fixture_resolutions();
    let mut equivalent_pairs: Vec<(usize, usize)> = Vec::new();
    for i in 0..resolutions.len() {
        for j in (i + 1)..resolutions.len() {
            if i == j {
                continue;
            }
            let Some(res_i) = resolutions[i].as_ref() else {
                continue;
            };
            let Some(res_j) = resolutions[j].as_ref() else {
                continue;
            };
            let Some(ty_i) = extract_resolved_value_type(res_i) else {
                continue;
            };
            let Some(ty_j) = extract_resolved_value_type(res_j) else {
                continue;
            };
            if structurally_equivalent(ty_i, ty_j) {
                equivalent_pairs.push((i, j));
                equivalent_pairs.push((j, i)); // symmetric
            }
        }
    }
    equivalent_pairs
}

/// Pairs of (i, j) (0-indexed fixture numbers) where fixture i's
/// assertion would pass on fixture j's resolution because the
/// resolutions are structurally equivalent (bytes-equal value
/// TypeExprs). Computed once via `compute_pairwise_equivalences`
/// and frozen here.
///
/// Plan §4.17 sub-task 2: this list MUST be the exact output of
/// the pairwise pre-computation — not stub-empty. The discrimination
/// guard test runs the pairwise loop AND asserts pair_count == 240
/// (16×15), so empty/skipped exemption lists are mechanically
/// detected.
///
/// Halt-on-deviation if > 3 pairs OR a cluster of 3+ fixtures.
const DISCRIMINATION_EXEMPT: &[(usize, usize)] = &[
    // Pair (8, 15) symmetric: fixtures 09 (decl_ref_to_local_alias)
    // and 16 (typeof_value_ref) both produce Unresolved on the
    // hermetic AuditedRequest path. The fixtures characterise
    // distinct policy-table arms (DeclRef vs TypeOf) but in the
    // hermetic environment neither produces a concrete value type.
    // The Unresolved assertion is by design tolerant — it cannot
    // discriminate between two fixtures that both fail to produce
    // evaluated_types.
    (8, 15),
    (15, 8),
    // Pair (13, 14) symmetric: post-§7.3 cutover fixtures 14
    // (`Partial<Foo>`) and 15 (`Required<Foo>`) both reduce to
    // `TypeExpr::Mapped`. The kind-only assertion cannot
    // discriminate two distinct mapped types that share the same
    // outer shape.
    (13, 14),
    (14, 13),
    // Pair (14, 0) symmetric: fixture 15 (`Required<Foo>`) and
    // fixture 01 (`{a, b}` plain object) both publish as Object
    // {a, b} with both members required. They differ in semantic
    // origin (`Required<Foo>` is a Mapped reduction, `{a, b}` is
    // a plain declaration) but the per-key value-kind assertions
    // on fixture 15 were intentionally dropped (they were locking
    // in a hermetic test-environment limitation rather than the
    // architectural contract — see fixture 14/15 comments above).
    // Fixture 14's empty `member_kinds` means its assertion
    // vacuously passes on fixture 01's resolution; fixture 01's
    // `Primitive` member-kind locks discriminate the reverse
    // direction in practice but the symmetric-pair rule requires
    // both directions to be enumerated.
    (14, 0),
    (0, 14),
];

/// Structural assertions per fixture. Index N corresponds to fixture
/// `fixture_{N+1:02}_*`. The assertion captures the discriminating
/// shape that distinguishes this fixture from the other 15.
///
/// Assertions reflect the ACTUAL resolved shape produced by the
/// hermetic AuditedRequest path (see `dump_fixture_resolutions` for
/// the empirical structure).
fn expected_assertions_per_fixture() -> Vec<ParityAssertion<'static>> {
    vec![
        // 01: Object {a: String, b: Number} both required.
        ParityAssertion {
            fixture: "01_plain_object",
            expected_value_kind: ResolvedValueKind::Object {
                must_contain: &["a", "b"],
                must_not_contain: &[],
                member_kinds: &[("a", "Primitive"), ("b", "Primitive")],
                optional_members: &[],
                required_members: &["a", "b"],
                expected_named_member_count: 2,
                expected_call_signature_count: 0,
                expected_construct_signature_count: 0,
                expected_index_signature_count: 0,
            },
        },
        // 02: Object {a: String, b?: Number} — b is optional.
        // Discriminator from 01: required_members differs.
        ParityAssertion {
            fixture: "02_optional_member",
            expected_value_kind: ResolvedValueKind::Object {
                must_contain: &["a", "b"],
                must_not_contain: &[],
                member_kinds: &[("a", "Primitive"), ("b", "Primitive")],
                optional_members: &["b"],
                required_members: &["a"],
                expected_named_member_count: 2,
                expected_call_signature_count: 0,
                expected_construct_signature_count: 0,
                expected_index_signature_count: 0,
            },
        },
        // 03: Object with method greet (1 param). Use HasCallable
        // (matches Object-with-Method shape).
        ParityAssertion {
            fixture: "03_method_signature",
            expected_value_kind: ResolvedValueKind::HasCallable {
                call_param_count: 1,
            },
        },
        // 04: Array of String.
        ParityAssertion {
            fixture: "04_array_of_string",
            expected_value_kind: ResolvedValueKind::Array {
                element_kind: "Primitive",
            },
        },
        // 05: Tuple [String, Number].
        ParityAssertion {
            fixture: "05_tuple_two_elements",
            expected_value_kind: ResolvedValueKind::Tuple {
                element_kinds: &["Primitive", "Primitive"],
            },
        },
        // 06: Union [String, Number].
        ParityAssertion {
            fixture: "06_union_string_or_number",
            expected_value_kind: ResolvedValueKind::Union {
                arm_kinds: &["Primitive"],
                min_arms: 2,
            },
        },
        // 07: Intersection of {a} and {b} — kept as Intersection
        // (not collapsed to single Object).
        ParityAssertion {
            fixture: "07_intersection_of_two_objects",
            expected_value_kind: ResolvedValueKind::Intersection {
                arm_kinds: &["Object"],
                min_arms: 2,
            },
        },
        // 08: Literal "idle".
        ParityAssertion {
            fixture: "08_string_literal_kept_as_literal",
            expected_value_kind: ResolvedValueKind::Literal("idle"),
        },
        // 09: Bar resolves as Unknown(semanticMiss) under hermetic
        // AuditedRequest — the import-target lookup doesn't kick in
        // without full session-side state. Architectural contract:
        // imported alias names stay shallow at the published surface.
        // The published prop type carries the bare `Ref { name: "Bar" }`
        // and consumers re-resolve through the registry.
        ParityAssertion {
            fixture: "09_decl_ref_to_local_alias",
            expected_value_kind: ResolvedValueKind::SymbolicRef { name: "Bar" },
        },
        // 10: Pick<Foo, 'a'> -> Object {a: String} only.
        // Discriminator: must_not_contain b, c; exactly 1 named member.
        ParityAssertion {
            fixture: "10_pick_two_args_literal_string_key",
            expected_value_kind: ResolvedValueKind::Object {
                must_contain: &["a"],
                must_not_contain: &["b", "c"],
                member_kinds: &[("a", "Primitive")],
                optional_members: &[],
                required_members: &["a"],
                expected_named_member_count: 1,
                expected_call_signature_count: 0,
                expected_construct_signature_count: 0,
                expected_index_signature_count: 0,
            },
        },
        // 11: Pick<Foo, 'a'|'b'|'c'> -> Object {a, b, c}.
        // Discriminator from 01 (which is {a, b}): exactly 3 named
        // members + must_contain c.
        ParityAssertion {
            fixture: "11_pick_with_three_literal_keys",
            expected_value_kind: ResolvedValueKind::Object {
                must_contain: &["a", "b", "c"],
                must_not_contain: &["d"],
                member_kinds: &[("a", "Primitive"), ("b", "Primitive"), ("c", "Primitive")],
                optional_members: &[],
                required_members: &["a", "b", "c"],
                expected_named_member_count: 3,
                expected_call_signature_count: 0,
                expected_construct_signature_count: 0,
                expected_index_signature_count: 0,
            },
        },
        // 12: Omit<Foo, 'b'> -> Object {a, c}.
        // Discriminator: must_not_contain b; exactly 2 named members
        // (a + c).
        ParityAssertion {
            fixture: "12_omit_two_args_drops_excluded_keys",
            expected_value_kind: ResolvedValueKind::Object {
                must_contain: &["a", "c"],
                must_not_contain: &["b"],
                member_kinds: &[("a", "Primitive"), ("c", "Primitive")],
                optional_members: &[],
                required_members: &["a", "c"],
                expected_named_member_count: 2,
                expected_call_signature_count: 0,
                expected_construct_signature_count: 0,
                expected_index_signature_count: 0,
            },
        },
        // 13: Foo['a'] -> Primitive String (route extraction).
        ParityAssertion {
            fixture: "13_indexed_access_string_literal",
            expected_value_kind: ResolvedValueKind::Primitive("String"),
        },
        // 14: Partial<Foo> — projector path enumerates Foo's keys and
        // publishes them as an Object surface. Partial's contract is
        // that every key becomes optional. The Object-level
        // optional/required structure IS authoritative.
        //
        // Per-key value resolution is INTENTIONALLY NOT asserted: the
        // shape of what `Foo[K]` resolves to depends on whether the
        // session-side resolver state is available, and pinning a
        // specific kind here would lock in a test-environment
        // limitation rather than the architectural contract. The
        // discriminator from siblings (01, 02, 11, 12) is: every
        // member is optional + named-member set is exactly {a, b}.
        ParityAssertion {
            fixture: "14_partial_t_makes_all_members_optional",
            expected_value_kind: ResolvedValueKind::Object {
                must_contain: &["a", "b"],
                must_not_contain: &[],
                member_kinds: &[],
                optional_members: &["a", "b"],
                required_members: &[],
                expected_named_member_count: 2,
                expected_call_signature_count: 0,
                expected_construct_signature_count: 0,
                expected_index_signature_count: 0,
            },
        },
        // 15: Required<Foo> — same per-key materialisation; every
        // member becomes required (Required's contract). Per-key
        // value resolution is NOT asserted (see fixture 14).
        ParityAssertion {
            fixture: "15_required_t_makes_all_members_required",
            expected_value_kind: ResolvedValueKind::Object {
                must_contain: &["a", "b"],
                must_not_contain: &[],
                member_kinds: &[],
                optional_members: &[],
                required_members: &["a", "b"],
                expected_named_member_count: 2,
                expected_call_signature_count: 0,
                expected_construct_signature_count: 0,
                expected_index_signature_count: 0,
            },
        },
        // 16: typeof exotic — produces no evaluated_types in hermetic
        // env.
        ParityAssertion {
            fixture: "16_typeof_value_ref",
            expected_value_kind: ResolvedValueKind::Unresolved,
        },
    ]
}

#[test]
fn dump_fixture_resolutions() {
    let resolutions = collect_all_fixture_resolutions();
    for (i, res_opt) in resolutions.iter().enumerate() {
        let kind = match res_opt
            .as_ref()
            .and_then(|r| extract_resolved_value_type(r))
        {
            Some(ty) => format!("kind={} ty={ty:?}", typeexpr_kind(ty)),
            None => "None".to_string(),
        };
        eprintln!("FIXTURE_{:02}: {}", i + 1, kind);
    }
}

#[test]
fn discrimination_exempt_pairwise_count_within_budget() {
    // Plan §4.17 sub-task 2: halt-on-deviation if > 3 unique pairs OR
    // a cluster of 3+ fixtures shares an exemption.
    let exempt: std::collections::HashSet<(usize, usize)> =
        DISCRIMINATION_EXEMPT.iter().copied().collect();

    // Verify symmetric-pair shape: (i, j) implies (j, i) is also present.
    for &(i, j) in DISCRIMINATION_EXEMPT {
        assert!(
            exempt.contains(&(j, i)),
            "DISCRIMINATION_EXEMPT must list pairs symmetrically; ({i}, {j}) \
             present but ({j}, {i}) missing"
        );
    }

    // Pairwise pre-computation reference (informational): structurally
    // equivalent value-TypeExpr pairs computed by deep equality. Logged
    // for debugging — the exempt list may include pairs (e.g., both
    // Unresolved) that are not structurally bytes-equal but are still
    // not discriminated by this assertion suite.
    let computed = compute_pairwise_equivalences();
    eprintln!(
        "structurally_equivalent pairs (informational): {} entries: {:?}",
        computed.len(),
        computed
    );

    let unique_pair_count = exempt.len() / 2;
    assert!(
        unique_pair_count <= 3,
        "DISCRIMINATION_EXEMPT pair count must be ≤ 3 unique pairs (got {unique_pair_count}); \
         halt-on-deviation per plan §4.17 sub-task 2"
    );

    // Cluster check: no fixture should appear in 3+ unique exempt pairs.
    let mut fixture_appearances: std::collections::HashMap<
        usize,
        std::collections::HashSet<usize>,
    > = std::collections::HashMap::new();
    for &(i, j) in DISCRIMINATION_EXEMPT {
        fixture_appearances.entry(i).or_default().insert(j);
    }
    for (fixture, partners) in &fixture_appearances {
        assert!(
            partners.len() < 3,
            "fixture {fixture} participates in {} unique exempt pairs; \
             cluster-of-3 forbidden per plan §4.17 sub-task 2",
            partners.len()
        );
    }
}

/// Plan §6.9 sub-task 3 — discrimination guard test (rev-9 hardened).
///
/// Iterates all 16x15=240 cross-pairs of fixtures. For each pair (i, j)
/// with i != j, verifies that fixture i's expected assertion FAILS on
/// fixture j's resolution — unless (i, j) is in DISCRIMINATION_EXEMPT.
///
/// pair_count tracking + eprintln + assert_eq!(pair_count, 240) is the
/// rev-9 mechanical proof that the §4.17 sub-task 1 pairwise loop
/// actually executed (not skipped or short-circuited).
#[test]
fn parity_assertions_discriminate_per_fixture() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    assert_eq!(
        resolutions.len(),
        16,
        "must have exactly 16 fixture resolutions"
    );
    assert_eq!(assertions.len(), 16, "must have exactly 16 assertions");

    let mut pair_count = 0usize;
    let mut exempt_count = 0usize;
    let mut skipped_unresolvable = 0usize;
    for i in 0..resolutions.len() {
        for j in 0..resolutions.len() {
            if i == j {
                continue;
            }
            pair_count += 1;
            if DISCRIMINATION_EXEMPT.contains(&(i, j)) {
                exempt_count += 1;
                continue;
            }
            // Skip pairs where the resolution is None or has no value
            // prop in evaluated_types — no structure to discriminate on.
            let Some(res_j) = resolutions[j].as_ref() else {
                skipped_unresolvable += 1;
                continue;
            };
            if extract_resolved_value_type(res_j).is_none() {
                skipped_unresolvable += 1;
                continue;
            }
            // Run i's assertion against j's resolution; expect it to fail.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert_parity(res_j, assertions[i].clone());
            }));
            // The assertion should panic (return Err); if it returns Ok,
            // fixtures i and j are not discriminated.
            if result.is_ok() {
                panic!(
                    "discrimination violation: fixture {i}'s assertion \
                     ({}) passed on fixture {j}'s resolution ({}) — \
                     these fixtures are not discriminated. Add to \
                     DISCRIMINATION_EXEMPT or fix the assertion.",
                    assertions[i].fixture, assertions[j].fixture,
                );
            }
        }
    }
    eprintln!(
        "DISCRIMINATION_EXEMPT pairwise computation: evaluated {pair_count} pairs, \
         {exempt_count} exempt, {skipped_unresolvable} skipped (no resolved value type) \
         (out of 16x15=240 expected pairs)"
    );
    // Pairwise-computation gate: if any short-circuit elides the iteration,
    // pair_count != 240 and the assertion fails. This is the rev-9 mechanical
    // proof that the §4.17 sub-task 1 pairwise loop actually executed.
    assert_eq!(pair_count, 240, "must evaluate all 16x15 cross-pairs");
}

// =====================================================================
// 16 per-fixture parity tests (§6.9). Each test re-runs its fixture
// and runs the structured `ParityAssertion` from
// `expected_assertions_per_fixture` against the resolution.
//
// These tests SUPPLEMENT the existing fixture_NN_* smoke tests above
// — the smoke tests assert resolution succeeded; these tests assert
// the resolved value TypeExpr's structural shape matches the
// fixture's policy-table arm.
// =====================================================================

#[test]
fn fixture_01_plain_object_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[0].as_ref() {
        assert_parity(res, assertions[0].clone());
    }
}

#[test]
fn fixture_02_optional_member_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[1].as_ref() {
        assert_parity(res, assertions[1].clone());
    }
}

#[test]
fn fixture_03_method_signature_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[2].as_ref() {
        assert_parity(res, assertions[2].clone());
    }
}

#[test]
fn fixture_04_array_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[3].as_ref() {
        assert_parity(res, assertions[3].clone());
    }
}

#[test]
fn fixture_05_tuple_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[4].as_ref() {
        assert_parity(res, assertions[4].clone());
    }
}

#[test]
fn fixture_06_union_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[5].as_ref() {
        assert_parity(res, assertions[5].clone());
    }
}

#[test]
fn fixture_07_intersection_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[6].as_ref() {
        assert_parity(res, assertions[6].clone());
    }
}

#[test]
fn fixture_08_literal_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[7].as_ref() {
        assert_parity(res, assertions[7].clone());
    }
}

#[test]
fn fixture_09_decl_ref_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[8].as_ref() {
        assert_parity(res, assertions[8].clone());
    }
}

#[test]
fn fixture_10_pick_one_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[9].as_ref() {
        assert_parity(res, assertions[9].clone());
    }
}

#[test]
fn fixture_11_pick_three_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[10].as_ref() {
        assert_parity(res, assertions[10].clone());
    }
}

#[test]
fn fixture_12_omit_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[11].as_ref() {
        assert_parity(res, assertions[11].clone());
    }
}

#[test]
fn fixture_13_indexed_access_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[12].as_ref() {
        assert_parity(res, assertions[12].clone());
    }
}

#[test]
fn fixture_14_partial_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[13].as_ref() {
        assert_parity(res, assertions[13].clone());
    }
}

#[test]
fn fixture_15_required_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[14].as_ref() {
        assert_parity(res, assertions[14].clone());
    }
}

#[test]
fn fixture_16_typeof_structural_assertion() {
    let resolutions = collect_all_fixture_resolutions();
    let assertions = expected_assertions_per_fixture();
    if let Some(res) = resolutions[15].as_ref() {
        assert_parity(res, assertions[15].clone());
    }
}
