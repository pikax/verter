//! Discriminating end-to-end tests for the owner-aware root value-binding
//! index, run through the REAL production wiring
//! (`build_script_analysis_with_scope_from_program_with_owners` →
//! `build_script_analysis_inner` → `RootBindingIndex::build` → the macro and
//! Options-API extraction consumers), not a synthetic unit-level harness.
//!
//! Each test is one row of the discriminating matrix in
//! `docs/arch/refactor/rev11/evidence/CM1/binding-index-design.md`. Every
//! test asserts the CORRECT outcome; several carry a comment naming the
//! WRONG outcome the pre-index unconditional-by-name fold used to produce,
//! so the discriminating property (this test fails on the pre-index tree,
//! passes on this one) is auditable without re-running against a revert.

use crate::analysis::scope::AnalysisScope;
use crate::analysis::top_level_owners::TopLevelOwnerTable;
use crate::analysis::types::AnalyzedMacroKind;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_type_expr::{ConstructorBindingOutcome, TopLevelOwnerId};

/// Parse `source` as an ordinary (single-owner) TS module and run the full
/// production analysis path.
fn analyze_ordinary(source: &str) -> crate::analysis::types::ScriptAnalysisSnapshot {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    crate::analysis::build_script_analysis_with_scope_from_program_with_owners(
        source,
        SourceType::ts(),
        &parsed.program,
        AnalysisScope::all(),
        &owners,
        !parsed.errors.is_empty(),
    )
}

/// Parse `source` as a classic (sloppy, non-module) script — the dialect
/// `with`/Annex-B fixtures need.
fn analyze_sloppy_script(source: &str) -> crate::analysis::types::ScriptAnalysisSnapshot {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    crate::analysis::build_script_analysis_with_scope_from_program_with_owners(
        source,
        SourceType::cjs(),
        &parsed.program,
        AnalysisScope::all(),
        &owners,
        !parsed.errors.is_empty(),
    )
}

/// Parse `source` with an explicit per-statement module/instance owner
/// split (real Vue `<script>` + `<script setup>` topology), one owner per
/// TOP-LEVEL statement in source order.
fn analyze_with_owners(
    source: &str,
    per_statement_owner: &[TopLevelOwnerId],
) -> crate::analysis::types::ScriptAnalysisSnapshot {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    assert_eq!(
        parsed.program.body.len(),
        per_statement_owner.len(),
        "fixture statement count must match the supplied owner list exactly"
    );
    let owners = TopLevelOwnerTable::try_from_statement_owners(
        parsed.program.body.len(),
        per_statement_owner.iter().copied(),
    )
    .expect("validated owner table");
    crate::analysis::build_script_analysis_with_scope_from_program_with_owners(
        source,
        SourceType::ts(),
        &parsed.program,
        AnalysisScope::all(),
        &owners,
        !parsed.errors.is_empty(),
    )
}

fn only_macro_prop_bindings(
    snap: &crate::analysis::types::ScriptAnalysisSnapshot,
) -> Vec<ConstructorBindingOutcome> {
    let mac = snap
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro present");
    mac.prop_fields[0]
        .constructor_bindings
        .iter()
        .map(|entry| entry.resolution.clone())
        .collect()
}

// ── Regression control ──────────────────────────────────────────────────

#[test]
fn regression_control_unshadowed_constructor_stays_global() {
    // Plain `defineProps({ label: String })`, no shadow anywhere — the one
    // case that must NOT regress relative to pre-index behavior.
    let snap = analyze_ordinary("defineProps({ label: String });");
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global]
    );
}

// ── Hoisted var at arbitrary nesting depth ──────────────────────────────

#[test]
fn hoisted_var_at_depth_shadows_constructor() {
    // Pre-index: closed-fact `String` primitive (WRONG — `var String` hoists
    // to function/module scope and shadows the global).
    let snap =
        analyze_ordinary("if (x) { if (y) { var String = 1; } }\ndefineProps({ label: String });");
    let bindings = only_macro_prop_bindings(&snap);
    assert!(matches!(bindings[0], ConstructorBindingOutcome::Local(_)));
}

// ── TS namespace: real vs ambient ───────────────────────────────────────

#[test]
fn real_namespace_shadows_constructor() {
    // Pre-index: closed-fact primitive (WRONG — a real, non-ambient
    // namespace is a runtime value binding).
    let snap = analyze_ordinary(
        "namespace String { export const x = 1; }\ndefineProps({ label: String });",
    );
    let bindings = only_macro_prop_bindings(&snap);
    assert!(matches!(bindings[0], ConstructorBindingOutcome::Local(_)));
}

#[test]
fn ambient_namespace_does_not_shadow_constructor() {
    // Ambient (`declare`) constructs are erased before binding — no runtime
    // binding, so the global constructor still applies.
    let snap = analyze_ordinary(
        "declare namespace String { export const x: 1; }\ndefineProps({ label: String });",
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global]
    );
}

// ── Imports ──────────────────────────────────────────────────────────────

#[test]
fn type_only_import_does_not_shadow_constructor() {
    let snap =
        analyze_ordinary("import type { String } from './x';\ndefineProps({ label: String });");
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global]
    );
}

#[test]
fn value_import_shadows_constructor() {
    // Pre-index: closed-fact primitive (WRONG — a value import is a real
    // runtime binding that shadows the global).
    let snap = analyze_ordinary("import { String } from './x';\ndefineProps({ label: String });");
    let bindings = only_macro_prop_bindings(&snap);
    assert!(matches!(bindings[0], ConstructorBindingOutcome::Local(_)));
}

// ── Non-primitive spelling locally shadowed ─────────────────────────────

#[test]
fn locally_declared_class_shadows_nonprimitive_constructor() {
    // Pre-index: hardcoded `"Array<any>"` display text (WRONG).
    let snap = analyze_ordinary("class Array {}\ndefineProps({ items: Array });");
    let bindings = only_macro_prop_bindings(&snap);
    assert!(matches!(bindings[0], ConstructorBindingOutcome::Local(_)));
}

// ── Destructuring / enum ────────────────────────────────────────────────

#[test]
fn destructured_binding_shadows_constructor() {
    let snap = analyze_ordinary("const { String } = obj;\ndefineProps({ label: String });");
    let bindings = only_macro_prop_bindings(&snap);
    assert!(matches!(bindings[0], ConstructorBindingOutcome::Local(_)));
}

#[test]
fn enum_declaration_shadows_constructor() {
    let snap = analyze_ordinary("enum String { A }\ndefineProps({ label: String });");
    let bindings = only_macro_prop_bindings(&snap);
    assert!(matches!(bindings[0], ConstructorBindingOutcome::Local(_)));
}

// ── Owner topology ───────────────────────────────────────────────────────

#[test]
fn module_owned_declaration_shadows_instance_macro_use() {
    // `<script>` declares `const String = 1`; `<script setup>` uses
    // `defineProps({ label: String })`. The runtime-constructor position
    // resolves from Program root, which sees the module-owned declaration.
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "const String = 1;\ndefineProps({ label: String });",
        &[module, instance],
    );
    let bindings = only_macro_prop_bindings(&snap);
    assert!(matches!(bindings[0], ConstructorBindingOutcome::Local(_)));
}

#[test]
fn instance_local_never_shadows_its_own_runtime_constructor_argument() {
    // v3-specific: Vue's compiler relocates the defineProps runtime argument
    // OUT of setup() before setup() runs, so an ordinary setup-local
    // (non-import) declaration sitting beside the macro call is NEVER
    // shadow-relevant to it — this is the inverse of the naive
    // owner-topology expectation and the whole point of the v3
    // StartScope::ProgramRoot fix.
    // This fixture includes a trivial module-owned statement (an import)
    // to isolate the property under test from the SEPARATE
    // `setup_only_component_with_no_module_region_still_resolves` case
    // below, which covers the (perfectly valid, non-degenerate) case of
    // no module region at all.
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "import './x.css';\nclass String {}\ndefineProps({ items: String });",
        &[module, instance, instance],
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global]
    );
}

#[test]
fn setup_only_component_with_no_module_region_still_resolves() {
    // A `<script setup>`-only SFC — no plain `<script>` block at all — has
    // NO Module owner. That is a VALID, extremely common real-world
    // topology, not a degenerate one: `unique_owner_of_kind` alone cannot
    // distinguish "no module region" from "ambiguous module region", so
    // `RootBindingIndex::build` must (and does) check presence separately,
    // matching the same absent-vs-ambiguous distinction already applied to
    // Instance. A setup-local (instance-owned, non-import) declaration
    // still never shadows its own runtime-constructor argument (Program
    // root sees no module-owned statements here either).
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "class String {}\ndefineProps({ items: String });",
        &[instance, instance],
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global]
    );
}

#[test]
fn setup_only_component_regression_control_unshadowed_stays_global() {
    // The single most common real-world Vue SFC shape: `<script setup>`
    // only, no shadow anywhere. Must resolve exactly like the ordinary-file
    // regression control above — this is the case the "no Module owner is
    // degenerate" bug would have broken for EVERY `<script setup>`-only
    // component's runtime-constructor props.
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners("defineProps({ label: String });", &[instance]);
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global]
    );
}

#[test]
fn setup_only_component_import_shadow_still_resolves_local() {
    // An import always lands at Program root regardless of owner kind —
    // including in a `<script setup>`-only file with no Module owner at
    // all — so it must still shadow the constructor.
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "import { String } from './x';\ndefineProps({ label: String });",
        &[instance, instance],
    );
    let bindings = only_macro_prop_bindings(&snap);
    assert!(matches!(bindings[0], ConstructorBindingOutcome::Local(_)));
}

#[test]
fn ambiguous_module_owner_topology_is_indeterminate() {
    // Genuinely degenerate: TWO conflicting module owners (an invalid
    // topology no real SFC produces) skips the clone/bind entirely — every
    // query answers `Indeterminate`, never a guess.
    let module_a = TopLevelOwnerId::module(0);
    let module_b = TopLevelOwnerId::module(1);
    let snap = analyze_with_owners(
        "const x = 1;\ndefineProps({ label: String });",
        &[module_a, module_b],
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

// ── `with` / direct `eval` (sloppy script only) ─────────────────────────

#[test]
fn with_statement_is_indeterminate_never_global() {
    // `defineProps` is only ever recognized at true top-level statement
    // position (Vue's own authoring constraint) — a `with`-wrapped macro
    // call is not a shape the analyzer's macro walk can reach at all, so
    // this row is tested directly against `RootBindingIndex`: a bare
    // `String` reference lexically inside a `with` block must resolve
    // `Indeterminate` regardless of `StartScope`, never `Global`.
    use crate::analysis::root_binding_index::{RootBindingIndex, StartScope};

    let source = "with (obj) { String; }";
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let index = RootBindingIndex::build(&parsed.program, &owners, !parsed.errors.is_empty());

    let offset = source.rfind("String").expect("fixture needle") as u32;
    let span = verter_span::Span::new(offset, offset + "String".len() as u32);
    let resolution = index.resolve_value_identifier(span, StartScope::ProgramRoot);
    assert!(matches!(
        resolution,
        crate::analysis::root_binding_index::BindingResolution::Indeterminate
    ));
}

#[test]
fn sloppy_direct_eval_in_scope_is_indeterminate() {
    let snap = analyze_sloppy_script("eval('var String = 1');\ndefineProps({ label: String });");
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(bindings, vec![ConstructorBindingOutcome::Indeterminate]);
}

// ── Constructor arrays ───────────────────────────────────────────────────

#[test]
fn constructor_array_resolves_each_element_independently() {
    // Pre-index: not handled at all (macros.rs only recognized a single
    // identifier; options.rs dropped an array outright as "no single
    // constructor").
    let snap = analyze_ordinary("defineProps({ label: [String, Number] });");
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(
        bindings,
        vec![
            ConstructorBindingOutcome::Global,
            ConstructorBindingOutcome::Global
        ]
    );
}

#[test]
fn constructor_array_unrecognized_element_is_indeterminate_never_shrinks_the_array() {
    // A spread element cannot be classified as an identifier or `null`.
    // `resolve_runtime_constructor_array` must still emit ONE entry per
    // authored array element (never silently shrink the vec, which would
    // let a partial array be treated as a complete one downstream).
    let snap =
        analyze_ordinary("const rest = [Number];\ndefineProps({ label: [String, ...rest] });");
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(
        bindings,
        vec![
            ConstructorBindingOutcome::Global,
            ConstructorBindingOutcome::Indeterminate
        ]
    );
}

#[test]
fn nullable_constructor_array_element_is_indeterminate_not_guessed() {
    // DEFERRED per the design doc: a `null` array element's Vue semantics
    // are unconfirmed — it routes through the same Indeterminate-shaped
    // failure channel as an unresolvable identifier rather than guessing.
    let snap = analyze_ordinary("defineProps({ label: [String, null] });");
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(
        bindings,
        vec![
            ConstructorBindingOutcome::Global,
            ConstructorBindingOutcome::Indeterminate
        ]
    );
}

// ── Options-API parity ───────────────────────────────────────────────────

#[test]
fn options_api_locally_shadowed_constructor_is_not_folded() {
    let snap = analyze_ordinary("class Array {}\nexport default { props: { items: Array } };");
    let opts = snap.options_api.expect("options_api present");
    assert!(matches!(
        opts.props[0].constructor_bindings[0].resolution,
        ConstructorBindingOutcome::Local(_)
    ));
    // The display-text-only route must not fire either — no name-based
    // global semantics survive a Local resolution.
    assert!(opts.props[0].type_constructor.is_none());
}

#[test]
fn options_api_constructor_array_resolves_each_element() {
    let snap = analyze_ordinary("export default { props: { label: [String, Number] } };");
    let opts = snap.options_api.expect("options_api present");
    let bindings: Vec<_> = opts.props[0]
        .constructor_bindings
        .iter()
        .map(|entry| entry.resolution.clone())
        .collect();
    assert_eq!(
        bindings,
        vec![
            ConstructorBindingOutcome::Global,
            ConstructorBindingOutcome::Global
        ]
    );
}
