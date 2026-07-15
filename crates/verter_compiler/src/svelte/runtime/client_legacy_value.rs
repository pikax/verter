//! The UNIFIED authored template-value preparation — the single owner of the
//! official `build_expression` legacy branch (svelte@5.56.3 client
//! `shared/utils.js`):
//!
//! ```js
//! (dep_read_1, dep_read_2, …, $.untrack(() => value))
//! ```
//!
//! Every authored template expression at an INVENTORIED authored-value surface
//! is PREPARED exactly once during fallible planning through
//! [`SupportedClientIr::prepare_template_value`], keyed by its semantic
//! [`AuthoredValueSurface`]. The surface-keyed [`policy`] is the ONE
//! wrap-vs-raw decision point; emitters choose topology (inline, memoized,
//! derived, thunked) but never decide whether legacy wrapping applies. Callers
//! pass the semantic surface — never a caller-selected wrap/raw boolean — and
//! the component mode is read authoritatively from `self.ir.component` (never a
//! caller argument). This routing is a fail-closed invariant over the
//! inventoried surfaces, NOT a backend-wide type guarantee: the narrow plan
//! still flattens some authored expressions into raw `String` fields — the
//! D-61 residual tracked in `docs/arch/svelte-native-compiler-plan.md`.
//!
//! In a DEFINITELY-legacy component (non-runes AND not the official
//! `maybe_runes` in-between mode), a `BuildExpression`-classified value whose
//! synchronous part has a call, a member expression, or an assignment rewrites
//! to a sequence of VISIBLE dependency reads followed by the `$.untrack`-ed
//! authored value — legacy reactivity is coarse-grained, so the statically
//! visible dependencies are read tracked while the value itself evaluates
//! untracked. Per-kind dep reads mirror the official rule set exactly:
//!
//! - `normal` non-import bindings (a plain local, a module-script binding, a
//!   `{#snippet}` name, a local function) never join the deps;
//! - a legacy prop / `{#await}` binding / `{@const}` / declaration-tag local
//!   (the official `bindable_prop` / `template` kinds) and every IMPORT wrap
//!   their read in `$.deep_read_state(…)`;
//! - a mutable-source local, an `{#each}` item, a store accessor, and a
//!   snippet parameter read PLAIN (`$.get(x)` / `$s()` / bare name).
//!
//! `Raw`-classified surfaces (spread operands, `class:` conditions,
//! declaration-tag initializers, lifecycle arguments, event handlers,
//! keyed-each keys, `<svelte:element this>`, `{@debug}` arguments) still route
//! through this entry point but produce a raw carrier — the routing is uniform;
//! the policy is central. Synthesized values (the `class:`/`style:` directive
//! objects, the `$.clsx(...)` class-base composite) are NOT authored
//! expressions: they never enter preparation and are carried as
//! [`SynthesizedTemplateValue`](super::synthesized_value::SynthesizedTemplateValue)
//! by type — the sealed carrier holds no wrap-typed state and no accessor
//! yields a wrapped rendering, so the carrier AS DEFINED cannot hold or emit
//! a wrap. The routing guard's owner-only wrap-syntax tripwire scans emitter
//! modules for wrap-syntax bytes fabricated outside this owner.

use super::client_codegen_helpers::{concise_arrow_expr_body, js_thunk};
use super::client_plan::SupportedClientIr;
use super::expr::{BindingRuntimeKind, ExprReference, ScopeId, UnwrappedRootKind};
use super::ir::{ExprId, SvelteMode};
use super::unsupported::UnsupportedSvelteRuntimeSurface;

/// A newtype over an authored template [`ExprId`] — the analyzed-expression
/// input shape of the preparation entry point, so a synthesized string can
/// never enter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AuthoredExpr(pub(super) ExprId);

/// The authored-input vocabulary of the sole preparation entry
/// ([`SupportedClientIr::prepare_template_value`]): a whole analyzed template
/// expression (by [`ExprId`]), or the `{@render}` dynamic-callee source slice
/// peeled from its trailing call — a `&str` slice paired with its
/// canonical-analysis facts. The wrap-vs-raw decision is centralized in
/// [`policy`]; authored provenance is enforced by ROUTING — every INVENTORIED
/// call site passes an analyzed expression id or the canonical render-callee
/// slice, pinned by the routing guard's call-site inventory (fail-closed over
/// the inventoried surfaces, not a backend-wide completeness proof — the D-61
/// residual) — not by this type.
#[derive(Clone, Copy, Debug)]
pub(super) enum AuthoredValueInput<'i> {
    /// A whole authored template expression (by analyzed [`ExprId`]).
    Expr(AuthoredExpr),
    /// The `{@render}` DYNAMIC-callee SLICE — peeled from the trailing call by
    /// the render planner. The slice's own reference / wrap-trigger /
    /// zero-arg-callee facts ride the CANONICAL-analysis
    /// [`RenderDynamicCalleeFacts`] (populated by the single parse — so the
    /// outer snippet CALL never mis-triggers the wrap, and no per-consumer
    /// reparse re-derives them).
    RenderCalleeSlice {
        /// The peeled callee source slice (sliced by the populated span).
        source: &'i str,
        /// The render expression's scope.
        scope: ScopeId,
        /// The slice's canonical-analysis facts.
        facts: &'i super::expr::RenderDynamicCalleeFacts,
        /// The already-rewritten callee (the shared source-preserving rewriter).
        rewritten: &'i str,
    },
}

impl From<AuthoredExpr> for AuthoredValueInput<'_> {
    fn from(authored: AuthoredExpr) -> Self {
        AuthoredValueInput::Expr(authored)
    }
}

/// The closed semantic-surface vocabulary of the INVENTORIED
/// authored-template-expression positions (the D-61 raw-string authored routes
/// are the deferred residual, not covered by this enum). The surface — not the
/// caller — selects the wrap policy through [`policy`]'s exhaustive match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AuthoredValueSurface {
    /// A reactive text interpolation (`{expr}`).
    ReactiveText,
    /// A regular DOM attribute / property value (dynamic or mixed chunk),
    /// including `autofocus` / media `muted` and the `bind:group` value.
    AttributeValue,
    /// An ordinary component prop value (`foo={expr}`).
    ComponentProp,
    /// An ordinary `<slot>` prop value.
    SlotProp,
    /// A `{@render}` argument (`{@render block(expr)}`).
    RenderArg,
    /// The `{@render}` DYNAMIC-callee slice — the callee expression peeled from
    /// a dynamic render call (`{@render obj.snip()(x)}` → the peeled callee
    /// `obj.snip()`), prepared through the [`AuthoredValueInput::RenderCalleeSlice`]
    /// input arm. Shares the `RenderArg` legacy-wrap policy (both route through
    /// the official `build_expression`), but is a distinct authored position so
    /// the peeled-callee wrap is independently classified.
    RenderCallee,
    /// The authored `class={expr}` single base — BEFORE any synthesized
    /// `$.clsx` wrap (official applies `build_expression` first).
    ClassBase,
    /// The authored `style={expr}` single base.
    StyleBase,
    /// An explicit `style:name={expr}` directive value, including each
    /// authored mixed chunk (`style:width="a{expr}b"`).
    StyleDirectiveValue,
    /// A co-located ordinary attribute value inside a regular or
    /// `<svelte:element>` `$.attribute_effect` fold.
    AttributeEffectValue,
    /// An `{#if}` / `{:else if}` test.
    IfCondition,
    /// The `{#each}` collection.
    EachCollection,
    /// The `{#await}` promise.
    AwaitPromise,
    /// The `{#key}` expression.
    KeyExpression,
    /// The `{@html}` payload.
    HtmlPayload,
    /// A `{@const}` initializer.
    ConstInitializer,
    /// An element-position `{@attach}` payload.
    AttachPayload,
    /// An authored `<title>` expression chunk inside `<svelte:head>`.
    TitleChunk,
    /// A component spread operand (`{...expr}`) — RAW but memoizable.
    ComponentSpreadOperand,
    /// A `<slot>` spread operand — RAW and never memoized.
    SlotSpreadOperand,
    /// A regular-element / `<svelte:element>` spread operand — RAW with
    /// respect to `build_expression`, but the attribute-effect memoizer still
    /// receives it (official `SpreadAttribute` + `Memoizer.add`).
    ElementSpreadOperand,
    /// A `class:name={expr}` directive condition — RAW; only the synthesized
    /// directive object may memoize as a whole.
    ClassDirectiveCondition,
    /// A `{const …}` / `{let …}` declaration-tag initializer — RAW and inert.
    DeclarationTagInitializer,
    /// A `use:action={arg}` argument — RAW.
    UseActionArg,
    /// A `transition:`/`in:`/`out:` params argument — RAW.
    TransitionParams,
    /// An `animate:` params argument — RAW.
    AnimationParams,
    /// An ordinary event-handler expression — RAW with respect to
    /// `build_expression` (official has separate handler wrapper logic).
    EventHandler,
    /// A keyed-each KEY expression — RAW.
    EachKeyExpression,
    /// The `<svelte:element this={expr}>` tag expression — RAW.
    SvelteElementThis,
    /// The `<svelte:component this={expr}>` dynamic component selector — RAW.
    ComponentSelector,
    /// A `{@debug}` argument — RAW under its dedicated snapshot lowering.
    DebugArg,
    /// A `<svelte:boundary>` `onerror` / `failed` / `pending` attribute prop —
    /// RAW (official `SvelteBoundary.js` visits the attribute expression
    /// without `build_expression`).
    BoundaryProp,
}

/// Whether the official legacy `build_expression` wrap applies to a surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LegacyPolicy {
    /// The surface routes through `build_expression` — the wrap applies in a
    /// definitely-legacy component when the trigger fires.
    BuildExpression,
    /// The surface is visited RAW — the wrap NEVER applies.
    Raw,
}

/// The official consumer behavior of a surface — informational topology (it
/// does not force one emitter; specialized carriers stay specialized).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ValueTopology {
    /// Embedded directly at its position (directive-object members,
    /// declaration initializers, handler slots, debug args).
    Inline,
    /// Routed through the combined `$.template_effect` memoizer.
    TemplateEffectMemo,
    /// Routed through the per-call `DerivedMemoizer` (component/slot/render).
    DerivedMemo,
    /// The `{#if}` test — call-bearing tests gain an outer `$.derived`.
    IfCondition,
    /// Routed through the per-effect `$.attribute_effect` memoizer.
    AttributeEffectMemo,
    /// The `{@const}` initializer — the mode-aware derived declaration.
    ConstDerived,
    /// Embedded as a getter-thunk body (`() => value`).
    Thunk,
}

/// The two orthogonal policy axes of one surface: the wrap question
/// ([`LegacyPolicy`]) and the official consumer behavior ([`ValueTopology`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ValuePolicy {
    /// Whether the official `build_expression` legacy wrap applies.
    pub(super) legacy: LegacyPolicy,
    /// The official consumer topology.
    pub(super) topology: ValueTopology,
}

/// The centralized surface policy — the ONE exhaustive, wildcard-free match
/// answering the wrap question per surface. Adding an [`AuthoredValueSurface`]
/// variant without classifying it here fails compilation.
pub(super) const fn policy(surface: AuthoredValueSurface) -> ValuePolicy {
    use AuthoredValueSurface as S;
    use LegacyPolicy::{BuildExpression, Raw};
    use ValueTopology as T;
    match surface {
        S::ReactiveText => ValuePolicy {
            legacy: BuildExpression,
            topology: T::TemplateEffectMemo,
        },
        S::AttributeValue => ValuePolicy {
            legacy: BuildExpression,
            topology: T::TemplateEffectMemo,
        },
        S::ComponentProp => ValuePolicy {
            legacy: BuildExpression,
            topology: T::DerivedMemo,
        },
        S::SlotProp => ValuePolicy {
            legacy: BuildExpression,
            topology: T::DerivedMemo,
        },
        S::RenderArg => ValuePolicy {
            legacy: BuildExpression,
            topology: T::DerivedMemo,
        },
        S::RenderCallee => ValuePolicy {
            legacy: BuildExpression,
            topology: T::Inline,
        },
        S::ClassBase => ValuePolicy {
            legacy: BuildExpression,
            topology: T::TemplateEffectMemo,
        },
        S::StyleBase => ValuePolicy {
            legacy: BuildExpression,
            topology: T::TemplateEffectMemo,
        },
        S::StyleDirectiveValue => ValuePolicy {
            legacy: BuildExpression,
            topology: T::Inline,
        },
        S::AttributeEffectValue => ValuePolicy {
            legacy: BuildExpression,
            topology: T::AttributeEffectMemo,
        },
        S::IfCondition => ValuePolicy {
            legacy: BuildExpression,
            topology: T::IfCondition,
        },
        S::EachCollection => ValuePolicy {
            legacy: BuildExpression,
            topology: T::Thunk,
        },
        S::AwaitPromise => ValuePolicy {
            legacy: BuildExpression,
            topology: T::Thunk,
        },
        S::KeyExpression => ValuePolicy {
            legacy: BuildExpression,
            topology: T::Thunk,
        },
        S::HtmlPayload => ValuePolicy {
            legacy: BuildExpression,
            topology: T::Thunk,
        },
        S::ConstInitializer => ValuePolicy {
            legacy: BuildExpression,
            topology: T::ConstDerived,
        },
        S::AttachPayload => ValuePolicy {
            legacy: BuildExpression,
            topology: T::Thunk,
        },
        S::TitleChunk => ValuePolicy {
            legacy: BuildExpression,
            topology: T::TemplateEffectMemo,
        },
        S::ComponentSpreadOperand => ValuePolicy {
            legacy: Raw,
            topology: T::DerivedMemo,
        },
        S::SlotSpreadOperand => ValuePolicy {
            legacy: Raw,
            topology: T::Thunk,
        },
        S::ElementSpreadOperand => ValuePolicy {
            legacy: Raw,
            topology: T::AttributeEffectMemo,
        },
        S::ClassDirectiveCondition => ValuePolicy {
            legacy: Raw,
            topology: T::Inline,
        },
        S::DeclarationTagInitializer => ValuePolicy {
            legacy: Raw,
            topology: T::Inline,
        },
        S::UseActionArg => ValuePolicy {
            legacy: Raw,
            topology: T::Thunk,
        },
        S::TransitionParams => ValuePolicy {
            legacy: Raw,
            topology: T::Thunk,
        },
        S::AnimationParams => ValuePolicy {
            legacy: Raw,
            topology: T::Thunk,
        },
        S::EventHandler => ValuePolicy {
            legacy: Raw,
            topology: T::Inline,
        },
        S::EachKeyExpression => ValuePolicy {
            legacy: Raw,
            topology: T::Inline,
        },
        S::SvelteElementThis => ValuePolicy {
            legacy: Raw,
            topology: T::Thunk,
        },
        S::ComponentSelector => ValuePolicy {
            legacy: Raw,
            topology: T::Thunk,
        },
        S::DebugArg => ValuePolicy {
            legacy: Raw,
            topology: T::Inline,
        },
        S::BoundaryProp => ValuePolicy {
            legacy: Raw,
            topology: T::Inline,
        },
    }
}

/// The prepared expression payload — closed: a value is either RAW (the
/// rewritten client expression) or the ONE final legacy sequence. There is no
/// `rewritten + Option<sequence>` double representation. PRIVATE to this
/// module: `LegacySequence` construction is owner-only, so no planner or
/// emitter can hand-build a wrapped carrier.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PreparedExpression {
    /// The raw rewritten client expression (no wrap applied).
    Raw(super::output::MappedCode),
    /// The bare legacy wrap sequence (`dep, …, $.untrack(…)`) — the carrier
    /// methods add only context-required parentheses.
    LegacySequence(super::output::MappedCode),
}

/// Plan-computed facts about the authored expression, recorded once at
/// preparation so no consumer re-derives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PreparedValueFacts {
    /// The official `metadata.expression.has_call` (memoize trigger).
    pub(super) has_call: bool,
    /// The official `metadata.expression.has_state` (effect membership).
    pub(super) has_state: bool,
    /// Whether the transparent-paren-unwrapped root is a `SequenceExpression`.
    pub(super) unwrapped_is_sequence: bool,
    /// The unwrapped root kind (drives `needs_clsx` and shape decisions).
    pub(super) root_kind: UnwrappedRootKind,
}

/// One PREPARED authored template value: the surface-policied expression plus
/// its plan-computed facts and the official `b.thunk` UNTHUNK fact (the
/// zero-arg identifier callee whose rewrite stays a plain identifier — from
/// the canonical analysis, never a reparse of generated text). Constructed
/// ONLY by [`SupportedClientIr::prepare_template_value`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PreparedTemplateValue {
    surface: AuthoredValueSurface,
    expression: PreparedExpression,
    facts: PreparedValueFacts,
    /// The REWRITTEN bare-identifier callee of a direct, non-optional,
    /// zero-argument identifier call (`rest()` → `Some("rest")`): the official
    /// `b.thunk` unthunk fact. `None` for every other shape, and for a callee
    /// whose rewrite is not a plain identifier (`$.get(x)()` /
    /// `$$props.x()` keep the full thunk).
    unthunk_callee: Option<String>,
}

impl PreparedTemplateValue {
    /// TEST-ONLY raw-carrier constructor for emitter unit tests that exercise
    /// serialization shapes without a full plan. Production construction goes
    /// exclusively through
    /// [`SupportedClientIr::prepare_template_value`].
    #[cfg(test)]
    pub(super) fn test_raw(surface: AuthoredValueSurface, text: &str) -> Self {
        Self {
            surface,
            expression: PreparedExpression::Raw(super::output::MappedCode::unmapped(text)),
            facts: PreparedValueFacts {
                has_call: false,
                has_state: false,
                unwrapped_is_sequence: false,
                root_kind: UnwrappedRootKind::Other,
            },
            unthunk_callee: None,
        }
    }

    /// The plan-computed facts.
    pub(super) fn facts(&self) -> &PreparedValueFacts {
        &self.facts
    }

    /// The official `has_call` memoize trigger.
    pub(super) fn has_call(&self) -> bool {
        self.facts.has_call
    }

    /// Whether the legacy wrap applied (the value is the one final sequence).
    pub(super) fn is_wrapped(&self) -> bool {
        matches!(self.expression, PreparedExpression::LegacySequence(_))
    }

    /// The value for an INLINE position: the raw expression unchanged, or the
    /// sequence with its ONE context-required paren pair (`(seq)`).
    pub(super) fn inline_expression(&self) -> String {
        match &self.expression {
            PreparedExpression::Raw(text) => text.as_str().to_string(),
            PreparedExpression::LegacySequence(seq) => format!("({})", seq.as_str()),
        }
    }

    /// Mapped counterpart of [`Self::inline_expression`].
    pub(super) fn inline_mapped_expression(&self) -> super::output::MappedCode {
        match &self.expression {
            PreparedExpression::Raw(text) => text.clone(),
            PreparedExpression::LegacySequence(seq) => seq.clone().wrapped("(", ")"),
        }
    }

    /// The value for a MEMOIZER input slot: the raw expression, or the BARE
    /// sequence (the deps-array arrow parenthesizes it into `() => (seq)`).
    pub(super) fn memo_input(&self) -> &str {
        match &self.expression {
            PreparedExpression::Raw(text) | PreparedExpression::LegacySequence(text) => {
                text.as_str()
            }
        }
    }

    /// The value handed to an effect/attribute memoizer's `add` (the official
    /// `memoize(build_expression(…))` order): a memoized (`has_call`) value
    /// passes the bare sequence / raw text; a non-memoized wrapped value embeds
    /// inline as the parenthesized sequence.
    pub(super) fn effect_value(&self) -> String {
        if self.facts.has_call {
            self.memo_input().to_string()
        } else {
            self.inline_expression()
        }
    }

    /// Mapped counterpart of [`Self::effect_value`].
    pub(super) fn effect_mapped_value(&self) -> super::output::MappedCode {
        if self.facts.has_call {
            match &self.expression {
                PreparedExpression::Raw(text) | PreparedExpression::LegacySequence(text) => {
                    text.clone()
                }
            }
        } else {
            self.inline_mapped_expression()
        }
    }

    /// The value as a getter-THUNK (`() => value`): a raw value routes through
    /// the shared [`js_thunk`] (the official `b.thunk` zero-arg unthunk,
    /// decided from the ANALYZED zero-arg-callee fact — never a reparse of the
    /// generated text); a wrapped value keeps the thunk over the parenthesized
    /// sequence.
    pub(super) fn thunk(&self) -> String {
        match &self.expression {
            PreparedExpression::Raw(text) => {
                js_thunk(self.unthunk_callee.as_deref(), text.as_str())
            }
            PreparedExpression::LegacySequence(seq) => format!("() => ({})", seq.as_str()),
        }
    }

    /// The value as a CONCISE-ARROW BODY (`() => <here>`), unconditionally
    /// parenthesized through the shared [`concise_arrow_expr_body`] wrap so an
    /// object-literal / sequence body stays a valid expression body.
    pub(super) fn arrow_body(&self) -> String {
        match &self.expression {
            PreparedExpression::Raw(text) => concise_arrow_expr_body(text.as_str()),
            PreparedExpression::LegacySequence(seq) => format!("({})", seq.as_str()),
        }
    }
}

impl<'a> SupportedClientIr<'a> {
    /// Whether the component is DEFINITELY legacy — the official
    /// `!runes && !maybe_runes` gate the value wrap (and ONLY the wrap) keys on.
    pub(super) fn is_definitely_legacy(&self) -> bool {
        self.ir.component.mode == SvelteMode::Legacy && !self.ir.component.maybe_runes
    }

    /// Prepare ONE authored template value for its semantic surface — the SOLE
    /// authored-value preparation entry point, over the closed
    /// [`AuthoredValueInput`] vocabulary (an analyzed expression or the
    /// `{@render}` peeled callee slice). Rewrites the expression through the
    /// shared value-position printer, computes the plan facts, and applies the
    /// surface policy: a `BuildExpression` surface legacy-wraps in a
    /// definitely-legacy component when the official trigger
    /// (`has_call || has_member_expression || has_assignment`) fires; a `Raw`
    /// surface always yields the raw carrier. The component mode is read from
    /// `self.ir.component` — never caller-supplied.
    pub(super) fn prepare_template_value<'i>(
        &self,
        authored: impl Into<AuthoredValueInput<'i>>,
        surface: AuthoredValueSurface,
    ) -> Result<PreparedTemplateValue, UnsupportedSvelteRuntimeSurface> {
        let (rewritten, facts, trigger_member, zero_arg_callee, root_ident, scope, references) =
            match authored.into() {
                AuthoredValueInput::Expr(AuthoredExpr(expr)) => {
                    let analyzed = self.ir.analysis.expressions.get(expr);
                    // The sync member/assignment wrap-trigger fact is populated by
                    // the CANONICAL analysis parse; a torn expression FAILS CLOSED
                    // here (never a silent-raw `false`).
                    let trigger_member = analyzed.has_sync_member_or_assignment.map_err(|()| {
                        UnsupportedSvelteRuntimeSurface::expression_fact_recovery(
                            "sync-member-or-assignment",
                        )
                    })?;
                    let facts = PreparedValueFacts {
                        has_call: self.expr_has_call(expr)?,
                        has_state: self.expr_has_state(expr)?,
                        unwrapped_is_sequence: analyzed.unwrapped_is_sequence,
                        root_kind: analyzed.unwrapped_root_kind,
                    };
                    // The transparent-paren-peeled BARE-identifier root — the
                    // accessor-read unthunk candidate (read from the shared
                    // typed matcher projection of the same canonical parse).
                    let root_ident = match &analyzed.matcher_expr {
                        super::expr::MatcherExpr::Identifier(name) => Some(name.as_str()),
                        _ => None,
                    };
                    (
                        self.rewrite_value_preserving_source(expr)?,
                        facts,
                        trigger_member,
                        analyzed.direct_zero_arg_call_callee.as_deref(),
                        root_ident,
                        analyzed.scope,
                        analyzed.references.as_slice(),
                    )
                }
                AuthoredValueInput::RenderCalleeSlice {
                    source,
                    scope,
                    facts,
                    rewritten,
                } => {
                    // The slice's reference / wrap-trigger / zero-arg facts come
                    // from the CANONICAL parse's callee-subtree analysis (the
                    // populated `RenderDynamicCalleeFacts`); only the scope-aware
                    // has_call / binding-impurity halves re-walk the slice, and
                    // both FAIL CLOSED on a recovery failure. The peeled callee is
                    // never a bare sequence (it was the callee OF a call), so the
                    // value-printer sequence facts are vacuous.
                    let value_facts = PreparedValueFacts {
                        has_call: super::reactive_analysis::expr_has_call(
                            source,
                            scope,
                            &self.ir.analysis.bindings,
                            &self.ir.analysis.scopes,
                            &self.declared_roots,
                        )
                        .map_err(|()| {
                            UnsupportedSvelteRuntimeSurface::expression_fact_recovery("has-call")
                        })?,
                        has_state: super::reactive_analysis::expr_references_signal(
                            &facts.references,
                            scope,
                            &self.ir.analysis.bindings,
                            &self.ir.analysis.scopes,
                        ) || super::reactive_analysis::expr_has_binding_impurity(
                            source,
                            scope,
                            &self.ir.analysis.bindings,
                            &self.ir.analysis.scopes,
                        )
                        .map_err(|()| {
                            UnsupportedSvelteRuntimeSurface::expression_fact_recovery(
                                "binding-impurity",
                            )
                        })?,
                        unwrapped_is_sequence: false,
                        root_kind: UnwrappedRootKind::Other,
                    };
                    (
                        super::output::MappedCode::unmapped(rewritten),
                        value_facts,
                        facts.has_sync_member_or_assignment,
                        facts.direct_zero_arg_call_callee.as_deref(),
                        facts.root_ident.as_deref(),
                        scope,
                        facts.references.as_slice(),
                    )
                }
            };
        // The official `b.thunk` unthunk fact — decided from the ANALYZED
        // shape facts + the shared rewriter (see `unthunk_callee`), never a
        // reparse of generated text.
        let unthunk_callee = self.unthunk_callee(zero_arg_callee, root_ident, scope)?;
        let expression = match policy(surface).legacy {
            LegacyPolicy::BuildExpression => {
                match self.legacy_wrap_rewritten(
                    facts.has_call,
                    trigger_member,
                    unthunk_callee.as_deref(),
                    scope,
                    references,
                    &rewritten,
                )? {
                    Some(seq) => {
                        PreparedExpression::LegacySequence(super::output::MappedCode::unmapped(seq))
                    }
                    None => PreparedExpression::Raw(rewritten),
                }
            }
            LegacyPolicy::Raw => PreparedExpression::Raw(rewritten),
        };
        Ok(PreparedTemplateValue {
            surface,
            expression,
            facts,
            unthunk_callee,
        })
    }

    /// The official `b.thunk` unthunk-callee fact, decided over the TRANSFORMED
    /// value's shape (official unthunks the transformed node) from typed
    /// analysis facts + the shared rewriter — never a parse/pattern scan of
    /// generated text. Two authored shapes produce a `<ident>()` transformed
    /// value:
    ///
    /// - an authored zero-arg identifier call `c()` whose callee REWRITES to a
    ///   plain identifier (a signal/prop callee rewrites to `$.get(c)` /
    ///   `$$props.c` — a member/call, so the full thunk stays);
    /// - an authored BARE identifier `x` whose READ lowers to the zero-arg
    ///   accessor call `x()` (a legacy prop / store accessor read), decided by
    ///   EXACT equality against the one constructed accessor form.
    fn unthunk_callee(
        &self,
        zero_arg_callee: Option<&str>,
        root_ident: Option<&str>,
        scope: ScopeId,
    ) -> Result<Option<String>, UnsupportedSvelteRuntimeSurface> {
        if let Some(name) = zero_arg_callee {
            let rewritten = self.rewrite_source(name, scope)?;
            return Ok(
                super::client_codegen_helpers::is_plain_js_identifier(&rewritten)
                    .then_some(rewritten),
            );
        }
        if let Some(name) = root_ident {
            let rewritten = self.rewrite_source(name, scope)?;
            if rewritten == format!("{name}()") {
                return Ok(Some(name.to_string()));
            }
        }
        Ok(None)
    }

    /// The wrap core over the caller-supplied trigger facts + references —
    /// called ONLY by [`prepare_template_value`](Self::prepare_template_value)
    /// (both input arms of the closed [`AuthoredValueInput`] vocabulary). The
    /// trigger halves arrive as FACTS (`has_call` from the shared scope-aware
    /// analysis, `has_member_or_assignment` from the canonical parse) — the
    /// wrap core re-derives nothing from source.
    #[allow(clippy::too_many_arguments)]
    fn legacy_wrap_rewritten(
        &self,
        has_call: bool,
        has_member_or_assignment: bool,
        unthunk_callee: Option<&str>,
        scope: ScopeId,
        references: &[ExprReference],
        rewritten: &str,
    ) -> Result<Option<String>, UnsupportedSvelteRuntimeSurface> {
        if !self.is_definitely_legacy() {
            return Ok(None);
        }
        // The official trigger: `has_call || has_member_expression || has_assignment`.
        if !has_call && !has_member_or_assignment {
            return Ok(None);
        }
        // The visible dependency reads: every reference that resolves to a
        // binding — INCLUDING one captured inside a nested fn/arrow body (the
        // official `metadata.references` Set records every visited reference;
        // only the wrap TRIGGER is synchronous-part-only) — deduped in
        // first-reference source order (the Set insertion order).
        let mut parts: Vec<String> = Vec::new();
        let mut seen: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
        for r in references {
            if !seen.insert(r.name.as_str()) {
                continue;
            }
            let Some(kind) =
                self.ir
                    .analysis
                    .bindings
                    .resolve_kind(&self.ir.analysis.scopes, scope, &r.name)
            else {
                // A global never joins the deps.
                continue;
            };
            match kind {
                // The official `normal` non-import skip: a plain local, a
                // module-script binding, a `{#snippet}` name (declaration kind
                // `function`) — never a dependency.
                BindingRuntimeKind::PlainLocal
                | BindingRuntimeKind::ModuleBinding
                | BindingRuntimeKind::SnippetName => {}
                // A legacy prop (official `bindable_prop`), an `{#await}` binding,
                // a `{@const}`, and a declaration-tag local (official `template`
                // kind) deep-read their rewritten read; an IMPORT (component or
                // value) deep-reads its live bare-name read.
                BindingRuntimeKind::Prop
                | BindingRuntimeKind::BindableProp
                | BindingRuntimeKind::AwaitSignal
                | BindingRuntimeKind::LegacyConstDerived
                | BindingRuntimeKind::TemplateDeclLocal
                | BindingRuntimeKind::ComponentImport
                | BindingRuntimeKind::ImportedValue => {
                    let read = self.rewrite_source(&r.name, scope)?;
                    parts.push(format!("$.deep_read_state({read})"));
                }
                // Every other declared binding reads PLAIN: a mutable-source
                // local (`$.get(x)`), an `{#each}` item (official `each` kind),
                // a store accessor (`$s()`), a snippet parameter (bare name),
                // and the runes-only kinds (unreachable in a definitely-legacy
                // component, but the official analog — a non-`normal` binding —
                // is a plain getter, so the mapping stays total and exact).
                BindingRuntimeKind::MutableSource
                | BindingRuntimeKind::EachSignal
                | BindingRuntimeKind::StoreSubscription
                | BindingRuntimeKind::SnippetParam
                | BindingRuntimeKind::StateSignal { .. }
                | BindingRuntimeKind::BareProxy
                | BindingRuntimeKind::StateProxy
                | BindingRuntimeKind::Derived
                | BindingRuntimeKind::EffectTrackingConst
                | BindingRuntimeKind::PropsIdConst => {
                    parts.push(self.rewrite_source(&r.name, scope)?);
                }
            }
        }
        // The untracked authored value — the official `$.untrack(b.thunk(value))`
        // with the shared zero-arg unthunk (`fn()` → `$.untrack(fn)`), decided
        // from the analyzed unthunk fact.
        parts.push(format!(
            "$.untrack({})",
            js_thunk(unthunk_callee, rewritten)
        ));
        Ok(Some(parts.join(", ")))
    }
}
