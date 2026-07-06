//! The PER-SURFACE legacy-mode (non-runes) dispatch gate.
//!
//! A LEGACY component is no longer refused wholesale: the mode-independent
//! `$store` auto-subscription surface (imports + store-source consts +
//! admitted functions + template store reads/writes) flows through the shared
//! default-deny pipeline, and each NOT-yet-lowered legacy surface fails closed
//! HERE with its own narrow diagnostic instead of a blanket mode refusal:
//!
//! - a rune NAME referenced under legacy mode ([`UnsupportedSvelteRuntimeSurface::LegacyRuneReference`]
//!   — under `runes={false}` a rune name is NOT a rune: official parses
//!   `$state` as a STORE subscription and lowers `let` state through
//!   `$.mutable_source`, semantics this backend must not mis-emit as runes);
//! - an instance-script `export` declaration ([`UnsupportedSvelteRuntimeSurface::LegacyExportProp`]
//!   — the legacy prop surface);
//! - a `$:` reactive statement ([`UnsupportedSvelteRuntimeSurface::LegacyReactiveStatement`]);
//! - a `createEventDispatcher` usage ([`UnsupportedSvelteRuntimeSurface::LegacyEventDispatcher`]);
//! - a `<slot>` element ([`UnsupportedSvelteRuntimeSurface::LegacySlotElement`]).
//!
//! Every check is structural — the typed IR, the parsed OXC program, and the
//! shared [`ClassifiedScriptImports`](super::client_surface_imports::ClassifiedScriptImports)
//! carrier (the `createEventDispatcher` referent is the ADMITTED import local
//! whose imported name is `createEventDispatcher` from the `svelte` module —
//! never a name-suffix or source-text sniff).

use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, Statement};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use rustc_hash::FxHashSet;

use super::client_imports::{ImportName, UserImportSlot, UserImportSpecifier};
use super::expr::{
    arrow_scope_names, block_scope_names, collect_direct_decls, collect_var_hoists,
    function_scope_names, reparse_module, ShadowStack,
};
use super::ir::{IrNode, SvelteRuntimeIr};
use super::rune_scan::RUNE_ROOT_NAMES;
use super::unsupported::UnsupportedSvelteRuntimeSurface;
use verter_span::Span;

/// Refuse the NOT-yet-lowered legacy surfaces of a LEGACY (non-runes) component,
/// each with its narrow per-surface diagnostic. `Ok(())` when none is present —
/// the component continues through the shared default-deny pipeline (whose own
/// gates keep every remaining unsupported shape fail-closed). The caller gates
/// this on `SvelteMode::Legacy`; it is never consulted for a runes component.
///
/// `store_exempt` is the rune-root ACCESSOR exemption set (see
/// [`rune_root_accessor_exemptions`](super::store_subscriptions::rune_root_accessor_exemptions)):
/// a `$state` whose base `state` is a declared store candidate is a STORE
/// ACCESSOR reference — official emits the subscription under legacy mode — so
/// the rune-reference gate skips it (the scope-aware store classifier, which
/// runs BEFORE this gate, owns its accept/scoped-reject decision).
pub(super) fn refuse_unsupported_legacy_surfaces(
    ir: &SvelteRuntimeIr,
    store_exempt: &FxHashSet<String>,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let instance_source = ir.analysis.scripts.instance_source;

    // (1) A rune NAME referenced under legacy mode — scripts (scope-aware: a
    // shadowing local is not a rune reference) plus every analyzed template
    // expression (the per-expression reference facts are already binder-pruned).
    // A store-exempt accessor name is not a rune reference in either position.
    for source in [instance_source, ir.analysis.scripts.module_source]
        .into_iter()
        .flatten()
    {
        if let Some(program) = reparse_module(&alloc, source) {
            let mut scan = LegacyRuneRefScan {
                scopes: ShadowStack::default(),
                found: None,
                store_exempt,
            };
            scan.visit_program(&program);
            if let Some((rune, span)) = scan.found {
                return Err(UnsupportedSvelteRuntimeSurface::LegacyRuneReference { rune, span });
            }
        }
    }
    for expr in ir.analysis.expressions.all() {
        for reference in &expr.references {
            if RUNE_ROOT_NAMES.contains(&reference.name.as_str())
                && !store_exempt.contains(&reference.name)
            {
                return Err(UnsupportedSvelteRuntimeSurface::LegacyRuneReference {
                    rune: reference.name.clone(),
                    span: Span::new(0, 0),
                });
            }
        }
    }

    // (2)+(3) Instance-script top-level walk: an `export` declaration (the legacy
    // prop surface) and a `$:` reactive label, refused in SOURCE order.
    if let Some(instance) = instance_source {
        if let Some(program) = reparse_module(&alloc, instance) {
            for stmt in &program.body {
                match stmt {
                    Statement::ExportNamedDeclaration(_)
                    | Statement::ExportDefaultDeclaration(_)
                    | Statement::ExportAllDeclaration(_) => {
                        let span = stmt.span();
                        return Err(UnsupportedSvelteRuntimeSurface::LegacyExportProp {
                            span: Span::new(span.start, span.end),
                        });
                    }
                    Statement::LabeledStatement(labeled) if labeled.label.name == "$" => {
                        let span = stmt.span();
                        return Err(UnsupportedSvelteRuntimeSurface::LegacyReactiveStatement {
                            span: Span::new(span.start, span.end),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // (4) A `createEventDispatcher` usage: the referent is the ADMITTED import
    // local whose IMPORTED name is `createEventDispatcher` from the `svelte`
    // module (read from the shared import carrier); any unshadowed instance
    // reference to that local is the legacy component-event surface.
    let mut dispatcher_locals: FxHashSet<String> = FxHashSet::default();
    for slot in [UserImportSlot::Module, UserImportSlot::Instance] {
        for import in ir.analysis.script_imports.admitted(slot) {
            if import.source != "svelte" {
                continue;
            }
            for spec in &import.specifiers {
                if let UserImportSpecifier::Named { imported, local } = spec {
                    if matches!(imported, ImportName::Ident(name) if name == "createEventDispatcher")
                    {
                        dispatcher_locals.insert(local.clone());
                    }
                }
            }
        }
    }
    if !dispatcher_locals.is_empty() {
        if let Some(instance) = instance_source {
            if let Some(program) = reparse_module(&alloc, instance) {
                let mut scan = NamedRefScan {
                    names: &dispatcher_locals,
                    scopes: ShadowStack::default(),
                    found: None,
                };
                scan.visit_program(&program);
                if let Some(span) = scan.found {
                    return Err(UnsupportedSvelteRuntimeSurface::LegacyEventDispatcher { span });
                }
            }
        }
    }

    // (5) A `<slot>` element — the legacy slot surface.
    for node in &ir.nodes {
        if let IrNode::Element(el) = node {
            if el.tag == "slot" {
                return Err(UnsupportedSvelteRuntimeSurface::LegacySlotElement { span: el.span });
            }
        }
    }

    Ok(())
}

/// A scope-aware scan for the FIRST unshadowed rune-name reference (name + span).
/// A STORE-EXEMPT accessor name (its base is a declared store candidate) is a
/// store accessor, not a rune reference — skipped.
struct LegacyRuneRefScan<'e> {
    scopes: ShadowStack,
    found: Option<(String, Span)>,
    store_exempt: &'e FxHashSet<String>,
}

impl<'a> Visit<'a> for LegacyRuneRefScan<'_> {
    fn visit_program(&mut self, it: &Program<'a>) {
        let mut frame = FxHashSet::default();
        collect_direct_decls(&it.body, &mut frame);
        collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        self.scopes.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.scopes.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        if self.found.is_none() {
            let name = it.name.as_str();
            if RUNE_ROOT_NAMES.contains(&name)
                && !self.store_exempt.contains(name)
                && !self.scopes.is_shadowed(name)
            {
                self.found = Some((name.to_string(), Span::new(it.span.start, it.span.end)));
            }
        }
        walk::walk_identifier_reference(self, it);
    }
}

/// A scope-aware scan for the FIRST unshadowed reference to any of `names`.
struct NamedRefScan<'a> {
    names: &'a FxHashSet<String>,
    scopes: ShadowStack,
    found: Option<Span>,
}

impl<'a> Visit<'a> for NamedRefScan<'_> {
    fn visit_program(&mut self, it: &Program<'a>) {
        let mut frame = FxHashSet::default();
        collect_direct_decls(&it.body, &mut frame);
        collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        self.scopes.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.scopes.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        if self.found.is_none() {
            let name = it.name.as_str();
            if self.names.contains(name) && !self.scopes.is_shadowed(name) {
                self.found = Some(Span::new(it.span.start, it.span.end));
            }
        }
        walk::walk_identifier_reference(self, it);
    }
}
