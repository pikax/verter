//! The PER-SURFACE legacy-mode (non-runes) dispatch gate.
//!
//! A LEGACY component is not refused wholesale: the mode-independent
//! `$store` auto-subscription surface (imports + store-source consts +
//! admitted functions + template store reads/writes), the legacy `<slot>`
//! outlet, and the `createEventDispatcher` component-event surface all flow
//! through the shared default-deny pipeline. The ONE remaining legacy gate
//! here is a rune NAME referenced under legacy mode
//! ([`UnsupportedSvelteRuntimeSurface::LegacyRuneReference`] — under
//! `runes={false}` a rune name is NOT a rune: official parses `$state` as a
//! STORE subscription and lowers `let` state through `$.mutable_source`,
//! semantics this backend must not mis-emit as runes).
//!
//! (An instance-script `export let` is the SUPPORTED legacy prop surface — it
//! lowers through the shared `$.prop` prop-source substrate — and a `$:`
//! reactive statement the SUPPORTED legacy reactivity surface — it lowers
//! through `$.legacy_pre_effect`; the other export forms fail closed at the
//! instance-script item allowlist with their own identities.)
//!
//! This module ALSO owns the RUNES-side twin gate
//! ([`refuse_runes_mode_legacy_script_constructs`]): under the FINAL lowered
//! runes mode an `export let` / `$:` statement is an OFFICIAL compile error
//! (`legacy_export_invalid` / `legacy_reactive_statement_invalid`). The
//! pre-lowering official-reject gate already rejects the explicit and
//! script-inferred runes cases; this classifier-side gate re-applies the same
//! rejects against the FINAL mode (which completes only after lowering — the
//! template-`$host` inference term), so a runes-mode `export let` / `$:` can
//! NEVER fall through to legacy lowering.
//!
//! Every check is structural — the typed IR and the parsed OXC program —
//! never a name-suffix or source-text sniff.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, Statement};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use rustc_hash::FxHashSet;

use super::expr::{
    arrow_scope_names, block_scope_names, collect_direct_decls, collect_var_hoists,
    function_scope_names, reparse_module, ShadowStack,
};
use super::ir::SvelteRuntimeIr;
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

    // (An instance `export` / `$:` label is NOT refused here: `export let` is
    // the SUPPORTED legacy prop surface and `$:` the SUPPORTED legacy
    // reactive-statement surface — both classified by the instance-script item
    // allowlist, where every other export/label form fails closed with its own
    // identity.)

    Ok(())
}

/// Refuse the RUNES-mode legacy-script constructs against the FINAL lowered mode:
/// an `export let` declaration (ANY binding pattern) is the official
/// `legacy_export_invalid` compile error and a `$:` labeled statement the official
/// `legacy_reactive_statement_invalid` — in instance-program SOURCE order (the
/// official analyze-walk order). Returned through the
/// [`UnsupportedSvelteRuntimeSurface::OfficialReject`] carrier, which the compile
/// boundary converts to a real `ClientCompileError::OfficialReject` — never an
/// unsupported-feature diagnostic. The caller gates this on `SvelteMode::Runes`;
/// it is never consulted for a legacy component. It is the airtight twin of the
/// pre-lowering official-reject scan: the pre-lowering gate covers the explicit
/// and script-inferred runes cases, THIS gate covers the residual final-mode
/// inference (the template-`$host` term) so a runes-mode `export let` / `$:` can
/// never reach legacy lowering.
pub(super) fn refuse_runes_mode_legacy_script_constructs(
    ir: &SvelteRuntimeIr,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    use super::official_rule::{CoreOfficialValidationRule, OfficialRejection};
    let Some(instance) = ir.analysis.scripts.instance_source else {
        return Ok(());
    };
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, instance) else {
        return Ok(());
    };
    for stmt in &program.body {
        match stmt {
            Statement::ExportNamedDeclaration(export)
                if matches!(
                    export.declaration,
                    Some(oxc_ast::ast::Declaration::VariableDeclaration(ref decl))
                        if decl.kind == oxc_ast::ast::VariableDeclarationKind::Let
                ) =>
            {
                let span = stmt.span();
                return Err(UnsupportedSvelteRuntimeSurface::OfficialReject {
                    rejection: OfficialRejection::of(
                        CoreOfficialValidationRule::LegacyExportInvalid,
                    ),
                    span: Span::new(span.start, span.end),
                });
            }
            Statement::LabeledStatement(labeled) if labeled.label.name == "$" => {
                let span = stmt.span();
                return Err(UnsupportedSvelteRuntimeSurface::OfficialReject {
                    rejection: OfficialRejection::of(
                        CoreOfficialValidationRule::LegacyReactiveStatementInvalid,
                    ),
                    span: Span::new(span.start, span.end),
                });
            }
            _ => {}
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
