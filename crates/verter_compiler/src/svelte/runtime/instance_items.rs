//! The strict finite instance-script item allowlist (the supported top-level
//! `<script>` declaration shapes) for the default-deny Svelte client surface.
//!
//! This is the script-side analogue of the element allowlist: the classifier
//! ([`classify_supported_instance_items`]) admits ONLY the enumerated
//! [`SupportedInstanceScriptItem`] shapes and the lowering consumes ONLY this enum —
//! there is NO "emit any non-rune statement" path. (A named `function` declaration is
//! admitted ONLY when its name is referenced by an accepted DOM function-pair bind; its
//! body lowers through the shared rewriter, not verbatim.) It also owns the scope-aware
//! magic-identifier scan ([`scan_magic_identifiers`]) that refuses a reference to a
//! compiler-magic object (`$$slots` / `$$props` / `$$restProps`).
//!
//! Every decision is driven from the typed OXC AST (statement kind, declarator
//! pattern, init shape, TS-annotation presence) + the scope-aware binding table,
//! never a raw text scan.

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Expression, Statement, VariableDeclarationKind};

use super::client::UnsupportedSvelteRuntimeSurface;
use super::expr::{is_derived_callee, is_props_callee, reparse_module, state_rune_call};
use verter_span::Span;

// ---------------------------------------------------------------------------
// Instance-script item allowlist (the strict finite supported-shape set)
// ---------------------------------------------------------------------------

/// A TYPED supported instance-script item — the closed allowlist of top-level
/// instance-script declaration shapes the client core lowers. This is the
/// script-side analogue of [`SupportedHtmlElement`](super::client_allowlist::SupportedHtmlElement):
/// the classifier ([`classify_supported_instance_items`]) admits ONLY these enumerated
/// shapes and the lowering consumes ONLY this enum — there is NO "emit any non-rune
/// statement" path. Every OTHER top-level item (a function NOT referenced by a DOM
/// function-pair bind / class / enum / namespace / interface / type / plain non-rune
/// `let`-`const`-`var` / arbitrary statement / `$:` label / `$`-`$$`-prefixed binding)
/// fails closed BY CONSTRUCTION at the classifier.
///
/// Each variant carries the lowering inputs (a binding name, the init payload text, or
/// the function source text), so the lowering is a thin per-variant transform — a
/// declaration's init / a function body lowers through the shared rewriter, never a
/// re-walk of an arbitrary statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SupportedInstanceScriptItem {
    /// `let name = $state(<primitive literal>);` — one declarator, `let` only,
    /// identifier binding, no TS annotation, a 0-1-arg `$state()` with a primitive
    /// literal init. Carries the binding name and the init payload (the primitive
    /// literal source text, or `None` for the no-arg `$state()` ⇒ `void 0` form).
    StatePrimitive {
        /// The declared signal name.
        name: String,
        /// The primitive-literal init source text (`'world'`, `0`, `-1`, `true`,
        /// `null`, …), or `None` for the no-arg `$state()` form.
        init: Option<String>,
    },
    /// A single no-default `$props()` destructure (`let { a } = $props()` /
    /// `let { a: b } = $props()`). A no-default destructure emits NO component-body
    /// declaration (the props are read directly off `$$props`), so this variant
    /// carries no lowering payload — it is the classification fact that the props
    /// destructure was accepted (the props reads are projected separately).
    PropsDestructure,
    /// `let el;` — a bare (no-init, no-annotation) `let` identifier used SOLELY as a
    /// supported `bind:this` target. Carries the binding name (lowered to `let el;`).
    BindThisLocal {
        /// The declared local name.
        name: String,
    },
    /// `let v = <literal-only init>;` / `let v;` — a PLAIN local (no rune call, no TS
    /// annotation) used SOLELY as a DOM bind TARGET (a `bind:value={v}` ident or the
    /// ROOT of a `bind:value={v.x}` member). Official emits the declaration VERBATIM (it
    /// stays a plain local — `let v = "x";` / `let o = { x: '' };` / `let v;`), so this
    /// variant carries the binding name + the LITERAL-ONLY init source (a string/number/
    /// bool/null/bigint literal, or an object/array literal whose values are recursively
    /// literal), emitted byte-for-byte — or `None` for an UNINITIALIZED `let v;` (the
    /// no-init plain-local form, lowered to a bare `let v;`). The init is restricted to a
    /// literal-only value precisely so the verbatim emit is correct without an init
    /// rewrite — a signal-bearing / identifier-bearing init (which official would
    /// `$.get`-rewrite) is a DISTINCT surface that fails closed.
    BindLocalLet {
        /// The declared local name.
        name: String,
        /// The literal-only init source text (emitted verbatim), or `None` for an
        /// uninitialized `let v;`.
        init: Option<String>,
    },
    /// A named top-level `function name(...) { ... }` declaration whose name is EXACTLY
    /// referenced by an accepted DOM function-pair bind (`bind:value={get, set}`).
    /// Official emits the declaration with its BODY signal reads/writes rewritten
    /// (`function get() { return $.get(value); }`), then passes the function ident
    /// directly to the helper. This variant carries the function's full source text; the
    /// body lowers through the shared FALLIBLE expression rewriter at projection time (so
    /// a signal read/write inside the body becomes `$.get`/`$.set`), NOT verbatim. The
    /// admission is gated on the function-pair-referenced name set — a function NOT
    /// referenced by such a bind is out-of-allowlist and fails closed.
    FunctionDecl {
        /// The declared function name (the function-pair-referenced ident).
        name: String,
        /// The function declaration's full source text (lowered via the rewriter).
        source: String,
    },
}

/// Classify the instance script's TOP-LEVEL items into the strict finite
/// [`SupportedInstanceScriptItem`] allowlist, or fail closed on the FIRST
/// out-of-allowlist item.
///
/// The four supported shapes are EXACTLY:
/// 1. `let name = $state(<primitive literal>);`
/// 2. a single no-default `$props()` destructure;
/// 3. `let el;` used solely as a supported `bind:this` target;
/// 4. `let v = <literal-only init>;` used solely as a DOM bind-TARGET lvalue root.
///
/// `bind_this_targets` is the set of local names used as a supported `bind:this`
/// target (from the accepted bind shapes) — a bare `let el;` is admitted ONLY when
/// its name is in this set; an unused / plain bare local fails closed.
///
/// `bind_lvalue_roots` is the set of plain-local names used as a DOM bind-target
/// lvalue ROOT (a `bind:value={v}` ident or the root of a `bind:value={v.x}` member)
/// — a `let v = <literal init>;` / `let v;` is admitted as the plain-local bind shape
/// ONLY when its name is in this set; an unused / non-target plain local fails closed.
///
/// `bind_function_pair_names` is the set of bare-identifier names referenced by an
/// accepted DOM FUNCTION-PAIR bind (`bind:value={get, set}`) — a top-level
/// `function name(...) {}` declaration is admitted ONLY when its name is in this set
/// (its body lowers via the shared rewriter at projection time). A function NOT
/// referenced by such a bind is out-of-allowlist and fails closed (there is NO wildcard
/// "emit any function" path).
///
/// Everything else fails closed: a plain `let x = 0` NOT used as a bind target, a
/// `const` / `var`, a top-level function NOT referenced by a bind pair / class / enum /
/// namespace / interface / type, an arbitrary expression / control-flow / empty
/// statement, a `$:` reactive label, an import / export, a `$` / `$$`-prefixed binding,
/// and the magic refs `$$slots` / `$$props` / `$$restProps`. The decision is driven from
/// the typed OXC AST (statement kind, declarator pattern, init shape, TS-annotation
/// presence), never a text scan.
///
/// Two whole-program pre-passes run FIRST so their PRECISE diagnostics win over the
/// generic item refusal: the rune-form / rune-position scan (owned by
/// [`super::client_surface`]) and the magic-identifier scan ([`scan_magic_identifiers`]).
pub(super) fn classify_supported_instance_items(
    instance_source: &str,
    bind_this_targets: &[String],
    bind_lvalue_roots: &[String],
    bind_function_pair_names: &[String],
) -> Result<Vec<SupportedInstanceScriptItem>, UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, instance_source) else {
        // An unparseable instance script is recorded as a script-parse diagnostic
        // upstream; classify yields no items (the upstream parse gate owns the refusal).
        return Ok(Vec::new());
    };

    let mut items = Vec::new();
    for stmt in &program.body {
        items.push(classify_instance_statement(
            stmt,
            instance_source,
            bind_this_targets,
            bind_lvalue_roots,
            bind_function_pair_names,
        )?);
    }
    Ok(items)
}

/// Classify ONE top-level instance-script statement into its supported item, or
/// fail closed. The supported statements are EXACTLY a `let`-variable declaration
/// matching a `$state` / `$props()` / `bind:this` / plain-local bind shape, OR a named
/// `function` declaration referenced by a DOM function-pair bind; every other statement
/// kind fails closed with a precise `construct` label.
fn classify_instance_statement(
    stmt: &Statement<'_>,
    instance_source: &str,
    bind_this_targets: &[String],
    bind_lvalue_roots: &[String],
    bind_function_pair_names: &[String],
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    match stmt {
        Statement::VariableDeclaration(decl) => classify_instance_variable_decl(
            decl,
            instance_source,
            bind_this_targets,
            bind_lvalue_roots,
        ),
        // A named top-level `function name(...) {}` is admitted ONLY when its name is
        // EXACTLY referenced by an accepted DOM function-pair bind (`bind:value={get,
        // set}`); its body lowers via the shared rewriter at projection time. A function
        // NOT referenced by such a bind, or an ANONYMOUS function declaration (no usable
        // name to bind), is out-of-allowlist and fails closed.
        Statement::FunctionDeclaration(func) => {
            classify_function_declaration(func, instance_source, bind_function_pair_names)
        }
        // Every OTHER NON-variable top-level statement fails closed with its construct
        // label. The labels are precise so the completeness gate can pin each family.
        other => Err(UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
            construct: top_level_statement_label(other),
            span: stmt_span(other),
        }),
    }
}

/// Classify a top-level `function name(...) {}` declaration into the
/// [`SupportedInstanceScriptItem::FunctionDecl`] item, or fail closed.
///
/// Admitted ONLY when the function has a name AND that name is EXACTLY in the
/// function-pair-referenced set (the bare-identifier names referenced by an accepted DOM
/// `bind:value={get, set}` pair). An anonymous function declaration (no name) or one
/// whose name is NOT a function-pair reference fails closed at the instance-script-item
/// gate (construct `function`) — this is the precise gate, NOT a wildcard function path.
fn classify_function_declaration(
    func: &oxc_ast::ast::Function<'_>,
    instance_source: &str,
    bind_function_pair_names: &[String],
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    let refuse = || UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
        construct: "function",
        span: Span::new(func.span.start, func.span.end),
    };
    let Some(name) = func.id.as_ref().map(|id| id.name.as_str()) else {
        // An anonymous top-level function declaration has no name to bind a pair to.
        return Err(refuse());
    };
    if !bind_function_pair_names.iter().any(|n| n == name) {
        return Err(refuse());
    }
    let source = instance_source
        .get(func.span.start as usize..func.span.end as usize)
        .unwrap_or_default()
        .to_string();
    Ok(SupportedInstanceScriptItem::FunctionDecl {
        name: name.to_string(),
        source,
    })
}

/// Classify a top-level `VariableDeclaration` into shape 1/2/3/4, or fail closed.
///
/// A `var` / `const` declaration, a multi-declarator declaration, or any declarator
/// that is not exactly one of the four supported shapes fails closed.
fn classify_instance_variable_decl(
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
    instance_source: &str,
    bind_this_targets: &[String],
    bind_lvalue_roots: &[String],
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    let refuse = |construct: &'static str| UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
        construct,
        span: Span::new(decl.span.start, decl.span.end),
    };
    // (1) `let` ONLY — a `const` / `var` declaration is a distinct official surface
    // (`var` reads use `$.safe_get`, a read-only `const $state` constant-folds), and
    // a plain `const`/`var` local is not core. Fail closed.
    if decl.kind != VariableDeclarationKind::Let {
        return Err(refuse(match decl.kind {
            VariableDeclarationKind::Const => "const declaration",
            VariableDeclarationKind::Var => "var declaration",
            _ => "non-let declaration",
        }));
    }
    // (2) EXACTLY ONE declarator — a multi-declarator `let a = $state(0), b = 1;`
    // mixes shapes and is not core. Fail closed.
    let [d] = decl.declarations.as_slice() else {
        return Err(refuse("multi-declarator let"));
    };
    // (3) NO TS annotation — `let c: number = $state(0)` / a definite `let c!: T`
    // is a TS-leniency form (a plain `<script>` parsed as TSX accepts the
    // annotation). The supported shapes carry NO annotation. Fail closed.
    if d.type_annotation.is_some() || d.definite {
        return Err(refuse("ts-annotated let"));
    }
    // (4) The binding name (an identifier pattern). A destructure pattern is handled
    // by the `$props()` shape below; an array pattern / non-identifier non-props
    // declarator is not core.
    match &d.id {
        BindingPattern::BindingIdentifier(id) => {
            let name = id.name.as_str();
            // A `$` / `$$`-prefixed binding (`let $$anchor`, `let $foo`) is reserved
            // (the `$$`-prefix is the compiler-magic namespace; the `$`-prefix is the
            // store-subscription namespace). Fail closed BEFORE the init shape.
            if name.starts_with('$') {
                return Err(refuse("$-prefixed binding"));
            }
            classify_identifier_declarator(
                d,
                name,
                decl,
                instance_source,
                bind_this_targets,
                bind_lvalue_roots,
            )
        }
        BindingPattern::ObjectPattern(_) => {
            // The ONLY supported destructure is a no-default `$props()` call. The
            // detailed shape (no defaults / rest / computed / nested / `$bindable`)
            // is enforced by `props_shape` upstream; here the declarator must be a
            // `$props()` call destructure.
            let Some(Expression::CallExpression(call)) = &d.init else {
                return Err(refuse("object-destructure let"));
            };
            if !is_props_callee(&call.callee) {
                return Err(refuse("object-destructure let"));
            }
            Ok(SupportedInstanceScriptItem::PropsDestructure)
        }
        BindingPattern::ArrayPattern(_) => Err(refuse("array-destructure let")),
        BindingPattern::AssignmentPattern(_) => Err(refuse("default-pattern let")),
    }
}

/// Classify a `let <ident> …` declarator (the identifier already known non-`$`-prefixed)
/// into shape 1 (`$state(<primitive>)`), shape 3 (bare `let el;` bind:this target), or
/// shape 4 (`let v = <literal init>;` DOM bind-target lvalue root), or fail closed.
fn classify_identifier_declarator(
    d: &oxc_ast::ast::VariableDeclarator<'_>,
    name: &str,
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
    instance_source: &str,
    bind_this_targets: &[String],
    bind_lvalue_roots: &[String],
) -> Result<SupportedInstanceScriptItem, UnsupportedSvelteRuntimeSurface> {
    let refuse = |construct: &'static str| UnsupportedSvelteRuntimeSurface::InstanceScriptItem {
        construct,
        span: Span::new(decl.span.start, decl.span.end),
    };
    match &d.init {
        // A bare `let name;` (no init): admitted as shape 3 (a `bind:this` clone-root
        // local) OR as the no-init plain-local DOM bind-target root (`let v; <input
        // bind:value={v}>` — official keeps the bare local verbatim and binds it with the
        // plain `() => v` / `($$value) => v = $$value` closures). An unused / plain bare
        // local that is NEITHER a `bind:this` target NOR a DOM bind-lvalue root fails
        // closed.
        None => {
            if bind_this_targets.iter().any(|t| t == name) {
                Ok(SupportedInstanceScriptItem::BindThisLocal {
                    name: name.to_string(),
                })
            } else if bind_lvalue_roots.iter().any(|t| t == name) {
                Ok(SupportedInstanceScriptItem::BindLocalLet {
                    name: name.to_string(),
                    init: None,
                })
            } else {
                Err(refuse("unused bare let"))
            }
        }
        // Shape 1: `let name = $state(<primitive literal>)`. The `$state` family,
        // arity (0-1), and primitive-literal init are validated here; the destructure
        // / non-primitive / multi-arg / `$state.raw` forms are owned by the upstream
        // `state_decl_shape` gate (which fails them as `AdvancedRune`), so on the
        // accept path a `$state(<primitive>)` identifier declarator reaches here.
        Some(Expression::CallExpression(call)) => {
            // A `$state` / `$state.raw` call.
            if state_rune_call(call).is_some() {
                // The init payload: the primitive-literal source text, or `None`
                // for the no-arg `$state()` form (lowered to `void 0`).
                let init = state_primitive_init_text(call, instance_source);
                return Ok(SupportedInstanceScriptItem::StatePrimitive {
                    name: name.to_string(),
                    init,
                });
            }
            // A `$derived` / `$props()` / other call init for an IDENTIFIER binding
            // is not a supported shape (a `$derived` identifier is a deferral; a
            // `$props()` identifier is a whole-object binding). Fail closed.
            if is_derived_callee(&call.callee) {
                return Err(refuse("$derived declarator"));
            }
            if is_props_callee(&call.callee) {
                return Err(refuse("$props() whole-object"));
            }
            // A plain non-rune call init (`let x = makeIt()`) is not core.
            Err(refuse("plain let with call init"))
        }
        // Shape 4: a plain non-rune `let v = <literal-only init>` used SOLELY as a DOM
        // bind-target lvalue ROOT (a `bind:value={v}` ident, or the root of a
        // `bind:value={v.x}` member). Official keeps the plain local verbatim, so it is
        // admitted ONLY when (a) its name is a recorded bind-lvalue root AND (b) the
        // init is a LITERAL-ONLY value (so the verbatim emit is correct without an init
        // rewrite). A plain local NOT used as a bind target, or one with a
        // signal-bearing / identifier-bearing init (which official would `$.get`-
        // rewrite — a distinct surface), fails closed.
        Some(init_expr) => {
            if !bind_lvalue_roots.iter().any(|t| t == name) {
                // A plain local that is not a DOM bind-target root is not core — a
                // template read is only a reactive `$state` signal or a no-default prop.
                return Err(refuse("plain let"));
            }
            if !init_is_literal_only(init_expr) {
                // A bind-target plain local whose init is NOT literal-only (it
                // references an identifier / member / call — which official rewrites)
                // is a distinct surface; fail closed rather than emit it verbatim wrong.
                return Err(refuse("plain let with non-literal init"));
            }
            use oxc_span::GetSpan;
            let span = init_expr.span();
            let init = instance_source
                .get(span.start as usize..span.end as usize)
                .unwrap_or_default()
                .to_string();
            Ok(SupportedInstanceScriptItem::BindLocalLet {
                name: name.to_string(),
                init: Some(init),
            })
        }
    }
}

/// Whether an init expression is a LITERAL-ONLY value — a string / number / boolean /
/// null / bigint / regexp / template-with-no-substitution literal, a unary `+`/`-`/`~`
/// over a literal, OR an object / array literal whose every element/property VALUE is
/// recursively literal-only. A literal-only init carries NO identifier / member / call
/// reference, so it has no signal read official would `$.get`-rewrite — it is safe to
/// emit VERBATIM as a plain-local declaration.
///
/// An identifier / member / call / arrow / `this` / etc. init is NOT literal-only (it
/// could read a reactive binding), so a plain-local bind target with such an init
/// fails closed (a distinct surface), never a verbatim mis-emit.
fn init_is_literal_only(expr: &Expression<'_>) -> bool {
    use oxc_ast::ast::{Expression as E, PropertyKey};
    match expr {
        E::StringLiteral(_)
        | E::NumericLiteral(_)
        | E::BooleanLiteral(_)
        | E::NullLiteral(_)
        | E::BigIntLiteral(_)
        | E::RegExpLiteral(_) => true,
        // A template literal is literal-only iff it has NO `${...}` substitution.
        E::TemplateLiteral(t) => t.expressions.is_empty(),
        // A unary `-1` / `+1` / `~0` / `!true` over a literal-only argument.
        E::UnaryExpression(u) => init_is_literal_only(&u.argument),
        // A parenthesized literal-only value.
        E::ParenthesizedExpression(p) => init_is_literal_only(&p.expression),
        // An array literal: every (present, non-spread) element must be literal-only.
        E::ArrayExpression(arr) => arr.elements.iter().all(|el| match el {
            oxc_ast::ast::ArrayExpressionElement::SpreadElement(_) => false,
            oxc_ast::ast::ArrayExpressionElement::Elision(_) => true,
            other => other.as_expression().is_some_and(init_is_literal_only),
        }),
        // An object literal: every property must be a non-computed plain key with a
        // literal-only value (no shorthand identifier, no spread, no computed key).
        E::ObjectExpression(obj) => obj.properties.iter().all(|prop| match prop {
            oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) => {
                !p.computed
                    && !p.shorthand
                    && matches!(
                        &p.key,
                        PropertyKey::StaticIdentifier(_)
                            | PropertyKey::StringLiteral(_)
                            | PropertyKey::NumericLiteral(_)
                    )
                    && init_is_literal_only(&p.value)
            }
            // A spread property (`{ ...x }`) reads `x` — not literal-only.
            oxc_ast::ast::ObjectPropertyKind::SpreadProperty(_) => false,
        }),
        _ => false,
    }
}

/// The primitive-literal init source text of a `$state(<arg>)` call, or `None` for
/// the no-arg `$state()` form. A primitive literal carries NO signal read and NO TS
/// syntax, so its source slice is emitted verbatim (matching official). The
/// over-arity / non-primitive forms are refused upstream, so the first argument is a
/// primitive literal here. The argument span is absolute into `instance_source` (the
/// SAME buffer the program was parsed from), so the slice is the exact user text.
fn state_primitive_init_text(
    call: &oxc_ast::ast::CallExpression<'_>,
    instance_source: &str,
) -> Option<String> {
    use oxc_span::GetSpan;
    let arg = call.arguments.first()?.as_expression()?;
    let span = arg.span();
    Some(instance_source[span.start as usize..span.end as usize].to_string())
}

/// Scan an instance-script (or template-expression) program for a compiler-MAGIC
/// identifier reference (`$$slots` / `$$props` / `$$restProps`). Returns the FIRST
/// magic-identifier surface, or `None`. A LOCAL binding shadowing the name (a
/// function param / nested `let` of the same name) is NOT a magic reference — the
/// scan reuses the shared lexical [`super::expr::ShadowStack`] model so the
/// shadowing semantics match the rune scan.
pub(super) fn scan_magic_identifiers(source: &str) -> Option<UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let program = reparse_module(&alloc, source)?;
    let mut scan = MagicIdentScan {
        scopes: super::expr::ShadowStack::default(),
        found: None,
    };
    use oxc_ast_visit::Visit;
    scan.visit_program(&program);
    scan.found
}

/// The Svelte compiler-MAGIC identifier names (the auto-injected legacy magic
/// objects). A reference to one of these in the runes client output would bind an
/// undefined identifier (a runtime `ReferenceError`).
const MAGIC_IDENT_NAMES: &[&str] = &["$$slots", "$$props", "$$restProps"];

/// The scope-aware scan state for a magic-identifier reference.
struct MagicIdentScan {
    scopes: super::expr::ShadowStack,
    found: Option<UnsupportedSvelteRuntimeSurface>,
}

impl<'a> oxc_ast_visit::Visit<'a> for MagicIdentScan {
    fn visit_program(&mut self, it: &oxc_ast::ast::Program<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        super::expr::collect_direct_decls(&it.body, &mut frame);
        super::expr::collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        oxc_ast_visit::walk::walk_program(self, it);
        self.scopes.pop();
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
        if self.found.is_none()
            && MAGIC_IDENT_NAMES.contains(&name)
            && !self.scopes.is_shadowed(name)
        {
            let magic: &'static str = match name {
                "$$slots" => "$$slots",
                "$$props" => "$$props",
                "$$restProps" => "$$restProps",
                _ => "$$magic",
            };
            self.found = Some(UnsupportedSvelteRuntimeSurface::MagicIdentifier {
                name: magic,
                span: Span::new(it.span.start, it.span.end),
            });
        }
        oxc_ast_visit::walk::walk_identifier_reference(self, it);
    }
}

/// A short construct label for a top-level instance-script statement that is NOT a
/// variable declaration. Each kind gets a precise label so the completeness gate
/// pins the family (function / class / enum / namespace / interface / type / `$:` /
/// import / export / expression / control-flow / empty / …).
fn top_level_statement_label(stmt: &Statement<'_>) -> &'static str {
    match stmt {
        Statement::FunctionDeclaration(_) => "function",
        Statement::ClassDeclaration(_) => "class",
        Statement::TSEnumDeclaration(_) => "enum",
        Statement::TSModuleDeclaration(_) => "namespace",
        Statement::TSInterfaceDeclaration(_) => "interface",
        Statement::TSTypeAliasDeclaration(_) => "type alias",
        Statement::TSImportEqualsDeclaration(_) => "import-equals",
        Statement::LabeledStatement(_) => "$: label",
        Statement::ImportDeclaration(_) => "import",
        Statement::ExportNamedDeclaration(_)
        | Statement::ExportAllDeclaration(_)
        | Statement::ExportDefaultDeclaration(_) => "export",
        Statement::ExpressionStatement(_) => "expression statement",
        Statement::IfStatement(_)
        | Statement::ForStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_)
        | Statement::WhileStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::SwitchStatement(_)
        | Statement::TryStatement(_)
        | Statement::BlockStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ContinueStatement(_)
        | Statement::WithStatement(_) => "control-flow statement",
        Statement::EmptyStatement(_) => "empty statement",
        Statement::DebuggerStatement(_) => "debugger statement",
        // Any other statement kind (a `using` declaration, …) is still
        // out-of-allowlist.
        _ => "instance-script statement",
    }
}

/// The verter span of a top-level statement (for the fail-closed diagnostic).
fn stmt_span(stmt: &Statement<'_>) -> Span {
    use oxc_span::GetSpan;
    let span = stmt.span();
    Span::new(span.start, span.end)
}
