//! Script-facing classification for the DEFAULT-DENY client syntax classifier
//! ([`super::client_surface`]): the `$props()` usage gate
//! ([`classify_props_usage`] — instance-script prop references and prop
//! `bind:` targets fail closed; TEMPLATE prop reads/writes and plain /
//! `$bindable` destructure defaults are supported through the `$.prop`
//! substrate), the instance/module script-item allowlist
//! ([`classify_script_items`]), and the scope-aware unsupported-rune-form scan
//! they drive. Every gate fails closed — an unrecognised script surface is a
//! typed [`UnsupportedSvelteRuntimeSurface`] refusal, never a pass.

use oxc_allocator::Allocator;

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_plan_types::UserImport;
use super::client_shapes::{self, ClientPropsUsage};
use super::expr::BindingRuntimeKind;
use super::expr_emit::{self, PropsShape, StateDeclShape};
use super::instance_items;
use super::ir::{AttrIr, IrNode, SvelteRuntimeIr};
use verter_span::Span;

/// Classify the `$props()` USAGE: prop reads/writes are supported in TEMPLATE
/// expressions (a template write makes the prop a PROP SOURCE, lowered through
/// the getter/setter), but an INSTANCE-SCRIPT reference to a prop local outside
/// its own `$props()` declaration, or a `bind:` target resolving to a prop (the
/// official 2-arg `$.bind_value(input, label)` form), fails closed. Returns the
/// [`ClientPropsUsage`] fact when the usage is inside the supported boundary.
///
/// Prop locals are resolved SCOPE-AWARELY through the binding table: a reference
/// to a SHADOWING local of the same name (an arrow param) is not a prop usage.
pub(super) fn classify_props_usage(
    ir: &SvelteRuntimeIr,
) -> Result<ClientPropsUsage, UnsupportedSvelteRuntimeSurface> {
    let prop_locals = client_shapes::collect_prop_locals(ir.analysis.scripts.instance_source);
    if prop_locals.is_empty() {
        return Ok(ClientPropsUsage { prop_locals });
    }

    // (a) Instance-script prop REFERENCES — the supported prop read position is a
    // template expression ONLY. ANY instance-script reference to a prop local
    // outside its own `$props()` declaration (a read `cb()` / `console.log(a)`,
    // a write `a += 1`, a mutating call) is the fail-closed non-interpolation
    // prop-usage surface. Observed structurally by scanning every NON-declaration
    // instance statement for a reference resolving to a prop binding. (A sibling
    // reference INSIDE the `$props()` declaration — a default reading another
    // prop — is part of the declaration and stays supported.)
    if let Some(instance) = ir.analysis.scripts.instance_source {
        if instance_script_references_a_prop(instance, ir) {
            return Err(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$props() non-interpolation usage",
                span: Span::new(0, 0),
            });
        }
    }

    // (b) `bind:` targets — a `bind:value={prop}` resolves to a prop local (the
    // bound prop is official's 2-arg `$.bind_value` form — a fail-closed
    // follow-up surface). The
    // attribute walk also catches this (via `classify_bind_shape`); this top-level
    // sweep keeps the prop-bind refusal owned by the prop-usage gate so a bound prop
    // is refused even when its element is otherwise unsupported-adjacent.
    for node in &ir.nodes {
        let IrNode::Element(el) = node else {
            continue;
        };
        for attr in &el.attrs {
            let AttrIr::Bind {
                target,
                expr: Some(expr_id),
            } = attr
            else {
                continue;
            };
            if target != "value" {
                continue;
            }
            let analyzed = ir.analysis.expressions.get(*expr_id);
            // A bare-identifier bind target that resolves to a prop is a bound prop.
            if resolves_to_prop(ir, analyzed.scope, analyzed.source.trim()) {
                return Err(UnsupportedSvelteRuntimeSurface::Binding {
                    target: target.clone(),
                    span: el.span,
                });
            }
        }
    }

    Ok(ClientPropsUsage { prop_locals })
}

/// Whether `name` resolves (scope-awarely, nearest binding up the chain) to a
/// `$props()` prop binding in `scope`.
fn resolves_to_prop(ir: &SvelteRuntimeIr, scope: super::expr::ScopeId, name: &str) -> bool {
    matches!(
        ir.analysis
            .bindings
            .resolve_kind(&ir.analysis.scopes, scope, name),
        Some(BindingRuntimeKind::Prop) | Some(BindingRuntimeKind::BindableProp)
    )
}

/// Whether the instance script REFERENCES (reads or writes) a `$props()` prop local
/// anywhere outside its own `$props()` declaration. The supported prop usage
/// positions are TEMPLATE expressions only, so any instance-script prop reference
/// (a read `cb()` / `console.log(a)`, a write `a += 1`) fails the prop gate.
///
/// Reparses the instance program ONCE and walks it with a scope-aware visitor that
/// SKIPS the `$props()` declarator subtrees (they BIND the prop, they do not read
/// it) and reports any identifier reference resolving to a prop binding. A reference
/// to a shadowing local of the same name is not a prop reference (the walk reuses the
/// shared `ShadowStack` lexical model).
fn instance_script_references_a_prop(instance_source: &str, ir: &SvelteRuntimeIr) -> bool {
    let alloc = Allocator::default();
    let Some(program) = super::expr::reparse_module(&alloc, instance_source) else {
        return false;
    };
    // The prop-local names declared at the instance root.
    let prop_locals: rustc_hash::FxHashSet<String> =
        client_shapes::collect_prop_locals(Some(instance_source))
            .into_iter()
            .collect();
    if prop_locals.is_empty() {
        return false;
    }
    let mut scan = PropRefScan {
        prop_locals: &prop_locals,
        scopes: super::expr::ShadowStack::default(),
        found: false,
    };
    use oxc_ast_visit::Visit;
    scan.visit_program(&program);
    let _ = ir;
    scan.found
}

/// A scope-aware scan for an instance-script reference to a `$props()` prop local
/// outside its declaration. Tracks the shared `ShadowStack` lexical model (so a
/// nested local shadowing a prop name is not a prop reference) and skips a
/// `$props()` declarator's init/pattern (the destructure binds, it does not read).
struct PropRefScan<'a> {
    prop_locals: &'a rustc_hash::FxHashSet<String>,
    scopes: super::expr::ShadowStack,
    found: bool,
}

impl<'a> oxc_ast_visit::Visit<'a> for PropRefScan<'_> {
    fn visit_program(&mut self, it: &oxc_ast::ast::Program<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        super::expr::collect_direct_decls(&it.body, &mut frame);
        super::expr::collect_var_hoists(&it.body, &mut frame);
        // The prop locals are declared at THIS scope; remove them from the frame so a
        // prop reference is not treated as shadowed by its own declaration.
        for name in self.prop_locals {
            frame.remove(name);
        }
        self.scopes.push(frame);
        oxc_ast_visit::walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_variable_declarator(&mut self, it: &oxc_ast::ast::VariableDeclarator<'a>) {
        // Skip a `$props()` declarator entirely (the destructure pattern + the
        // `$props()` callee are not a prop READ). Any OTHER declarator is walked.
        if let Some(oxc_ast::ast::Expression::CallExpression(call)) = &it.init {
            if super::expr::is_props_callee(&call.callee) {
                return;
            }
        }
        oxc_ast_visit::walk::walk_variable_declarator(self, it);
    }

    fn visit_function(
        &mut self,
        it: &oxc_ast::ast::Function<'a>,
        flags: oxc_syntax::scope::ScopeFlags,
    ) {
        self.scopes.push(super::expr::function_scope_names(it));
        oxc_ast_visit::walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.scopes.push(super::expr::arrow_scope_names(it));
        oxc_ast_visit::walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.scopes.push(super::expr::block_scope_names(it));
        oxc_ast_visit::walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        let name = it.name.as_str();
        if self.prop_locals.contains(name) && !self.scopes.is_shadowed(name) {
            self.found = true;
        }
        oxc_ast_visit::walk::walk_identifier_reference(self, it);
    }
}

/// Classify the instance + module script items, returning the admitted module-scope
/// user imports (the `.svelte`-component-default subset). A `<script module>`,
/// every NON-`.svelte`-default instance import (named / namespace / side-effect /
/// mixed / default-non-`.svelte`), a non-basic / default-bearing `$props()` form, a
/// destructured / non-primitive `$state`, or an advanced rune call/member fails closed
/// (no wildcard accept).
pub(super) fn classify_script_items(
    ir: &SvelteRuntimeIr,
) -> Result<Vec<UserImport>, UnsupportedSvelteRuntimeSurface> {
    // A `<script module>` is the broad static-import-prelude / module-item deferral —
    // fail closed BEFORE the rune-shape gates. (The component `.svelte` default import
    // is admitted below; every OTHER static import form — named / namespace /
    // side-effect / non-`.svelte` default — plus arbitrary `<script module>` items stay
    // closed until the broad script-import prelude is supported.)
    if let Some(module) = ir.analysis.scripts.module_source {
        let _ = module;
        return Err(UnsupportedSvelteRuntimeSurface::ScriptImport {
            construct: "module script",
            span: Span::new(0, 0),
        });
    }
    // Instance-script imports: ADMIT a default `.svelte` component import (hoisted to
    // module scope as the component callee), REFUSE every other form (not yet supported).
    let user_imports = if let Some(instance) = ir.analysis.scripts.instance_source {
        super::client_surface_imports::classify_instance_imports(instance)?
    } else {
        Vec::new()
    };
    // A non-basic `$props()` form (rest / whole-object / computed / numeric /
    // nested destructure) is an advanced rune form that fails closed (defaults —
    // plain and `$bindable` — are part of the basic prop-source surface).
    if let Some(instance) = ir.analysis.scripts.instance_source {
        // A NON-`let` rune declarator (`var`/`const` `$state` / `$derived` /
        // `$props`) is a distinct official surface (`var` reads use `$.safe_get`; a
        // read-only `const $state` constant-folds to an empty reactive topology) —
        // fail closed BEFORE the shape / static-interpolation checks, so a
        // `const c = $state(0)` read fails at the decl-kind gate, not as a
        // const-fold (the const-fold sub-contract).
        client_shapes::classify_rune_declaration_kind(instance)?;
        match expr_emit::props_shape(instance) {
            PropsShape::None | PropsShape::BasicDestructure => {}
            PropsShape::Advanced { rune } => {
                return Err(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune,
                    span: Span::new(0, 0),
                });
            }
        }
        // A DESTRUCTURED or NON-PRIMITIVE `$state` declarator (ANY declarator across
        // ALL statements, multi-declarator scanned) is the advanced state form (5g)
        // — fail closed before lowering so the primitive-identifier lowering never
        // sees a destructure or a deep-reactive proxy init.
        match expr_emit::state_decl_shape(instance) {
            StateDeclShape::None | StateDeclShape::Identifier => {}
            StateDeclShape::Advanced { rune } => {
                return Err(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune,
                    span: Span::new(0, 0),
                });
            }
        }
    }

    // A scope-aware, POSITION-SENSITIVE scan over the instance script. It has
    // supported rune positions (a top-level `$state` / `$props()` declarator init);
    // `$derived` / `$effect` have NONE (they are refused entirely). The scan also
    // refuses an advanced FORM (`$state.snapshot` / `$effect.pre` / `$host` /
    // `$props.id`) the binding classifier does not see as a top-level declarator.
    // The `$inspect` family is NOT refused here — it is production-ELIDED (the
    // instance-item classifier / the body rewriter own the elision, and the
    // rewriter fails a non-statement-position reference closed). A SHADOWED rune
    // name is never refused. (A `<script module>` was already refused above as a
    // script-hoisting deferral.)
    let mut alloc = Allocator::default();
    if let Some(instance) = ir.analysis.scripts.instance_source {
        if let Some(reason) = scan_unsupported_rune_forms(&alloc, instance, true) {
            return Err(reason);
        }
        alloc.reset();
    }
    // The SAME scan over every analyzed TEMPLATE expression (an interpolation /
    // handler / bind expression) — an unsupported rune inside an expression
    // (`{$state.snapshot(x)}`) must fail closed too. A template expression hosts no
    // supported rune position (`is_instance = false`).
    for expr in ir.analysis.expressions.all() {
        let wrapped = format!("({});", expr.source);
        if let Some(reason) = scan_unsupported_rune_forms(&alloc, &wrapped, false) {
            return Err(reason);
        }
        alloc.reset();
    }
    // A compiler-MAGIC identifier (`$$slots` / `$$props` / `$$restProps`) is an
    // auto-injected legacy object; a raw reference in the runes client output binds
    // an undefined identifier (a `ReferenceError`). Scan the instance script AND every
    // template expression (a shadowing local of the same name is not a magic ref); the
    // precise `MagicIdentifier` diagnostic wins over the generic instance-script-item
    // refusal the allowlist would otherwise produce.
    if let Some(instance) = ir.analysis.scripts.instance_source {
        if let Some(reason) = instance_items::scan_magic_identifiers(instance) {
            return Err(reason);
        }
    }
    for expr in ir.analysis.expressions.all() {
        let wrapped = format!("({});", expr.source);
        if let Some(reason) = instance_items::scan_magic_identifiers(&wrapped) {
            return Err(reason);
        }
    }
    Ok(user_imports)
}

/// Scope-aware, POSITION-SENSITIVE scan of a script for an UNSUPPORTED rune form or
/// position. Returns the FIRST unsupported occurrence, or `None`. `is_instance`
/// marks the instance-script program — the only program with supported rune
/// positions; a module-script / template-expression program passes `false`, so its
/// supported-position set is empty and every rune reference refuses. A shadowed
/// rune name is not a rune reference.
fn scan_unsupported_rune_forms(
    alloc: &Allocator,
    source: &str,
    is_instance: bool,
) -> Option<UnsupportedSvelteRuntimeSurface> {
    let program = super::expr::reparse_module(alloc, source)?;
    let mut scan = super::rune_scan::UnsupportedRuneScan::for_program(&program, is_instance);
    use oxc_ast_visit::Visit;
    scan.visit_program(&program);
    scan.into_surface()
}
