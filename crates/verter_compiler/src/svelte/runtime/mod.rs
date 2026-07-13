//! The Svelte runtime pre-lowering substrate.
//!
//! This module owns the SHARED pre-lowering surface the client (`svelte/internal/client`)
//! and server (`svelte/internal/server`) backends build on. It is physically
//! separate from the IDE TSX projection ([`crate::svelte::ide`]) — the two
//! codegen paths consume the same [`ParsedSvelte`](crate::svelte::ParsedSvelte)
//! AST but never share code (the two-codegen-paths rule).
//!
//! The pipeline is:
//!
//! ```text
//! ParsedSvelte
//!   → expr.rs    RuntimeAnalysis     (OXC reparse + binding/scope classification)
//!   → ir.rs      SvelteRuntimeIr     (the semantic template IR + reactive ops)
//!   → html.rs    StaticTemplatePlan  (static-HTML skeleton + DOM-path plan)
//!   → topology.rs ClientTopologyPlan (the structural helper/import/delegate summary)
//!   → client.rs  ClientModule        (the emitted `svelte/internal/client` JS)
//! ```
//!
//! [`compile_client`] drives this end-to-end and is wired into the Svelte carrier's
//! `compile_bundle` (`crate::svelte::carrier`): a supported runes component
//! populates `bundle.main.body_code` (so `has_runtime_surface()` becomes true and
//! the host emits the `Main` virtual node), and every unsupported surface FAILS
//! CLOSED with a typed [`client::UnsupportedSvelteRuntimeSurface`] carrying its
//! owning vertical. The expression / script emission routes its source-derived
//! rewrites through [`CodeTransform`](crate::code_transform::CodeTransform); the
//! synthesized helper scaffolding is unmapped.

mod attr_lowering;
mod bind_target;
mod bind_target_names;
pub mod client;
mod client_allowlist;
mod client_attr_emit;
mod client_bind;
mod client_block_emit;
mod client_block_plan;
mod client_codegen_helpers;
mod client_compile;
mod client_component_emit;
mod client_component_plan;
mod client_custom_element;
mod client_effect;
mod client_emit;
mod client_event;
mod client_imports;
mod client_legacy_value;
mod client_lifecycle;
mod client_module_frame;
mod client_plan;
mod client_plan_attr_value;
mod client_plan_bind;
mod client_plan_block_types;
mod client_plan_element_ops;
mod client_plan_rewrite;
mod client_plan_script;
mod client_plan_spread_html;
mod client_plan_types;
mod client_shapes;
mod client_slot_plan;
mod client_spread_html_emit;
mod client_surface;
mod client_surface_element_query;
mod client_surface_imports;
mod client_surface_refuse;
mod client_surface_script;
mod client_surface_slot;
mod client_surface_special;
mod client_svelte_boundary;
mod client_svelte_element;
mod client_svelte_head;
mod client_walk;
/// The OPT-IN conformance-observability side channel — feature-gated so the
/// DEFAULT build compiles it (and every producer hook referencing it) out
/// entirely; an ungated production reference cannot compile without the
/// feature.
#[cfg(feature = "conformance-trace")]
pub mod conformance_trace;
mod cross_slot_redeclaration;
mod css;
mod css_reject;
mod custom_element;
mod declaration_tag_lowering;
mod entity_decode;
mod entity_table;
mod events;
pub mod expr;
pub mod expr_emit;
pub mod expr_rewrite;
pub mod helpers;
mod host_attr_gate;
pub mod html;
mod instance_item_shapes;
mod instance_items;
pub mod ir;
mod legacy_reactive;
mod legacy_surface;
mod lower_component;
mod naming;
mod needs_context;
mod official_reject;
mod official_rule;
mod ops;
mod parse_refusal;
mod reactive_analysis;
mod reactive_fold;
mod reactive_fold_tristate;
mod render_callees;
mod rune_scan;
mod script_body_parse;
mod state_prep;
mod state_scan;
mod store_subscriptions;
mod synthesized_value;
pub mod topology;
mod unsupported;
mod whitespace;

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "diff_oracle_tests.rs"]
mod diff_oracle_tests;

use oxc_allocator::Allocator;

use crate::svelte::parser::{
    forced_runes_option, ParsedSvelte, SvelteBlock, SvelteBlockKind, SvelteClauseKind,
    SvelteElement, SvelteElementKind, SvelteNode, SvelteTag, SvelteTagKind,
};
use verter_span::Span;

use attr_lowering::lower_attributes;
use expr::{
    collect_expr_references, parse_debug_identifier_spans, parse_pattern_names, reparse_module,
    AnalyzedExpr, BindingInfo, BindingRuntimeKind, BindingTable, ExprArena, ScopeGraph, ScopeId,
    ScriptAnalysis,
};
use html::StaticTemplatePlan;
use ir::{
    BlockIr, ComponentIr, ComponentIrNode, ComponentSlots, DebugArg, DeclKind, ElementIr,
    EscapeMode, ExprId, IfBranch, IrNode, NodeId, PatternBindings, PatternId, RenderCallee,
    RuntimeAnalysis, RuntimeOp, SpecialElementIr, SpecialKind, SvelteMode, SvelteRuntimeIr, TagIr,
    TemplateScope, TemplateScopeId,
};
use state_scan::{instance_forces_definite_legacy, script_uses_runes};

/// Re-export the public IR + analysis + planning surface so consumers reach it
/// through one module path. (`emit_client_module` is module-private — the client
/// emission entry consumers use is [`compile_client`], which builds the narrow
/// plan; the emitter never accepts the broad IR.)
pub use client::{ClientModule, UnsupportedSvelteRuntimeSurface};
pub use client_compile::{compile_client, ClientCompileError};
pub use expr::StateLowering;
pub use helpers::SvelteHelperMask;
pub use html::{DynamicSlot, NodePathPlan, PathBase};
pub use ir::BindingId;
pub use official_reject::official_reject_gate;
pub use official_rule::{CoreOfficialValidationRule, OfficialRejection};
pub use reactive_fold_tristate::{live_fallback_ledger, LiveFallbackLedgerRow};
pub use topology::{plan_client_topology, ClientTopologyPlan};

/// The options the runtime lowering reads.
///
/// Minimal by design: it carries only the inputs the binding/scope analysis and
/// component-identity derivation actually consult. Backend output knobs (SSR,
/// source maps) live on the backends' own options, not here — this substrate
/// emits no backend artifact.
#[derive(Debug, Clone, Default)]
pub struct SvelteRuntimeOptions {
    /// The carrier file name, used to derive the component-function name (stem →
    /// JS-identifier-sanitized). `None` derives `_unknown_`.
    pub filename: Option<String>,
    /// An explicit component-name override (the `name` compile option). When set,
    /// it overrides the filename-derived name.
    pub name: Option<String>,
    /// The explicit `runes` COMPILE-OPTION override. An in-source
    /// `<svelte:options runes={…}>` directive (read via
    /// [`forced_runes_option`](crate::svelte::parser::forced_runes_option))
    /// OVERRIDES this option in both directions — matching official; the
    /// option applies when the source carries no directive, and only if
    /// neither is present does the lowering infer the mode from rune USAGE.
    pub runes: Option<bool>,
    /// Production mode (strips dev-only instrumentation downstream).
    pub is_production: bool,
    /// A DEV-MODE codegen request (the `dev: true` axis — validation wrappers,
    /// `$.add_locations`, dev `$inspect` / `$.trace`). The client backend emits ONLY
    /// the PRODUCTION runes output; a dev-codegen request FAILS CLOSED (5k) rather
    /// than silently emitting production output. This is a SEPARATE signal from
    /// `is_production` (which gates downstream stripping, not the dev-codegen axis):
    /// the dev-mode output shape is a distinct compiler mode the host opts into.
    pub dev_codegen: bool,
    /// The `customElement: true` COMPILE OPTION: compile the component as a custom
    /// element with NO registration (`$.create_custom_element(…)` is emitted, but
    /// `customElements.define` is left to the user — there is no tag). An in-source
    /// `<svelte:options customElement>` value WINS over this option (the official
    /// `customElementOptions ?? customElement` precedence); a
    /// `customElement={null}` options value falls back to it.
    pub custom_element: bool,
}

/// A runtime-lowering diagnostic.
///
/// A thin wrapper carrying the same severity vocabulary as the
/// neutral [`RuntimeDiagnostic`](crate::framework_common::carrier_compiler::RuntimeDiagnostic),
/// so a lowering problem surfaces as a typed diagnostic rather than a silent
/// catch-all IR node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLoweringDiagnostic {
    /// A machine-stable code.
    pub code: &'static str,
    /// A human-readable message.
    pub message: String,
    /// The source span the diagnostic refers to.
    pub span: Span,
}

/// The collected lowering errors — a non-empty set fails [`lower_parsed_svelte_to_ir`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeLoweringErrors {
    /// The diagnostics, in discovery order.
    pub diagnostics: Vec<RuntimeLoweringDiagnostic>,
}

impl RuntimeLoweringErrors {
    /// Whether any diagnostic was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Push a diagnostic.
    pub(super) fn push(&mut self, code: &'static str, message: String, span: Span) {
        self.diagnostics.push(RuntimeLoweringDiagnostic {
            code,
            message,
            span,
        });
    }
}

use naming::derive_component_name;

/// The lowering context: the source, the arenas being built, and the analysis
/// state.
pub(super) struct LoweringCtx<'a> {
    pub(super) source: &'a str,
    nodes: Vec<IrNode>,
    ops: Vec<RuntimeOp>,
    pub(super) template_scopes: Vec<TemplateScope>,
    expressions: ExprArena<'a>,
    /// The binding-pattern arena: each entry is the declared binding ids a
    /// pattern introduces (one per declared name, so a destructure does not
    /// collapse onto a single binding). Retained on the final analysis.
    patterns: Vec<PatternBindings>,
    pub(super) scopes: ScopeGraph,
    pub(super) bindings: BindingTable,
    pub(super) errors: RuntimeLoweringErrors,
    /// Pending `{@render}` tags whose callee is resolved AFTER lowering (so a
    /// forward-referenced snippet declared later in the same scope still resolves).
    pending_renders: Vec<PendingRender>,
    /// BLOCK-scoped rune declarators (`{let x = $state(0)}`) registered during template
    /// lowering, tracked for the post-template `$state` finalizer (a template write flips
    /// the lowering to `$.state`) — the same write-gated pipeline as instance-script state.
    block_rune_tracking: Vec<state_prep::TrackedState>,
    /// The STATIC slot-FILLER host set (the direct static-`slot=`-declaring component
    /// children, any node kind), recorded by the component slot decomposition and
    /// retained on [`SvelteRuntimeIr::static_slot_filler_hosts`] — see that field for
    /// the contract.
    pub(super) static_slot_filler_hosts: rustc_hash::FxHashSet<NodeId>,
    /// The DIRECT component-child set (every source-level direct child of a
    /// component-family node, fragment-hoisted children excluded), recorded by the
    /// component slot decomposition and retained on
    /// [`SvelteRuntimeIr::direct_slot_attr_child_hosts`] — see that field for the
    /// contract.
    pub(super) direct_slot_attr_child_hosts: rustc_hash::FxHashSet<NodeId>,
    /// The DIRECT `{#snippet}`-body child set (every lowered source-level direct
    /// child of a `{#snippet}` block body), recorded at the snippet lowering call
    /// site and retained on
    /// [`SvelteRuntimeIr::direct_snippet_slot_attr_child_hosts`] — see that field
    /// for the contract.
    pub(super) direct_snippet_slot_attr_child_hosts: rustc_hash::FxHashSet<NodeId>,
}

/// A `{@render}` tag awaiting callee resolution: the node to finalize, the
/// interned provisional expression (whose stored `render_callee` fact the
/// resolution pass reads), the inner expression span, and the scope it
/// renders in.
struct PendingRender {
    /// The `IrNode::Tag(TagIr::Render { .. })` node to finalize.
    node: NodeId,
    /// The interned provisional callee expression (the whole inner text) —
    /// its stored `render_callee` fact drives the resolution (no reparse).
    expr: ExprId,
    /// The tag's inner expression span (the `callee(args)` text).
    inner: Span,
    /// The scope the render expression evaluates in.
    scope: ScopeId,
}

impl<'a> LoweringCtx<'a> {
    /// Intern a node, returning its id.
    fn push_node(&mut self, node: IrNode) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    /// Intern a template scope, returning its id. The scope's `roots`/`local_ops`
    /// are filled in by the caller after lowering its children.
    fn push_template_scope(&mut self, scope: ScopeId) -> TemplateScopeId {
        let id = TemplateScopeId(self.template_scopes.len() as u32);
        self.template_scopes.push(TemplateScope {
            scope,
            roots: Vec::new(),
            local_ops: Vec::new(),
        });
        id
    }

    /// Intern a template expression: reparse its references and record its scope.
    /// A fragment that does not parse cleanly records a parse diagnostic so the
    /// failure is surfaced rather than silently dropped to no references.
    pub(super) fn push_expr(&mut self, span: Span, scope: ScopeId) -> ExprId {
        let text = span_text(self.source, span);
        match collect_expr_references(text) {
            Ok(facts) => self
                .expressions
                .push(AnalyzedExpr::interned(text, scope, facts)),
            Err(()) => {
                self.errors.push(
                    "svelte-runtime-expr-parse",
                    format!("could not parse template expression `{text}`"),
                    span,
                );
                self.expressions.push(AnalyzedExpr::torn(text, scope))
            }
        }
    }

    /// Intern a binding pattern in `scope`: parse its declared names from the
    /// OXC-parsed pattern, create ONE binding row per declared name, declare each
    /// in `scope`, and record the declared binding ids in the pattern arena.
    ///
    /// A destructuring pattern (`{a, b}` / `[x, y]`) therefore declares two
    /// distinct bindings, not one collapsed binding. A pattern whose text fails to
    /// parse records a diagnostic and declares no names.
    fn push_pattern(&mut self, span: Span, scope: ScopeId, kind: BindingRuntimeKind) -> PatternId {
        let text = span_text(self.source, span);
        let names = match parse_pattern_names(text) {
            Ok(names) => names,
            Err(()) => {
                self.errors.push(
                    "svelte-runtime-pattern-parse",
                    format!("could not parse binding pattern `{text}`"),
                    span,
                );
                Vec::new()
            }
        };
        let mut declared = Vec::with_capacity(names.len());
        for name in names {
            let binding = self.bindings.push(BindingInfo {
                name: name.clone(),
                scope,
                kind,
                state: None,
            });
            self.scopes.declare(scope, &name, binding);
            declared.push(binding);
        }
        let id = PatternId(self.patterns.len() as u32);
        self.patterns.push(PatternBindings { bindings: declared });
        id
    }

    /// Intern a binding pattern from already-parsed declared NAMES (used for
    /// `{@const}` / declaration-tag declarators whose names + init were parsed
    /// together by [`parse_declarators`]): create one binding row per name,
    /// declare each in `scope`, and record the declared ids in the pattern arena.
    fn push_pattern_names(
        &mut self,
        names: &[String],
        scope: ScopeId,
        kind: BindingRuntimeKind,
    ) -> PatternId {
        let mut declared = Vec::with_capacity(names.len());
        for name in names {
            let binding = self.bindings.push(BindingInfo {
                name: name.clone(),
                scope,
                kind,
                state: None,
            });
            self.scopes.declare(scope, name, binding);
            declared.push(binding);
        }
        let id = PatternId(self.patterns.len() as u32);
        self.patterns.push(PatternBindings { bindings: declared });
        id
    }
}

/// The expression sub-span of a spread attribute span (`{...rest}`): the span
/// with the leading `...` (and any whitespace before/after it) stripped, so the
/// remaining text (`rest`) parses as a standalone expression.
pub(super) fn spread_expr_span(source: &str, span: Span) -> Span {
    let text = span_text(source, span);
    let trimmed = text.trim_start();
    let after_dots = trimmed.strip_prefix("...").unwrap_or(trimmed);
    let leading = (text.len() - after_dots.len()) as u32;
    Span::new(span.start + leading, span.end)
}

/// Extract the text of a span from the source.
pub(super) fn span_text(source: &str, span: Span) -> &str {
    &source[span.start as usize..span.end as usize]
}

/// Locate the directive's LOCAL-name identifier span within the attribute span,
/// for synthesizing a shorthand directive's implied expression (`class:active`
/// ⇒ the `active` reference). The local name appears in the directive head after
/// the `prefix:` and before any `|modifier` / `=`; it is a real JS identifier in
/// the source, so reparsing its span yields a genuine identifier reference (no
/// synthesized text). Returns `None` when the name cannot be located (defensive —
/// the caller then emits no synthesized op rather than a forged one).
pub(super) fn local_name_span(source: &str, attr_span: Span, local: &str) -> Option<Span> {
    if local.is_empty() {
        return None;
    }
    let head = span_text(source, attr_span);
    // The local name follows the first `:` (the `prefix:` separator). Search the
    // head AFTER that colon for the exact local-name token.
    let after_colon = head.find(':').map(|c| c + 1).unwrap_or(0);
    let rel = head[after_colon..].find(local)? + after_colon;
    let start = attr_span.start + rel as u32;
    Some(Span::new(start, start + local.len() as u32))
}

/// Lower a parsed Svelte component into the runtime IR.
///
/// Reparses the scripts + every template expression with OXC, builds the
/// scope/binding analysis (classifying `$state` bindings into their lowering and
/// modeling block/snippet/each/await/declaration-tag scopes), and synthesises the
/// semantic template IR. Emits NO JS string.
///
/// An unrecoverable malformed construct records a diagnostic and returns
/// [`Err`] with the collected errors — never a silent catch-all node.
pub fn lower_parsed_svelte_to_ir<'a>(
    source: &'a str,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
    alloc: &'a Allocator,
) -> Result<SvelteRuntimeIr<'a>, RuntimeLoweringErrors> {
    // --- Mode inference: an explicit in-source `<svelte:options runes={…}>`
    // override wins (Svelte's own forced-mode switch, shared with the IDE
    // projector via `forced_runes_option` — official lets it override the
    // compile option in both directions); then the `runes` compile-option
    // override; otherwise infer from rune USAGE. `runes={true}` forces runes
    // mode despite zero rune calls; `runes={false}` forces legacy mode even
    // when a rune name is present. The SCRIPT half of the usage inference runs
    // here; the TEMPLATE half (a `$host` occurrence in a template expression
    // is a runes indicator too) reads the per-expression free-reference facts
    // the template lowering collects, so the final mode decision completes
    // AFTER the lowering below — nothing between here and there consumes the
    // mode. ---
    let instance_source = parsed.instance_content().map(|s| span_text(source, s));
    let module_source = parsed.module_content().map(|s| span_text(source, s));
    let explicit_runes = forced_runes_option(source, &parsed.template).or(opts.runes);
    // Classify BOTH slots' static imports ONCE — the single import authority
    // (the binding preparation below and the surface classifier consume the
    // SAME carrier). Hoisted above the mode inference because the `$store`
    // CANDIDATE set — declarations + admitted import locals — feeds the
    // rune-vs-store-accessor exemption the mode detector consults: official
    // DELETES store-classified names from the reference set BEFORE the
    // `some(is_rune)` inference, so `const state = writable(0)` + a `$state`
    // reference stays LEGACY mode.
    let script_imports =
        client_surface_imports::classify_script_imports(module_source, instance_source);
    let store_candidates =
        store_subscriptions::store_base_candidates(alloc, instance_source, &script_imports);
    let store_exempt = store_subscriptions::rune_root_accessor_exemptions(&store_candidates);
    let script_runes = instance_source
        .map(|t| script_uses_runes(alloc, t, &store_exempt))
        .unwrap_or(false)
        || module_source
            .map(|t| script_uses_runes(alloc, t, &store_exempt))
            .unwrap_or(false);

    // A MALFORMED instance / module script (a non-empty OXC error set, not just a
    // panic) yields a partial AST that must NOT silently feed rune / mode / state
    // analysis — record a diagnostic so the failure surfaces rather than analyzing
    // a torn parse. `reparse_module` fails closed on `parsed.errors`, so a `None`
    // here for a present script means the script did not parse cleanly.
    let mut errors = RuntimeLoweringErrors::default();
    if let (Some(text), Some(span)) = (instance_source, parsed.instance_content()) {
        if reparse_module(alloc, text).is_none() {
            errors.push(
                "svelte-runtime-script-parse",
                "could not parse the instance `<script>` (malformed)".to_string(),
                span,
            );
        }
    }
    if let (Some(text), Some(span)) = (module_source, parsed.module_content()) {
        if reparse_module(alloc, text).is_none() {
            errors.push(
                "svelte-runtime-script-parse",
                "could not parse the module `<script>` (malformed)".to_string(),
                span,
            );
        }
    }

    // --- Scope + binding analysis over the module + instance scripts ---
    //
    // The lexical chain is `module → root (instance + template)`: the
    // `<script module>` bindings live in a MODULE scope that is the PARENT of the
    // template root scope, so an instance / template binding of the same name
    // shadows the module one (root-scope resolution wins) while an un-shadowed
    // module read still resolves up the chain.
    let (mut scopes, module_scope_id) = ScopeGraph::with_root();
    let root_scope_id = scopes.push_scope(Some(module_scope_id));
    let mut bindings = BindingTable::new();
    // Declare each module-script `$state` binding in the MODULE scope. Its writes
    // are observed on the module-script side only — a template write to a name
    // shadowed by an instance binding resolves to the instance binding, never the
    // module one.
    let module_state_tracking = state_prep::prepare_state_bindings(
        module_source,
        alloc,
        module_scope_id,
        &mut scopes,
        &mut bindings,
    );
    // Declare each instance-script `$state` binding in the root scope with its
    // SCRIPT-side observed uses; the template-side writes are attributed AFTER the
    // template scope graph is built (so a shadowing template binding is honoured).
    let state_tracking = state_prep::prepare_state_bindings(
        instance_source,
        alloc,
        root_scope_id,
        &mut scopes,
        &mut bindings,
    );
    // Declare the non-`$state` rune bindings (`$derived` → Derived, `$props()`
    // destructures → Prop / BindableProp) in their owning scope so a template read
    // resolves SCOPE-AWARELY to the right kind. These kinds are fixed at
    // declaration (no write-gated finalization, unlike `$state`). Module-script
    // rune bindings live in the module scope; instance-script ones in the root
    // scope (an instance binding of the same name shadows the module one).
    state_scan::prepare_rune_bindings(
        module_source,
        alloc,
        module_scope_id,
        &mut scopes,
        &mut bindings,
    );
    state_scan::prepare_rune_bindings(
        instance_source,
        alloc,
        root_scope_id,
        &mut scopes,
        &mut bindings,
    );
    // Declare each instance-script `export let <ident>` declarator as a `Prop`
    // binding — the LEGACY prop surface (`$.prop($$props, key, 8)`; reads are
    // accessor calls). Registered mode-independently like the rune passes: a
    // RUNES-mode `export let` is an official compile error rejected before
    // these bindings are ever consumed.
    state_scan::prepare_legacy_export_prop_bindings(
        instance_source,
        alloc,
        root_scope_id,
        &mut scopes,
        &mut bindings,
    );
    // The single `ClassifiedScriptImports` carrier was computed BEFORE the mode
    // inference above; the binding preparation below consumes the admitted
    // carriers, and the surface classifier later propagates a retained slot
    // refusal off this SAME carrier (a refused slot declares no bindings and
    // the component fails closed before any binding is consumed).
    //
    // Declare every top-level static import local as its typed NON-reactive import
    // binding — a default `.svelte`-COMPONENT import as `ComponentImport` (so a
    // `<Child/>` static callee RESOLVES to the import and reads emit the bare callee,
    // never `$.get`), every other imported local as the non-writable `ImportedValue`.
    // Module-script imports declare into the MODULE scope (the lexical parent), so an
    // un-shadowed template read resolves up the chain; instance imports declare into
    // the root scope. An import name does not collide with a `$state`/rune/plain-local
    // name in valid source, so the pass order relative to the local-binding passes is
    // immaterial.
    state_scan::prepare_import_bindings(
        script_imports.admitted(client_imports::UserImportSlot::Module),
        module_scope_id,
        &mut scopes,
        &mut bindings,
    );
    state_scan::prepare_import_bindings(
        script_imports.admitted(client_imports::UserImportSlot::Instance),
        root_scope_id,
        &mut scopes,
        &mut bindings,
    );
    // Declare the remaining top-level PLAIN-local instance-script bindings
    // (`let v = …` that is NOT a `$state` / `$derived` / `$props()` rune) as
    // `PlainLocal`. Runs AFTER the `$state` / rune passes so a name already declared
    // as a reactive binding is never re-registered as a plain local. A plain local is
    // a NON-reactive binding (a template read of it is a static fold, NOT `$.get`); it
    // becomes relevant as a DOM bind-target lvalue root, whose plain setter
    // (`name = $$value` / `o.x = $$value`) needs the binding to resolve to `PlainLocal`.
    state_prep::prepare_plain_local_bindings(
        instance_source,
        alloc,
        root_scope_id,
        &mut scopes,
        &mut bindings,
    );
    // Collect the LEGACY `let` → `$.mutable_source` promotion candidates (every
    // registered top-level PlainLocal `let`) with their SCRIPT-side writes-only
    // uses. The promotion itself is decided AFTER the template lowering + the
    // final mode inference (`finalize_legacy_let_promotions` below): a written
    // legacy `let` promotes; an unwritten one stays a plain local.
    let legacy_let_tracking = state_prep::prepare_legacy_let_tracking(
        instance_source,
        alloc,
        root_scope_id,
        &scopes,
        &bindings,
    );
    // Declare one `$store` subscription ACCESSOR binding (`$count`) per declared
    // candidate base — every top-level instance declaration name with a
    // non-rune init plus every ADMITTED import local (both slots, read from the
    // shared carrier), computed ONCE before the mode inference above. The
    // `$`-namespace is disjoint from every other binding pass, so the order is
    // immaterial; an unreferenced accessor binding is inert. A `$count`
    // read/write anywhere then resolves scope-awarely to the store-subscription
    // kind (a shadowing local of the same `$name` wins; a rune-root-NAMED
    // accessor over a declared store base — `$state` with `const state =
    // writable(0)` — resolves as a store accessor, matching official).
    store_subscriptions::prepare_store_subscription_bindings(
        &store_candidates,
        root_scope_id,
        &mut scopes,
        &mut bindings,
    );

    // --- Template IR lowering ---
    let mut ctx = LoweringCtx {
        source,
        nodes: Vec::new(),
        ops: Vec::new(),
        template_scopes: Vec::new(),
        expressions: ExprArena::new(),
        patterns: Vec::new(),
        scopes,
        bindings,
        errors,
        pending_renders: Vec::new(),
        block_rune_tracking: Vec::new(),
        static_slot_filler_hosts: rustc_hash::FxHashSet::default(),
        direct_slot_attr_child_hosts: rustc_hash::FxHashSet::default(),
        direct_snippet_slot_attr_child_hosts: rustc_hash::FxHashSet::default(),
    };

    // The root template scope owns the top-level template nodes.
    let root_template = ctx.push_template_scope(root_scope_id);
    let mut root_nodes = Vec::new();
    for node in &parsed.template {
        if let Some(id) = lower_node(&mut ctx, node, root_scope_id) {
            root_nodes.push(id);
        }
    }
    ctx.template_scopes[root_template.0 as usize].roots = root_nodes;

    // Now the full scope graph exists: resolve every `{@render}` callee (a
    // forward-referenced snippet declared later in the same scope now resolves).
    render_callees::resolve_render_callees(&mut ctx);

    // Attribute scope-resolved TEMPLATE writes to the tracked `$state` bindings
    // (instance + module) and finalize each binding's classification. A write is
    // attributed to a binding only when it scope-resolves to that EXACT binding, so
    // a template write to a name shadowed by an instance binding never reaches the
    // shadowed module binding.
    state_prep::finalize_state_classifications(&mut ctx, &module_state_tracking);
    state_prep::finalize_state_classifications(&mut ctx, &state_tracking);
    // Finalize the BLOCK-scoped rune declarators (`{let x = $state(0)}`) discovered during
    // template lowering — the same write-gated pass, so a template `x++` flips `x` to
    // `$.state`. (Taken out of `ctx` to satisfy the borrow checker — the finalizer needs
    // `&mut ctx`.)
    let block_rune_tracking = std::mem::take(&mut ctx.block_rune_tracking);
    state_prep::finalize_state_classifications(&mut ctx, &block_rune_tracking);

    // Populate the reactive runtime ops for every reactive surface the lowering
    // detected, attaching each op to its owning template scope.
    ops::populate_runtime_ops(&ctx.nodes, &mut ctx.template_scopes, &mut ctx.ops);

    // Resolve the custom-element descriptor (the `<svelte:options customElement>`
    // value, falling back to the `customElement: true` compile option). Runs after
    // the official-reject gate, so the value is official-ACCEPTED; a shape the gate
    // should have rejected is a loud lowering diagnostic, never a silent
    // plain-component downgrade.
    let custom_element = match custom_element::resolve_custom_element(parsed, opts.custom_element) {
        Ok(descriptor) => descriptor,
        Err(diagnostic) => {
            ctx.errors
                .push(diagnostic.code, diagnostic.message, diagnostic.span);
            None
        }
    };

    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }

    // --- Mode-inference completion: a `$host` occurrence in a TEMPLATE
    // expression (a handler, an interpolation, a bind) is a runes-mode
    // indicator too — official treats every unresolved rune-name reference as
    // the mode trigger, template positions included (a scriptless
    // customElement whose only rune is a template `$host()` is runes mode).
    // This reads the FREE-REFERENCE facts the lowering above already collected
    // per analyzed expression (one parse, no re-scan): a `$host` bound by a
    // local (an arrow param) is pruned from the free set, and a string /
    // comment occurrence is never a reference. An EXPLICIT override — the
    // compile option or `<svelte:options runes={…}>` — still wins in BOTH
    // directions. Only `$host` is template-inferable here: every other rune
    // name in a template expression without a script rune stays inert for the
    // MODE (their unsupported forms are refused downstream by the rune scan).
    // A `$host` whose base `host` is a declared store candidate is a STORE
    // ACCESSOR reference, never a rune — it must not flip the mode (official
    // deletes store-classified names before the inference; the store
    // classifier owns its accept/reject downstream).
    let template_uses_host_rune = !store_exempt.contains("$host")
        && ctx
            .expressions
            .all()
            .iter()
            .any(|expr| expr.references.iter().any(|r| r.name == "$host"));
    let runes = explicit_runes.unwrap_or(script_runes || template_uses_host_rune);
    let mode = if runes {
        SvelteMode::Runes
    } else {
        SvelteMode::Legacy
    };
    // The official in-between MAYBE-RUNES fact (`analysis.maybe_runes`): a
    // non-runes component with no explicit `runes: false` override and no
    // definitively-legacy instance construct (a top-level labeled statement or an
    // `export let`). Drives EXACTLY the legacy value-wrap gate — see
    // [`ComponentIr::maybe_runes`]. (`$$props`/`$$restProps` references — the
    // remaining official exclusion — are refused upstream as magic identifiers,
    // so they never reach a consumer of this fact.)
    let maybe_runes = !runes
        && explicit_runes != Some(false)
        && !instance_source
            .map(|t| instance_forces_definite_legacy(alloc, t))
            .unwrap_or(false);

    // LEGACY-mode `let` promotion: with the FINAL mode decided and the template
    // scope graph complete, promote each WRITTEN tracked top-level `let`
    // (script write, template write, or `bind:` write-back) to the
    // `$.mutable_source` binding kind — the demand-driven legacy reactivity.
    // Never run for a runes component (a runes plain local stays plain).
    // The IMPLICIT `$:` assignment-target declarations follow: a top-level
    // `$: <target> = …` binding a name not otherwise declared mints a
    // `$.mutable_source` binding at the root scope (the official implicit
    // `legacy_reactive` declaration), so every read/write of the target —
    // template and script alike — routes through the shared signal rewriter.
    let legacy_reactive_targets = if mode == SvelteMode::Legacy {
        state_prep::finalize_legacy_let_promotions(&mut ctx, &legacy_let_tracking);
        legacy_reactive::declare_reactive_assignment_targets(
            instance_source,
            alloc,
            root_scope_id,
            &mut ctx.scopes,
            &mut ctx.bindings,
        )
    } else {
        Vec::new()
    };

    let component = ComponentIr {
        name: derive_component_name(opts),
        filename: opts.filename.clone(),
        mode,
        maybe_runes,
        custom_element,
    };
    let analysis = RuntimeAnalysis {
        scripts: ScriptAnalysis {
            instance_source,
            module_source,
        },
        script_imports,
        expressions: ctx.expressions,
        scopes: ctx.scopes,
        bindings: ctx.bindings,
        patterns: ctx.patterns,
        legacy_reactive_targets,
    };

    Ok(SvelteRuntimeIr {
        component,
        analysis,
        root: root_template,
        template_scopes: ctx.template_scopes,
        nodes: ctx.nodes,
        ops: ctx.ops,
        static_slot_filler_hosts: ctx.static_slot_filler_hosts,
        direct_slot_attr_child_hosts: ctx.direct_slot_attr_child_hosts,
        direct_snippet_slot_attr_child_hosts: ctx.direct_snippet_slot_attr_child_hosts,
    })
}

/// Lower one template node into the IR, returning its node id (or `None` for a
/// node that does not contribute a runtime node — e.g. whitespace handling is
/// preserved as text).
pub(super) fn lower_node(
    ctx: &mut LoweringCtx,
    node: &SvelteNode,
    scope: ScopeId,
) -> Option<NodeId> {
    match node {
        SvelteNode::Text(span) => {
            let text = span_text(ctx.source, *span).to_string();
            Some(ctx.push_node(IrNode::Text { span: *span, text }))
        }
        SvelteNode::Comment(span) => {
            let text = span_text(ctx.source, *span).to_string();
            Some(ctx.push_node(IrNode::Comment { span: *span, text }))
        }
        SvelteNode::Interpolation(span) => {
            let expr = ctx.push_expr(*span, scope);
            Some(ctx.push_node(IrNode::Interpolation {
                span: *span,
                expr,
                escape: EscapeMode::Escaped,
            }))
        }
        SvelteNode::Element(el) => lower_element(ctx, el, scope),
        SvelteNode::Block(block) => lower_block(ctx, block, scope),
        SvelteNode::Tag(tag) => lower_tag(ctx, tag, scope),
    }
}

/// Lower an element / component / special element. An unrecognised `<svelte:*>`
/// special element records a diagnostic and contributes no node (it is NOT
/// coerced to a fragment).
fn lower_element(ctx: &mut LoweringCtx, el: &SvelteElement, scope: ScopeId) -> Option<NodeId> {
    // A `<slot>` element is the BLOCK-semantic slot outlet (the official
    // `SlotElement`) — its own IR node kind, never an intrinsic `ElementIr`: it
    // renders through a `<!>` anchor + `$.slot(...)`, its attributes take slot
    // PROPERTY semantics, and its children form the fallback template region.
    if matches!(el.kind, SvelteElementKind::Intrinsic) && el.name == "slot" {
        return Some(lower_component::lower_slot_element(ctx, el, scope));
    }
    // The attribute host kind decides how an `on*` event lowers (the official
    // `metadata.delegated` parent-kind rule): a regular element delegates, a
    // component forwards the handler as a prop, a `<svelte:element>` runs it through
    // `$.attribute_effect`, and a window/body/document binds a direct global
    // listener. Compute it from the parser element kind BEFORE lowering attributes.
    let host = lower_component::attr_host_for(&el.kind);
    let attrs = lower_attributes(ctx, &el.attributes, scope, host);
    // The special kind is resolved up front: a component-FAMILY node (a `<Foo>`
    // component, `<svelte:component>`, `<svelte:self>`, or `<svelte:fragment>`)
    // decomposes its children into SLOT regions (the official `Component.js`
    // grouping); every other element / special lowers its children FLAT in `scope`.
    let special_kind = match &el.kind {
        SvelteElementKind::Special(special) => {
            match lower_component::lower_special_kind(*special) {
                Some(kind) => Some(kind),
                None => {
                    ctx.errors.push(
                        "svelte-runtime-unknown-special-element",
                        format!("unrecognised `<svelte:{}>` special element", el.name),
                        el.open_span,
                    );
                    return None;
                }
            }
        }
        _ => None,
    };
    let is_component_family = matches!(el.kind, SvelteElementKind::Component)
        || matches!(
            special_kind,
            Some(SpecialKind::Component | SpecialKind::SelfRef | SpecialKind::Fragment)
        );

    // A RENDERABLE special whose body is a CALLBACK region (`<svelte:element>` /
    // `<svelte:boundary>`): its children render INSIDE the special's callback, so they form
    // their OWN body template scope (see `lower_renderable_special_region`), NOT part of the
    // enclosing region.
    let is_renderable_region_special = matches!(
        special_kind,
        Some(SpecialKind::Element | SpecialKind::Boundary)
    );
    // A `<svelte:head>` is a renderable-region special whose `<title>` child is SPECIAL: it
    // renders no DOM node (it drives `$.document.title`), so it is separated from the
    // body-region DOM children (`<meta>` / `<link>` / …) at lowering (the official
    // `SvelteHead` fragment + `TitleElement` split).
    let is_head = matches!(special_kind, Some(SpecialKind::Head));
    let (children, slots, body_region, head_title) = if is_component_family {
        let (children, slots) = lower_component::lower_component_slots(ctx, el, scope);
        (children, slots, None, None)
    } else if is_head {
        let (children, body_region, head_title) =
            lower_component::lower_head_region(ctx, el, scope);
        (children, ComponentSlots::default(), body_region, head_title)
    } else if is_renderable_region_special {
        let (children, slots, body_region) = lower_component::lower_renderable_special_region(
            ctx,
            el,
            scope,
            matches!(special_kind, Some(SpecialKind::Boundary)),
        );
        (children, slots, body_region, None)
    } else {
        let mut children = Vec::new();
        for child in &el.children {
            if let Some(id) = lower_node(ctx, child, scope) {
                children.push(id);
            }
        }
        (children, ComponentSlots::default(), None, None)
    };

    let node = match &el.kind {
        SvelteElementKind::Intrinsic | SvelteElementKind::NestedStyle => {
            ctx.push_node(IrNode::Element(ElementIr {
                tag: el.name.clone(),
                span: el.open_span,
                attrs,
                children,
                scope,
            }))
        }
        SvelteElementKind::Component => ctx.push_node(IrNode::Component(ComponentIrNode {
            name: el.name.clone(),
            span: el.open_span,
            attrs,
            children,
            scope,
            slots,
        })),
        SvelteElementKind::Special(_) => {
            let kind = special_kind.expect("special kind resolved above");
            // A `<svelte:element this={…}>` / `<svelte:component this={C}>` carries
            // its dynamic-tag / component selector in the `this` attribute. That is
            // NOT a DOM attribute — official reads `node.tag` / `node.expression` —
            // so split it out into the distinct `this_expr` fact and DROP it from the
            // generic attribute list (it must not surface as a `set_attribute` /
            // attribute slot). Only Element / Component specials consume `this`.
            let (this_expr, static_tag, attrs) =
                if matches!(kind, SpecialKind::Element | SpecialKind::Component) {
                    lower_component::extract_this_expr(attrs)
                } else {
                    (None, None, attrs)
                };
            ctx.push_node(IrNode::Special(SpecialElementIr {
                kind,
                span: el.open_span,
                attrs,
                this_expr,
                static_tag,
                children,
                scope,
                slots,
                body_region,
                head_title,
            }))
        }
    };
    Some(node)
}

/// Lower a block construct into the IR, creating its body template scopes.
fn lower_block(ctx: &mut LoweringCtx, block: &SvelteBlock, scope: ScopeId) -> Option<NodeId> {
    match &block.kind {
        SvelteBlockKind::If => Some(lower_if_block(ctx, block, scope)),
        SvelteBlockKind::Each { item, index, key } => {
            Some(lower_each_block(ctx, block, *item, *index, *key, scope))
        }
        SvelteBlockKind::Await {
            then_binding,
            catch_binding,
        } => Some(lower_await_block(
            ctx,
            block,
            *then_binding,
            *catch_binding,
            scope,
        )),
        SvelteBlockKind::Key => Some(lower_key_block(ctx, block, scope)),
        SvelteBlockKind::Snippet {
            name,
            name_text,
            params,
        } => Some(lower_snippet_block(
            ctx, block, *name, name_text, *params, scope,
        )),
    }
}

/// Lower an `{#if}` chain into branches (the primary branch + `{:else if}` /
/// `{:else}` clauses).
fn lower_if_block(ctx: &mut LoweringCtx, block: &SvelteBlock, scope: ScopeId) -> NodeId {
    let mut branches = Vec::new();
    // The primary `{#if expr}` branch.
    let condition = block.head_expr.map(|s| ctx.push_expr(s, scope));
    let body = lower_branch_body(ctx, &block.children, scope);
    branches.push(IfBranch { condition, body });
    // The `{:else if}` / `{:else}` clauses.
    for clause in &block.clauses {
        let condition = match clause.kind {
            SvelteClauseKind::ElseIf => clause.expr.map(|s| ctx.push_expr(s, scope)),
            SvelteClauseKind::Else => None,
            // `{:then}` / `{:catch}` never appear on an `{#if}` — defensive skip.
            SvelteClauseKind::Then | SvelteClauseKind::Catch => continue,
        };
        let body = lower_branch_body(ctx, &clause.children, scope);
        branches.push(IfBranch { condition, body });
    }
    ctx.push_node(IrNode::Block(BlockIr::If { branches }))
}

/// Lower a run of children into a fresh template scope under `parent_scope`.
fn lower_branch_body(
    ctx: &mut LoweringCtx,
    children: &[SvelteNode],
    parent_scope: ScopeId,
) -> TemplateScopeId {
    let body_scope = ctx.scopes.push_scope(Some(parent_scope));
    let ts = ctx.push_template_scope(body_scope);
    let mut roots = Vec::new();
    for child in children {
        if let Some(id) = lower_node(ctx, child, body_scope) {
            roots.push(id);
        }
    }
    ctx.template_scopes[ts.0 as usize].roots = roots;
    ts
}

/// Lower an `{#each}` block. The ITEM binding is a SIGNAL read (`EachSignal`),
/// declared in the body scope so a same-name outer signal is shadowed. The INDEX
/// binding is a signal ONLY for a KEYED each (where items reorder, so an item's
/// index can change — official sets `EACH_INDEX_REACTIVE` and reads `$.get(i)`);
/// for an UNKEYED each the index is positional and INERT (`PlainLocal`, read as
/// the plain callback parameter `i`, NOT `$.get(i)`), matching the official
/// `flags |= EACH_INDEX_REACTIVE` gate (`keyed && index`).
fn lower_each_block(
    ctx: &mut LoweringCtx,
    block: &SvelteBlock,
    item: Option<Span>,
    index: Option<Span>,
    key: Option<Span>,
    scope: ScopeId,
) -> NodeId {
    let source = block
        .head_expr
        .map(|s| ctx.push_expr(s, scope))
        .unwrap_or_else(|| ctx.push_expr(Span::new(0, 0), scope));
    // The body scope binds the item as a signal; the index is reactive ONLY when
    // the each is keyed (the official `keyed && index` reactivity gate).
    let body_scope = ctx.scopes.push_scope(Some(scope));
    let item_pat = item.map(|s| ctx.push_pattern(s, body_scope, BindingRuntimeKind::EachSignal));
    let index_kind = if key.is_some() {
        BindingRuntimeKind::EachSignal
    } else {
        BindingRuntimeKind::PlainLocal
    };
    let index_pat = index.map(|s| ctx.push_pattern(s, body_scope, index_kind));
    // The KEY expression of a keyed each is rewritten in its OWN callback scope: the
    // item / index are PLAIN callback params there (`(item) => item.id` — read plainly,
    // shadowing any same-name OUTER signal), DISTINCT from the body scope where the item
    // is a signal. This mirrors the official `key_state`, which deletes the item's signal
    // transform so the key reads it directly.
    let key_expr = key.map(|s| {
        let key_scope = ctx.scopes.push_scope(Some(scope));
        if let Some(item_span) = item {
            ctx.push_pattern(item_span, key_scope, BindingRuntimeKind::PlainLocal);
        }
        if let Some(index_span) = index {
            ctx.push_pattern(index_span, key_scope, BindingRuntimeKind::PlainLocal);
        }
        ctx.push_expr(s, key_scope)
    });
    let ts = ctx.push_template_scope(body_scope);
    let mut roots = Vec::new();
    for child in &block.children {
        if let Some(id) = lower_node(ctx, child, body_scope) {
            roots.push(id);
        }
    }
    ctx.template_scopes[ts.0 as usize].roots = roots;
    // An `{:else}` clause on the each block.
    let else_body = block
        .clauses
        .iter()
        .find(|c| c.kind == SvelteClauseKind::Else)
        .map(|c| lower_branch_body(ctx, &c.children, scope));
    ctx.push_node(IrNode::Block(BlockIr::Each {
        source,
        item: item_pat,
        index: index_pat,
        key: key_expr,
        body: ts,
        else_body,
    }))
}

/// Lower an `{#await}` block. The `{:then x}` / `{:catch e}` bindings are SIGNAL
/// reads (`AwaitSignal`), declared in their branch scope.
fn lower_await_block(
    ctx: &mut LoweringCtx,
    block: &SvelteBlock,
    then_binding: Option<Span>,
    catch_binding: Option<Span>,
    scope: ScopeId,
) -> NodeId {
    let promise = block
        .head_expr
        .map(|s| ctx.push_expr(s, scope))
        .unwrap_or_else(|| ctx.push_expr(Span::new(0, 0), scope));

    // The block's IMMEDIATE children belong to exactly ONE role, decided by the
    // form. The parser promotes a `{:then v}` clause's binding onto the block
    // kind's `then_binding`, so for the canonical CLAUSE form
    // (`{#await p}<pending>{:then v}<then>{:catch e}<catch>{/await}`) BOTH a
    // `then_binding` span AND a `Then` clause are present — the clause list, NOT
    // the inline binding span, decides the form:
    //
    // - ANY `{:then}`/`{:catch}` clause present  ⇒ CLAUSE form: immediate children
    //   are the PENDING body; each clause owns its branch children + binding.
    // - else inline then (`{#await p then v}`)    ⇒ children are the THEN body,
    //   no pending branch.
    // - else inline catch (`{#await p catch e}`)  ⇒ children are the CATCH body,
    //   no pending branch.
    // - else (`{#await p}<x>{/await}`)            ⇒ children are the PENDING body.
    let then_clause = block
        .clauses
        .iter()
        .find(|c| c.kind == SvelteClauseKind::Then);
    let catch_clause = block
        .clauses
        .iter()
        .find(|c| c.kind == SvelteClauseKind::Catch);
    let has_branch_clause = then_clause.is_some() || catch_clause.is_some();

    // Resolve the role of the immediate children, plus the inline branch bodies.
    let mut pending = None;
    let mut then_binding_pat = None;
    let mut then_body = None;
    let mut catch_binding_pat = None;
    let mut catch_body = None;

    if has_branch_clause {
        // CLAUSE form: immediate children are the pending body.
        pending = Some(lower_branch_body(ctx, &block.children, scope));
    } else if let Some(then_span) = then_binding {
        // Inline then: the immediate children ARE the then body (no pending).
        let then_scope = ctx.scopes.push_scope(Some(scope));
        let p = ctx.push_pattern(then_span, then_scope, BindingRuntimeKind::AwaitSignal);
        then_binding_pat = Some(p);
        then_body = Some(lower_children_in_scope(ctx, &block.children, then_scope));
    } else if let Some(catch_span) = catch_binding {
        // Inline catch: the immediate children ARE the catch body (no pending).
        let catch_scope = ctx.scopes.push_scope(Some(scope));
        let p = ctx.push_pattern(catch_span, catch_scope, BindingRuntimeKind::AwaitSignal);
        catch_binding_pat = Some(p);
        catch_body = Some(lower_children_in_scope(ctx, &block.children, catch_scope));
    } else {
        // Plain `{#await p}<x>{/await}`: the immediate children are the pending body.
        pending = Some(lower_branch_body(ctx, &block.children, scope));
    }

    // The `{:then}` clause (CLAUSE form) owns its own children + binding.
    if let Some(then_clause) = then_clause {
        let then_scope = ctx.scopes.push_scope(Some(scope));
        then_binding_pat = then_clause
            .expr
            .map(|s| ctx.push_pattern(s, then_scope, BindingRuntimeKind::AwaitSignal));
        then_body = Some(lower_children_in_scope(
            ctx,
            &then_clause.children,
            then_scope,
        ));
    }

    // The `{:catch}` clause (CLAUSE form) owns its own children + binding.
    if let Some(catch_clause) = catch_clause {
        let catch_scope = ctx.scopes.push_scope(Some(scope));
        catch_binding_pat = catch_clause
            .expr
            .map(|s| ctx.push_pattern(s, catch_scope, BindingRuntimeKind::AwaitSignal));
        catch_body = Some(lower_children_in_scope(
            ctx,
            &catch_clause.children,
            catch_scope,
        ));
    }

    ctx.push_node(IrNode::Block(BlockIr::Await {
        promise,
        pending,
        then_binding: then_binding_pat,
        then_body,
        catch_binding: catch_binding_pat,
        catch_body,
    }))
}

/// Lower a run of children into an EXISTING scope (used by await branches that
/// declared their binding in the scope before lowering children).
pub(super) fn lower_children_in_scope(
    ctx: &mut LoweringCtx,
    children: &[SvelteNode],
    body_scope: ScopeId,
) -> TemplateScopeId {
    let ts = ctx.push_template_scope(body_scope);
    let mut roots = Vec::new();
    for child in children {
        if let Some(id) = lower_node(ctx, child, body_scope) {
            roots.push(id);
        }
    }
    ctx.template_scopes[ts.0 as usize].roots = roots;
    ts
}

/// Lower a `{#key}` block.
fn lower_key_block(ctx: &mut LoweringCtx, block: &SvelteBlock, scope: ScopeId) -> NodeId {
    let expr = block
        .head_expr
        .map(|s| ctx.push_expr(s, scope))
        .unwrap_or_else(|| ctx.push_expr(Span::new(0, 0), scope));
    let body = lower_branch_body(ctx, &block.children, scope);
    ctx.push_node(IrNode::Block(BlockIr::Key { expr, body }))
}

/// Lower a `{#snippet}` block. The snippet name is a binding in the ENCLOSING
/// scope; its params are INERT (`SnippetParam`) locals in the body scope.
fn lower_snippet_block(
    ctx: &mut LoweringCtx,
    block: &SvelteBlock,
    _name_span: Span,
    name_text: &str,
    params: Option<Span>,
    scope: ScopeId,
) -> NodeId {
    // The snippet name binds in the enclosing scope (callable by siblings via
    // `{@render name(...)}`).
    let name_binding = ctx.bindings.push(BindingInfo {
        name: name_text.to_string(),
        scope,
        kind: BindingRuntimeKind::SnippetName,
        state: None,
    });
    ctx.scopes.declare(scope, name_text, name_binding);

    let body_scope = ctx.scopes.push_scope(Some(scope));
    let mut param_pats = Vec::new();
    if let Some(params_span) = params {
        let p = ctx.push_pattern(params_span, body_scope, BindingRuntimeKind::SnippetParam);
        param_pats.push(p);
    }
    let ts = lower_children_in_scope(ctx, &block.children, body_scope);
    // Record the lowered SOURCE-LEVEL DIRECT children of the snippet body: the
    // unified slot choke-point accepts a STATIC `slot=` on these hosts (official
    // validates a snippet direct child as component-owned placement) while rejecting
    // the dynamic/mixed forms. Populated at the SNIPPET call site — never inside
    // `lower_children_in_scope`, which `{#await}` bodies share (their roots are NOT
    // snippet children).
    let snippet_roots: Vec<NodeId> = ctx.template_scopes[ts.0 as usize].roots.clone();
    ctx.direct_snippet_slot_attr_child_hosts
        .extend(snippet_roots);
    ctx.push_node(IrNode::Block(BlockIr::Snippet {
        name: name_binding,
        params: param_pats,
        body: ts,
    }))
}

/// Lower a standalone tag.
fn lower_tag(ctx: &mut LoweringCtx, tag: &SvelteTag, scope: ScopeId) -> Option<NodeId> {
    match tag.kind {
        SvelteTagKind::Render => {
            // `{@render callee(args)}` — the callee + args are resolved AFTER
            // lowering (so a forward-referenced snippet resolves). Push a
            // provisional node + remember it; `resolve_render_callees` finalizes it.
            let provisional = ctx.push_expr(tag.inner, scope);
            let node = ctx.push_node(IrNode::Tag(TagIr::Render {
                callee: RenderCallee::Dynamic(provisional),
                args: Vec::new(),
                // Resolved in `resolve_render_callees`: set to the tag span only when a
                // spread argument is detected (`render_tag_invalid_spread_argument`).
                spread_arg_span: None,
            }));
            ctx.pending_renders.push(PendingRender {
                node,
                expr: provisional,
                inner: tag.inner,
                scope,
            });
            Some(node)
        }
        SvelteTagKind::Html => {
            let expr = ctx.push_expr(tag.inner, scope);
            Some(ctx.push_node(IrNode::Tag(TagIr::Html { expr })))
        }
        SvelteTagKind::LegacyConst => {
            // `{@const x = expr}` — a block-local derived binding. The names + the
            // initializer span both come from the OXC-parsed declarator (so a
            // destructuring `{@const {a, b} = obj}` declares two bindings, not one).
            declaration_tag_lowering::lower_at_const(ctx, tag, scope)
        }
        SvelteTagKind::Const => {
            declaration_tag_lowering::lower_declaration_tag(ctx, tag, DeclKind::Const, scope)
        }
        SvelteTagKind::Let => {
            declaration_tag_lowering::lower_declaration_tag(ctx, tag, DeclKind::Let, scope)
        }
        SvelteTagKind::Debug => {
            // `{@debug a, b}` lowers to ONE debug expression PER comma-separated
            // argument (the official `DebugTag` walks `node.identifiers`
            // individually), NOT a single `SequenceExpression` — the spans come from
            // the OXC parse, never a byte scan. A non-identifier argument is the official
            // `debug_tag_invalid_arguments` reject (the snapshot/object key must be a bare
            // name) — fail closed, never emit an invalid object key.
            let inner_text = span_text(ctx.source, tag.inner);
            let idents = match parse_debug_identifier_spans(inner_text) {
                Ok(idents) => idents,
                Err(()) => {
                    ctx.errors.push(
                        "svelte-runtime-debug-invalid-arguments",
                        format!(
                            "`{{@debug}}` arguments must be identifiers, not arbitrary \
                             expressions (`{}`)",
                            inner_text.trim()
                        ),
                        tag.span,
                    );
                    return None;
                }
            };
            // The object KEY is the parsed identifier name (carried on `DebugArg`); the
            // span only seeds the snapshot expression. So a Unicode-escaped argument keys
            // on its decoded name, not its raw escape bytes.
            let args = idents
                .into_iter()
                .map(|ident| {
                    let arg_span =
                        Span::new(tag.inner.start + ident.start, tag.inner.start + ident.end);
                    DebugArg {
                        name: ident.name,
                        expr: ctx.push_expr(arg_span, scope),
                    }
                })
                .collect();
            Some(ctx.push_node(IrNode::Tag(TagIr::Debug { args })))
        }
        SvelteTagKind::Attach => {
            let expr = ctx.push_expr(tag.inner, scope);
            Some(ctx.push_node(IrNode::Tag(TagIr::Attach { expr })))
        }
        SvelteTagKind::Unknown => {
            ctx.errors.push(
                "svelte-runtime-unknown-tag",
                "unrecognised standalone tag".to_string(),
                tag.span,
            );
            None
        }
    }
}

/// Plan the static templates, dynamic slots, and client-side node paths for a
/// component's runtime IR. (A thin re-export of [`html::plan_static_templates`]
/// at the module's public surface.) `css` is the proven `<style>` plan's
/// scope-injection facts — the skeleton serializer bakes the scope class into
/// scoped elements' static `class` attributes; `None` for a style-less
/// component.
#[must_use]
pub fn plan_static_templates(
    ir: &SvelteRuntimeIr,
    css: Option<&css::types::CssScopeFacts>,
) -> StaticTemplatePlan {
    html::plan_static_templates(ir, css)
}
