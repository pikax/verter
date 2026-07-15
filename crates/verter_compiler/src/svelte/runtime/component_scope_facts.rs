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
//! [`SvelteScopeProjection`] rewrites the reparsed program to mirror svelte's
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
//! own outcome rather than a hand-authored value. See [`statement_is_scope_erased`].
//!
//! FAIL-CLOSED: a PRESENT script that fails to parse or fails semantic analysis
//! yields a refusal (`Err(slot)` naming the failing script), never partial facts —
//! a fabricated, un-deconflicted component name would emit broken JS.

use oxc_allocator::{Allocator, TakeIn};
use oxc_ast::ast::{
    AccessorPropertyType, ClassBody, ClassElement, Declaration, ExportNamedDeclaration, Expression,
    FormalParameters, MethodDefinitionType, PropertyDefinitionType, Statement, TSAccessibility,
};
use oxc_ast::{match_member_expression, AstBuilder};
use oxc_ast_visit::{walk_mut, VisitMut};
use oxc_semantic::SemanticBuilder;
use oxc_span::GetSpan;
use rustc_hash::FxHashSet;

use super::client_imports::UserImportSlot;
use super::client_surface_imports::{import_binding_entries, ClassifiedScriptImports};
use super::expr::{reparse_module, ExprArena};

/// The pinned `svelte` release whose `remove_typescript_nodes ∘ create_scopes`
/// scope-view this projection mirrors. A `svelte` dependency bump forces re-
/// verification of the [`statement_is_scope_erased`] / [`declaration_is_scope_erased`]
/// / [`SvelteScopeProjection::visit_expression`] classification against the new
/// release: the in-crate conformance module
/// (`component_scope_projection_conformance_tests.rs`) reads this constant and asserts
/// it equals the `svelte` version pinned in `pnpm-lock.yaml`, fingerprints the vendored
/// `remove_typescript_nodes` handler bodies as a drift tripwire, and — through the
/// `HandlerCoverage` rail — asserts a bijection between svelte's handler inventory and
/// the committed name-parity corpus (every handler mapped to ≥1 corpus axis exercised by
/// the production projection). It is a committed tooling anchor, not linked at runtime.
#[allow(dead_code)]
pub(super) const SVELTE_ORACLE_VERSION: &str = "5.56.3";

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
/// scope view ([`SvelteScopeProjection`]) IN THE SAME arena — no second parse — and
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
    // PLACE (same arena), then bind the projected program.
    SvelteScopeProjection {
        ast: AstBuilder::new(alloc),
    }
    .visit_program(&mut program);
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

/// The svelte SCOPE-VIEW projection: an in-place, single-arena rewrite of the
/// reparsed program that mirrors svelte@[`SVELTE_ORACLE_VERSION`]'s
/// `remove_typescript_nodes ∘ create_scopes` erasure, so binding the result with
/// `SemanticBuilder` yields svelte's exact runtime scope surface.
///
/// It ERASES (→ [`Statement::EmptyStatement`], which binds nothing) every statement
/// [`statement_is_scope_erased`] classifies as scope-erased, and UNWRAPS the five TS
/// expression carriers to their inner runtime expression. Every other node is kept
/// and recursed, so nested erasure / unwrap (inside blocks, function bodies,
/// initializers) is handled. It NEVER re-parses and allocates only in the borrowed
/// arena.
struct SvelteScopeProjection<'a> {
    ast: AstBuilder<'a>,
}

impl<'a> VisitMut<'a> for SvelteScopeProjection<'a> {
    fn visit_statement(&mut self, stmt: &mut Statement<'a>) {
        if statement_is_scope_erased(stmt) {
            // Erased: replace with an empty statement (no binding), do NOT recurse —
            // an erased declaration contributes nothing to the scope view.
            *stmt = self.ast.statement_empty(stmt.span());
            return;
        }
        walk_mut::walk_statement(self, stmt);
    }

    fn visit_expression(&mut self, expr: &mut Expression<'a>) {
        // EXHAUSTIVE over `Expression` — the expression-level drift rail. The five TS
        // wrapper expressions UNWRAP to their inner runtime expression (svelte erases
        // the type carrier); every other expression is KEPT and recursed. NO wildcard
        // for a TS node kind — a new OXC expression variant breaks the build.
        let unwrapped: Option<Expression<'a>> = match expr {
            Expression::TSAsExpression(e) => Some(e.expression.take_in(self.ast.allocator)),
            Expression::TSSatisfiesExpression(e) => Some(e.expression.take_in(self.ast.allocator)),
            Expression::TSTypeAssertion(e) => Some(e.expression.take_in(self.ast.allocator)),
            Expression::TSNonNullExpression(e) => Some(e.expression.take_in(self.ast.allocator)),
            Expression::TSInstantiationExpression(e) => {
                Some(e.expression.take_in(self.ast.allocator))
            }
            Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::TemplateLiteral(_)
            | Expression::Identifier(_)
            | Expression::MetaProperty(_)
            | Expression::Super(_)
            | Expression::ArrayExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::AssignmentExpression(_)
            | Expression::AwaitExpression(_)
            | Expression::BinaryExpression(_)
            | Expression::CallExpression(_)
            | Expression::ChainExpression(_)
            | Expression::ClassExpression(_)
            | Expression::ConditionalExpression(_)
            | Expression::FunctionExpression(_)
            | Expression::ImportExpression(_)
            | Expression::LogicalExpression(_)
            | Expression::NewExpression(_)
            | Expression::ObjectExpression(_)
            | Expression::ParenthesizedExpression(_)
            | Expression::SequenceExpression(_)
            | Expression::TaggedTemplateExpression(_)
            | Expression::ThisExpression(_)
            | Expression::UnaryExpression(_)
            | Expression::UpdateExpression(_)
            | Expression::YieldExpression(_)
            | Expression::PrivateInExpression(_)
            | Expression::JSXElement(_)
            | Expression::JSXFragment(_)
            | Expression::V8IntrinsicExpression(_)
            | match_member_expression!(Expression) => None,
        };
        match unwrapped {
            Some(inner) => {
                *expr = inner;
                // The unwrapped inner may itself be a wrapper (`x as A as B`) or hold
                // further TS to erase / unwrap — re-visit it.
                self.visit_expression(expr);
            }
            None => walk_mut::walk_expression(self, expr),
        }
    }

    fn visit_class_body(&mut self, body: &mut ClassBody<'a>) {
        // ERASE the TS class members svelte's `ClassBody` / `MethodDefinition` handlers
        // remove BEFORE binding, so OXC never binds an abstract-method parameter, visits
        // a `declare` field's computed key, or binds a `declare`/type-only member. A
        // physical removal (there is no empty class element) — kept members are then
        // recursed by the walk below (nested erasure inside method bodies + computed keys
        // of KEPT members, which are real references).
        body.body
            .retain(|element| !class_element_is_scope_erased(element));
        walk_mut::walk_class_body(self, body);
    }

    fn visit_formal_parameters(&mut self, params: &mut FormalParameters<'a>) {
        // Drop ctor param-properties (`constructor(public/private/protected/readonly X)`)
        // so their name is not bound. svelte HARD-ERRORS on these (reject-parity is
        // tracked cat-4 debt); for scope the name must not reserve. A plain param (no
        // modifier) is untouched and stays bound.
        params
            .items
            .retain(|param| !formal_parameter_is_scope_erased(param));
        walk_mut::walk_formal_parameters(self, params);
    }
}

/// Whether a statement is ERASED from svelte's scope view (svelte emits no runtime
/// binding for it), mirroring svelte@[`SVELTE_ORACLE_VERSION`]'s
/// `remove_typescript_nodes` handlers plus its `create_scopes` scope-inert set.
///
/// EXHAUSTIVE over OXC's `Statement` (through the inherited `Declaration` /
/// `ModuleDeclaration` variants) — NO wildcard for a TS node kind, so a new OXC
/// statement/declaration variant breaks the build and forces reclassification. The
/// committed drift guard ties this classification to the pinned svelte release.
fn statement_is_scope_erased(stmt: &Statement<'_>) -> bool {
    match stmt {
        // Runtime value declarations survive UNLESS ambient (`declare`) or, for a
        // function, a lone bodiless overload signature (`function f(): void;`, OXC
        // `Function { body: None }`, svelte's `TSDeclareFunction` → empty).
        Statement::VariableDeclaration(d) => d.declare,
        Statement::FunctionDeclaration(f) => f.declare || f.body.is_none(),
        Statement::ClassDeclaration(c) => c.declare,
        // Pure TS declarations svelte's `remove_typescript_nodes` erases, plus the
        // scope-INERT forms `create_scopes` declares nothing for (`import X =
        // require(...)`, `export = X`, `export as namespace X`).
        Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSEnumDeclaration(_)
        | Statement::TSModuleDeclaration(_)
        | Statement::TSGlobalDeclaration(_)
        | Statement::TSImportEqualsDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => true,
        // Module declarations: a whole-statement type-only `import`/`export *` is
        // erased; the runtime forms are kept (mixed-specifier `type` members bind as
        // type-only imports and are dropped by the value filter, not here).
        Statement::ImportDeclaration(i) => i.import_kind.is_type(),
        Statement::ExportAllDeclaration(e) => e.export_kind.is_type(),
        Statement::ExportDefaultDeclaration(_) => false,
        Statement::ExportNamedDeclaration(e) => export_named_is_scope_erased(e),
        // Runtime control-flow / expression statements — always kept (recursed for
        // nested erasure / unwrap).
        Statement::BlockStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ContinueStatement(_)
        | Statement::DebuggerStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::EmptyStatement(_)
        | Statement::ExpressionStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_)
        | Statement::ForStatement(_)
        | Statement::IfStatement(_)
        | Statement::LabeledStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::SwitchStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::TryStatement(_)
        | Statement::WhileStatement(_)
        | Statement::WithStatement(_) => false,
    }
}

/// Whether an `export … ` named declaration is erased from the scope view. A
/// whole-statement `export type { … }` is erased; an `export <decl>` is erased iff
/// the inner declaration is (svelte reduces `export interface` / `export type` /
/// `export enum` / `export declare …` to empty); an `export { a, type b }` specifier
/// list is kept (the value specifiers survive; `type` specifiers bind as type-only).
fn export_named_is_scope_erased(export: &ExportNamedDeclaration<'_>) -> bool {
    if export.export_kind.is_type() {
        return true;
    }
    if let Some(declaration) = &export.declaration {
        return declaration_is_scope_erased(declaration);
    }
    false
}

/// Whether a `Declaration` (in statement position or nested under `export`) is
/// erased from the scope view. EXHAUSTIVE over OXC's `Declaration` — NO wildcard —
/// so a new declaration variant breaks the build; mirrors the declaration arms of
/// [`statement_is_scope_erased`].
fn declaration_is_scope_erased(declaration: &Declaration<'_>) -> bool {
    match declaration {
        Declaration::VariableDeclaration(d) => d.declare,
        Declaration::FunctionDeclaration(f) => f.declare || f.body.is_none(),
        Declaration::ClassDeclaration(c) => c.declare,
        Declaration::TSTypeAliasDeclaration(_)
        | Declaration::TSInterfaceDeclaration(_)
        | Declaration::TSEnumDeclaration(_)
        | Declaration::TSModuleDeclaration(_)
        | Declaration::TSGlobalDeclaration(_)
        | Declaration::TSImportEqualsDeclaration(_) => true,
    }
}

/// Whether a class member is ERASED from svelte's scope view, mirroring
/// svelte@[`SVELTE_ORACLE_VERSION`]'s `ClassBody` / `MethodDefinition` /
/// `PropertyDefinition` handlers. EXHAUSTIVE over OXC's `ClassElement` — and, at every
/// TS-carrier sub-enum (`MethodDefinitionType`, `PropertyDefinitionType`,
/// `AccessorPropertyType`), an EXPLICIT match rather than a `matches!` soft-wildcard, so
/// a NEW OXC member OR member-subtype variant breaks the build and forces
/// reclassification instead of being silently kept.
fn class_element_is_scope_erased(element: &ClassElement<'_>) -> bool {
    match element {
        // Runtime static initializer block — KEEP (svelte keeps it; it binds locals).
        ClassElement::StaticBlock(_) => false,
        // An abstract method is erased whole (params + computed key gone); a normal /
        // constructor method is KEPT and recursed.
        ClassElement::MethodDefinition(method) => match method.r#type {
            MethodDefinitionType::TSAbstractMethodDefinition => true,
            MethodDefinitionType::MethodDefinition => false,
        },
        // A `declare` field is dropped by svelte's `ClassBody` (its computed key /
        // initializer / type ref never visited). An abstract (non-declare) field and a
        // normal field are KEPT: svelte visits their computed keys (real references) and
        // the value-position filter drops their type refs. The `declare` disposition is
        // orthogonal to the subtype — an explicit match over `PropertyDefinitionType`
        // documents that both subtypes route through the same `declare` decision (and a
        // new subtype breaks the build).
        ClassElement::PropertyDefinition(property) => match property.r#type {
            PropertyDefinitionType::PropertyDefinition
            | PropertyDefinitionType::TSAbstractPropertyDefinition => property.declare,
        },
        // An `accessor` field (either subtype) is a svelte HARD ERROR
        // (`typescript_invalid_feature`); for scope the projection drops it (reject-parity
        // is tracked cat-4 debt).
        ClassElement::AccessorProperty(accessor) => match accessor.r#type {
            AccessorPropertyType::AccessorProperty
            | AccessorPropertyType::TSAbstractAccessorProperty => true,
        },
        // A type-only index signature (`[key: string]: T`) binds nothing — ERASE.
        ClassElement::TSIndexSignature(_) => true,
    }
}

/// Whether a formal parameter is a ctor param-property to DROP from the scope view. A
/// `public`/`private`/`protected`/`readonly` modifier makes it a param-property (a
/// svelte hard error); a plain parameter has neither and stays bound. The
/// `accessibility` decision is an EXHAUSTIVE match over `TSAccessibility` (no soft
/// `is_some()`) so a future accessibility variant forces a reclassification instead of
/// silently dropping the parameter.
fn formal_parameter_is_scope_erased(param: &oxc_ast::ast::FormalParameter<'_>) -> bool {
    param.readonly
        || match param.accessibility {
            None => false,
            Some(
                TSAccessibility::Public | TSAccessibility::Private | TSAccessibility::Protected,
            ) => true,
        }
}

/// Run ONLY the svelte scope-view projection over `source` and return the mutated
/// program, for AST-shape assertions (e.g. that TS expression wrappers are gone). The
/// same `reparse_module` + `SvelteScopeProjection` the analysis path uses, without the
/// subsequent `SemanticBuilder` binding.
#[cfg(test)]
pub(super) fn project_source_for_test<'a>(
    alloc: &'a Allocator,
    source: &str,
) -> Option<oxc_ast::ast::Program<'a>> {
    let mut program = reparse_module(alloc, source)?;
    SvelteScopeProjection {
        ast: AstBuilder::new(alloc),
    }
    .visit_program(&mut program);
    Some(program)
}

#[cfg(test)]
#[path = "component_scope_facts_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "component_scope_projection_conformance_tests.rs"]
mod projection_conformance_tests;
