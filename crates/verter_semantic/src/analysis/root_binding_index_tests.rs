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
use verter_type_expr::{ConstructorBindingOutcome, DeclBindingKey, TopLevelOwnerId};

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

/// Parse `source` as a sloppy TypeScript classic script. Distinct from
/// `analyze_ordinary`'s `SourceType::ts()` (unambiguous, typically a module
/// and therefore strict): TypeScript type wrappers (`as` / `satisfies` /
/// `!` / type assertion) only parse under a TS source type, and sloppy
/// direct-eval `var` leak is only observable in a non-strict script.
fn analyze_sloppy_ts_script(source: &str) -> crate::analysis::types::ScriptAnalysisSnapshot {
    let source_type = SourceType::script().with_typescript(true);
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    crate::analysis::build_script_analysis_with_scope_from_program_with_owners(
        source,
        source_type,
        &parsed.program,
        AnalysisScope::all(),
        &owners,
        !parsed.errors.is_empty(),
    )
}

/// Parse `source` as a REAL `ModuleKind::Script` classic script — distinct
/// from `analyze_sloppy_script`'s CommonJS, which wraps its whole body in a
/// function at load time and so never aliases the true global object. Only
/// `ModuleKind::Script`'s outermost scope is Annex-B global-object-aliased
/// (`function`/`var` declared there ARE properties of `globalThis`).
fn analyze_classic_script(source: &str) -> crate::analysis::types::ScriptAnalysisSnapshot {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::script()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    crate::analysis::build_script_analysis_with_scope_from_program_with_owners(
        source,
        SourceType::script(),
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

fn only_expose_referenced_binding(
    snap: &crate::analysis::types::ScriptAnalysisSnapshot,
) -> Option<DeclBindingKey> {
    let mac = snap
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineExpose)
        .expect("defineExpose macro present");
    mac.expose_fields[0].referenced_binding.clone()
}

#[test]
fn expose_identifier_captures_instance_shadow_key_not_module_parent() {
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "const shared = 'module';\nconst shared = 1;\ndefineExpose({ shared });",
        &[module, instance, instance],
    );
    let key =
        only_expose_referenced_binding(&snap).expect("instance shadow is a Local expose binding");
    assert_eq!(key.owner, instance);
    assert_eq!(key.name.as_ref(), "shared");
}

#[test]
fn expose_identifier_captures_module_parent_when_instance_does_not_shadow() {
    // An instance `defineExpose({ moduleOnly })` may legitimately resolve a
    // module parent. Capturing `mac.owner` (instance) would drop it.
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "const moduleOnly = true;\ndefineExpose({ moduleOnly });",
        &[module, instance],
    );
    let key = only_expose_referenced_binding(&snap)
        .expect("visible module parent is a Local expose binding");
    assert_eq!(key.owner, module);
    assert_eq!(key.name.as_ref(), "moduleOnly");
}

/// An exposed identifier binding no local declaration resolves `Global` and
/// stores NO key: a name-only fallback would first-name-join an unrelated
/// same-spelling `.bindings` row instead of reporting the honest "no
/// resolvable local binding". The shadowed contrast pins that the `None`
/// comes from the resolution outcome, not from the capture silently
/// failing to see the identifier at all.
#[test]
fn expose_identifier_resolving_global_captures_no_binding() {
    let instance = TopLevelOwnerId::instance(0);
    assert_eq!(
        only_expose_referenced_binding(&analyze_with_owners(
            "defineExpose({ String });",
            &[instance],
        )),
        None,
        "an unshadowed global spelling is not a local declaration"
    );

    let shadowed = only_expose_referenced_binding(&analyze_with_owners(
        "const String = 1;\ndefineExpose({ String });",
        &[instance, instance],
    ))
    .expect("the shadowing declaration makes the same spelling Local");
    assert_eq!(shadowed.owner, instance);
    assert_eq!(shadowed.name.as_ref(), "String");
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

#[test]
fn strict_class_field_eval_does_not_leak_into_sloppy_var_env() {
    // Direct eval from a strict caller (class body / class field
    // initializer) gets a fresh variable environment (ECMA-262
    // PerformEval) and MUST NOT leak `var` into the surrounding sloppy
    // Program var env. Climbing to the nearest `is_var()` ancestor
    // first, then checking THAT scope's strictness, records the sloppy
    // Program and wrongly marks the sibling constructor Indeterminate.
    let snap = analyze_sloppy_script(
        "class Probe {\n  value = eval('var String = 1')\n}\ndefineProps({ label: String });",
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global],
        "strict class-field eval must not leak var into the sloppy Program \
         var env; String at the constructor position stays Global"
    );
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
fn nullable_constructor_array_element_resolves_global_null() {
    // Confirmed directly against `@vue/runtime-core`'s own
    // `getType`/`assertType` (`packages/runtime-core/src/componentProps.ts`):
    // `getType(null)` returns the string `"null"`, and `assertType`
    // special-cases `expectedType === "null"` as `valid = value === null` —
    // i.e. `[String, null]` means "String-typed value OR literal `null`",
    // the ordinary nullable-constructor idiom. A literal `null` is never an
    // identifier, so it is never binding-index-gated and always resolves
    // `Global` — see `resolve_runtime_constructor_array`.
    let snap = analyze_ordinary("defineProps({ label: [String, null] });");
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(
        bindings,
        vec![
            ConstructorBindingOutcome::Global,
            ConstructorBindingOutcome::Global
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

// ── Program-root binding owner attribution ─────────────────────────────

#[test]
fn hoisted_setup_import_keeps_instance_owner_not_module_owner() {
    // A `<script setup>`-owned import always lands at Program root
    // regardless of owner kind (matching Vue's real import-hoisting
    // behavior), but its CANONICAL owner — the key `ShallowFileState`
    // indexes it under (`shallow_file_state.rs`'s `binding.owner`) — stays
    // Instance. Program-root landing is a binder-topology detail, not a
    // change of authored owner: with a SIBLING module owner present too
    // (a real `<script>` block), the module owner must not steal an
    // unrelated instance-owned import's attribution.
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "const unrelated = 1;\nimport { Custom } from './x';\ndefineProps({ value: Custom });",
        &[module, instance, instance],
    );
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(
        bindings,
        vec![ConstructorBindingOutcome::Local(DeclBindingKey::new(
            instance, "Custom"
        ))]
    );
}

#[test]
fn type_only_import_erased_before_binding_does_not_steal_surviving_owner() {
    // A whole-statement `import type` is erased from the clone BEFORE
    // binding (`RuntimeSurvivalProjection` in `build_and_bind`) — it
    // produces no `SymbolId` at all. A pre-erasure name->owner mapping
    // populated from raw import text (rather than the actual bound symbol)
    // would wrongly relabel the SURVIVING module-owned `const Custom` with
    // the erased import's Instance owner; the span-correlated post-bind
    // map must not.
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "const Custom = 1;\nimport type { Custom } from './x';\ndefineProps({ value: Custom });",
        &[module, instance, instance],
    );
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(
        bindings,
        vec![ConstructorBindingOutcome::Local(DeclBindingKey::new(
            module, "Custom"
        ))]
    );
}

#[test]
fn frontmatter_owned_root_declaration_keeps_frontmatter_owner() {
    // `TopLevelOwnerKind` has a third kind, Frontmatter, that the wrap loop
    // in `build_and_bind` does not special-case (it only wraps `Instance`)
    // — a Frontmatter-owned declaration therefore lands at Program root
    // exactly like a Module declaration does. Its AUTHORED owner must stay
    // Frontmatter, never fall back to a sibling Module/Instance owner.
    let module = TopLevelOwnerId::module(0);
    let frontmatter = TopLevelOwnerId::frontmatter(0);
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "const unrelated = 1;\nconst Custom = 1;\ndefineProps({ value: Custom });",
        &[module, frontmatter, instance],
    );
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(
        bindings,
        vec![ConstructorBindingOutcome::Local(DeclBindingKey::new(
            frontmatter,
            "Custom"
        ))]
    );
}

#[test]
fn duplicate_cross_owner_import_names_are_indeterminate_not_guessed() {
    // Two imports of the same local name under different owners collide at
    // Program root scope; OXC's binder merges them onto ONE canonical
    // symbol (recording the collision in `symbol_redeclarations`) rather
    // than rejecting the parse outright. No single authored owner is
    // correct here — must fail closed, never silently attribute whichever
    // declaration the binder happened to keep as `symbol_span`.
    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let snap = analyze_with_owners(
        "import { Custom } from './a';\nimport { Custom } from './b';\ndefineProps({ value: Custom });",
        &[module, instance, instance],
    );
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(bindings, vec![ConstructorBindingOutcome::Indeterminate]);
}

/// An unmapped declaration span must fail closed to `Indeterminate`, not
/// silently keep the owner computed from the remaining mapped spans.
///
/// Clone construction otherwise guarantees containment: every Program-root
/// statement is recorded with its own span, and CloneIn preserves identifier
/// spans inside it. A silent `None => {}` skip is therefore unobservable on
/// any source-text fixture (one mapped owner stays `Local`; two mapped
/// owners are already `Indeterminate`). The discriminating case is a
/// SAME-owner `var` redeclaration whose SECOND statement span is moved so
/// it no longer contains its declaring identifier — one span maps, one
/// does not. Production treats that as ambiguous; a silent skip would
/// publish `Local`.
#[test]
fn unmapped_redeclaration_span_is_ambiguous_not_silent_owner() {
    use oxc_ast::ast::Statement;

    let source = "var Custom = 1;\nvar Custom = 2;\ndefineProps({ value: Custom });";
    let allocator = Allocator::default();
    let mut parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    match parsed.program.body.get_mut(1) {
        Some(Statement::VariableDeclaration(decl)) => {
            decl.span = oxc_span::Span::new(u32::MAX - 8, u32::MAX);
        }
        other => {
            panic!("fixture premise: the second statement must be `var Custom = 2`; got {other:?}")
        }
    }
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let snap = crate::analysis::build_script_analysis_with_scope_from_program_with_owners(
        source,
        SourceType::ts(),
        &parsed.program,
        AnalysisScope::all(),
        &owners,
        !parsed.errors.is_empty(),
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate],
        "an unmapped redeclaration span must fail closed, never silently \
         attribute the remaining mapped owner"
    );
}

#[test]
fn owner_natural_scope_fails_closed_on_ambiguous_root_binding() {
    // The SAME cross-owner collision as
    // `duplicate_cross_owner_import_names_are_indeterminate_not_guessed`,
    // but queried through `StartScope::OwnerNaturalScope` directly against
    // `RootBindingIndex` — `defineProps` only ever queries `ProgramRoot`, so
    // this is the discriminating row for `resolve_natural`'s own owner
    // attribution. Before unifying both arms onto the SAME symbol-keyed
    // `root_binding_owner_by_symbol` / `root_ambiguous_binding_symbols`
    // authority, `resolve_natural` derived its owner from
    // `owner_of_scope(decl_scope)` alone — it never consulted the
    // ambiguity set at all — so it confidently returned
    // `Local(module_owner, "Custom")` here instead of failing closed.
    use crate::analysis::root_binding_index::{RootBindingIndex, StartScope};

    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let source = "import { Custom } from './a';\nimport { Custom } from './b';\nCustom;\n";
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let owners = TopLevelOwnerTable::try_from_statement_owners(
        parsed.program.body.len(),
        [module, instance, module],
    )
    .expect("validated owner table");
    let index = RootBindingIndex::build(&parsed.program, &owners, !parsed.errors.is_empty());

    let offset = source.rfind("Custom;").expect("fixture needle") as u32;
    let span = verter_span::Span::new(offset, offset + "Custom".len() as u32);
    let resolution = index.resolve_value_identifier(span, StartScope::OwnerNaturalScope);
    assert!(matches!(
        resolution,
        crate::analysis::root_binding_index::BindingResolution::Indeterminate
    ));
}

#[test]
fn owner_natural_scope_attributes_hoisted_setup_import_to_instance_not_module() {
    // The natural-scope twin of `hoisted_setup_import_keeps_instance_owner_
    // not_module_owner`: an Instance-owned import lands at Program root
    // (Vue's real import-hoisting behavior) beside a sibling Module region.
    // Before unification, `resolve_natural` attributed owner via
    // `owner_of_scope(decl_scope)` alone — for a Program-root-landing
    // symbol, `decl_scope == program_root_scope_id` is never inside the
    // wrapper, so `owner_of_scope` fell through to `module_owner.or(
    // instance_owner)` and WRONGLY returned the sibling Module owner
    // instead of the import's own authored Instance owner. The unified
    // `owner_for_symbol` consults `root_binding_owner_by_symbol` first (the
    // SAME span-correlated authority `resolve_from_program_root` uses) and
    // gets it right.
    use crate::analysis::root_binding_index::{RootBindingIndex, StartScope};

    let module = TopLevelOwnerId::module(0);
    let instance = TopLevelOwnerId::instance(0);
    let source = "const unrelated = 1;\nimport { Custom } from './x';\nCustom;\n";
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let owners = TopLevelOwnerTable::try_from_statement_owners(
        parsed.program.body.len(),
        [module, instance, instance],
    )
    .expect("validated owner table");
    let index = RootBindingIndex::build(&parsed.program, &owners, !parsed.errors.is_empty());

    let offset = source.rfind("Custom;").expect("fixture needle") as u32;
    let span = verter_span::Span::new(offset, offset + "Custom".len() as u32);
    let resolution = index.resolve_value_identifier(span, StartScope::OwnerNaturalScope);
    assert_eq!(
        resolution,
        crate::analysis::root_binding_index::BindingResolution::Local(DeclBindingKey::new(
            instance, "Custom"
        ))
    );
}

#[test]
fn owner_natural_scope_fails_closed_on_nested_sibling_locals_instead_of_colliding() {
    // Reviewer counterexample: two DISTINCT nested locals sharing a name,
    // declared under two DIFFERENT authored owners, neither of which is
    // Program-root-bound (each `X` lives inside its own function's body
    // scope, one level below its owning function's own Program-root
    // binding). Before unifying `owner_for_symbol` onto a scope-EXACT
    // check, the scope-topology fallback (`owner_of_scope`) only ever
    // distinguished "inside the instance wrapper" from "everything else"
    // and collapsed everything else onto `module_owner.or(instance_owner)`
    // — with no instance owner here, BOTH nested `X` symbols wrongly
    // resolved to the SAME `Local(Module(0), "X")` key despite `fm`'s `X`
    // being authored under Frontmatter, not Module, and despite the two
    // `X`s being entirely distinct runtime bindings. Neither symbol is
    // Program-root-bound and neither sits exactly at the instance
    // wrapper's own top-level scope, so both must now fail closed instead.
    use crate::analysis::root_binding_index::{BindingResolution, RootBindingIndex, StartScope};

    let module = TopLevelOwnerId::module(0);
    let frontmatter = TopLevelOwnerId::frontmatter(0);
    let source = "function fm() { let X = 1; X; }\nfunction mod() { let X = 2; X; }\n";
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let owners = TopLevelOwnerTable::try_from_statement_owners(
        parsed.program.body.len(),
        [frontmatter, module],
    )
    .expect("validated owner table");
    let index = RootBindingIndex::build(&parsed.program, &owners, !parsed.errors.is_empty());

    let fm_offset = source.find("X;").expect("fm needle") as u32;
    let fm_span = verter_span::Span::new(fm_offset, fm_offset + "X".len() as u32);
    let mod_offset = source.rfind("X;").expect("mod needle") as u32;
    let mod_span = verter_span::Span::new(mod_offset, mod_offset + "X".len() as u32);

    assert_eq!(
        index.resolve_value_identifier(fm_span, StartScope::OwnerNaturalScope),
        BindingResolution::Indeterminate,
        "fm's nested X must not be guessed as Module-owned"
    );
    assert_eq!(
        index.resolve_value_identifier(mod_span, StartScope::OwnerNaturalScope),
        BindingResolution::Indeterminate,
        "mod's nested X must not collide with fm's distinct X under the same key"
    );
}

#[test]
fn same_owner_redeclaration_still_resolves_normally() {
    // A same-owner redeclaration (`var Custom = 1; var Custom = 2;`, legal
    // JS) merges onto one `SymbolId` exactly like a cross-owner collision
    // does, but every declaration/redeclaration span resolves to the SAME
    // owner here — the owner-SET check must not over-widen ambiguity to
    // this ordinary, unambiguous case.
    let module = TopLevelOwnerId::module(0);
    let snap = analyze_with_owners(
        "var Custom = 1;\nvar Custom = 2;\ndefineProps({ value: Custom });",
        &[module, module, module],
    );
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(
        bindings,
        vec![ConstructorBindingOutcome::Local(DeclBindingKey::new(
            module, "Custom"
        ))]
    );
}

// ── Sloppy direct `eval` leak reaches the nearest variable environment ──

#[test]
fn sloppy_direct_eval_in_nested_block_shadows_sibling_reference() {
    // The `eval` call sits inside an unrelated nested block; the
    // constructor reference is a SIBLING top-level statement, never a
    // descendant of that block. Sloppy direct `eval`'s `var` declarations
    // attach to the nearest VARIABLE environment (the enclosing
    // function/program), not the exact lexical block containing the call —
    // so this reference must ALSO be `Indeterminate`, exactly like an
    // ordinary hoisted `var` at the same depth would shadow it.
    let snap =
        analyze_sloppy_script("{ eval('var String = 1'); }\ndefineProps({ label: String });");
    let bindings = only_macro_prop_bindings(&snap);
    assert_eq!(bindings, vec![ConstructorBindingOutcome::Indeterminate]);
}

#[test]
fn locally_shadowed_eval_is_not_treated_as_direct_eval() {
    // A local `eval` binding (a function declaration named `eval`, valid in
    // sloppy mode) is an ordinary function call through a shadowed name —
    // never the spec's direct-eval form, which requires the callee to
    // resolve to the untouched global `eval` intrinsic. It carries no
    // scope-injection power and must never mark its enclosing scope
    // indeterminate.
    let snap = analyze_sloppy_script(
        "function eval() {}\neval('var String = 1');\ndefineProps({ label: String });",
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global]
    );
}

// The four `_stays_indeterminate` tests below assert an Indeterminate
// outcome. An Indeterminate-outcome test can NEVER discriminate against the
// pre-index baseline (unconditional-by-name eval poisoning: ANY textual
// `eval(...)` call in a non-strict scope was already Indeterminate,
// regardless of binding) — that baseline is strictly MORE conservative than
// the current binding-aware classifier, so it produces the identical
// Indeterminate verdict on every fixture below no matter how the fixture is
// constructed. Only a Global-outcome assertion (like
// `locally_shadowed_eval_is_not_treated_as_direct_eval` above) can ever
// discriminate that baseline. Each test below instead discriminates the
// SPECIFIC current-code clause named in its comment: deleting just that
// clause from `provably_safe` flips this exact fixture to (wrongly)
// `Global` — verified by temporarily deleting the named clause, confirming
// this test goes RED, then restoring it and confirming GREEN again.

#[test]
fn var_bound_eval_stays_indeterminate_not_provably_non_intrinsic() {
    // Discriminates `flags.contains(SymbolFlags::Function)`: a `var`/
    // `let`/`const` binding named `eval` does not create a value provably
    // distinct from the intrinsic `%eval%` — `var eval = <the real eval>;`
    // is still spec-direct eval. Deleting the Function-flag clause (leaving
    // only the mutation/redeclaration/global-aliasable checks, all
    // vacuously true here — no write reference, no redeclaration, no
    // Function-flagged symbol at all) flips this fixture to `Global`.
    let snap = analyze_sloppy_script(
        "var eval = something;\neval('var String = 1');\ndefineProps({ label: String });",
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

#[test]
fn eval_reassigned_after_function_declaration_stays_indeterminate() {
    // Discriminates `!scoping.symbol_is_mutated(symbol_id)`: a
    // `function eval() {}` declaration's OWN value is provably a fresh,
    // non-intrinsic function object — but the BINDING can still be
    // reassigned afterward (`eval = trueEval;`), and the spec's direct-eval
    // test is a value check at the CALL, not at the declaration. Deleting
    // just the mutation clause (leaving Function-flag + redeclarations-
    // empty + not-global-aliasable, all true here) flips this fixture to
    // `Global`.
    let snap = analyze_sloppy_script(
        "function eval() {}\neval = trueEval;\neval('var String = 1');\ndefineProps({ label: String });",
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

#[test]
fn eval_redeclared_via_initializer_stays_indeterminate() {
    // Discriminates `scoping.symbol_redeclarations(symbol_id).is_empty()`:
    // `var eval = trueEval;` REDECLARES the `function eval() {}` binding
    // with its own initializer — a declaration, not an ordinary write
    // `Reference`, so `symbol_is_mutated` alone cannot see it. Deleting
    // just the redeclarations clause (leaving Function-flag + not-mutated +
    // not-global-aliasable, all true here) flips this fixture to `Global`.
    let snap = analyze_sloppy_script(
        "function eval() {}\nvar eval = trueEval;\neval('var String = 1');\ndefineProps({ label: String });",
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

#[test]
fn with_shadowed_local_eval_callee_still_recorded_as_possible_direct_eval() {
    // Counterexample (a): a `with` object can supply its OWN `eval`
    // property at runtime, intercepting the lookup before it ever reaches
    // the statically resolved binding — OXC documents walking past `with`
    // scopes as a known resolution limitation
    // (`oxc_semantic::is_global_reference`). Every OTHER `provably_safe`
    // clause holds here (Function-flagged, unmutated, no redeclarations,
    // not global-aliasable), so before the `with_shadow_possible` check
    // this callee was wrongly `continue`d past and never recorded:
    //
    // ```js
    // const trueEval = eval;
    // (function () {
    //   function eval() {}
    //   with ({ eval: trueEval }) {
    //     eval("var String = 1");
    //   }
    //   console.log(String); // 1
    // })();
    // ```
    //
    // A consuming query (`resolve_value_identifier`) physically inside (or
    // nested within) the `with` block is ALREADY forced `Indeterminate` by
    // the independent `with`-ancestor check
    // (`with_statement_is_indeterminate_never_global` above), so that path
    // can never discriminate this specific fix — the internal record is
    // the only observable signal. Deleting the `with_shadow_possible`
    // clause flips this fixture's count to 0.
    use crate::analysis::root_binding_index::RootBindingIndex;

    let source = "function eval() {}\nwith (obj) { eval('var String = 1'); }";
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let index = RootBindingIndex::build(&parsed.program, &owners, !parsed.errors.is_empty());
    assert_eq!(
        index.sloppy_eval_scope_count(),
        1,
        "a with-shadowed callee that resolves to a locally declared, unmutated, \
         non-redeclared function must still be recorded as a possible direct eval"
    );
}

#[test]
fn eval_function_at_classic_script_root_stays_indeterminate_even_when_unmutated() {
    // Counterexample (b): a `function eval() {}` declared at the OUTERMOST
    // scope of a real classic (`ModuleKind::Script`) script is ALSO
    // installed as a property of the global object (Annex B
    // `GlobalDeclarationInstantiation`) — a property write through any
    // alias of the global object (`globalThis.eval = ...`, possibly from
    // another script entirely) mutates the very binding an unqualified
    // `eval` resolves to WITHOUT ever emitting a `Reference` to the `eval`
    // identifier itself:
    //
    // ```js
    // // A prior script saved the intrinsic:
    // globalThis.trueEval = globalThis.eval;
    //
    // function eval() {}
    // globalThis.eval = globalThis.trueEval;
    // eval("var String = 1");
    // defineProps({ label: String });
    // ```
    //
    // `symbol_is_mutated`/`symbol_redeclarations` see nothing to veto and
    // every other `provably_safe` clause holds. Unlike the `with` case
    // above, there is no independent redundant check here: this must be
    // caught directly by the callee's declaring scope, and the outcome is
    // observable through the ordinary consuming-query path. Deleting the
    // `declared_at_global_aliasable_scope` clause flips this fixture to
    // `Global`.
    let snap = analyze_classic_script(
        "function eval() {}\nglobalThis.eval = globalThis.trueEval;\neval('var String = 1');\ndefineProps({ label: String });",
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

// ── Spec-direct vs spec-indirect `eval` callee shapes ────────────────────
//
// Direct eval is a Reference-identity check (EvaluateCall: the callee
// evaluates to a Reference whose referenced name is `"eval"` AND whose
// value is `%eval%`). The grouping operator does NOT apply GetValue, so a
// parenthesized identifier is still that Reference — including nested
// parens, and TypeScript type wrappers that compile away and leave the
// identifier (possibly still parenthesized) behind.
//
// Comma-expression and optional-call forms DO apply GetValue / use a
// different Call production, so they are spec-indirect: `var` injected by
// the call lands on the global object, never the local variable
// environment, and a sibling constructor must stay `Global`.

#[test]
fn parenthesized_eval_callee_is_still_spec_direct_eval() {
    // `(eval)('var String = 1')` is spec-direct eval. The collector that
    // only matched `Expression::Identifier` callees missed this shape and
    // left the sibling constructor `Global` (fail-open). Confirmed against
    // Node: the injected `var` overwrites the local `String`.
    let snap = analyze_sloppy_script("(eval)('var String = 1');\ndefineProps({ label: String });");
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

#[test]
fn nested_parenthesized_eval_callee_is_still_spec_direct_eval() {
    // Nested grouping is still Evaluation-of-Expression with no GetValue.
    // One-paren special-casing would miss this.
    let snap =
        analyze_sloppy_script("((eval))('var String = 1');\ndefineProps({ label: String });");
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

#[test]
fn parenthesized_ts_as_eval_callee_is_still_spec_direct_eval() {
    // `as` is erased before bind (runtime-survival projection), leaving
    // `(eval)(...)` — the same parenthesized Reference as the JS case.
    let snap = analyze_sloppy_ts_script(
        "(eval as any)('var String = 1');\ndefineProps({ label: String });",
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

#[test]
fn parenthesized_ts_satisfies_eval_callee_is_still_spec_direct_eval() {
    // `satisfies` is another compile-away wrapper
    // `Expression::get_inner_expression` peels. Same Reference as
    // `(eval)(...)`.
    let snap = analyze_sloppy_ts_script(
        "(eval satisfies any)('var String = 1');\ndefineProps({ label: String });",
    );
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

#[test]
fn ts_nonnull_eval_callee_is_still_spec_direct_eval() {
    // `(eval!)(...)` puts the non-null assertion ON THE CALLEE. Bare
    // `eval!(...)` does not discriminate `get_inner_expression` (the
    // peel-bypass plant left that fixture GREEN — the callee was already
    // an Identifier). Parenthesizing forces `TSNonNullExpression` as the
    // CallExpression callee so the peel is load-bearing.
    let snap =
        analyze_sloppy_ts_script("(eval!)('var String = 1');\ndefineProps({ label: String });");
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

#[test]
fn ts_type_assertion_eval_callee_is_still_spec_direct_eval() {
    // `(<any>eval)(...)` puts the angle-bracket assertion ON THE CALLEE.
    // Bare `<any>eval(...)` is `<any>(eval(...))` — assertion around the
    // CALL, callee already Identifier — so the peel-bypass plant left it
    // GREEN. Parenthesizing forces `TSTypeAssertion` as the callee.
    // Not JSX: this fixture is a TS classic script, not TSX.
    let snap =
        analyze_sloppy_ts_script("(<any>eval)('var String = 1');\ndefineProps({ label: String });");
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Indeterminate]
    );
}

#[test]
fn optional_eval_call_is_indirect_and_stays_global() {
    // `eval?.(...)` is an optional Call production, not spec-direct eval.
    // Node: the injected `var` is global-only. Must stay Global.
    let snap = analyze_sloppy_script("eval?.('var String = 1');\ndefineProps({ label: String });");
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global]
    );
}

#[test]
fn comma_expression_eval_callee_is_indirect_and_stays_global() {
    // `(0, eval)(...)` evaluates a SequenceExpression, which returns a
    // value, not a Reference. Spec-indirect. Node: global-only leak.
    let snap =
        analyze_sloppy_script("(0, eval)('var String = 1');\ndefineProps({ label: String });");
    assert_eq!(
        only_macro_prop_bindings(&snap),
        vec![ConstructorBindingOutcome::Global]
    );
}
