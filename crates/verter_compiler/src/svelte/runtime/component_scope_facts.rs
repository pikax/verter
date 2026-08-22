//! The single canonical component-scope facts, sourced from OXC's authoritative
//! scope tree.
//!
//! Each original module / instance script is parsed ONCE (the retained OXC parse
//! snapshot from [`reparse_module`]) and analyzed with
//! [`oxc_semantic::SemanticBuilder`]; the built scope tree is the authority for
//! every declared name — its VALUE-space symbols at EVERY lexical nesting level —
//! and every free reference — the root scope's unresolved references. The template
//! lowering contributes its authored declarations and its already-stored
//! [`AnalyzedExpr`](super::expr::AnalyzedExpr) references into the SAME facts. The
//! module→instance scope topology is preserved: the module script's top-level roots
//! are the instance script's parent frame, so an instance reference to a
//! module-declared name is bound, not free.
//!
//! Sourcing declared names from OXC's own binder — rather than a hand-rolled
//! per-frame visitor — captures every binding kind at every nesting level with no
//! frame bookkeeping: a class-EXPRESSION id, a `static { … }` block binding, a
//! braceless switch-case declaration, function / arrow / catch parameters, and
//! deeply nested locals all land in the scope tree. svelte's deconfliction domain
//! reserves every such binding (properties — method names, object keys — are not
//! bindings and do not reserve).
//!
//! This is the SOLE authority for two consumers:
//!
//! - the component-function name deconfliction ([`super::naming::derive_component_name`]),
//!   which reads `source_declarations ∪ free_references` — svelte's
//!   `module.scope.generate` check domain (`references ∪ declarations ∪ conflicts`);
//! - the `is_pure` scope resolution (`declared_roots`), which reads the top-level
//!   declared roots.
//!
//! A source-form binder DISTINGUISHES authored declarations from synthesized
//! runtime bindings: a `const Foo = writable(0)` store declares the base `Foo`, so
//! `Foo` is retained; the synthesized `$Foo` auto-subscription accessor is reserved
//! ONLY when the source itself references `$Foo` (then `$Foo` is an unresolved
//! value reference). This avoids over-reserving inert synthesized `$Foo` bindings.
//!
//! The deconfliction domain mirrors svelte's runtime value bindings, derived by a
//! POSITIVE scope-view projection rather than an exclusion blocklist. Before binding,
//! the shared `RuntimeSurvivalProjection` rewrites the reparsed program to mirror svelte's
//! `remove_typescript_nodes ∘ create_scopes` scope view (svelte@[`SVELTE_ORACLE_VERSION`]):
//! it ERASES the constructs that leave no runtime binding — the TS declarations svelte's
//! `remove_typescript_nodes` / `create_scopes` scope-erases (`interface` / `type` alias,
//! a type-only namespace-`module` / `global`, ambient `declare const/function/class`, a
//! lone bodiless function-overload signature (`function f(): void;`), type-only
//! `import`/`export`, and the scope-inert `import X = require(...)` / `export = X`), PLUS
//! every `enum` — which svelte REJECTS outright, so Verter erases it DEFENSIVELY (the name
//! never reserves) rather than mirroring a svelte compile — and UNWRAPS the TS expression
//! carriers (`x as T`, `x satisfies T`, `x!`, `<T>x`, `x<T>`) to their inner runtime
//! expression. Binding the PROJECTED program with OXC's
//! `SemanticBuilder` then yields svelte's runtime scope surface for the constructs
//! svelte COMPILES, so a plain value-space symbol filter is the complete, principled
//! selector — no per-construct exclusion list to keep chasing. A name referenced only
//! in TYPE position (including `ValueAsType`, e.g. `typeof x` in a type) still carries
//! no value-position reference and is excluded by the value-reference filter. Same-name
//! merges follow naturally: `interface X` + `const X` keeps `X` (the const survives
//! projection); `declare const X` + `interface X` drops `X` (both erased).
//!
//! Parity has THREE buckets, with ZERO overclaim. For the constructs svelte COMPILES
//! (bucket 1 — normal value bindings, ambient `declare const/function/class` erasure, a
//! type-only namespace and pure `interface`/`type`/lone-overload erasure, the runtime
//! `export * as ns` re-export name, the `as`/`satisfies`/`!` unwraps, abstract/`declare`
//! class-member erasure) the projection matches svelte EXACTLY — this is the parity the
//! oracle-derived name-parity corpus pins. For the constructs svelte HARD-ERRORS
//! (bucket 2 — EVERY `enum` including an ambient `declare enum`, a value `namespace`, a
//! ctor param-property, an `accessor` field) svelte emits NO component and therefore NO
//! name, so name-parity is VACUOUS: the projection erases them DEFENSIVELY (never
//! fabricating a name) EXCEPT a decorator and an `export default`, which the projection
//! LEAVES UNTOUCHED — a known reject-parity gap (svelte rejects the whole component), not
//! a defensive erase. A class index-signature is a DISTINCT case: pinned svelte CRASHES
//! uncoded on it (an uncoded `TypeError`, NOT a typed diagnostic — a `crash` corpus
//! outcome), and Verter defensively erases it (the class name still reserves) — a
//! crash-parity gap, not a typed reject. For the angle-bracket `<T>x` assertion
//! (bucket 3) the production `SourceType::tsx()` reparse fails to parse it BEFORE
//! projection, so Verter fail-closes the whole component (svelte itself compiles it and
//! reserves the inner runtime reference). NOTE the enum handler is UNCONDITIONAL: svelte
//! rejects a `declare enum` exactly like a plain `enum`, so an ambient enum is bucket-2
//! defensive-erase, NOT a bucket-1 compile-to-bare; only a type-only `namespace` COMPILES
//! to bare (bucket 1).
//!
//! The projection's per-construct classification is EXHAUSTIVE over OXC's
//! `Statement` / `Declaration` / `Expression` variants (no wildcard for a TS node
//! kind): a newly-added OXC variant breaks the build. The in-crate conformance module
//! (`component_scope_projection_conformance_tests.rs`) ties this classification to the
//! pinned svelte release two ways: a source-derived handler-bijection + body-fingerprint
//! rail over the vendored `remove_typescript_nodes` visitor set, and the ORACLE-derived
//! name-parity matrix — a committed corpus generated by running the PINNED svelte
//! compiler (`scripts/gen-svelte-name-parity-corpus.mjs`) whose emitted-name pins come
//! from svelte itself, so a projection that drops a reserved name REDs against svelte's
//! own outcome rather than a hand-authored value. See the shared `runtime_survival_erasure::statement_is_scope_erased`.
//!
//! FAIL-CLOSED: a PRESENT script that fails to parse or fails semantic analysis
//! yields a refusal (`Err(slot)` naming the failing script), never partial facts —
//! a fabricated, un-deconflicted component name would emit broken JS.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_semantic::SemanticBuilder;
use rustc_hash::FxHashSet;
use verter_semantic::analysis::runtime_survival_erasure::{
    ErasureDelta, RuntimeSurvivalProjection,
};

use super::client_imports::UserImportSlot;
use super::client_surface_imports::{import_binding_entries, ClassifiedScriptImports};
use super::expr::{reparse_module, ExprArena};

/// The pinned `svelte` release whose `remove_typescript_nodes ∘ create_scopes`
/// scope-view this projection mirrors. A `svelte` dependency bump forces re-
/// verification of the the shared `runtime_survival_erasure::statement_is_scope_erased` / `declaration_is_scope_erased`
/// / the shared `RuntimeSurvivalProjection::visit_expression` classification against the new
/// release: the in-crate conformance module
/// (`component_scope_projection_conformance_tests.rs`) reads this constant and asserts
/// it equals the `svelte` version pinned in `pnpm-lock.yaml`, fingerprints the vendored
/// `remove_typescript_nodes` handler bodies as a drift tripwire, and — through the
/// `HandlerCoverage` rail — asserts a bijection between svelte's handler inventory and
/// the committed name-parity corpus (every handler mapped to ≥1 corpus axis exercised by
/// the production projection). It is a committed tooling anchor, not linked at runtime.
#[allow(dead_code)]
pub(super) const SVELTE_ORACLE_VERSION: &str = "5.56.10";

/// The canonical component-scope facts: the deconfliction inputs both the
/// component-name derivation and the `is_pure` declared-root resolution read.
#[derive(Debug, Default)]
pub(super) struct ComponentScopeFacts {
    /// Every RUNTIME-SURVIVING value-binding name across the module + instance
    /// scripts (at EVERY lexical nesting level) and the template's authored
    /// declarations — store BASE names (`Foo`, never the synthesized `$Foo`),
    /// `$props()` destructure locals, `const`/`var`/`function`/`class`/`let` (bare
    /// AND the `export`-prefixed forms), destructure patterns, nested bindings,
    /// import locals (default / named / namespace), and the each / await /
    /// snippet-name / slot / `{@const}` / `{@let}` template locals. A type-only
    /// declaration (`type` / `interface` / type parameter) or an AMBIENT `declare`
    /// declaration is NOT admitted — svelte COMPILES the component and its
    /// `remove_typescript_nodes` handler erases these, so they never reserve. An `enum`
    /// or enum MEMBER is likewise not admitted, but svelte REJECTS every enum outright;
    /// Verter erases it DEFENSIVELY so a name still deconflicts.
    source_declarations: FxHashSet<String>,
    /// Every FREE / unresolved VALUE reference across the module + instance scripts
    /// and every template expression — an identifier used in value position but not
    /// bound by an enclosing lexical scope. A source `$Foo` auto-subscription READ
    /// lands here (its base `Foo` is declared; the `$Foo` identifier itself is
    /// unbound), so a synthesized `$Foo` is reserved IFF the source actually
    /// references `$Foo`. A name referenced only in TYPE position is excluded.
    free_references: FxHashSet<String>,
    /// The TOP-LEVEL declared root names of the module + instance scripts only
    /// (imports + top-level `let`/`const`/`var`/`function`/`class` + `$props()`
    /// destructure locals) — the `is_pure` scope-resolution input.
    top_level_roots: FxHashSet<String>,
}

impl ComponentScopeFacts {
    /// The component-name deconfliction set: `source_declarations ∪
    /// free_references` — svelte's `module.scope.generate` check domain.
    #[must_use]
    pub(super) fn name_conflicts(&self) -> FxHashSet<String> {
        let mut out = self.source_declarations.clone();
        out.extend(self.free_references.iter().cloned());
        out
    }

    /// The top-level declared root names — the `declared_roots` / `is_pure` input.
    #[must_use]
    pub(super) fn declared_roots(&self) -> &FxHashSet<String> {
        &self.top_level_roots
    }
}

/// The scope facts sourced from a SINGLE script's OXC scope tree: its VALUE-space
/// declared names (every nesting level), its free VALUE references (root unresolved
/// references), and its top-level root bindings.
struct ScriptScopeFacts {
    declarations: FxHashSet<String>,
    free_references: FxHashSet<String>,
    top_level_roots: FxHashSet<String>,
}

/// Build the canonical [`ComponentScopeFacts`] for one component from the OXC scope
/// trees of the module and instance scripts (each parsed ONCE), unioned with the
/// template's authored declarations and stored expression references.
///
/// The module→instance scope topology is preserved: the module script's top-level
/// roots are the instance script's parent frame, so an instance reference to a
/// module-declared root is bound, not free.
///
/// Returns [`Err`] with the FAILING script's slot ([`UserImportSlot::Module`] /
/// [`UserImportSlot::Instance`]) when a PRESENT script fails to parse or fails
/// semantic analysis — never partial facts. The caller maps the slot to that
/// script's span for a precise refusal diagnostic.
pub(super) fn build_component_scope_facts(
    alloc: &Allocator,
    module_source: Option<&str>,
    instance_source: Option<&str>,
    script_imports: &ClassifiedScriptImports,
    template_declarations: &FxHashSet<String>,
    expressions: &ExprArena<'_>,
) -> Result<ComponentScopeFacts, UserImportSlot> {
    let mut facts = ComponentScopeFacts::default();

    // Import locals per slot — read from the single classified-imports carrier,
    // never a raw import re-walk. Each is a top-level declaration of its slot; the
    // union captures injected / admitted locals a bare source scan would miss.
    // The module script: base frame = its own top-level roots + module import
    // locals.
    let mut module_top = import_locals(script_imports, UserImportSlot::Module);
    let mut module_free = FxHashSet::default();
    if let Some(src) = module_source {
        let script = analyze_script_scope(alloc, src).ok_or(UserImportSlot::Module)?;
        facts.source_declarations.extend(script.declarations);
        module_free = script.free_references;
        module_top.extend(script.top_level_roots);
    }

    // The instance script: base frame = its own top-level roots + instance import
    // locals (the module top-level roots form the PARENT frame, applied below).
    let mut instance_top = import_locals(script_imports, UserImportSlot::Instance);
    let mut instance_free = FxHashSet::default();
    if let Some(src) = instance_source {
        let script = analyze_script_scope(alloc, src).ok_or(UserImportSlot::Instance)?;
        facts.source_declarations.extend(script.declarations);
        instance_free = script.free_references;
        instance_top.extend(script.top_level_roots);
    }

    // module→instance topology: the module top-level roots are the instance's
    // PARENT frame, so an instance reference to a module-declared root is bound,
    // not free. (The instance scope tree, built in isolation, records such a
    // reference as unresolved; removing the module roots models the parent frame.)
    for name in &module_top {
        instance_free.remove(name);
    }

    facts.free_references.extend(module_free);
    facts.free_references.extend(instance_free);

    // The top-level declared roots (the `is_pure` input): both scripts' top-level
    // root bindings plus all import locals (already folded into module_top /
    // instance_top).
    facts.top_level_roots.extend(module_top);
    facts.top_level_roots.extend(instance_top);

    // The template's authored declarations (each / await / snippet / slot /
    // `{@const}` / `{@let}` locals) contribute to the source-form declaration set.
    facts
        .source_declarations
        .extend(template_declarations.iter().cloned());

    // The template's stored expression references (collected ONCE by the canonical
    // analysis parse) contribute to the free-reference set — no re-walk.
    for expr in expressions.all() {
        for r in &expr.references {
            facts.free_references.insert(r.name.clone());
        }
    }

    Ok(facts)
}

/// The admitted import LOCAL names of `slot` — from the single classified-imports
/// carrier, never a raw import re-walk.
fn import_locals(
    script_imports: &ClassifiedScriptImports,
    slot: UserImportSlot,
) -> FxHashSet<String> {
    let mut out = FxHashSet::default();
    for import in script_imports.admitted(slot) {
        for (local, _kind) in import_binding_entries(import) {
            out.insert(local.to_string());
        }
    }
    out
}

/// Analyze one script's svelte scope-view, returning its runtime-surviving value-
/// binding declarations (every nesting level), its free VALUE references, and its
/// top-level root bindings.
///
/// The script is REPARSED once via the sanctioned [`reparse_module`] helper (the
/// same single-reparse the IDE scanners use), then PROJECTED to svelte's TS-erased
/// scope view (the shared `RuntimeSurvivalProjection`) IN THE SAME arena — no second parse — and
/// bound with [`oxc_semantic::SemanticBuilder`]. Binding the projected program means
/// a plain value-space symbol filter is the complete selector: the constructs svelte
/// erases are already gone. No thread-local OXC cache.
///
/// FAIL-CLOSED: a torn parse or a non-empty semantic-analysis error set refuses
/// (`None`) rather than returning partial facts.
fn analyze_script_scope(alloc: &Allocator, source: &str) -> Option<ScriptScopeFacts> {
    // Reparse the script once (the sanctioned `reparse_module` pattern the IDE
    // scanners use). `reparse_module` already fails closed on a torn parse (panic /
    // error set).
    let mut program = reparse_module(alloc, source)?;
    // Project to svelte's `remove_typescript_nodes ∘ create_scopes` scope view IN
    // PLACE (same arena, the shared framework-neutral projection), then bind the
    // projected program.
    use oxc_ast_visit::VisitMut;
    RuntimeSurvivalProjection::new(alloc, ErasureDelta::svelte()).visit_program(&mut program);
    let built = SemanticBuilder::new().build(&program);
    // FAIL-CLOSED: a semantic-analysis error on an otherwise-parsed script refuses,
    // rather than feeding a partial scope tree into the name deconfliction.
    if !built.errors.is_empty() {
        return None;
    }
    let scoping = built.semantic.scoping();

    // Declared names: every VALUE binding at EVERY nesting level of the PROJECTED
    // program — sourced from OXC's own binder, so a class-expression id / static-
    // block binding / switch-case declaration / parameter / catch var / nested local
    // is captured with no per-frame bookkeeping. The projection already removed the
    // TS constructs svelte emits nothing for, so `is_value()` (which still excludes a
    // surviving type parameter or a `type` import specifier) is the complete filter.
    let mut declarations = FxHashSet::default();
    for symbol_id in scoping.symbol_ids() {
        if scoping.symbol_flags(symbol_id).is_value() {
            declarations.insert(scoping.symbol_name(symbol_id).to_string());
        }
    }

    // A namespace re-export (`export * as ns from "m"`) reserves `ns` in svelte's
    // `module.scope.generate` conflict domain, but OXC's binder creates NO module-local
    // symbol for it (the `ns` name is an export name, not a referenceable local), so the
    // symbol scan above never sees it. Surface the runtime re-export name into the
    // declaration set so the component-name deconfliction reserves it as svelte's
    // `module.scope.generate` does on this compile row (svelte emits `ns_1`). A type-only
    // `export type * as ns` binds nothing (erased), and a string-literal
    // export name (`export * as "ns"`) is not a JS identifier and cannot collide with a
    // component name — both are excluded. It is NOT a referenceable module-local root, so
    // it stays out of `top_level_roots` (the `is_pure` input), which reads real bindings.
    for statement in &program.body {
        if let Statement::ExportAllDeclaration(export) = statement {
            if export.export_kind.is_type() {
                continue;
            }
            if let Some(exported) = &export.exported {
                if let Some(name) = exported.identifier_name() {
                    declarations.insert(name.as_str().to_string());
                }
            }
        }
    }

    // Free references: the root scope's unresolved references, keeping only names
    // with at least one VALUE-position reference. A name referenced solely in type
    // position — including a `ValueAsType` use (`typeof x` in a type) — carries no
    // value reference and is erased, matching svelte's TypeScript handling.
    let mut free_references = FxHashSet::default();
    for (name, reference_ids) in scoping.root_unresolved_references() {
        let has_value_reference = reference_ids
            .iter()
            .any(|&reference_id| scoping.get_reference(reference_id).is_value());
        if has_value_reference {
            free_references.insert(name.as_str().to_string());
        }
    }

    // Top-level roots: the root (module/program) scope's own value bindings — the
    // `is_pure` declared-root input. A nested binding lives in a child scope and is
    // excluded here.
    let mut top_level_roots = FxHashSet::default();
    for (name, &symbol_id) in scoping.get_bindings(scoping.root_scope_id()) {
        if scoping.symbol_flags(symbol_id).is_value() {
            top_level_roots.insert(name.as_str().to_string());
        }
    }

    Some(ScriptScopeFacts {
        declarations,
        free_references,
        top_level_roots,
    })
}

/// Run ONLY the shared svelte scope-view projection over `source` and return the
/// mutated program, for AST-shape assertions (e.g. that TS expression wrappers are
/// gone). The same `reparse_module` + [`RuntimeSurvivalProjection`] the analysis
/// path uses, without the subsequent `SemanticBuilder` binding.
#[cfg(test)]
pub(super) fn project_source_for_test<'a>(
    alloc: &'a Allocator,
    source: &str,
) -> Option<oxc_ast::ast::Program<'a>> {
    use oxc_ast_visit::VisitMut;
    let mut program = reparse_module(alloc, source)?;
    RuntimeSurvivalProjection::new(alloc, ErasureDelta::svelte()).visit_program(&mut program);
    Some(program)
}

#[cfg(test)]
#[path = "component_scope_facts_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "component_scope_projection_conformance_tests.rs"]
mod projection_conformance_tests;
