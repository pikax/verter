//! The executable VALUE-WRAP × SURFACE coverage gate: every typed
//! [`ValueWrapSurface`] cell compiles through Verter's PRODUCTION client
//! pipeline in BOTH wrap modes, and the wrap observation (`$.untrack`
//! presence in the emitted module) must match the cell's exhaustive
//! [`classify_value_wrap`] classification:
//!
//! - `BuildExpression` × definite-legacy ⇒ the wrap IS observed;
//! - `Raw` × definite-legacy ⇒ the wrap is NOT observed (the synthesized /
//!   raw-role guardrail — spread operands, `class:` conditions, event
//!   handlers, keyed-each keys, `{@debug}` args never wrap);
//! - EVERY surface × maybe-runes ⇒ the wrap is NOT observed (the mode gate).
//!
//! Reject-unclassified: `classify_value_wrap` is an exhaustive match (a new
//! surface variant fails compilation until classified), and the FRESHNESS pin
//! below asserts the conformance vocabulary equals the compiler's
//! `AuthoredValueSurface` variant inventory — a surface added to the compiler
//! without a covered cell here fails in-tree.

use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::{compile_client, SvelteRuntimeOptions};
use verter_svelte_conformance::value_wrap::{
    classify_value_wrap, render_cell_fixture, value_wrap_cells, LegacyWrapPolicy,
    TriggerReachability, ValueWrapSurface, WrapMode,
};

/// Compile one cell fixture through the production client pipeline.
fn compile_cell(surface: ValueWrapSurface, mode: WrapMode) -> String {
    let source = render_cell_fixture(surface, mode);
    let alloc = Allocator::default();
    let parsed = parse_svelte(&source);
    let opts = SvelteRuntimeOptions {
        filename: Some("Cell.svelte".to_string()),
        ..Default::default()
    };
    compile_client(&source, &parsed, &opts, &alloc, false, false)
        .unwrap_or_else(|e| {
            panic!(
                "value-wrap cell {surface:?} [{mode:?}] must compile; fixture:\n{source}\nerror: {e:?}"
            )
        })
        .code
}

/// The wrap observation: whether the emitted module contains the legacy
/// `$.untrack(` wrap marker.
fn observes_wrap(js: &str) -> bool {
    js.contains("$.untrack(")
}

#[test]
fn every_cell_is_classified_and_the_vocabulary_is_total() {
    let cells = value_wrap_cells();
    assert_eq!(
        cells.len(),
        ValueWrapSurface::ALL.len(),
        "every surface has exactly one classified cell"
    );
    // Slug uniqueness (stable cell identity).
    let mut slugs: Vec<&str> = ValueWrapSurface::ALL.iter().map(|s| s.slug()).collect();
    slugs.sort_unstable();
    slugs.dedup();
    assert_eq!(
        slugs.len(),
        ValueWrapSurface::ALL.len(),
        "cell slugs are unique"
    );
    // Non-vacuity of both policy families.
    assert!(
        cells
            .iter()
            .any(|c| c.policy == LegacyWrapPolicy::BuildExpression),
        "the wrap family is non-empty"
    );
    assert!(
        cells.iter().any(|c| c.policy == LegacyWrapPolicy::Raw),
        "the raw family is non-empty"
    );
}

/// Extract the compiler's `AuthoredValueSurface` variant name-set from its
/// module source — REAL `syn` AST parsing (comments and layout are discarded
/// by parsing, so an inline-comment variant (`Foo, // note`), a doc comment,
/// or any line-layout drift can never silently hide a variant from the
/// freshness pin, unlike a line/char heuristic). Every variant is REQUIRED to
/// be a UNIT variant — the surface vocabulary is a plain closed tag set, and a
/// data-bearing variant would need its own cell-rendering design, so it fails
/// the gate loudly rather than being skipped.
fn compiler_surface_variants(src: &str) -> Vec<String> {
    let file = syn::parse_file(src).expect("client_legacy_value.rs parses as Rust");
    let item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(e) if e.ident == "AuthoredValueSurface" => Some(e),
            _ => None,
        })
        .expect("AuthoredValueSurface enum present");
    item.variants
        .iter()
        .map(|v| {
            assert!(
                matches!(v.fields, syn::Fields::Unit),
                "AuthoredValueSurface::{} must be a UNIT variant (the surface \
                 vocabulary is a closed tag set; a data-bearing variant needs a \
                 conscious coverage-design change here)",
                v.ident
            );
            v.ident.to_string()
        })
        .collect()
}

#[test]
fn freshness_gate_discovers_an_inline_comment_variant() {
    // A unit variant carrying an INLINE comment (`Foo, // rationale`) must be
    // DISCOVERED by the freshness extraction — a line/char heuristic silently
    // ignores it (the whole line fails the "all alphanumeric" filter), opening
    // a manifest coverage gap where a compiler surface lands with no
    // conformance cell. Real AST parsing discards comments by construction.
    let planted = "pub(super) enum AuthoredValueSurface {\n    /// documented\n    Bar,\n    Foo, // rationale note\n    Baz,\n}\n";
    let mut got = compiler_surface_variants(planted);
    got.sort_unstable();
    assert_eq!(
        got,
        vec!["Bar".to_string(), "Baz".to_string(), "Foo".to_string()],
        "an inline-comment variant must not be silently dropped from the \
         freshness pin"
    );
}

#[test]
fn mirror_matches_compiler_surface_vocabulary() {
    // FRESHNESS pin (reject-unclassified across the crate boundary): the
    // conformance vocabulary must equal the compiler's `AuthoredValueSurface`
    // variant inventory. A surface added to the compiler without a covered
    // cell here fails this pin.
    let compiler_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../verter_compiler/src/svelte/runtime/client_legacy_value.rs");
    let src = std::fs::read_to_string(&compiler_src)
        .unwrap_or_else(|e| panic!("read {}: {e}", compiler_src.display()));
    let mut compiler_variants = compiler_surface_variants(&src);
    compiler_variants.sort_unstable();
    let mut mirror: Vec<String> = ValueWrapSurface::ALL
        .iter()
        .map(|s| s.variant_name().to_string())
        .collect();
    mirror.sort_unstable();
    assert_eq!(
        mirror, compiler_variants,
        "the value-wrap coverage vocabulary must mirror the compiler's \
         AuthoredValueSurface variants exactly (add the new surface's cell + \
         classification in value_wrap.rs)"
    );
}

#[test]
fn definite_legacy_cells_observe_the_classified_wrap_policy() {
    for cell in value_wrap_cells() {
        let js = compile_cell(cell.surface, WrapMode::DefiniteLegacy);
        let wrapped = observes_wrap(&js);
        match (cell.policy, cell.reachability) {
            (LegacyWrapPolicy::BuildExpression, TriggerReachability::Observable) => {
                assert!(
                    wrapped,
                    "cell {} is classified BuildExpression: the definite-legacy \
                     emission must observe the wrap:\n{js}",
                    cell.surface.slug()
                );
            }
            (LegacyWrapPolicy::Raw, _)
            | (LegacyWrapPolicy::BuildExpression, TriggerReachability::TriggerUnreachable) => {
                assert!(
                    !wrapped,
                    "cell {} is classified Raw (or trigger-unreachable): the \
                     definite-legacy emission must NOT observe a wrap:\n{js}",
                    cell.surface.slug()
                );
            }
        }
    }
}

#[test]
fn maybe_runes_cells_never_observe_a_wrap() {
    for cell in value_wrap_cells() {
        let js = compile_cell(cell.surface, WrapMode::MaybeRunes);
        assert!(
            !observes_wrap(&js),
            "cell {} must not wrap in maybe-runes mode:\n{js}",
            cell.surface.slug()
        );
    }
}

/// Extract the compiler's per-surface `legacy:` classification from the
/// `policy()` match arms — STRUCTURAL syn AST analysis of the owner module
/// (each arm's `ValuePolicy { legacy: … }` field path), not a substring scan.
fn policy_arms_of(file: &syn::File) -> std::collections::BTreeMap<String, String> {
    let f = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == "policy" => Some(f),
            _ => None,
        })
        .expect("const fn policy present");
    let m = f
        .block
        .stmts
        .iter()
        .find_map(|stmt| match stmt {
            syn::Stmt::Expr(syn::Expr::Match(m), _) => Some(m),
            _ => None,
        })
        .expect("policy body is a match");
    let mut arms = std::collections::BTreeMap::new();
    for arm in &m.arms {
        let syn::Pat::Path(pat) = &arm.pat else {
            continue;
        };
        let variant = pat.path.segments.last().unwrap().ident.to_string();
        let syn::Expr::Struct(body) = arm.body.as_ref() else {
            continue;
        };
        let legacy = body
            .fields
            .iter()
            .find_map(|field| match (&field.member, &field.expr) {
                (syn::Member::Named(name), syn::Expr::Path(p)) if name == "legacy" => {
                    Some(p.path.segments.last().unwrap().ident.to_string())
                }
                _ => None,
            })
            .expect("policy arm carries a path-valued `legacy` field");
        arms.insert(variant, legacy);
    }
    arms
}

/// The compiler policy() arms, parsed from the owner module.
fn compiler_policy_arms() -> std::collections::BTreeMap<String, String> {
    let compiler_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../verter_compiler/src/svelte/runtime/client_legacy_value.rs");
    let src = std::fs::read_to_string(&compiler_src)
        .unwrap_or_else(|e| panic!("read {}: {e}", compiler_src.display()));
    policy_arms_of(&syn::parse_file(&src).expect("client_legacy_value.rs parses"))
}

fn expected_policy_path(policy: LegacyWrapPolicy) -> &'static str {
    match policy {
        LegacyWrapPolicy::BuildExpression => "BuildExpression",
        LegacyWrapPolicy::Raw => "Raw",
    }
}

#[test]
fn compiler_policy_classification_matches_every_cell() {
    // POLICY-CLASSIFICATION parity, per cell — the discriminator the byte
    // observation cannot provide for `BuildExpression + TriggerUnreachable`
    // cells: the EventHandler / DebugArg grammar cannot express the wrap
    // trigger, so a WRONG compiler classification (Raw ↔ BuildExpression)
    // emits identical bytes either way. This asserts the compiler's `policy()`
    // arm classification DIRECTLY against each cell's declared policy.
    let arms = compiler_policy_arms();
    for cell in value_wrap_cells() {
        let compiler = arms
            .get(cell.surface.variant_name())
            .unwrap_or_else(|| panic!("policy() classifies {}", cell.surface.variant_name()));
        assert_eq!(
            compiler,
            expected_policy_path(cell.policy),
            "cell {}: the compiler policy() classification must match the \
             declared cell policy (byte observation alone cannot discriminate \
             a trigger-unreachable surface)",
            cell.surface.slug()
        );
    }
    // Non-vacuity: the trigger-unreachable family this parity check covers
    // beyond the observation gate is present.
    assert!(
        value_wrap_cells()
            .iter()
            .any(|c| c.reachability == TriggerReachability::TriggerUnreachable),
        "the trigger-unreachable family must stay non-empty"
    );
}

#[test]
fn policy_parity_discriminates_a_planted_wrong_classification() {
    // A planted `EventHandler => BuildExpression` arm emits byte-identical
    // output (the accepted handler grammar cannot fire the trigger), so the
    // observation gate stays green — the parity check must catch it.
    let planted: syn::File = syn::parse_str(
        "pub(super) const fn policy(surface: AuthoredValueSurface) -> ValuePolicy {\n\
             match surface {\n\
                 S::EventHandler => ValuePolicy { legacy: BuildExpression, topology: T::Inline },\n\
             }\n\
         }",
    )
    .expect("planted snippet parses");
    let arms = policy_arms_of(&planted);
    let cell = classify_value_wrap(ValueWrapSurface::EventHandler);
    assert_eq!(cell.reachability, TriggerReachability::TriggerUnreachable);
    assert_ne!(
        arms.get("EventHandler").expect("planted arm extracted"),
        expected_policy_path(cell.policy),
        "the parity check must flag a planted wrong trigger-unreachable \
         classification"
    );
}
