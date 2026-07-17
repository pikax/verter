//! Svelte 5 rune-position and unsupported-form classification.
//!
//! The component-mode decision lives in
//! [`verter_parser::svelte_reactivity`]. This module owns the runtime compiler's
//! supported-position inventory and unsupported rune diagnostics after that
//! shared mode decision has been made.

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
    arrow_scope_names, bindable_call_is_well_formed, block_scope_names, collect_direct_decls,
    collect_pattern_names, collect_var_hoists, effect_family_call_fact, for_left_names,
    function_scope_names, is_bindable_call, is_props_callee, is_props_id_callee, peel_parens,
    props_id_call_is_well_formed, state_rune_call, statement_position_user_effect_span,
    EffectFamilyCallKind, ShadowStack,
};

/// The Svelte 5 rune NAMES (`compiler/utils.js` `RUNES`, minus the `$state.snapshot`
/// / `.raw` / `.by` member keypaths the detector reaches through the root
/// identifier). A component is in runes mode iff any of these appears as an
/// UNRESOLVED reference (not bound to a local) anywhere in a script — matching the
/// official `Array.from(scope.references.keys()).some(is_rune)`.
pub(super) const RUNE_ROOT_NAMES: &[&str] = verter_parser::svelte_reactivity::RUNE_ROOT_NAMES;

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
/// `$derived` has NO supported position — it is a deferral-ledger follow-up, so
/// every `$derived` reference (in any position) fails closed at the position scan
/// BY CONSTRUCTION. A bare `$effect` identifier likewise has no PRE-COLLECTED
/// position here: its supported positions are the WELL-FORMED family CALL callee
/// spans ([`super::expr::effect_family_call_fact`]), admitted into the same
/// supported set DURING the walk by [`UnsupportedRuneScan::visit_call_expression`].
/// The admission is position-checked per member: `$effect(fn)` / `$effect.pre(fn)`
/// are STATEMENT-ONLY (official `effect_invalid_placement` rejects every value
/// position), while `$effect.root(fn)` / `$effect.tracking()` are
/// expression-valued at any depth. A non-callee `$effect` reference still
/// refuses.
///
// TODO(follow-up): lower `$derived(e)` / `$derived.by(fn)` → `$.derived(() => e)` /
// `$.derived(fn)` read via `$.get`, instead of failing closed. Owned by the
// runes-completion block (5g).
///
/// `is_instance` is `false` for a `<script module>` program and for a wrapped
/// template expression — neither hosts ANY supported PRE-COLLECTED rune position,
/// so the set starts empty there and every rune reference outside the walk-time
/// effect-family call exemption refuses. The returned spans are the OXC
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
                // `$props()` in a DESTRUCTURE declarator position OR a WHOLE-OBJECT
                // IDENTIFIER capture (`let all = $props()`) — both lower through the
                // `$.rest_props` / prop-read path (the shape gate already accepted
                // the identifier form as a basic capture).
                if (is_destructure || is_ident) && is_props_callee(&call.callee) {
                    if let Some(span) = rune_root_ident_span(&call.callee, "$props") {
                        spans.insert(span);
                    }
                }
            }
        }
    }
    spans
}

/// Collect the SUPPORTED-POSITION `$props.id(...)` and `$bindable(...)` CALL
/// spans of an instance program (empty for a module-script / template-expression
/// program):
///
/// - `$props.id(...)` — the init of a TOP-LEVEL `let`/`const` IDENTIFIER
///   declarator, matched through the STRICT shared callee predicate
///   ([`is_props_id_callee`]: no parens, no optional chaining). A `var`
///   declarator stays OUT (the non-`let` rune-declarator boundary), as does any
///   non-identifier declarator id. Position only — the call FORM (arity /
///   spread / optionality) is validated at the call visitor.
/// - `$bindable(...)` — a DIRECT `$props()` destructure member DEFAULT
///   (`let { x = $bindable(...) } = $props()`), matched through the strict
///   bare-identifier callee ([`is_bindable_call`]: no parens). A parenthesized
///   default spelling (`($bindable)(0)` / `($bindable(0))`) never enters the
///   set, so it fails closed at the visitors. Position only — form is validated
///   at the call visitor.
fn supported_props_rune_call_spans(
    program: &Program<'_>,
    is_instance: bool,
) -> SupportedPropsRuneSpans {
    let mut props_id = FxHashSet::default();
    let mut bindable = FxHashSet::default();
    if !is_instance {
        return SupportedPropsRuneSpans { props_id, bindable };
    }
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let Some(Expression::CallExpression(call)) = &d.init else {
                continue;
            };
            // `$props.id(...)` in a `let`/`const` IDENTIFIER-declarator init.
            if matches!(
                decl.kind,
                VariableDeclarationKind::Let | VariableDeclarationKind::Const
            ) && matches!(&d.id, BindingPattern::BindingIdentifier(_))
                && is_props_id_callee(&call.callee)
            {
                props_id.insert((call.span.start, call.span.end));
            }
            // `$bindable(...)` as a direct `$props()` destructure member default.
            if !is_props_callee(&call.callee) {
                continue;
            }
            if let BindingPattern::ObjectPattern(obj) = &d.id {
                for prop in &obj.properties {
                    if let BindingPattern::AssignmentPattern(assign) = &prop.value {
                        if is_bindable_call(&assign.right) {
                            if let Expression::CallExpression(bc) = &assign.right {
                                bindable.insert((bc.span.start, bc.span.end));
                            }
                        }
                    }
                }
            }
        }
    }
    SupportedPropsRuneSpans { props_id, bindable }
}

/// The SUPPORTED-POSITION call-span sets [`supported_props_rune_call_spans`]
/// collects: the `$props.id(...)` declarator-init spans and the `$bindable(...)`
/// destructure-default spans.
struct SupportedPropsRuneSpans {
    props_id: FxHashSet<(u32, u32)>,
    bindable: FxHashSet<(u32, u32)>,
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
/// the SAME lexical `ShadowStack` model as the unsupported-rune classifier.
#[derive(Default)]
pub(super) struct UnsupportedRuneScan {
    /// The first unsupported rune form / position found.
    found: Option<UnsupportedSvelteRuntimeSurface>,
    /// The active lexical-scope shadow stack.
    scopes: ShadowStack,
    /// The byte spans of the rune-root identifier references in a SUPPORTED
    /// position. Seeded from the program's top-level structure (`$state` /
    /// `$props()` declarator inits; empty for a template-expression scan) and
    /// EXTENDED during the walk by the effect-family call exemption (a
    /// well-formed STATEMENT-POSITION `$effect(fn)` callee is a supported
    /// position at any depth).
    supported: FxHashSet<(u32, u32)>,
    /// The call spans of STATEMENT-POSITION `$effect(...)` / `$effect.pre(...)`
    /// calls — recorded by `visit_expression_statement` BEFORE the call visitor
    /// reaches the call. The user-effect members are statement-ONLY (official
    /// `effect_invalid_placement`); a family call whose span is NOT in this set
    /// is a value position and refuses.
    stmt_effect_spans: FxHashSet<(u32, u32)>,
    /// The SUPPORTED-POSITION `$props.id(...)` call spans (a top-level instance
    /// `let`/`const` IDENTIFIER-declarator init). A `$props.id` call whose span
    /// is NOT in this set is an invalid placement and refuses (official
    /// `props_id_invalid_placement`).
    props_id_positions: FxHashSet<(u32, u32)>,
    /// Whether the component compiles as a CUSTOM ELEMENT (a resolved
    /// `<svelte:options customElement>` / compile-option descriptor). The
    /// zero-arg `$host()` call is supported ONLY under an active custom element
    /// (official `host_invalid_placement` otherwise); the flag gates the call
    /// visitor's `$host` admission.
    custom_element_active: bool,
    /// Whether an ADMITTED `$host()` call was seen — the zero-arg, non-optional
    /// call under an active custom element (the ONLY accepted `$host` form; any
    /// other `$host` shape records a refusal instead). The plan build consumes
    /// this FACT to decide the `$$props`-parameter question (the rewritten
    /// `$$props.$$host` member must never reference an unbound `$$props`, so a
    /// host use without an independent props-parameter trigger fails closed) —
    /// recorded here, in the one existing rune scan, so no later stage re-parses
    /// source to re-discover the usage.
    uses_host: bool,
    /// The span of the FIRST admitted `$host()` call (in the coordinates of the
    /// scanned program — the instance script or the wrapped template
    /// expression), carried so the plan build's degenerate-host refusal points
    /// at the offending call instead of a zero span.
    first_host_span: Option<Span>,
    /// Whether a valid `$props.id()` use was already accepted — a SECOND valid
    /// use refuses (official `props_duplicate`).
    props_id_used: bool,
    /// The SUPPORTED-POSITION `$bindable(...)` call spans (a direct `$props()`
    /// destructure member default). A `$bindable` call whose span is NOT in this
    /// set is an invalid location and refuses (official
    /// `bindable_invalid_location`).
    bindable_positions: FxHashSet<(u32, u32)>,
    /// Whether the PROGRAM-DIRECT statements are the SYNTHETIC wrapper of a
    /// WRAPPED template-expression program (`({expr});`) — never authored
    /// statement positions, so the program walk bypasses the statement-position
    /// seed for EVERY program-direct statement (a handler value
    /// `onclick={$effect(fn)}` must not read as a statement; a brace-matched
    /// expression source that smuggles extra `);(`-separated statements past the
    /// wrap stays a value position too). `false` for the instance program, whose
    /// top-level statements are real.
    synthetic_program_stmts: bool,
    /// The STORE-EXEMPT accessor names (`$state` whose base `state` is a
    /// declared store candidate — see
    /// [`rune_root_accessor_exemptions`](super::store_subscriptions::rune_root_accessor_exemptions)):
    /// such a reference is a STORE ACCESSOR, not a rune, so the position /
    /// member / call classification skips it entirely (the scope-aware store
    /// classifier owns its accept/reject decision — a base-shadowed reference
    /// rejects there as the official `store_invalid_scoped_subscription`).
    store_exempt: FxHashSet<String>,
}

impl UnsupportedRuneScan {
    /// Build a scan whose supported-position set is derived from `program`.
    /// `is_instance` marks the instance-script program (the only program with
    /// supported rune positions, and the only one whose program-direct
    /// statements are REAL statement positions); a wrapped-template-expression
    /// program passes `false`, so its supported set is empty, every rune
    /// reference outside the walk-time call exemptions refuses, and its
    /// program-direct statements are the SYNTHETIC `({expr});` wrapper (never a
    /// statement position). (A `<script module>` never reaches this scan — it is
    /// refused upstream as the script-hoisting deferral.)
    /// `custom_element_active` marks a component with a resolved custom-element
    /// descriptor — the only context whose zero-arg `$host()` call is admitted.
    pub(super) fn for_program(
        program: &Program<'_>,
        is_instance: bool,
        custom_element_active: bool,
        store_exempt: FxHashSet<String>,
    ) -> Self {
        let props_rune_spans = supported_props_rune_call_spans(program, is_instance);
        Self {
            found: None,
            scopes: ShadowStack::default(),
            supported: supported_rune_root_spans(program, is_instance),
            stmt_effect_spans: FxHashSet::default(),
            props_id_positions: props_rune_spans.props_id,
            props_id_used: false,
            bindable_positions: props_rune_spans.bindable,
            synthetic_program_stmts: !is_instance,
            custom_element_active,
            uses_host: false,
            first_host_span: None,
            store_exempt,
        }
    }

    /// The unsupported surface found, if any.
    pub(super) fn into_surface(self) -> Option<UnsupportedSvelteRuntimeSurface> {
        self.found
    }

    /// Whether the walk admitted a `$host()` call (the zero-arg form under an
    /// active custom element) — the `$host`-usage FACT the plan build consumes.
    pub(super) fn uses_host(&self) -> bool {
        self.uses_host
    }

    /// The span of the first admitted `$host()` call, if any (program-relative).
    pub(super) fn first_host_span(&self) -> Option<Span> {
        self.first_host_span
    }

    /// Whether `name` is an unshadowed rune root reference. A STORE-EXEMPT
    /// accessor name (its base is a declared store candidate) is a store
    /// accessor, never a rune — the scan skips it wholesale.
    fn is_unshadowed_rune(&self, name: &str) -> bool {
        RUNE_ROOT_NAMES.contains(&name)
            && !self.store_exempt.contains(name)
            && !self.scopes.is_shadowed(name)
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
    /// module-script rune, `foo($state)`, …) and fails closed. The
    /// position-sensitive runes (`$state` / `$derived` / `$props` / `$effect`),
    /// `$bindable`, and `$host` are classified here — ALL unshadowed known rune
    /// roots fail closed unless in a supported position. `$bindable` has NO
    /// supported BARE position at all (its CALLED form is consumed by the call
    /// visitor, so a bare reference reaching here is the UNCALLED rune — official
    /// `rune_missing_parentheses` — or a paren-wrapped callee spelling, both
    /// fail-closed). `$host` likewise has NO supported bare position: its
    /// supported form is the zero-arg CALL inside an active customElement
    /// component, whose callee span the call visitor admits into the supported
    /// set — an UNCALLED `$host` (official `rune_missing_parentheses`) and the
    /// parenthesized-callee `($host)()` spelling (official
    /// `host_invalid_placement`; the inner bare reference lands here) both fail
    /// closed. The CALLED `$host(...)` forms are classified by
    /// [`Self::classify_rune_call`], and the production-elided `$inspect` family
    /// is owned by the instance-item classifier / the body rewriter.
    fn classify_rune_position(&mut self, root: &str, span: Span) {
        if !matches!(
            root,
            "$state" | "$derived" | "$props" | "$effect" | "$bindable" | "$host"
        ) {
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
            "$bindable" => "$bindable",
            "$host" => "$host",
            _ => return,
        };
        self.record(UnsupportedSvelteRuntimeSurface::AdvancedRune { rune, span });
    }

    /// Classify a member expression whose (paren-peeled) object is an unshadowed
    /// rune root (`$state.raw`, `$effect.pre`, `($effect).pending`, `$props.id`, …).
    /// Returns `true` when the caller must NOT descend into the object — no current
    /// form does: every classified form returns `false` and the caller descends. A
    /// supported form (`$derived.by`, `$state.raw`, `$inspect.*`) records nothing and
    /// lets the object be position-scanned (a `$state.raw` declarator's `$state` is in
    /// the supported-position set; a bare `$derived` is refused there, unchanged); an
    /// unsupported form records the precise member refusal BEFORE the descent, so
    /// first-found-wins keeps it over the coarser receiver position diagnostic the
    /// descent may record.
    fn classify_rune_member(&mut self, root: &str, member: &str, span: Span) -> bool {
        let surface = match (root, member) {
            // `$state.snapshot` reaches this MEMBER handler ONLY in an UNCALLED
            // value position (`x = $state.snapshot`, `foo($state.snapshot)`): the
            // supported CALLED form `$state.snapshot(x)` is exempted upstream by
            // `visit_call_expression` (which never descends into the callee member).
            // Only the called form is supported (the client expression rewriter
            // rewrites the callee to `$.snapshot`); an uncalled `$state.snapshot` is
            // an advanced rune the client backend does NOT emit — official errors on
            // it (`rune_missing_parentheses`: "Cannot use rune without parentheses").
            // Record the refusal (do NOT stop the descent) so it fails closed rather
            // than slipping past the scan and emitting a raw `$state.snapshot`
            // ReferenceError.
            ("$state", "snapshot") => UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$state.snapshot",
                span,
            },
            // `$derived.by` is the supported member form (a Derived binding); descend so
            // a bare `$derived` in a non-declarator position is still refused.
            ("$derived", "by") => return false,
            // `$inspect.<member>` (incl. `.trace`) is never refused here: the `$inspect`
            // family is SUPPORTED as production ELISION. A statement-position
            // `$inspect.trace()` is dropped in place by the shared body rewriter; a
            // non-statement-position reference fails closed at the rewriter. `$inspect`
            // is not position-sensitive, so the descent is harmless.
            ("$inspect", _) => return false,
            // `$state.raw(...)` is a SUPPORTED state declarator flavour (the raw opt-out):
            // the binding classifier reads the flavour via `state_rune_call` and lowers it
            // to a `$.state(<init>)` signal (no `$.proxy`). The `$state` receiver IS a
            // declarator position (in the supported-position set), so the descent is safe.
            ("$state", "raw") => return false,
            // Experimental-async rune members (5j).
            ("$state", "eager") => UnsupportedSvelteRuntimeSurface::ExperimentalAsync {
                surface: "$state.eager",
                span,
            },
            ("$effect", "pending") => UnsupportedSvelteRuntimeSurface::ExperimentalAsync {
                surface: "$effect.pending",
                span,
            },
            // The `$effect.pre` / `$effect.root` / `$effect.tracking` family
            // members reach this MEMBER handler ONLY in an UNCALLED value position
            // (`const f = $effect.pre;`, `foo($effect.tracking)`): the CALLED
            // forms — well-formed AND malformed — are consumed upstream by
            // `visit_call_expression` (which never descends into the callee
            // member). Only the called well-formed form is supported (the client
            // expression rewriter rewrites the callee to `$.user_pre_effect` /
            // `$.effect_root` / `$.effect_tracking`); an uncalled member is an
            // advanced rune the client backend does NOT emit — official errors on
            // it (`rune_missing_parentheses`). Record the refusal (do NOT stop the
            // descent) so it fails closed rather than emitting a raw rune member.
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
            // `$props.id` reaches this MEMBER handler ONLY as an UNCALLED value
            // reference (`const f = $props.id;` — official
            // `rune_missing_parentheses`) or through a PARENTHESIZED callee
            // spelling (`($props.id)()` / `($props).id()` — the strict supported
            // spelling does not peel): the supported CALLED form `$props.id()`
            // is consumed upstream by `visit_call_expression` (which validates
            // form + position + single use and never descends into the callee
            // member). Record the refusal (do NOT stop the descent) so it fails
            // closed rather than emitting a raw rune member.
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
        false
    }

    /// Classify a bare-rune-root reference used in a non-supported position (a
    /// `$host(...)` call outside its one admitted form — the zero-arg,
    /// non-optional call inside an ACTIVE customElement component, which the
    /// call visitor admits into the supported set instead of routing here). The
    /// supported bare calls (`$state` / `$derived` / `$props` / `$effect`) are
    /// skipped by the caller before reaching here, and a `$bindable(...)` call
    /// is consumed by its POSITION-AWARE arm in `visit_call_expression` (valid
    /// only as a `$props()` destructure-member default). A `$inspect(...)` call
    /// is NOT refused: the `$inspect` family is SUPPORTED as production ELISION
    /// (the statement-position forms are elided by the instance-item classifier
    /// / the body rewriter; a non-statement-position reference fails closed at
    /// the rewriter, never emitted raw).
    fn classify_rune_call(&mut self, root: &str, span: Span) {
        let surface = match root {
            "$host" => UnsupportedSvelteRuntimeSurface::HostOrCustomElement {
                surface: "$host",
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
        if self.synthetic_program_stmts {
            // The program-direct statements of a WRAPPED template-expression
            // program (`({expr});`) are the SYNTHETIC wrapper — never authored
            // statement positions — so their INNER expressions are visited
            // directly, bypassing the statement-position seed in
            // `visit_expression_statement` (nested REAL statements inside the
            // expression still seed normally). This holds for EVERY
            // program-direct statement, so a brace-matched expression source
            // that smuggles extra statements past the wrap never gains a
            // statement position either.
            for stmt in &it.body {
                match stmt {
                    Statement::ExpressionStatement(es) => self.visit_expression(&es.expression),
                    other => self.visit_statement(other),
                }
            }
        } else {
            walk::walk_program(self, it);
        }
        self.scopes.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.scopes.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(it));
        if it.r#expression {
            // A CONCISE (expression-bodied) arrow: OXC models the body as ONE
            // synthetic `ExpressionStatement`, but it is an EXPRESSION position —
            // official rejects a user-effect call there
            // (`effect_invalid_placement`). Visit the params and the body
            // EXPRESSION directly so the statement admission below never fires
            // on the synthetic statement (the same bypass the emission-grade
            // occurrence collector uses).
            self.visit_formal_parameters(&it.params);
            if let [Statement::ExpressionStatement(stmt)] = it.body.statements.as_slice() {
                self.visit_expression(&stmt.expression);
            } else {
                self.visit_function_body(&it.body);
            }
        } else {
            walk::walk_arrow_function_expression(self, it);
        }
        self.scopes.pop();
    }

    fn visit_expression_statement(&mut self, it: &oxc_ast::ast::ExpressionStatement<'a>) {
        // A statement-position `$effect(...)` / `$effect.pre(...)` call — the ONE
        // official-legal position for the user-effect members
        // (`effect_invalid_placement` is a direct-parent rule; parens are
        // transparent). Record the call span BEFORE the walk descends so the
        // call visitor admits it. (The SYNTHETIC program-direct statements of a
        // wrapped template expression never reach this visitor — the program
        // walk visits their inner expressions directly.)
        if let Some(span) = statement_position_user_effect_span(&it.expression) {
            self.stmt_effect_spans.insert((span.start, span.end));
        }
        walk::walk_expression_statement(self, it);
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
        // `$props.id`, …) — classify the FORM (recorded BEFORE the walk descends into
        // the object identifier, so the form-specific diagnostic wins over the
        // per-identifier position diagnostic). The RECEIVER is paren-transparent
        // (official's ESTree AST has no paren nodes, so `($effect).pending` is the
        // SAME member surface as `$effect.pending` — the shared peel keeps the full
        // paren-inclusive member span on the diagnostic). An UNCALLED
        // `$state.snapshot` reaching here (a value position — the CALLED form is
        // exempted at `visit_call_expression`) records the `$state.snapshot` refusal
        // and descends normally (its `$state` receiver would ALSO refuse as an
        // unsupported position, but the earlier `$state.snapshot` form diagnostic
        // wins). Every other form descends normally.
        if let MemberExpression::StaticMemberExpression(m) = it {
            if let Expression::Identifier(obj) = peel_parens(&m.object) {
                let root = obj.name.as_str();
                if self.is_unshadowed_rune(root)
                    && self.classify_rune_member(root, m.property.name.as_str(), to_span(m.span))
                {
                    return;
                }
            }
        }
        walk::walk_member_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &oxc_ast::ast::CallExpression<'a>) {
        // A `$state.snapshot(...)` CALL. Only the WELL-FORMED single-non-spread-arg
        // form is SUPPORTED (the client expression rewriter rewrites the callee to
        // `$.snapshot`): EXEMPT its callee member — do NOT let the walk descend into it
        // (which would refuse the `$state.snapshot` FORM as an uncalled advanced rune) —
        // but DO scan the ARGUMENT for a nested unsupported rune
        // (`$state.snapshot($host())` still refuses). BOTH callee paren positions are
        // transparent — the RECEIVER (`($state).snapshot(x)`) and the WHOLE CALLEE
        // (`($state.snapshot)(x)`), at any nesting depth — official's ESTree AST has
        // no paren nodes, so every spelling is the same call (oracle-verified accepts
        // against `svelte@5.56.3`), and the rewriter's callee matcher peels the SAME
        // way, so the scan model and the `$.snapshot` rewrite agree. Only the CALLED
        // form reaches here; an UNCALLED `$state.snapshot` (value position, however
        // parenthesized) has no enclosing call, so it reaches
        // `visit_member_expression` and fails closed.
        //
        // A MALFORMED call — ZERO args / >=2 args (official `rune_invalid_arguments_length`)
        // or a SPREAD arg (official `rune_invalid_spread`), both oracle-verified against
        // `svelte@5.56.3` at every paren position — must FAIL CLOSED as an advanced rune
        // rather than slip past the exemption into a raw `$.snapshot()` / `$.snapshot(a, b)`
        // / `$.snapshot(...o)` miscompile: record the `$state.snapshot` refusal
        // (first-found wins, so it is the reported surface) and STILL scan the
        // arguments for nested runes.
        if let Expression::StaticMemberExpression(m) = peel_parens(&it.callee) {
            if let Expression::Identifier(obj) = peel_parens(&m.object) {
                if self.is_unshadowed_rune(obj.name.as_str())
                    && obj.name.as_str() == "$state"
                    && m.property.name.as_str() == "snapshot"
                {
                    let well_formed =
                        it.arguments.len() == 1 && it.arguments[0].as_expression().is_some();
                    if !well_formed {
                        self.record(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                            rune: "$state.snapshot",
                            span: to_span(it.span),
                        });
                    }
                    for arg in &it.arguments {
                        match arg.as_expression() {
                            Some(expr) => self.visit_expression(expr),
                            None => {
                                if let oxc_ast::ast::Argument::SpreadElement(s) = arg {
                                    self.visit_expression(&s.argument);
                                }
                            }
                        }
                    }
                    return;
                }
            }
        }
        // A `$props.id(...)` CALL through the STRICT callee spelling (the bare
        // `$props` object + `.id`, no parens, no optional member) on the
        // unshadowed `$props` root. Supported ONLY as a WELL-FORMED (zero-arg,
        // non-optional) call in a TOP-LEVEL instance `let`/`const`
        // IDENTIFIER-declarator init, ONCE per component — official
        // `props_id_invalid_placement` / `rune_invalid_arguments` /
        // `rune_invalid_spread` / `props_duplicate`. The callee member is
        // CONSUMED here (the member handler owns only UNCALLED / paren-spelled
        // references); the arguments still scan for nested runes. A
        // parenthesized callee spelling (`($props.id)()` / `($props).id()`)
        // never matches the strict predicate: it falls through to the generic
        // walk, whose member handler fails it closed.
        if is_props_id_callee(&it.callee) && self.is_unshadowed_rune("$props") {
            let well_formed = props_id_call_is_well_formed(it);
            let position_ok = self
                .props_id_positions
                .contains(&(it.span.start, it.span.end));
            if !well_formed || !position_ok {
                self.record(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune: "$props.id",
                    span: to_span(it.span),
                });
            } else if self.props_id_used {
                // A SECOND valid `$props.id()` use — official `props_duplicate`
                // ("Cannot use `$props.id()` more than once"); the second call
                // carries the refusal.
                self.record(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                    rune: "$props.id duplicate",
                    span: to_span(it.span),
                });
            } else {
                self.props_id_used = true;
            }
            for arg in &it.arguments {
                match arg.as_expression() {
                    Some(expr) => self.visit_expression(expr),
                    None => {
                        if let oxc_ast::ast::Argument::SpreadElement(s) = arg {
                            self.visit_expression(&s.argument);
                        }
                    }
                }
            }
            return;
        }
        // A `$bindable(...)` CALL through the STRICT bare-identifier callee on
        // the unshadowed root. Supported ONLY as a WELL-FORMED (zero-or-one
        // non-spread arg, non-optional) DIRECT `$props()` destructure-member
        // DEFAULT — official `bindable_invalid_location` /
        // `rune_invalid_arguments_length` / `rune_invalid_spread`; every other
        // position or form fails closed. The callee identifier is CONSUMED here
        // (the bare-reference position arm owns only UNCALLED references), and
        // the arguments still scan for nested runes. A parenthesized callee
        // (`($bindable)(0)`) does not match the strict predicate: its callee
        // identifier reaches the bare-reference arm and fails closed; a
        // parenthesized CALL (`($bindable(0))`) matches here but its span was
        // never collected as a supported default position, so it fails closed
        // too. The VALID position's emission is owned by the props lowering —
        // this arm records nothing for it (no double-refusal).
        if let Expression::Identifier(id) = &it.callee {
            if id.name.as_str() == "$bindable" && self.is_unshadowed_rune("$bindable") {
                let well_formed = bindable_call_is_well_formed(it);
                let position_ok = self
                    .bindable_positions
                    .contains(&(it.span.start, it.span.end));
                if !well_formed || !position_ok {
                    self.record(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                        rune: "$bindable",
                        span: to_span(it.span),
                    });
                }
                for arg in &it.arguments {
                    match arg.as_expression() {
                        Some(expr) => self.visit_expression(expr),
                        None => {
                            if let oxc_ast::ast::Argument::SpreadElement(s) = arg {
                                self.visit_expression(&s.argument);
                            }
                        }
                    }
                }
                return;
            }
        }
        // An effect-family rune CALL (`$effect(fn)` / `$effect.pre(fn)` /
        // `$effect.root(fn)` / `$effect.tracking()`) on the unshadowed rune root.
        // A WELL-FORMED call in a LEGAL POSITION is SUPPORTED at ANY depth. The
        // user-effect members are STATEMENT-ONLY and NON-OPTIONAL — official
        // rejects EVERY other position AND every optional invocation
        // (`effect_invalid_placement`: a concise-arrow body, a declarator init,
        // a `return` / call argument, a sequence element, a `$effect?.(fn)` /
        // `$effect.pre?.(fn)` / `$effect?.pre(fn)` chain); the admissible
        // statement-position call spans were recorded by
        // `visit_expression_statement` before this visitor ran. `.root` /
        // `.tracking` are expression-valued (no position requirement; optional
        // invocations admit and the emission normalizes the `?.` away). The bare
        // form's root-identifier span enters the supported set (its position
        // check then admits it); a member form's callee is consumed here (the
        // walk covers the ARGUMENTS only, so the uncalled-member arm in
        // `classify_rune_member` never sees a called form). The scan still
        // descends into the arguments, so a nested unsupported rune
        // (`$effect($host())`) refuses. A MALFORMED call — wrong arity (official
        // `rune_invalid_arguments_length` / `rune_invalid_arguments`) or a
        // spread argument (official `rune_invalid_spread`) — or a VALUE-POSITION
        // user-effect call fails closed under the precise family label.
        // `$effect.pending` (the experimental-async member) and unknown
        // `$effect.<member>` forms are NOT family calls (the fact classifier
        // returns `None`), so their refusal arms are unchanged.
        if self.is_unshadowed_rune("$effect") {
            if let Some(fact) = effect_family_call_fact(it) {
                let position_ok = match fact.kind {
                    EffectFamilyCallKind::UserEffect | EffectFamilyCallKind::UserPreEffect => {
                        !fact.optional
                            && self
                                .stmt_effect_spans
                                .contains(&(it.span.start, it.span.end))
                    }
                    EffectFamilyCallKind::EffectRoot | EffectFamilyCallKind::EffectTracking => true,
                };
                if !fact.well_formed || !position_ok {
                    self.record(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                        rune: fact.kind.rune_label(),
                        span: to_span(it.span),
                    });
                }
                match fact.kind {
                    // The bare form falls through to the generic walk: the callee
                    // identifier's position check consults the (now-extended)
                    // supported set, and the arguments walk normally.
                    EffectFamilyCallKind::UserEffect => {
                        if fact.well_formed && position_ok {
                            self.supported
                                .insert((fact.root_ident_span.start, fact.root_ident_span.end));
                        }
                    }
                    // A member form: consume the callee (the member handler owns
                    // ONLY uncalled references) and scan the arguments.
                    EffectFamilyCallKind::UserPreEffect
                    | EffectFamilyCallKind::EffectRoot
                    | EffectFamilyCallKind::EffectTracking => {
                        for arg in &it.arguments {
                            match arg.as_expression() {
                                Some(expr) => self.visit_expression(expr),
                                None => {
                                    if let oxc_ast::ast::Argument::SpreadElement(s) = arg {
                                        self.visit_expression(&s.argument);
                                    }
                                }
                            }
                        }
                        return;
                    }
                }
            }
        }
        // A `$host(...)` CALL through the STRICT bare-identifier callee on the
        // unshadowed root. Supported ONLY as the ZERO-ARG, NON-OPTIONAL call
        // inside an ACTIVE customElement component (it lowers to
        // `$$props.$$host`): on accept, the callee identifier span joins the
        // supported set — exactly like the admitted `$effect(fn)` callee — so
        // the position scan admits it when the walk descends, and the
        // `uses_host` FACT plus the first admitted call span are recorded (the
        // plan build gates the `$$props`-parameter decision on them). A
        // malformed call (any argument — official `rune_invalid_arguments` — a
        // spread, or an optional `$host?.()`) or ANY `$host(...)` call OUTSIDE
        // a customElement component (official `host_invalid_placement`)
        // records the host/custom-element refusal. A parenthesized callee
        // (`($host)()`) never matches the strict spelling: its inner bare
        // `$host` reaches the position scan and fails closed. The walk still
        // descends into the arguments, so a nested unsupported rune inside a
        // malformed call refuses too.
        if let Expression::Identifier(id) = &it.callee {
            if id.name.as_str() == "$host" && self.is_unshadowed_rune("$host") {
                let well_formed = !it.optional && it.arguments.is_empty();
                if well_formed && self.custom_element_active {
                    self.supported.insert((id.span.start, id.span.end));
                    self.uses_host = true;
                    if self.first_host_span.is_none() {
                        self.first_host_span = Some(to_span(it.span));
                    }
                } else {
                    self.classify_rune_call("$host", to_span(it.span));
                }
            }
        }
        // A BARE rune call to an ADVANCED-only rune (a standalone
        // `$bindable(...)`) has no supported bare position at all — classify it
        // here (`$inspect(...)` is production-elided, never refused here; a
        // `$host(...)` call was classified by its dedicated arm above). The
        // position-sensitive runes (`$state` / `$derived` / `$props` / `$effect`)
        // are classified per-identifier in `visit_identifier_reference` (a
        // supported position is exempt; everything else refuses), so a `$state(0)`
        // in a default-param / call-arg fails closed.
        if let Expression::Identifier(id) = &it.callee {
            let root = id.name.as_str();
            if self.is_unshadowed_rune(root)
                && !matches!(root, "$state" | "$derived" | "$props" | "$effect" | "$host")
            {
                self.classify_rune_call(root, to_span(it.span));
            }
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        // A POSITION-SENSITIVE rune reference: an unshadowed `$state` / `$derived` /
        // `$props` / `$effect` identifier is supported ONLY when its byte span is in
        // the supported-position set (a top-level instance-script declarator init
        // for `$state`, the single top-level destructure for `$props()`, a
        // well-formed `$effect(fn)` callee admitted by the walk-time call
        // exemption); a reference anywhere else (a default-param `$state(0)`, a
        // call-arg `$props()`, a module-script rune, a bare-identifier
        // `foo($state)` / `foo($effect)`) fails closed. An unshadowed `$bindable`
        // identifier reaching here is UNCALLED (its called forms are consumed by
        // the call visitor) — official `rune_missing_parentheses` — or a
        // paren-wrapped callee spelling; both fail closed (no supported bare
        // position exists). `$host` splits the same way: an ADMITTED zero-arg
        // `$host()` callee span was added to the supported set by the call
        // visitor (exempt at the position check), a MALFORMED / out-of-context
        // `$host(...)` call is refused by the call handler, and an UNCALLED
        // bare `$host` reference (including the inner identifier of a
        // parenthesized-callee `($host)()` spelling) fails closed HERE through
        // the position scan — official `rune_missing_parentheses`. The
        // production-elided `$inspect` family is owned by the instance-item
        // classifier / the body rewriter.
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
