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
use super::client_imports::{UserImport, UserImportSlot};
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

/// The accepted script-item facts [`classify_script_items`] returns: the admitted
/// module-scope user imports, plus the `$host` FACTS the rune scan recorded
/// (an admitted zero-arg `$host()` call in the instance script or any template
/// expression, and its first span) — the plan build decides the
/// `$$props`-parameter question from them, so no later stage re-parses source
/// to re-discover the usage.
pub(super) struct ClassifiedScriptItems {
    /// The admitted module-scope USER imports — every static import form, module
    /// slot first, each slot in source order (the two-slot emission reads the slot
    /// discriminant off each carrier).
    pub(super) user_imports: Vec<UserImport>,
    /// Whether an ADMITTED `$host()` call was seen (instance script or any
    /// template expression). Only meaningful under an active custom element —
    /// every other `$host` shape refused during the scan.
    pub(super) uses_host: bool,
    /// The span of the FIRST admitted `$host()` call, in the coordinates of the
    /// scanned program it was found in (the instance script, or the wrapped
    /// `({expr});` template-expression program — the same coordinate space the
    /// scan's own refusal spans use). Carried so the plan build's
    /// degenerate-host refusal points at the offending call.
    pub(super) first_host_span: Option<Span>,
}

/// Classify the instance + module script items, returning the admitted module-scope
/// user imports (every static import form, both script slots) plus the `$host`-usage
/// fact. A `<script module>` is admitted IFF every top-level statement is a static
/// `import` declaration — any non-import module item (a declaration, an export, an
/// expression, control flow) is the module-item completion residual and fails closed
/// with the precise [`ModuleScriptItem`](UnsupportedSvelteRuntimeSurface::ModuleScriptItem)
/// diagnostic. On the instance side, a non-basic / default-bearing `$props()` form, a
/// destructured / non-primitive `$state`, or an advanced rune call/member fails
/// closed (no wildcard accept).
pub(super) fn classify_script_items(
    ir: &SvelteRuntimeIr,
    store_exempt: &rustc_hash::FxHashSet<String>,
) -> Result<ClassifiedScriptItems, UnsupportedSvelteRuntimeSurface> {
    // The `<script module>` IMPORT-ONLY admit predicate: every top-level statement
    // must be an `ImportDeclaration`; the FIRST non-import statement refuses with the
    // precise module-item diagnostic. (Arbitrary module statements + exports are the
    // module-item completion surface, still fail-closed.)
    if let Some(module) = ir.analysis.scripts.module_source {
        refuse_first_non_import_module_item(module)?;
    }
    // The RETAINED per-slot import classification (the single authority, computed
    // ONCE at IR construction — the same carriers the binding preparation consumed):
    // propagate a slot's retained refusal (the residual non-static forms — type-only /
    // phase / `assert { … }` — fail closed here); collect the admitted carriers,
    // module slot first so the prelude emits them BEFORE the runtime namespace, the
    // instance slot after it.
    let mut user_imports = Vec::new();
    for slot in [UserImportSlot::Module, UserImportSlot::Instance] {
        match ir.analysis.script_imports.slot(slot) {
            Ok(imports) => user_imports.extend(imports.iter().cloned()),
            Err(surface) => return Err(surface.clone()),
        }
    }
    // The `$props()` shape gate. A rest element (`{ …, ...rest }`) and a
    // whole-object binding (`let all = $props()`) are BASIC — they lower through
    // the `$.rest_props` capture path — alongside the named / aliased / string-key
    // members and their plain / `$bindable` defaults. Only a genuinely non-basic
    // form — a computed / numeric / nested-destructure member, over-arity args, or
    // a duplicate `$props()` — is an advanced rune form that fails closed.
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
    // name is never refused. (An admitted `<script module>` is IMPORT-ONLY — import
    // declarations host no rune positions, so it needs no rune scan; a module rune
    // was already refused above as a non-import module item.) The custom-element
    // fact gates the `$host()` admission: the zero-arg call is supported only under
    // an active custom-element descriptor — an admitted call records the
    // `uses_host` FACT this classification returns.
    let custom_element_active = ir.component.custom_element.is_some();
    let mut uses_host = false;
    let mut first_host_span: Option<Span> = None;
    let mut alloc = Allocator::default();
    if let Some(instance) = ir.analysis.scripts.instance_source {
        let scan = scan_unsupported_rune_forms(
            &alloc,
            instance,
            true,
            custom_element_active,
            store_exempt,
        );
        if let Some(reason) = scan.refusal {
            return Err(reason);
        }
        uses_host |= scan.uses_host;
        first_host_span = first_host_span.or(scan.first_host_span);
        alloc.reset();
    }
    // The SAME scan over every analyzed TEMPLATE expression (an interpolation /
    // handler / bind expression) — an unsupported rune inside an expression
    // (`{$state.snapshot(x)}`) must fail closed too. A template expression hosts no
    // supported rune position (`is_instance = false`).
    for expr in ir.analysis.expressions.all() {
        let wrapped = format!("({});", expr.source);
        let scan = scan_unsupported_rune_forms(
            &alloc,
            &wrapped,
            false,
            custom_element_active,
            store_exempt,
        );
        if let Some(reason) = scan.refusal {
            return Err(reason);
        }
        uses_host |= scan.uses_host;
        first_host_span = first_host_span.or(scan.first_host_span);
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
    Ok(ClassifiedScriptItems {
        user_imports,
        uses_host,
        first_host_span,
    })
}

/// Refuse the FIRST non-import top-level statement of a `<script module>` — the
/// import-only admit predicate. Any other module item (a variable / function / class
/// declaration, an export or re-export, an expression, control flow, a module rune)
/// is the module-item completion residual: fail closed with the precise
/// [`ModuleScriptItem`](UnsupportedSvelteRuntimeSurface::ModuleScriptItem) diagnostic
/// carrying the statement family + its module-relative span. An unparseable module
/// script yields no refusal here (the upstream script-parse diagnostic owns it).
fn refuse_first_non_import_module_item(
    module_source: &str,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let Some(program) = super::expr::reparse_module(&alloc, module_source) else {
        return Ok(());
    };
    for stmt in &program.body {
        use oxc_ast::ast::Statement;
        use oxc_span::GetSpan;
        let construct: &'static str = match stmt {
            Statement::ImportDeclaration(_) => continue,
            Statement::VariableDeclaration(_) => "variable declaration",
            Statement::FunctionDeclaration(_) => "function",
            Statement::ClassDeclaration(_) => "class",
            Statement::ExportNamedDeclaration(_)
            | Statement::ExportAllDeclaration(_)
            | Statement::ExportDefaultDeclaration(_) => "export",
            Statement::ExpressionStatement(_) => "expression statement",
            Statement::EmptyStatement(_) => "empty statement",
            Statement::TSEnumDeclaration(_) => "enum",
            Statement::TSModuleDeclaration(_) => "namespace",
            Statement::TSInterfaceDeclaration(_) => "interface",
            Statement::TSTypeAliasDeclaration(_) => "type alias",
            Statement::TSImportEqualsDeclaration(_) => "import-equals",
            Statement::LabeledStatement(_) => "labeled statement",
            _ => "module statement",
        };
        let span = stmt.span();
        return Err(UnsupportedSvelteRuntimeSurface::ModuleScriptItem {
            construct,
            span: Span::new(span.start, span.end),
        });
    }
    Ok(())
}

/// One program's rune-scan outcome: the first unsupported occurrence (if any)
/// plus the `$host` facts the walk recorded.
struct RuneScanOutcome {
    /// The FIRST unsupported rune form / position found, or `None`.
    refusal: Option<UnsupportedSvelteRuntimeSurface>,
    /// Whether the walk admitted a zero-arg `$host()` call.
    uses_host: bool,
    /// The span of the first admitted `$host()` call (program-relative).
    first_host_span: Option<Span>,
}

/// Scope-aware, POSITION-SENSITIVE scan of a script for an UNSUPPORTED rune form or
/// position. Returns the FIRST unsupported occurrence plus the `$host`-usage fact.
/// `is_instance` marks the instance-script program — the only program with
/// supported rune positions; a module-script / template-expression program passes
/// `false`, so its supported-position set is empty and every rune reference
/// refuses. A shadowed rune name is not a rune reference. `custom_element_active`
/// marks a component with a resolved custom-element descriptor — the only context
/// whose zero-arg `$host()` call is admitted (and recorded as `uses_host`).
fn scan_unsupported_rune_forms(
    alloc: &Allocator,
    source: &str,
    is_instance: bool,
    custom_element_active: bool,
    store_exempt: &rustc_hash::FxHashSet<String>,
) -> RuneScanOutcome {
    let Some(program) = super::expr::reparse_module(alloc, source) else {
        return RuneScanOutcome {
            refusal: None,
            uses_host: false,
            first_host_span: None,
        };
    };
    let mut scan = super::rune_scan::UnsupportedRuneScan::for_program(
        &program,
        is_instance,
        custom_element_active,
        store_exempt.clone(),
    );
    use oxc_ast_visit::Visit;
    scan.visit_program(&program);
    RuneScanOutcome {
        uses_host: scan.uses_host(),
        first_host_span: scan.first_host_span(),
        refusal: scan.into_surface(),
    }
}
