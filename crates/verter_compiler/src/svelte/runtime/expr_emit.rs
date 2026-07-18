//! The Svelte client INSTANCE-SCRIPT declaration lowering + `$props()` / `$state`
//! shape classification.
//!
//! This module owns the SCRIPT-side emission: lowering the instance script's
//! top-level rune declarations (`$state` / `$derived` / `$effect` / basic
//! `$props()`) into their emitted client forms, classifying the `$props()` shape
//! (basic vs advanced) and the `$state` declarator shape, and collecting the
//! per-name `$props()` read forms. The per-expression rewriting it drives (every
//! source-derived payload routes through it) is the FALLIBLE two-pass rewriter in
//! [`super::expr_rewrite`].

use std::sync::Arc;

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, CallExpression, Expression, Statement};
use rustc_hash::{FxHashMap, FxHashSet};

use super::expr::{
    expr_is_proxiable, is_bindable_call, is_props_callee, peel_parens, reparse_module,
    state_rune_call, BindingTable, ProxyInit, StateLowering,
};
use super::expr_rewrite::{PropRead, PropReads};

/// The structural shape of a `$state` / `$state.raw` declarator — the fail-closed
/// gate that distinguishes a SUPPORTED plain-identifier state declarator (a
/// primitive `let c = $state(0)` OR a proxiable object/array/identifier/call
/// `let o = $state({})` — the deep-reactive `$.proxy` form) from the ADVANCED forms
/// the Svelte client emitter refuses rather than partially lowering: a destructured
/// one (`let { a } = $state(...)` / `let [x] = $state(...)`), an over-arity call, a
/// spread argument, or the narrowed form Verter's `should_proxy` predicate mis-decides
/// (a REACTIVE-shadowed bare `undefined` init). A plain-identifier declarator whose init is a
/// primitive OR a proxiable value is SUPPORTED — the declarator emitter lowers it per
/// the resolved [`StateLowering`], routing a proxiable init through the shared
/// expression rewriter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateDeclShape {
    /// No `$state` declarator (the common case).
    None,
    /// A plain-identifier `$state` / `$state.raw` declarator (primitive OR proxiable
    /// init) — supported.
    Identifier,
    /// A destructured `$state` declarator, an over-arity call, a spread argument, or a
    /// reactive-shadowed-`undefined` init — ADVANCED, fails closed.
    Advanced {
        /// A short label for the diagnostic.
        rune: &'static str,
    },
}

/// Classify EVERY `$state` / `$state.raw` declarator's shape in an instance-script
/// source. Returns [`StateDeclShape::Advanced`] if ANY `$state` declarator (not just
/// the first) has a non-plain-identifier pattern (an object/array destructure), an
/// over-arity call, a spread argument, or the narrowed reactive-shadowed-`undefined`
/// init; [`StateDeclShape::Identifier`]
/// when at least one plain-identifier `$state` declarator exists and ALL are supported
/// (a primitive OR proxiable init); and [`StateDeclShape::None`] when there is no
/// `$state` declarator. Drives the fail-closed gate BEFORE lowering, so a
/// destructured `$state` never reaches the declarator emitter.
///
/// Scanning ALL declarators (across all statements AND all declarators within a
/// single multi-declarator statement) is load-bearing: `let ok = $state(0); let {
/// a } = $state({ a: 1 })` must fail closed on the SECOND declarator rather than
/// classify on the first and silently drop the destructured one (which would leave
/// `a` undefined → a runtime `ReferenceError`).
#[must_use]
pub fn state_decl_shape(instance_source: &str) -> StateDeclShape {
    state_decl_shape_for_grammar(instance_source, false)
}

/// [`state_decl_shape`] under the component's authoritative script dialect.
/// TypeScript permits erased wrapper spines around a `$state` initializer; the
/// proxy decision is made over the surviving inner runtime expression.
pub fn state_decl_shape_for_grammar(instance_source: &str, typescript: bool) -> StateDeclShape {
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, instance_source) else {
        return StateDeclShape::None;
    };
    // Whether the instance script shadows `undefined` with a REACTIVE-RUNE binding (a
    // top-level `let undefined = $state(…)` / `$state.raw(…)` / `$derived(…)` /
    // `$derived.by(…)`). This gate is PRE-LOWERING and INSTANCE-ONLY, so it
    // CONSERVATIVELY fails closed over the WHOLE reactive-rune-`undefined`-shadow class —
    // see `state_init_is_shadowed_undefined` for the honest rationale (the LIVE-signal
    // subcase is unsupportable and this gate cannot tell demoted from live here). A
    // PLAIN-local `undefined` shadow (`let undefined = 5`) is NOT reactive: it reads plain
    // and lowers to `$.state(undefined)` matching official — supported.
    let undefined_reactive_shadow = top_level_undefined_shadow_is_reactive_rune(&program);
    let mut saw_state = false;
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let Some(Expression::CallExpression(call)) = &d.init else {
                continue;
            };
            if state_rune_call(call).is_none() {
                continue;
            }
            saw_state = true;
            // `$state` / `$state.raw` accept ZERO or ONE argument; a SECOND argument is
            // the official `rune_invalid_arguments_length` compile error ("$state must
            // be called with zero or one arguments", CallExpression.js). Fail closed
            // rather than silently dropping the extra argument (`$state(0, 1)` →
            // `$.state(0)`). The check precedes the destructure / init-shape gates so an
            // over-arity call fails on arity regardless of its pattern.
            if call.arguments.len() > 1 {
                return StateDeclShape::Advanced {
                    rune: "$state() invalid arguments",
                };
            }
            // A SPREAD argument (`$state(...x)` / `$state.raw(...x)`) is the official
            // `rune_invalid_spread` compile error ("`$state` cannot be called with a
            // spread argument"). A single spread arg is `arguments.len() == 1` and its
            // `as_expression()` is `None`, so it slips past BOTH the arity gate above
            // and the init-shape gates below (`state_primitive_init_text` would emit
            // `void 0` for the missing expression) — fail closed instead. Checked over
            // EVERY argument (`.any(as_expression().is_none())`) so no non-expression
            // argument form reaches the `void 0` init fallback.
            if call.arguments.iter().any(|a| a.as_expression().is_none()) {
                return StateDeclShape::Advanced {
                    rune: "$state() spread argument",
                };
            }
            // An object / array destructure of `$state(...)` is the advanced form — the
            // official compiler lowers it through a destructure closure; that lowering
            // is a distinct surface, so it fails closed (returned immediately so a later
            // identifier declarator cannot mask it).
            if !matches!(&d.id, BindingPattern::BindingIdentifier(_)) {
                return StateDeclShape::Advanced {
                    rune: "$state() destructure",
                };
            }
            // CONSERVATIVE fail-close: a `$state(undefined)` init whose `undefined` is
            // shadowed by a REACTIVE-RUNE binding (`$state` / `$state.raw` / `$derived`).
            // The correct output depends on how that shadow LOWERS (oracle-verified
            // against `svelte@5.56.3`):
            //   - a LIVE-signal shadow (reassigned as a whole variable → lowers to
            //     `$.state(…)`, read via `$.get`) makes the init read `$.get(undefined)`,
            //     which official proxies → `$.state($.proxy($.get(undefined)))`. Verter
            //     CANNOT reproduce this (its `expr_is_proxiable` hardcodes the `undefined`
            //     identifier non-proxiable and would omit the `$.proxy`), so the LIVE
            //     subcase is genuinely UNSUPPORTABLE.
            //   - a DEMOTED shadow (never reassigned → lowers to a plain `let undefined =
            //     …` / `$.proxy(…)`, read BARE) makes the init read a bare `undefined` →
            //     official emits `$.state(undefined)`, which Verter CAN reproduce.
            // This gate is PRE-LOWERING and INSTANCE-ONLY: it cannot know whether the
            // shadow will be demoted or live, because a whole-variable reassignment that
            // promotes the shadow to a live signal can happen in a TEMPLATE handler this
            // gate never sees (`onclick={() => undefined = 1}` — oracle-verified to promote
            // the shadow to `$.state(…)`). Distinguishing demoted-vs-live SAFELY requires
            // the whole-component resolved binding lowering, computed downstream in
            // `state_prep` AFTER template writes are attributed — which inverts this gate's
            // pre-lowering ordering. So Verter CONSERVATIVELY refuses the WHOLE
            // reactive-rune-`undefined`-shadow class (fail-closed-safe). This OVER-REFUSES
            // the demoted subcase; a PLAIN-local shadow (`let undefined = 5`) is NOT
            // reactive and stays SUPPORTED (`$.state(undefined)`).
            // TODO(follow-up): make this gate LOWERING-AWARE — consult the whole-component
            // resolved `StateLowering` of the `undefined` shadow (demoted `PlainLet` /
            // `BareProxy` → support `$.state(undefined)`; live `StateSignal` / `StateProxy`
            // / `RawStateSignal` → fail closed). Requires running the shared write analysis
            // (instance + template) BEFORE this pre-lowering gate; the current over-refusal
            // is fail-closed-safe until then.
            if state_init_is_shadowed_undefined(call, undefined_reactive_shadow) {
                return StateDeclShape::Advanced {
                    rune: "$state() shadowed undefined init",
                };
            }
            // A TS-wrapped init is legal only under the authoritative TypeScript
            // grammar. The lowering path peels the wrapper spine before making the
            // runtime proxiability decision and erases it on the shared transform;
            // a plain script fails closed instead of accepting syntax the official
            // compiler rejects.
            if state_init_has_ts_wrapper(call) && !typescript {
                return StateDeclShape::Advanced {
                    rune: "$state() ts-wrapped init",
                };
            }
            // Everything else — a primitive literal OR a proxiable object / array /
            // identifier / call / member / `NaN` / `Infinity` / unshadowed `undefined`
            // init — is SUPPORTED: the declarator emitter lowers it per the resolved
            // `StateLowering`, routing a proxiable init through the shared rewriter so a
            // signal read inside it becomes `$.get`.
        }
    }
    if saw_state {
        StateDeclShape::Identifier
    } else {
        StateDeclShape::None
    }
}

/// Whether a `$state(...)` call's argument is a bare `undefined` identifier that is
/// shadowed by a REACTIVE-RUNE binding (`undefined_reactive_shadow`). Verter
/// CONSERVATIVELY fails this closed — see the call site for the full rationale: the
/// LIVE-signal subcase official emits as `$.state($.proxy($.get(undefined)))` is
/// unsupportable (Verter's `expr_is_proxiable` hardcodes the `undefined` identifier
/// non-proxiable and would omit the `$.proxy`), and this PRE-LOWERING / INSTANCE-ONLY
/// gate cannot distinguish it from the DEMOTED subcase (which official supports as
/// `$.state(undefined)`) without the whole-component resolved binding lowering. A
/// PLAIN-local `undefined` shadow (`let undefined = 5`) reads plain and lowers to
/// `$.state(undefined)` matching official — supported. An unshadowed `undefined` (the
/// void-0 global) and a no-arg `$state()` are likewise supported (`$.state(undefined)` /
/// `$.state(void 0)`).
fn state_init_is_shadowed_undefined(
    call: &CallExpression<'_>,
    undefined_reactive_shadow: bool,
) -> bool {
    let Some(arg) = call.arguments.first() else {
        // `$state()` — the void-0 primitive form, never shadowed-undefined.
        return false;
    };
    let Some(Expression::Identifier(id)) = arg.as_expression() else {
        return false;
    };
    id.name.as_str() == "undefined" && undefined_reactive_shadow
}

/// Whether a `$state(...)` init's argument is a TOP-LEVEL TypeScript wrapper — an
/// `as` / `satisfies` / non-null `!` / type-assertion expression (paren-transparent).
/// Such an init is a distinct surface (official's plain-JS parse rejects the cast, and
/// the proxiability predicate mis-reads the wrapped node), so it fails closed.
fn state_init_has_ts_wrapper(call: &CallExpression<'_>) -> bool {
    let Some(arg) = call.arguments.first() else {
        return false;
    };
    let Some(mut expr) = arg.as_expression() else {
        return false;
    };
    while let Expression::ParenthesizedExpression(p) = expr {
        expr = &p.expression;
    }
    matches!(
        expr,
        Expression::TSAsExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSNonNullExpression(_)
            | Expression::TSTypeAssertion(_)
    )
}

/// Whether the program's TOP-LEVEL declarators shadow `undefined` with a
/// REACTIVE-RUNE binding — a `let undefined = $state(…)` / `$state.raw(…)` /
/// `$derived(…)` / `$derived.by(…)`. Such a shadow MAY lower to a live signal (read via
/// `$.get`), in which case official proxies the init (`$.state($.proxy($.get(undefined)))`)
/// — a form Verter cannot reproduce. This PRE-LOWERING / INSTANCE-ONLY gate cannot tell
/// the live subcase from the DEMOTED subcase (a shadow reassigned only in a template
/// handler still becomes a live signal, and this gate never sees the handler), so Verter
/// CONSERVATIVELY flags the WHOLE class — see `state_init_is_shadowed_undefined` and its
/// call site. A PLAIN-local `undefined` shadow (`let undefined = 5`) is NOT reactive: it
/// reads plain and lowers to `$.state(undefined)` matching official, so it is not flagged.
fn top_level_undefined_shadow_is_reactive_rune(program: &oxc_ast::ast::Program<'_>) -> bool {
    program.body.iter().any(|stmt| {
        let Statement::VariableDeclaration(decl) = stmt else {
            return false;
        };
        decl.declarations.iter().any(|d| {
            // The declarator must BIND `undefined`.
            let mut names = Vec::new();
            super::expr::collect_pattern_names(&d.id, &mut names);
            if !names.iter().any(|n| n == "undefined") {
                return false;
            }
            // …with a REACTIVE-RUNE initializer (`$state` / `$state.raw` / `$derived`
            // / `$derived.by`). A plain / non-rune init is a non-reactive shadow.
            let Some(Expression::CallExpression(call)) = &d.init else {
                return false;
            };
            state_rune_call(call).is_some() || super::expr::is_derived_callee(&call.callee)
        })
    })
}

// ---------------------------------------------------------------------------
// Instance-script declaration lowering
// ---------------------------------------------------------------------------

/// The shape of a component's `$props()` usage — drives the basic-vs-advanced
/// fail-closed decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropsShape {
    /// No `$props()` call.
    None,
    /// A basic destructure: `let { a, b = 1, c = $bindable(0) } = $props()` —
    /// named / aliased / string-key members, with optional defaults (including
    /// `$bindable(...)` defaults). ALSO carries the `$.rest_props` capture forms:
    /// a `{ …, ...rest }` rest element and a whole-object `let all = $props()`
    /// identifier binding (both lower their `rest_excludes` Set + `$.rest_props`
    /// declarator through the destructure path). No computed / numeric keys, no
    /// nested destructure.
    BasicDestructure,
    /// An advanced form that fails closed (a computed / numeric key, or a nested
    /// destructure).
    Advanced {
        /// A short rune label for the diagnostic.
        rune: &'static str,
    },
}

/// The UNIFIED `$props()` declarator facts — the SINGLE scan authority for a
/// component's ONE accepted `$props()` declarator, built ONCE (after the
/// malformed shapes are refused upstream) and threaded to every consumer: the
/// prop-read forms ([`Self::prop_reads`]), the `$.rest_props` module hoist, and
/// the `$.prop` destructure lowering. There is no second scan / second authority
/// for the same declarator.
///
/// It carries the named member plans, the optional rest / whole-object capture,
/// and (on the capture) BOTH the ordered exclude `Vec` (the emitted
/// `new Set([…])` order) and a shared `Arc<FxHashSet>` membership set (the hot
/// member-visit exclude lookup). The prop-source / read forms are computed on
/// demand from the caller's `updated_locals` (which itself is derived from this
/// plan's member default spans — the one input the read forms depend on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PropsDeclaratorPlan {
    /// The named / aliased / string-key member plans, in source order.
    pub(super) members: Vec<PropsMemberPlan>,
    /// The rest (`{ …, ...rest }`) / whole-object (`let all = $props()`) capture,
    /// or `None` for a plain destructure with no rest and no whole-object binding.
    pub(super) rest: Option<PropsRestCapture>,
}

impl PropsDeclaratorPlan {
    /// Walk the instance script's single accepted `$props()` declarator ONCE into
    /// the unified plan (the named member rows + the optional rest / whole-object
    /// capture), or `None` when there is no `$props()`. The malformed shapes
    /// (rest / computed keys / nested destructures / duplicate `$props()`) are
    /// refused UPSTREAM ([`props_shape`] + the rune scan), so this walker only
    /// classifies the accepted subset. `custom_element` marks a component with a
    /// resolved custom-element descriptor — its rest excludes carry the extra
    /// `'$$host'` key (the official `if (analysis.custom_element)
    /// seen.push('$$host')`).
    #[must_use]
    pub(super) fn build(
        alloc: &Allocator,
        instance_source: &str,
        custom_element: bool,
    ) -> Option<PropsDeclaratorPlan> {
        let program = reparse_module(alloc, instance_source)?;
        let proxy_inits = super::state_scan::collect_proxy_inits(&program);
        let mut members = Vec::new();
        let mut rest: Option<PropsRestCapture> = None;
        for stmt in &program.body {
            let Statement::VariableDeclaration(decl) = stmt else {
                continue;
            };
            for d in &decl.declarations {
                let Some(Expression::CallExpression(call)) = &d.init else {
                    continue;
                };
                if !is_props_callee(&call.callee) {
                    continue;
                }
                match &d.id {
                    // A whole-object capture (`let all = $props()`) — the exclude
                    // Set carries ONLY the fixed prefix (every non-`$$` member read
                    // de-localizes to `$$props.KEY`).
                    BindingPattern::BindingIdentifier(id) => {
                        rest.get_or_insert_with(|| {
                            build_rest_capture(id.name.to_string(), &[], custom_element)
                        });
                    }
                    BindingPattern::ObjectPattern(obj) => {
                        for prop in &obj.properties {
                            if let Some(member) = build_member_plan(prop, &proxy_inits) {
                                members.push(member);
                            }
                        }
                        // A `{ …, ...rest }` object pattern — the exclude Set is the
                        // fixed prefix then each non-rest member's SOURCE key in
                        // source order.
                        if let Some(rest_el) = &obj.rest {
                            if let Some(local) = single_ident(&rest_el.argument) {
                                let source_keys: Vec<String> =
                                    obj.properties.iter().map(prop_key_name).collect();
                                rest.get_or_insert_with(|| {
                                    build_rest_capture(
                                        local.to_string(),
                                        &source_keys,
                                        custom_element,
                                    )
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Some(PropsDeclaratorPlan { members, rest })
    }

    /// Walk the instance script's LEGACY `export let` statements ONCE into the
    /// unified plan — one [`PropsMemberPlan`] per exported identifier declarator
    /// (`source_key` = `local` = the exported name; never bindable; the
    /// declarator INIT is the member DEFAULT, lowered through the SAME official
    /// simple/lazy algorithm as a `$props()` destructure default), or `None`
    /// when there is no `export let`. The legacy prop surface has no rest /
    /// whole-object capture. Destructured / TS-annotated export-let declarators
    /// contribute nothing (they fail closed at the instance-script item
    /// allowlist before this plan is consumed).
    #[must_use]
    pub(super) fn build_legacy_exports(
        alloc: &Allocator,
        instance_source: &str,
    ) -> Option<PropsDeclaratorPlan> {
        let program = reparse_module(alloc, instance_source)?;
        let proxy_inits = super::state_scan::collect_proxy_inits(&program);
        let mut members = Vec::new();
        for stmt in &program.body {
            let Statement::ExportNamedDeclaration(export) = stmt else {
                continue;
            };
            let Some(oxc_ast::ast::Declaration::VariableDeclaration(decl)) = &export.declaration
            else {
                continue;
            };
            if decl.kind != oxc_ast::ast::VariableDeclarationKind::Let {
                continue;
            }
            for d in &decl.declarations {
                let BindingPattern::BindingIdentifier(id) = &d.id else {
                    continue;
                };
                members.push(PropsMemberPlan {
                    source_key: id.name.to_string(),
                    local: id.name.to_string(),
                    bindable: false,
                    default: d
                        .init
                        .as_ref()
                        .map(|init| props_default_facts(init, &proxy_inits)),
                });
            }
        }
        if members.is_empty() {
            return None;
        }
        Some(PropsDeclaratorPlan {
            members,
            rest: None,
        })
    }

    /// Project the LEGACY per-name prop read forms: EVERY legacy `export let`
    /// prop is a prop source (official declares `$.prop` even for a bare,
    /// never-written prop) and ALWAYS reads as the accessor call (`name()`) —
    /// unlike a runes no-default prop, which reads `$$props.name` directly.
    #[must_use]
    pub(super) fn prop_reads_legacy(&self) -> PropReads {
        let mut reads = PropReads::default();
        for plan in &self.members {
            reads.insert(plan.local.clone(), PropRead::Getter);
        }
        reads
    }

    /// Project the per-name `$props()` read forms: a PROP-SOURCE member (a
    /// default-bearing OR written member — the official `is_prop_source` — or
    /// ANY member under `accessors`) is declared via `$.prop` and reads as a
    /// getter call (`name()`); a non-source member reads directly off the props
    /// object (`$$props.name`). The rest / whole-object capture's BARE read
    /// stays the verbatim real local, and its member reads de-localize
    /// key-awarely through the SHARED membership set (carried as a cheap `Arc`
    /// clone, never a cloned key `Vec`).
    #[must_use]
    pub(super) fn prop_reads(
        &self,
        updated_locals: &FxHashSet<String>,
        accessors: bool,
    ) -> PropReads {
        let mut reads = PropReads::default();
        for plan in &self.members {
            let read = if plan.is_prop_source(updated_locals, accessors) {
                PropRead::Getter
            } else {
                PropRead::PropsMember {
                    source_key: plan.source_key.clone(),
                }
            };
            reads.insert(plan.local.clone(), read);
        }
        if let Some(rest) = &self.rest {
            reads.insert(
                rest.local.clone(),
                PropRead::RestBinding {
                    excludes: rest.exclude_set.clone(),
                },
            );
        }
        reads
    }
}

/// Build one member plan from a `$props()` destructure property (a plain
/// identifier member OR an assignment-pattern member with a plain / `$bindable`
/// default). A nested-destructure member is refused upstream ([`props_shape`]),
/// so it yields no plan row here.
fn build_member_plan(
    prop: &oxc_ast::ast::BindingProperty<'_>,
    proxy_inits: &FxHashMap<String, ProxyInit>,
) -> Option<PropsMemberPlan> {
    let source_key = prop_key_name(prop);
    match &prop.value {
        BindingPattern::BindingIdentifier(id) => Some(PropsMemberPlan {
            source_key,
            local: id.name.to_string(),
            bindable: false,
            default: None,
        }),
        BindingPattern::AssignmentPattern(assign) => {
            let local = single_ident(&assign.left)
                .unwrap_or(&source_key)
                .to_string();
            let bindable = is_bindable_call(&assign.right);
            let default = if bindable {
                let Expression::CallExpression(bc) = &assign.right else {
                    unreachable!("a bindable default is a call expression");
                };
                bc.arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .map(|arg| props_default_facts(arg, proxy_inits))
            } else {
                Some(props_default_facts(&assign.right, proxy_inits))
            };
            Some(PropsMemberPlan {
                source_key,
                local,
                bindable,
                default,
            })
        }
        // A nested destructure member is refused upstream; no plan row.
        _ => None,
    }
}

/// Build a rest / whole-object [`PropsRestCapture`] from the local binding name
/// and the ordered non-rest member SOURCE keys (empty for the whole-object
/// form). The exclude keys are the fixed [`REST_EXCLUDE_PREFIX`] — plus
/// `'$$host'` under a custom element (the official
/// `if (analysis.custom_element) seen.push('$$host')`, so a rest spread never
/// surfaces the host element) — then each source key in source order; the
/// membership `FxHashSet` is derived from the same keys and shared (`Arc`) into
/// the read form.
fn build_rest_capture(
    local: String,
    source_keys: &[String],
    custom_element: bool,
) -> PropsRestCapture {
    let mut excludes: Vec<String> = REST_EXCLUDE_PREFIX.iter().map(|k| k.to_string()).collect();
    if custom_element {
        excludes.push("$$host".to_string());
    }
    excludes.extend(source_keys.iter().cloned());
    let exclude_set: FxHashSet<String> = excludes.iter().cloned().collect();
    PropsRestCapture {
        local,
        excludes,
        exclude_set: Arc::new(exclude_set),
    }
}

/// One `$props()` destructure member's typed lowering facts — the SINGLE
/// authority (on the [`PropsDeclaratorPlan`]) BOTH the prop-read projection and
/// the `$.prop` declarator lowering consume, so the read form and the emitted
/// declaration can never diverge. The `updated` axis is NOT stored here (it is
/// derived per-consumer from the caller's `updated_locals`, which is itself
/// harvested from these members' default spans).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PropsMemberPlan {
    /// The SOURCE prop key (the destructure key; may differ from the local
    /// under aliasing).
    pub(super) source_key: String,
    /// The LOCAL binding name.
    pub(super) local: String,
    /// Whether the member default is a `$bindable(...)` call (the BINDABLE flag
    /// axis; the `$.proxy` wrap is bindable-only).
    pub(super) bindable: bool,
    /// The DEFAULT initializer facts (`None` = no default; for a bindable
    /// member the `$bindable(...)` ARGUMENT is the default, so a zero-arg
    /// `$bindable()` carries `None`).
    pub(super) default: Option<PropsDefaultFacts>,
}

impl PropsMemberPlan {
    /// The official `is_prop_source` predicate on Verter's runes-only surface:
    /// the member has a default initial OR is UPDATED (its local written anywhere
    /// — a template-expression or `$props()`-default reassign / deep-mutate,
    /// runes-mode `reassigned || mutated`) OR the component compiles with
    /// `accessors` (a CUSTOM ELEMENT forces it — every prop is then a source, so
    /// the `$$exports` get/set accessors have a getter/setter binding to drive).
    /// A non-source member emits NO `$.prop` declaration and reads directly off
    /// `$$props`.
    pub(super) fn is_prop_source(
        &self,
        updated_locals: &FxHashSet<String>,
        accessors: bool,
    ) -> bool {
        accessors || self.default.is_some() || updated_locals.contains(&self.local)
    }
}

/// The typed facts of ONE `$props()` member default initializer, computed over
/// the paren-PEELED expression (official's ESTree AST has no paren nodes, so
/// author parens around a default VALUE are transparent at every level).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PropsDefaultFacts {
    /// The peeled default expression's byte span into the instance source (the
    /// slice the rewriter lowers).
    pub(super) span: (u32, u32),
    /// Whether the peeled root is a TOP-LEVEL TS wrapper (`as` / `satisfies` /
    /// non-null `!` / type assertion) — a distinct surface that fails closed
    /// (the official lazy/simple decision runs over the TS node).
    pub(super) ts_wrapped: bool,
    /// The official `is_simple_expression` decision SHAPE over the peeled
    /// node, evaluated with VISITED-node semantics — official runs the
    /// predicate on the initializer AFTER reference rewriting
    /// (`initial = context.visit(binding.initial)`), where a FUNCTION
    /// literal's outer node kind survives inner rewrites but a rewritten
    /// identifier LEAF becomes a getter call / `$$props` member (not
    /// simple). `None` when the skeleton is structurally non-simple (always
    /// LAZY); `Some(leaves)` when the skeleton is simple — a literal /
    /// identifier / arrow / function root, or a conditional / binary / logical
    /// over simple parts — where `leaves` collects every identifier in a
    /// non-function leaf position: the initial passes RAW (no LAZY bit) iff
    /// every collected leaf stays unrewritten. Function roots/parts collect no
    /// leaves (a rewrite inside a body never changes the node kind the
    /// official predicate sees).
    pub(super) simple_ident_leaves: Option<Vec<String>>,
    /// The identifier name when the peeled root IS a bare identifier (drives
    /// the rewritten-getter collapse and the bindable proxy follow).
    pub(super) bare_ident: Option<String>,
    /// The callee name when the peeled root is a NON-optional ZERO-ARG call on
    /// a bare identifier callee — the official callee-collapse optimization
    /// (`{ a = foo() }` → `$.prop($$props, 'a', 19, foo)`).
    pub(super) zero_arg_ident_callee: Option<String>,
    /// Whether the peeled root is an ObjectExpression (the emitted thunk body
    /// needs the `() => ({ … })` parenthesization).
    pub(super) object_root: bool,
    /// Whether the peeled root is a SequenceExpression (the emitted thunk body
    /// / proxy argument needs explicit parenthesization so the comma expression
    /// stays ONE value: `() => (1, 2)`, `$.proxy((1, 2))`).
    pub(super) sequence_root: bool,
    /// The official `should_proxy` over the peeled node SHAPE (with the one-hop
    /// identifier follow against the instance program's top-level bindings) —
    /// consulted only for a BINDABLE default whose root identifier does not
    /// rewrite.
    pub(super) proxiable_by_shape: bool,
    /// Whether the peeled root is a BOOLEAN literal (`true` / `false`) — the
    /// custom-element prop-definition type inference (a type-less explicit prop
    /// def whose binding initial is a boolean literal infers `type: 'Boolean'`).
    pub(super) boolean_literal: bool,
}

/// Compute the typed [`PropsDefaultFacts`] of one default initializer
/// expression (paren-peeled).
fn props_default_facts(
    expr: &Expression<'_>,
    proxy_inits: &rustc_hash::FxHashMap<String, super::expr::ProxyInit>,
) -> PropsDefaultFacts {
    use oxc_span::GetSpan;
    let peeled = peel_parens(expr);
    let span = peeled.span();
    let ts_wrapped = matches!(
        peeled,
        Expression::TSAsExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSNonNullExpression(_)
            | Expression::TSTypeAssertion(_)
            | Expression::TSInstantiationExpression(_)
    );
    let runtime_expr = peel_typescript_runtime_expression(peeled);
    let bare_ident = match runtime_expr {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    };
    let zero_arg_ident_callee = match runtime_expr {
        Expression::CallExpression(call) if !call.optional && call.arguments.is_empty() => {
            match peel_parens(&call.callee) {
                Expression::Identifier(id) => Some(id.name.to_string()),
                _ => None,
            }
        }
        _ => None,
    };
    PropsDefaultFacts {
        span: (span.start, span.end),
        ts_wrapped,
        simple_ident_leaves: simple_ident_leaves(runtime_expr),
        bare_ident,
        zero_arg_ident_callee,
        object_root: matches!(runtime_expr, Expression::ObjectExpression(_)),
        sequence_root: matches!(runtime_expr, Expression::SequenceExpression(_)),
        proxiable_by_shape: expr_is_proxiable(runtime_expr, Some(proxy_inits)),
        boolean_literal: matches!(runtime_expr, Expression::BooleanLiteral(_)),
    }
}

fn peel_typescript_runtime_expression<'a>(mut expr: &'a Expression<'a>) -> &'a Expression<'a> {
    loop {
        expr = peel_parens(expr);
        expr = match expr {
            Expression::TSAsExpression(node) => &node.expression,
            Expression::TSSatisfiesExpression(node) => &node.expression,
            Expression::TSNonNullExpression(node) => &node.expression,
            Expression::TSTypeAssertion(node) => &node.expression,
            Expression::TSInstantiationExpression(node) => &node.expression,
            _ => return expr,
        };
    }
}

/// The official `is_simple_expression` skeleton over a default initializer
/// (paren-transparent at EVERY recursion level — official's ESTree AST carries
/// no paren nodes): a literal (string / number / boolean / null / bigint /
/// regexp), an identifier, an arrow / function expression, a conditional over
/// simple parts, or a binary / logical over simple parts. Everything else
/// (a template literal, an object / array, a call, a member, an assignment, …)
/// is NOT simple (`None`) and rides the LAZY thunk.
///
/// Official applies the predicate to the VISITED initializer, so the skeleton
/// alone does not decide: `Some(leaves)` carries every identifier in a
/// non-function leaf position, and the consumer re-checks each against the
/// shared reference rewriter (a rewritten leaf becomes a getter call /
/// `$$props` member — not simple). Function roots/parts contribute NO
/// leaves: a rewrite inside a body never changes the outer node kind. A
/// private-`in` test (`#x in obj`) is a distinct OXC node
/// (`PrivateInExpression`), so official's `PrivateIdentifier` left-operand
/// exclusion falls out of the catch-all arm.
fn simple_ident_leaves(expr: &Expression<'_>) -> Option<Vec<String>> {
    fn collect(expr: &Expression<'_>, leaves: &mut Vec<String>) -> bool {
        match peel_parens(expr) {
            Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::FunctionExpression(_) => true,
            Expression::Identifier(id) => {
                leaves.push(id.name.to_string());
                true
            }
            Expression::ConditionalExpression(cond) => {
                collect(&cond.test, leaves)
                    && collect(&cond.consequent, leaves)
                    && collect(&cond.alternate, leaves)
            }
            Expression::BinaryExpression(bin) => {
                collect(&bin.left, leaves) && collect(&bin.right, leaves)
            }
            Expression::LogicalExpression(log) => {
                collect(&log.left, leaves) && collect(&log.right, leaves)
            }
            _ => false,
        }
    }
    let mut leaves = Vec::new();
    collect(expr, &mut leaves).then_some(leaves)
}

/// Classify the instance script's `$props()` usage, scanning EVERY `$props()`
/// declarator (across ALL statements AND all declarators within a multi-declarator
/// statement) — not just the first.
///
/// The official compiler supports exactly ONE top-level `$props()` destructure: a
/// second `$props()` call is `props_duplicate`, and a non-basic shape (a computed
/// / numeric / nested key) is `props_invalid_pattern`. A rest element
/// (`{ …, ...rest }`) and a whole-object identifier binding (`let all = $props()`)
/// are BASIC — they lower through the `$.rest_props` capture path. A member
/// DEFAULT — plain or `$bindable(...)` — is part
/// of the BASIC destructure (the shared `$.prop` prop-source path). Scanning ALL
/// declarators is load-bearing: `let {a}=$props(), {[k]:b}=$props()` must fail
/// closed on the SECOND (computed-key) declarator rather than classify on the
/// first basic one and silently emit a raw prop read for `b`. The FIRST advanced
/// shape is reported; if every shape is basic but there are 2+ `$props()` calls,
/// the duplicate is reported.
#[must_use]
pub fn props_shape(instance_source: &str) -> PropsShape {
    let alloc = Allocator::default();
    let Some(program) = reparse_module(&alloc, instance_source) else {
        return PropsShape::None;
    };
    let mut count = 0usize;
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let Some(Expression::CallExpression(call)) = &d.init else {
                continue;
            };
            if !is_props_callee(&call.callee) {
                continue;
            }
            count += 1;
            // `$props()` accepts ZERO arguments; ANY argument is the official
            // `rune_invalid_arguments` compile error ("$props cannot be called with
            // arguments", CallExpression.js). Fail closed rather than emitting the
            // prop reads regardless of the bogus argument. Checked on the FIRST
            // over-arity declarator so a later valid one cannot mask it.
            if !call.arguments.is_empty() {
                return PropsShape::Advanced {
                    rune: "$props() invalid arguments",
                };
            }
            // Return on the FIRST advanced shape so a later basic declarator cannot
            // mask it.
            match classify_props_pattern(&d.id) {
                PropsShape::Advanced { rune } => return PropsShape::Advanced { rune },
                PropsShape::BasicDestructure | PropsShape::None => {}
            }
        }
    }
    match count {
        0 => PropsShape::None,
        1 => PropsShape::BasicDestructure,
        // Two or more `$props()` calls (all basic) — `props_duplicate`. Fail closed
        // rather than emit two conflicting prop-read surfaces.
        _ => PropsShape::Advanced {
            rune: "$props() duplicate",
        },
    }
}

/// Classify a `$props()` declarator pattern.
fn classify_props_pattern(pattern: &BindingPattern<'_>) -> PropsShape {
    match pattern {
        // A whole-object identifier binding (`let all = $props()`) is the
        // prefix-only `$.rest_props` capture — a basic shape (its `rest_excludes`
        // Set + `$.rest_props` declarator lower through the destructure path).
        BindingPattern::BindingIdentifier(_) => PropsShape::BasicDestructure,
        BindingPattern::ObjectPattern(obj) => {
            // A `{ …, ...rest }` rest element is the `$.rest_props` capture — a
            // basic shape. Its named siblings still validate below (a computed /
            // numeric / nested sibling fails closed at the surviving arms); the
            // rest binding itself lowers through the destructure path.
            for prop in &obj.properties {
                // A COMPUTED key (`{ [k]: a }`) is rejected by official
                // (`props_invalid_pattern`) — fail closed rather than read the
                // wrong key.
                if prop.computed {
                    return PropsShape::Advanced {
                        rune: "$props() computed key",
                    };
                }
                // Only identifier + string-literal keys are supported. A NUMERIC key
                // (`{ 0: zero }`) reads (in official) off `$$props['0']` — a distinct
                // bracket-access lowering that the basic-destructure path does not
                // produce — so it fails closed rather than reading the wrong key.
                if !matches!(
                    &prop.key,
                    oxc_ast::ast::PropertyKey::StaticIdentifier(_)
                        | oxc_ast::ast::PropertyKey::StringLiteral(_)
                ) {
                    return PropsShape::Advanced {
                        rune: "$props() numeric/computed key",
                    };
                }
                // The member VALUE is a plain identifier, optionally with a
                // DEFAULT (`{ a = 1 }` — the official prop-source
                // `$.prop($$props, key, flags[, default])` path, shared with
                // `$bindable(...)` defaults; the `$bindable` call's own
                // form/position validity is owned by the rune scan). A NESTED
                // destructure — as the member value OR as a default's left —
                // is rejected by official (`props_invalid_pattern`).
                match &prop.value {
                    BindingPattern::BindingIdentifier(_) => {}
                    BindingPattern::AssignmentPattern(assign) => {
                        if !matches!(&assign.left, BindingPattern::BindingIdentifier(_)) {
                            return PropsShape::Advanced {
                                rune: "$props() nested destructure",
                            };
                        }
                    }
                    // An object / array nested destructure value is invalid.
                    BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_) => {
                        return PropsShape::Advanced {
                            rune: "$props() nested destructure",
                        };
                    }
                }
            }
            PropsShape::BasicDestructure
        }
        // An array destructure / top-level default of `$props()` is unusual —
        // treat as advanced (fail closed rather than partially emit).
        BindingPattern::ArrayPattern(_) | BindingPattern::AssignmentPattern(_) => {
            PropsShape::Advanced {
                rune: "$props() destructure",
            }
        }
    }
}

/// The lowering of ONE supported instance-script item that needs NO expression
/// rewriter — the simple declaration variants. Returns the emitted client-body
/// statement, [`SimpleItemLowering::None`] for a variant that emits nothing (a
/// no-default `$props()` destructure), or [`SimpleItemLowering::NeedsRewriter`] for a
/// variant whose payload lowers through the FALLIBLE expression rewriter (the
/// [`StatePrimitive`](super::instance_items::SupportedInstanceScriptItem::StatePrimitive)
/// init and the [`FunctionDecl`](super::instance_items::SupportedInstanceScriptItem::FunctionDecl)
/// body), owned by the caller that holds the rewriter —
/// [`SupportedClientIr::build_script_items`](super::client_plan::SupportedClientIr).
///
/// The simple variants are a thin per-variant transform:
/// - [`StatePrimitive`](super::instance_items::SupportedInstanceScriptItem::StatePrimitive)
///   → `NeedsRewriter`: the init routes through `rewrite_source` (a signal read inside a
///   proxiable object init → `$.get`, TS stripped) before the `StateLowering` wrapper
///   (`$.state` / `$.proxy` / `$.state($.proxy(…))`) is applied;
/// - [`PropsDestructure`](super::instance_items::SupportedInstanceScriptItem::PropsDestructure)
///   → `NeedsRewriter`: the PROP-SOURCE members lower to the
///   `let <local> = $.prop($$props, <key>, <flags>[, <default>])` declaration
///   (default expressions rewrite through the shared rewriter; a destructure with
///   no prop-source member emits nothing — those props read off `$$props`);
/// - [`PropsIdDecl`](super::instance_items::SupportedInstanceScriptItem::PropsIdDecl)
///   → the hoisted `const <name> = $.props_id();` rides the plan's body-top slot;
///   the literal-only siblings emit as one verbatim declaration in the source
///   slot (or nothing when the id declarator stood alone);
/// - [`BindThisLocal`](super::instance_items::SupportedInstanceScriptItem::BindThisLocal)
///   → `let name;` (the `bind:this` clone-root local);
/// - [`BindLocalLet`](super::instance_items::SupportedInstanceScriptItem::BindLocalLet)
///   → `let name = <init>;` / `let name;` (a plain-local DOM bind-target root, verbatim
///   literal init, or the uninitialized no-init form);
/// - [`InspectElided`](super::instance_items::SupportedInstanceScriptItem::InspectElided)
///   → NOTHING (a production-elided `$inspect(...)` / `$inspect(...).with(...)`
///   statement).
#[must_use]
pub(super) fn lower_simple_instance_item(
    item: &super::instance_items::SupportedInstanceScriptItem,
) -> SimpleItemLowering {
    use super::instance_items::SupportedInstanceScriptItem as Item;
    match item {
        Item::GeneralStatement { .. } => SimpleItemLowering::NeedsRewriter,
        // A `$state` / `$state.raw` declarator lowers through the FALLIBLE rewriter: its
        // init routes through `rewrite_source` (a signal read inside a proxiable object
        // init becomes `$.get`, TS is stripped) BEFORE the `StateLowering` wrapper is
        // applied — the caller (which holds the rewriter) handles it.
        Item::StatePrimitive { .. } => SimpleItemLowering::NeedsRewriter,
        // A `$props()` destructure lowers through the FALLIBLE rewriter: its
        // PROP-SOURCE members (default-bearing or written) emit the
        // `let <local> = $.prop($$props, <key>, <flags>[, <default>])` declaration
        // (default expressions rewrite through the shared rewriter); a destructure
        // with NO prop-source member emits nothing (reads go directly off
        // `$$props`). The caller (which holds the rewriter + the write facts)
        // handles it.
        Item::PropsDestructure => SimpleItemLowering::NeedsRewriter,
        // A `$props.id()` declarator: the hoisted `const <name> = $.props_id();`
        // rides the plan's dedicated body-top slot (populated by the caller); the
        // literal-only SIBLING declarators emit as one verbatim declaration in the
        // item's source slot — or nothing when the id declarator stood alone.
        Item::PropsIdDecl { .. } => SimpleItemLowering::NeedsRewriter,
        Item::BindThisLocal { name } => SimpleItemLowering::Statement(format!("let {name};")),
        // The component-event dispatcher declaration — a caller-owned lowering
        // (the caller emits the keyword-preserving verbatim declaration; the
        // dispatcher call stays plain, never a runtime-helper rewrite).
        Item::DispatcherDecl { .. } => SimpleItemLowering::NeedsRewriter,
        // A LEGACY `export let` prop statement lowers through the FALLIBLE
        // rewriter: each declarator emits its own `let <local> = $.prop($$props,
        // <key>, <flags>[, <default>]);` declaration (default expressions
        // rewrite through the shared rewriter) — the caller (which holds the
        // rewriter + the unified declarator plan) handles it.
        Item::ExportLetProps { .. } => SimpleItemLowering::NeedsRewriter,
        // A promoted LEGACY `let` lowers through the FALLIBLE rewriter: its init
        // (which may read sibling signals) rewrites, then wraps in the
        // `$.mutable_source(...)` cell — the caller handles it.
        Item::MutableSourceLet { .. } => SimpleItemLowering::NeedsRewriter,
        // A plain-local DOM bind-target root: the declaration stays a verbatim plain
        // `let name = <literal init>;` (official keeps the plain local), or a bare
        // `let name;` for the uninitialized form. The init was restricted to a
        // literal-only value at classification, so it carries no signal read to
        // rewrite — emitted byte-for-byte.
        Item::BindLocalLet { name, init } => SimpleItemLowering::Statement(match init {
            Some(init) => format!("let {name} = {init};"),
            None => format!("let {name};"),
        }),
        // A named function-pair function: its body lowers through the FALLIBLE rewriter,
        // which lives on the projection — the caller handles it.
        Item::FunctionDecl { .. } => SimpleItemLowering::NeedsRewriter,
        // A `$store` SOURCE const: its init lowers through the FALLIBLE rewriter
        // (a store read/write inside the init rewrites; a shadowed `$a` callback
        // param stays verbatim) — the caller (which holds the rewriter) handles it.
        Item::StoreSourceDecl { .. } => SimpleItemLowering::NeedsRewriter,
        // A `$store` DEPENDENCY class (a local store implementation reached from
        // a subscribed base) emits its body VERBATIM — official frames the store
        // via `new`, class body unchanged; a class body is plain JS with no
        // signal-bearing reactive surface, so no rewriter pass runs.
        Item::StoreClassDecl { source, .. } => SimpleItemLowering::Statement(source.clone()),
        // A top-level `$inspect(...);` / `$inspect(...).with(...);` statement is
        // production-ELIDED: it emits NOTHING (no helper, no import, no dev form —
        // official `dev:false` drops the statement, leaving only a cosmetic `;;`
        // residue Verter does not reproduce). The `.with` context-frame fact is
        // owned by the `needs_context` scan, not this lowering.
        Item::InspectElided => SimpleItemLowering::None,
        // A `$effect(fn);` / `$effect.pre(fn);` statement and a `$effect.root(fn)`
        // / `$effect.tracking()` declarator init lower their payload through the
        // FALLIBLE rewriter (the callee → the registered helper, body signal reads
        // → `$.get`, an `await` refuses) — the caller (which holds the rewriter)
        // handles them.
        Item::EffectStatement { .. } | Item::EffectRuneInit { .. } => {
            SimpleItemLowering::NeedsRewriter
        }
        // A LEGACY `$:` reactive statement lowers through the FALLIBLE rewriter
        // into a `$.legacy_pre_effect` REGISTRATION emitted after every other
        // body statement in dependency order — the caller (which holds the
        // rewriter + the registration-order walk) intercepts it before this
        // dispatch.
        Item::ReactiveStatement { .. } => SimpleItemLowering::NeedsRewriter,
    }
}

/// The outcome of lowering one simple (rewriter-free) instance-script item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SimpleItemLowering {
    /// The emitted client-body statement.
    Statement(String),
    /// The item emits no component-body declaration (a no-default props
    /// destructure, a production-elided `$inspect` statement).
    None,
    /// The item is a `StatePrimitive` / `FunctionDecl` / `EffectStatement` /
    /// `EffectRuneInit` whose payload lowers through the FALLIBLE expression
    /// rewriter — the caller (which holds the rewriter) handles it.
    NeedsRewriter,
}

/// Lower a supported `$state` / `$state.raw` instance item to its emitted declaration.
///
/// The wrapper choice comes from the binding's resolved write-gated [`StateLowering`]
/// (`PlainLet` / `StateSignal` / `RawStateSignal` / `BareProxy` / `StateProxy`), so the
/// emission matches official. `init` is the ALREADY-REWRITTEN init text (a signal read
/// inside a proxiable object init became `$.get`, TS stripped) supplied by the caller; a
/// no-arg `$state()` is the SHADOW-ROBUST `void 0` form (never the bare identifier
/// `undefined`).
pub(super) fn lower_state_primitive_item(
    name: &str,
    init: Option<&str>,
    bindings: &BindingTable,
) -> String {
    // The instance-script `$state` is a ROOT-scope binding (first in the table), so a name
    // lookup is unambiguous here. A BLOCK declarator that could SHADOW an instance binding
    // resolves its lowering by BINDING ID and calls `state_primitive_decl` directly.
    let lowering = bindings
        .all()
        .iter()
        .find(|b| b.name.as_str() == name)
        .and_then(|b| b.state.map(|s| s.lowering));
    state_primitive_decl(name, init, lowering)
}

/// The emitted `$state` / `$state.raw` declaration for an already-RESOLVED write-gated
/// [`StateLowering`], matching pinned `svelte@5.56.3`:
///
/// - `PlainLet`       → `let o = <init>;`                     (never reactively read)
/// - `StateSignal`    → `let o = $.state(<init>);`            (primitive, reassigned)
/// - `RawStateSignal` → `let o = $.state(<init>);`            (`$state.raw`, reassigned — NO `$.proxy`)
/// - `BareProxy`      → `let o = $.proxy(<init>);`            (proxiable, never reassigned)
/// - `StateProxy`     → `let o = $.state($.proxy(<init>));`   (proxiable, reassigned)
///
/// `init` is the FINAL init text — a primitive-literal slice for a block declarator, or
/// the FULLY-REWRITTEN init (signal reads → `$.get`, TS stripped) for an instance-script
/// declarator. The caller resolves `lowering` from the binding it OWNS (by binding id for
/// a block declarator, by root-scope name for an instance item), so a SHADOWING same-name
/// binding can never select the wrong wrapper. A no-arg `$state()` is the SHADOW-ROBUST
/// `void 0` form (never the bare `undefined`).
pub(super) fn state_primitive_decl(
    name: &str,
    init: Option<&str>,
    lowering: Option<StateLowering>,
) -> String {
    let arg = init.unwrap_or("void 0");
    match lowering {
        Some(StateLowering::PlainLet) => format!("let {name} = {arg};"),
        // A primitive signal AND a reassigned `$state.raw` are both a bare
        // `$.state(<init>)` (raw NEVER proxies).
        Some(StateLowering::StateSignal) | Some(StateLowering::RawStateSignal) => {
            format!("let {name} = $.state({arg});")
        }
        // A proxiable object/array `$state` never reassigned is a bare `$.proxy(<init>)`
        // (deep-reactive, NOT a signal).
        Some(StateLowering::BareProxy) => format!("let {name} = $.proxy({arg});"),
        // A proxiable object/array `$state` that is reassigned wraps the proxy in a
        // signal box.
        Some(StateLowering::StateProxy) => format!("let {name} = $.state($.proxy({arg}));"),
        // An unclassified state (no binding row) is a compiler-invariant violation —
        // emit the bare signal form (the never-live defensive arm; the classifier is the
        // authority).
        None => format!("let {name} = $.state({arg});"),
    }
}

/// The fixed leading `rest_excludes` keys the official compiler always prepends
/// (in this order) before the source keys of the non-rest members — the
/// auto-injected magic slots a `$.rest_props` object never surfaces.
pub(super) const REST_EXCLUDE_PREFIX: [&str; 3] = ["$$slots", "$$events", "$$legacy"];

/// The `$props()` rest / whole-object CAPTURE binding facts on the unified
/// [`PropsDeclaratorPlan`]: the local binding name, the ORDERED exclude keys (the
/// fixed [`REST_EXCLUDE_PREFIX`] then each non-rest member's SOURCE key in source
/// order — empty for the whole-object form), and the shared membership
/// `FxHashSet` (the O(1) hot member-visit exclude lookup, handed to the read form
/// as an `Arc`).
///
/// Both the rest (`{ …, ...rest }`) and whole-object (`let all = $props()`) forms
/// lower to `let <local> = $.rest_props($$props, rest_excludes)` and hoist a
/// `var rest_excludes = new Set([<quoted excludes>])`; the ONLY difference is the
/// exclude-key content (a whole-object capture excludes only the fixed prefix).
/// The capture KIND is not carried — no consumer needs to distinguish rest from
/// whole-object (both lower identically off the exclude keys).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PropsRestCapture {
    /// The local binding name (`rest` / `all`).
    pub(super) local: String,
    /// The ORDERED exclude keys — the fixed prefix then each non-rest member's
    /// SOURCE key, in source order (the emitted `new Set([…])` order).
    pub(super) excludes: Vec<String>,
    /// The exclude-key membership set, shared (`Arc`) into the read form so the
    /// hot member-visit exclude lookup is O(1) and never clones the key `Vec`.
    pub(super) exclude_set: Arc<FxHashSet<String>>,
}

/// The destructure key name of an object-pattern property.
fn prop_key_name(prop: &oxc_ast::ast::BindingProperty<'_>) -> String {
    match &prop.key {
        oxc_ast::ast::PropertyKey::StaticIdentifier(id) => id.name.to_string(),
        oxc_ast::ast::PropertyKey::StringLiteral(s) => s.value.to_string(),
        _ => single_ident(&prop.value).unwrap_or("").to_string(),
    }
}

/// The single declared identifier name of a binding pattern, or `None` for a
/// destructure.
fn single_ident<'a>(pattern: &'a BindingPattern<'a>) -> Option<&'a str> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "expr_emit_tests.rs"]
mod tests;
