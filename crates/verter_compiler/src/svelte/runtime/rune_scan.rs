//! Scope-aware Svelte 5 rune-use detection.
//!
//! Reports whether any rune NAME (`$state` / `$derived` / `$props` / `$effect` /
//! `$bindable` / `$inspect` / `$host`) appears as an UNRESOLVED reference in a
//! script — i.e. a reference NOT bound to a local of the same name. This is the
//! syntax-side input to the runes-vs-legacy MODE inference.
//!
//! It reuses the SAME lexical-scope `ShadowStack` model the [`ScriptUseCollector`]
//! (in [`super::expr`]) uses — program / function / arrow / block / catch /
//! for-loop frames — so the shadowing semantics are identical across the two
//! syntax-side collectors; there is no second scope model.

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, BlockStatement, CatchClause, Expression,
    ForInStatement, ForOfStatement, ForStatement, Function, IdentifierReference, MemberExpression,
    Program, Statement, VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use rustc_hash::FxHashSet;
use verter_span::Span;

use super::client::UnsupportedSvelteRuntimeSurface;
use super::expr::{
    arrow_scope_names, block_scope_names, collect_direct_decls, collect_pattern_names,
    collect_var_hoists, for_left_names, function_scope_names, is_props_callee, state_rune_call,
    ShadowStack,
};

/// The Svelte 5 rune NAMES (`compiler/utils.js` `RUNES`, minus the `$state.snapshot`
/// / `.raw` / `.by` member keypaths the detector reaches through the root
/// identifier). A component is in runes mode iff any of these appears as an
/// UNRESOLVED reference (not bound to a local) anywhere in a script — matching the
/// official `Array.from(scope.references.keys()).some(is_rune)`.
pub(super) const RUNE_ROOT_NAMES: &[&str] = &[
    "$state",
    "$derived",
    "$props",
    "$effect",
    "$bindable",
    "$inspect",
    "$host",
];

/// A SCOPE-AWARE rune-use detector: it reports whether any rune NAME
/// (`$state`/`$derived`/`$props`/`$effect`/`$bindable`/`$inspect`/`$host`) appears
/// as an UNRESOLVED reference in a script — i.e. a reference NOT bound to a local
/// of the same name.
///
/// This mirrors the official runes-mode detection
/// (`phases/2-analyze/index.js`: `Array.from(scope.references.keys()).some(is_rune)`
/// over the binder-pruned reference set, where `get_global_keypath` returns null
/// when `scope.get(name) !== null`). A shadowing local — most importantly a
/// function PARAMETER named `$state` (`function f($state){ return $state }`) — is a
/// declared binding, so its references do NOT count as rune uses and the component
/// stays in LEGACY mode. The reference need NOT be a call (`const h = $host;` is a
/// runes-mode marker — though `$host` without parentheses is a separate official
/// error the runtime backend raises, the MODE is still runes).
#[derive(Default)]
pub(super) struct ScopeAwareRuneDetector {
    /// Whether an unresolved rune-name reference was seen.
    used: bool,
    /// The active lexical-scope shadow stack.
    scopes: ShadowStack,
}

impl ScopeAwareRuneDetector {
    /// Whether `name` is a rune name that is NOT shadowed by a local binding (so
    /// the reference counts as a rune use, forcing runes mode).
    fn is_unshadowed_rune(&self, name: &str) -> bool {
        RUNE_ROOT_NAMES.contains(&name) && !self.scopes.is_shadowed(name)
    }

    /// Whether the detector observed any unresolved rune reference.
    #[must_use]
    pub(super) fn used(&self) -> bool {
        self.used
    }
}

impl<'a> Visit<'a> for ScopeAwareRuneDetector {
    fn visit_program(&mut self, it: &Program<'a>) {
        // The program (script) scope: its own top-level declarations. A top-level
        // `let $state` is a parse error (the `$` prefix is reserved for non-params),
        // so the program frame normally carries no rune name — but pushing it is
        // harmless and keeps the scope model uniform with `ScriptUseCollector`.
        let mut frame = rustc_hash::FxHashSet::default();
        collect_direct_decls(&it.body, &mut frame);
        collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        // A function PARAMETER named `$state` shadows the rune — the canonical X5
        // legacy case (`function f($state){ return $state }`).
        self.scopes.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.scopes.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(param) = &it.param {
            let mut names = Vec::new();
            collect_pattern_names(&param.pattern, &mut names);
            frame.extend(names);
        }
        self.scopes.push(frame);
        walk::walk_catch_clause(self, it);
        self.scopes.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &it.init {
            if !matches!(decl.kind, VariableDeclarationKind::Var) {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    frame.extend(names);
                }
            }
        }
        self.scopes.push(frame);
        walk::walk_for_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_of_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_in_statement(self, it);
        self.scopes.pop();
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        // An unresolved reference to a rune name is a rune use. A reference that
        // resolves to a local binding of the same name (the function-param `$state`)
        // is shadowed and does NOT count.
        if self.is_unshadowed_rune(it.name.as_str()) {
            self.used = true;
        }
        walk::walk_identifier_reference(self, it);
    }
}

/// Collect the byte spans of the rune-ROOT identifier references that occupy a
/// SUPPORTED rune position in a `program`. A bare rune is supported ONLY in its
/// exact legal position:
///
/// - `$state` — the init of a TOP-LEVEL instance-script IDENTIFIER declarator
///   (`let c = $state(0)`; `$state.raw` is a supported POSITION even though it is
///   later refused as an advanced FORM — position and form are orthogonal; the
///   non-primitive `$state` init is refused at the state-shape gate);
/// - `$props` — the init of a TOP-LEVEL instance-script DESTRUCTURE declarator
///   (`let { a } = $props()`); the shape's validity is enforced separately by
///   `props_shape`.
///
/// `$derived` and `$effect` have NO supported position — they are deferral-ledger
/// follow-ups (5g), so every `$derived` / `$effect` reference (in any position)
/// fails closed at the position scan BY CONSTRUCTION.
///
// TODO(follow-up): lower `$derived(e)` / `$derived.by(fn)` → `$.derived(() => e)` /
// `$.derived(fn)` read via `$.get`, and `$effect(fn)` → `$.user_effect(fn)` with the
// `$.push`/`$.pop` component context (the runes effect topology), instead of failing
// closed. Owned by the runes-completion block (5g).
///
/// `is_instance` is `false` for a `<script module>` program and for a wrapped
/// template expression — neither hosts ANY supported rune position, so the set is
/// empty there and every rune reference refuses. The returned spans are the OXC
/// callee/object IDENTIFIER spans, matched against an [`IdentifierReference`]'s
/// span during the walk.
fn supported_rune_root_spans(program: &Program<'_>, is_instance: bool) -> FxHashSet<(u32, u32)> {
    let mut spans = FxHashSet::default();
    if !is_instance {
        // A module-script program / a wrapped template expression hosts no
        // supported rune position — every rune reference refuses.
        return spans;
    }
    for stmt in &program.body {
        // A top-level variable declaration: an identifier declarator init of
        // `$state` (incl. the `.raw` member form), or a destructure declarator init
        // of `$props()`. `$derived` / `$effect` have NO supported position.
        if let Statement::VariableDeclaration(decl) = stmt {
            for d in &decl.declarations {
                let Some(Expression::CallExpression(call)) = &d.init else {
                    continue;
                };
                let is_ident = matches!(&d.id, BindingPattern::BindingIdentifier(_));
                let is_destructure = matches!(
                    &d.id,
                    BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_)
                );
                // `$state` / `$state.raw` in an IDENTIFIER declarator position.
                if is_ident && state_rune_call(call).is_some() {
                    if let Some(span) = rune_root_ident_span(&call.callee, "$state") {
                        spans.insert(span);
                    }
                }
                // `$props()` in a DESTRUCTURE declarator position.
                if is_destructure && is_props_callee(&call.callee) {
                    if let Some(span) = rune_root_ident_span(&call.callee, "$props") {
                        spans.insert(span);
                    }
                }
            }
        }
    }
    spans
}

/// The span of the rune-ROOT identifier `root` in a callee that is either a bare
/// identifier (`$state(...)`) or a static member whose object is that identifier
/// (`$state.raw(...)` / `$derived.by(...)`). `None` for any other callee shape.
fn rune_root_ident_span(callee: &Expression<'_>, root: &str) -> Option<(u32, u32)> {
    match callee {
        Expression::Identifier(id) if id.name.as_str() == root => {
            Some((id.span.start, id.span.end))
        }
        Expression::StaticMemberExpression(m) => match &m.object {
            Expression::Identifier(id) if id.name.as_str() == root => {
                Some((id.span.start, id.span.end))
            }
            _ => None,
        },
        _ => None,
    }
}

/// A SCOPE-AWARE, POSITION-SENSITIVE scan for an UNSUPPORTED rune FORM or an
/// unsupported rune POSITION — a rune the client backend does not emit. It records
/// the FIRST unsupported occurrence it sees (as a typed
/// [`UnsupportedSvelteRuntimeSurface`]), so the fail-closed gate refuses the
/// component instead of emitting a raw `ReferenceError`-bound rune.
///
/// Two orthogonal axes are refused:
///
/// - an unsupported rune FORM — an advanced member (`$state.snapshot` /
///   `$effect.pre` / `$props.id` / …, with `$derived.by` the single supported
///   member form) or an advanced bare call (`$host(...)` / a standalone
///   `$bindable(...)`). The `$inspect` family is NOT refused here: it is
///   SUPPORTED as production ELISION (the statement-position `$inspect(...)` /
///   `$inspect(...).with(...)` / body `$inspect.trace()` forms are elided by
///   the instance-item classifier / the body rewriter, and a
///   non-statement-position `$inspect` reference fails closed at the rewriter);
/// - an unsupported rune POSITION — a bare `$state` / `$derived` / `$props` /
///   `$effect` reference that is NOT in its exact legal supported position (a
///   default-param `$state(0)`, a call-arg `$props()`, a function-body `$effect`, a
///   module-script rune, a bare-identifier `foo($state)`). The supported positions
///   are pre-collected into [`Self::supported`] from the INSTANCE program's
///   top-level structure; everything else fails closed (default-deny).
///
/// A rune name SHADOWED by a local binding (a function param `$state`, a nested
/// `let $inspect`) is NOT a rune reference and is never refused — the scan reuses
/// the SAME lexical `ShadowStack` model as [`ScopeAwareRuneDetector`].
#[derive(Default)]
pub(super) struct UnsupportedRuneScan {
    /// The first unsupported rune form / position found.
    found: Option<UnsupportedSvelteRuntimeSurface>,
    /// The active lexical-scope shadow stack.
    scopes: ShadowStack,
    /// The byte spans of the rune-root identifier references in a SUPPORTED
    /// position (empty for a module-script / template-expression scan, so every
    /// rune reference there refuses).
    supported: FxHashSet<(u32, u32)>,
}

impl UnsupportedRuneScan {
    /// Build a scan whose supported-position set is derived from `program`.
    /// `is_instance` marks the instance-script program (the only program with
    /// supported rune positions); a module-script / wrapped-template-expression
    /// program passes `false`, so its supported set is empty and every rune
    /// reference refuses.
    pub(super) fn for_program(program: &Program<'_>, is_instance: bool) -> Self {
        Self {
            found: None,
            scopes: ShadowStack::default(),
            supported: supported_rune_root_spans(program, is_instance),
        }
    }

    /// The unsupported surface found, if any.
    pub(super) fn into_surface(self) -> Option<UnsupportedSvelteRuntimeSurface> {
        self.found
    }

    /// Whether `name` is an unshadowed rune root reference.
    fn is_unshadowed_rune(&self, name: &str) -> bool {
        RUNE_ROOT_NAMES.contains(&name) && !self.scopes.is_shadowed(name)
    }

    /// Record `surface` as the first unsupported form (later finds are ignored).
    fn record(&mut self, surface: UnsupportedSvelteRuntimeSurface) {
        if self.found.is_none() {
            self.found = Some(surface);
        }
    }

    /// Classify a bare rune-root IDENTIFIER reference by POSITION: a reference in a
    /// supported position (its span is in [`Self::supported`]) is fine; a reference
    /// anywhere else is an unsupported rune form (`$state` in a default-param, a
    /// module-script rune, `foo($state)`, …) and fails closed. Only the
    /// position-sensitive runes (`$state` / `$derived` / `$props` / `$effect`) are
    /// classified here; the advanced-only runes (`$bindable` / `$host`) are
    /// classified by [`Self::classify_rune_call`] / member handling and have no
    /// supported bare position at all, and the production-elided `$inspect`
    /// family is owned by the instance-item classifier / the body rewriter.
    fn classify_rune_position(&mut self, root: &str, span: Span) {
        if !matches!(root, "$state" | "$derived" | "$props" | "$effect") {
            return;
        }
        if self.supported.contains(&(span.start, span.end)) {
            return;
        }
        let rune: &'static str = match root {
            "$state" => "$state",
            "$derived" => "$derived",
            "$props" => "$props",
            "$effect" => "$effect",
            _ => return,
        };
        self.record(UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, span });
    }

    /// Classify a member expression whose object is an unshadowed rune root
    /// (`$state.raw`, `$effect.pre`, `$props.id`, …). The supported member form is
    /// `$derived.by` (handled by the binding classifier); everything else is an
    /// unsupported rune surface.
    fn classify_rune_member(&mut self, root: &str, member: &str, span: Span) {
        let surface = match (root, member) {
            // `$derived.by` is the supported member form (a Derived binding).
            ("$derived", "by") => return,
            // `$inspect.<member>` (incl. `.trace`) is never refused here: the
            // `$inspect` family is SUPPORTED as production ELISION. A
            // statement-position `$inspect.trace()` is dropped in place by the
            // shared body rewriter; a non-statement-position reference fails
            // closed at the rewriter (never emitted raw).
            ("$inspect", _) => return,
            // Experimental-async rune members (5j).
            ("$state", "eager") => UnsupportedSvelteRuntimeSurface::ExperimentalAsync {
                surface: "$state.eager",
                span,
            },
            ("$effect", "pending") => UnsupportedSvelteRuntimeSurface::ExperimentalAsync {
                surface: "$effect.pending",
                span,
            },
            // Advanced `$state` / `$effect` / `$props` members (5g).
            ("$state", "raw") => UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$state.raw",
                span,
            },
            ("$state", "snapshot") => UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$state.snapshot",
                span,
            },
            ("$effect", "pre") => UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$effect.pre",
                span,
            },
            ("$effect", "root") => UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$effect.root",
                span,
            },
            ("$effect", "tracking") => UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$effect.tracking",
                span,
            },
            ("$props", "id") => UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$props.id",
                span,
            },
            // Any other member of a rune root is an advanced rune form.
            (root, member) => {
                let rune: &'static str = match root {
                    "$state" => "$state.<member>",
                    "$effect" => "$effect.<member>",
                    "$props" => "$props.<member>",
                    "$derived" => "$derived.<member>",
                    _ => "$rune.<member>",
                };
                let _ = member;
                UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, span }
            }
        };
        self.record(surface);
    }

    /// Classify a bare-rune-root reference used in a non-supported position (a
    /// `$host()` call, a `$bindable(...)` outside a `$props()` default). The
    /// supported bare calls (`$state` / `$derived` / `$props` / `$effect`) are
    /// skipped by the caller before reaching here. A `$inspect(...)` call is NOT
    /// refused: the `$inspect` family is SUPPORTED as production ELISION (the
    /// statement-position forms are elided by the instance-item classifier / the
    /// body rewriter; a non-statement-position reference fails closed at the
    /// rewriter, never emitted raw).
    fn classify_rune_call(&mut self, root: &str, span: Span) {
        let surface = match root {
            "$host" => UnsupportedSvelteRuntimeSurface::HostOrCustomElement {
                surface: "$host",
                span,
            },
            "$bindable" => UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$bindable",
                span,
            },
            _ => return,
        };
        self.record(surface);
    }
}

impl<'a> Visit<'a> for UnsupportedRuneScan {
    fn visit_program(&mut self, it: &Program<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        collect_direct_decls(&it.body, &mut frame);
        collect_var_hoists(&it.body, &mut frame);
        self.scopes.push(frame);
        walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.scopes.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.scopes.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(param) = &it.param {
            let mut names = Vec::new();
            collect_pattern_names(&param.pattern, &mut names);
            frame.extend(names);
        }
        self.scopes.push(frame);
        walk::walk_catch_clause(self, it);
        self.scopes.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &it.init {
            if !matches!(decl.kind, VariableDeclarationKind::Var) {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    frame.extend(names);
                }
            }
        }
        self.scopes.push(frame);
        walk::walk_for_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_of_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_in_statement(self, it);
        self.scopes.pop();
    }

    fn visit_member_expression(&mut self, it: &MemberExpression<'a>) {
        // A member access on an unshadowed rune root (`$state.raw`, `$effect.pre`,
        // `$props.id`, …) — classify the unsupported FORM (recorded BEFORE the walk
        // descends into the object identifier, so the form-specific diagnostic wins
        // over the per-identifier position diagnostic). `$derived.by` is the single
        // supported member form (skipped in `classify_rune_member`).
        if let MemberExpression::StaticMemberExpression(m) = it {
            if let Expression::Identifier(obj) = &m.object {
                let root = obj.name.as_str();
                if self.is_unshadowed_rune(root) {
                    self.classify_rune_member(root, m.property.name.as_str(), to_span(m.span));
                }
            }
        }
        walk::walk_member_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &oxc_ast::ast::CallExpression<'a>) {
        // A BARE rune call to an ADVANCED-only rune (`$host(...)`, a standalone
        // `$bindable(...)`) has no supported bare position at all — classify it
        // here (`$inspect(...)` is production-elided, never refused here). The
        // position-sensitive runes (`$state` / `$derived` / `$props` / `$effect`)
        // are classified per-identifier in `visit_identifier_reference` (a
        // supported position is exempt; everything else refuses), so a `$state(0)`
        // in a default-param / call-arg fails closed.
        if let Expression::Identifier(id) = &it.callee {
            let root = id.name.as_str();
            if self.is_unshadowed_rune(root)
                && !matches!(root, "$state" | "$derived" | "$props" | "$effect")
            {
                self.classify_rune_call(root, to_span(it.span));
            }
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        // A POSITION-SENSITIVE rune reference: an unshadowed `$state` / `$derived` /
        // `$props` / `$effect` identifier is supported ONLY when its byte span is in
        // the pre-collected supported-position set (a top-level instance-script
        // declarator init for `$state` / `$derived`, the single top-level
        // destructure for `$props()`, a top-level statement for `$effect`); a
        // reference anywhere else (a default-param `$state(0)`, a call-arg
        // `$props()`, a module-script rune, a bare-identifier `foo($state)`) fails
        // closed. The advanced-only runes (`$bindable` / `$host`) are refused by
        // the call / member handlers, never here; the production-elided `$inspect`
        // family is owned by the instance-item classifier / the body rewriter.
        let name = it.name.as_str();
        if self.is_unshadowed_rune(name) {
            self.classify_rune_position(name, to_span(it.span));
        }
        walk::walk_identifier_reference(self, it);
    }
}

/// Convert an OXC span to a verter span.
fn to_span(span: oxc_span::Span) -> Span {
    Span::new(span.start, span.end)
}
