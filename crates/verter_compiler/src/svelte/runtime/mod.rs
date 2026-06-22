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
pub mod client;
mod client_allowlist;
mod client_codegen_helpers;
mod client_effect;
mod client_plan;
mod client_plan_types;
mod client_shapes;
mod client_surface;
mod client_walk;
mod css_reject;
mod entity_decode;
mod entity_table;
mod events;
pub mod expr;
pub mod expr_emit;
pub mod expr_rewrite;
pub mod helpers;
pub mod html;
pub mod ir;
mod naming;
mod official_reject;
mod official_rule;
mod ops;
mod options_reject;
mod parse_refusal;
mod reactive_analysis;
mod reactive_fold;
mod reactive_fold_tristate;
mod rune_scan;
mod state_scan;
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
    SvelteElement, SvelteElementKind, SvelteNode, SvelteSpecialKind, SvelteTag, SvelteTagKind,
};
use verter_span::Span;

use attr_lowering::{lower_attributes, AttrHost};
use expr::{
    classify_state_lowering, collect_expr_references, parse_declarators, parse_pattern_names,
    parse_render_call, reparse_module, AnalyzedExpr, BindingInfo, BindingRuntimeKind, BindingTable,
    BindingUseSet, DeclaratorKeyword, ExprArena, ExprRefKind, RenderCalleeShape, ScopeGraph,
    ScopeId, ScriptAnalysis, ScriptUseCollector, StateClassification, StateRuneKind,
};
use html::StaticTemplatePlan;
use ir::{
    AttrIr, BlockIr, ComponentIr, ComponentIrNode, DeclKind, ElementIr, EscapeMode, ExprId,
    IfBranch, IrNode, NodeId, PatternBindings, PatternId, RenderCallee, RuntimeAnalysis, RuntimeOp,
    SpecialElementIr, SpecialKind, SvelteMode, SvelteRuntimeIr, TagIr, TemplateDeclarator,
    TemplateScope, TemplateScopeId,
};
use state_scan::{collect_state_declarations, script_uses_runes};

/// Re-export the public IR + analysis + planning surface so consumers reach it
/// through one module path. (`emit_client_module` is module-private — the client
/// emission entry consumers use is [`compile_client`], which builds the narrow
/// plan; the emitter never accepts the broad IR.)
pub use client::{ClientModule, UnsupportedSvelteRuntimeSurface};
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
    /// The explicit `runes` COMPILE-OPTION override. When `Some`, it wins outright.
    /// When `None`, the lowering next honors an in-source `<svelte:options runes={…}>`
    /// directive (read via [`forced_runes_option`](crate::svelte::parser::forced_runes_option)),
    /// and only if neither is present does it infer the mode from rune USAGE.
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
use parse_refusal::parse_domain_gate;

/// The lowering context: the source, the arenas being built, and the analysis
/// state.
pub(super) struct LoweringCtx<'a> {
    pub(super) source: &'a str,
    nodes: Vec<IrNode>,
    ops: Vec<RuntimeOp>,
    template_scopes: Vec<TemplateScope>,
    expressions: ExprArena<'a>,
    /// The binding-pattern arena: each entry is the declared binding ids a
    /// pattern introduces (one per declared name, so a destructure does not
    /// collapse onto a single binding). Retained on the final analysis.
    patterns: Vec<PatternBindings>,
    scopes: ScopeGraph,
    bindings: BindingTable,
    pub(super) errors: RuntimeLoweringErrors,
    /// Pending `{@render}` tags whose callee is resolved AFTER lowering (so a
    /// forward-referenced snippet declared later in the same scope still resolves).
    pending_renders: Vec<PendingRender>,
}

/// A `{@render}` tag awaiting callee resolution: the node to finalize, the inner
/// expression span, and the scope it renders in.
struct PendingRender {
    /// The `IrNode::Tag(TagIr::Render { .. })` node to finalize.
    node: NodeId,
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
            Ok(references) => self.expressions.push(AnalyzedExpr {
                source: text,
                scope,
                references,
            }),
            Err(()) => {
                self.errors.push(
                    "svelte-runtime-expr-parse",
                    format!("could not parse template expression `{text}`"),
                    span,
                );
                self.expressions.push(AnalyzedExpr {
                    source: text,
                    scope,
                    references: Vec::new(),
                })
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
    // --- Mode inference: explicit compile-option override wins; then an explicit
    // `<svelte:options runes={…}>` override (Svelte's own forced-mode switch,
    // shared with the IDE projector via `forced_runes_option`); otherwise infer
    // from rune USAGE. `runes={true}` forces runes mode despite zero rune calls;
    // `runes={false}` forces legacy mode even when a rune name is present. ---
    let instance_source = parsed.instance_content().map(|s| span_text(source, s));
    let module_source = parsed.module_content().map(|s| span_text(source, s));
    let runes = opts
        .runes
        .or_else(|| forced_runes_option(source, &parsed.template))
        .unwrap_or_else(|| {
            instance_source
                .map(|t| script_uses_runes(alloc, t))
                .unwrap_or(false)
                || module_source
                    .map(|t| script_uses_runes(alloc, t))
                    .unwrap_or(false)
        });
    let mode = if runes {
        SvelteMode::Runes
    } else {
        SvelteMode::Legacy
    };

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
    let module_state_tracking = prepare_state_bindings(
        module_source,
        alloc,
        module_scope_id,
        &mut scopes,
        &mut bindings,
    );
    // Declare each instance-script `$state` binding in the root scope with its
    // SCRIPT-side observed uses; the template-side writes are attributed AFTER the
    // template scope graph is built (so a shadowing template binding is honoured).
    let state_tracking = prepare_state_bindings(
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
    resolve_render_callees(&mut ctx);

    // Attribute scope-resolved TEMPLATE writes to the tracked `$state` bindings
    // (instance + module) and finalize each binding's classification. A write is
    // attributed to a binding only when it scope-resolves to that EXACT binding, so
    // a template write to a name shadowed by an instance binding never reaches the
    // shadowed module binding.
    finalize_state_classifications(&mut ctx, &module_state_tracking);
    finalize_state_classifications(&mut ctx, &state_tracking);

    // Populate the reactive runtime ops for every reactive surface the lowering
    // detected, attaching each op to its owning template scope.
    ops::populate_runtime_ops(&ctx.nodes, &mut ctx.template_scopes, &mut ctx.ops);

    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }

    let component = ComponentIr {
        name: derive_component_name(opts),
        mode,
    };
    let analysis = RuntimeAnalysis {
        scripts: ScriptAnalysis {
            instance_source,
            module_source,
        },
        expressions: ctx.expressions,
        scopes: ctx.scopes,
        bindings: ctx.bindings,
        patterns: ctx.patterns,
    };

    Ok(SvelteRuntimeIr {
        component,
        analysis,
        root: root_template,
        template_scopes: ctx.template_scopes,
        nodes: ctx.nodes,
        ops: ctx.ops,
    })
}

/// One tracked instance-script `$state` binding awaiting final classification:
/// its declaration facts, its SCRIPT-side observed uses, and the [`BindingId`] of
/// its root-scope binding row.
struct TrackedState {
    /// The declared rune flavour.
    declared: StateRuneKind,
    /// Whether the initializer is PROXIABLE (`should_proxy(init)`). Init-shape
    /// only — it never changes after declaration.
    proxiable: bool,
    /// The uses observed on the SCRIPT side (refined by template writes later).
    script_uses: BindingUseSet,
    /// The root-scope binding row to finalize.
    binding: BindingId,
}

/// Declare the instance-script `$state` bindings in the root scope with a
/// PROVISIONAL (script-side) classification, returning the tracking data the
/// post-template finalizer needs.
///
/// The classification is only provisional here because a `$state` binding's
/// lowering is WRITE-gated and a write may live in a TEMPLATE expression
/// (`onclick={() => count++}`) whose scope graph does not exist until the
/// template is lowered. The final classification happens in
/// [`finalize_state_classifications`] once the scope graph is complete and a
/// shadowing template binding can be resolved.
fn prepare_state_bindings(
    instance_source: Option<&str>,
    alloc: &Allocator,
    root_scope: ScopeId,
    scopes: &mut ScopeGraph,
    bindings: &mut BindingTable,
) -> Vec<TrackedState> {
    let Some(text) = instance_source else {
        return Vec::new();
    };
    let Some(program) = reparse_module(alloc, text) else {
        return Vec::new();
    };
    let decls = collect_state_declarations(&program);
    if decls.is_empty() {
        return Vec::new();
    }

    // Collect script-side uses (reassign / deep-mutate) for each declared state
    // name, scope-aware (a nested local of the same name shadows).
    let names: Vec<String> = decls.iter().map(|(n, _, _)| n.clone()).collect();
    let mut collector = ScriptUseCollector::tracking(&names);
    use oxc_ast_visit::Visit;
    collector.visit_program(&program);

    let mut tracked = Vec::with_capacity(decls.len());
    for (name, declared, proxiable) in decls {
        let script_uses = collector.use_set(&name);
        let lowering = classify_state_lowering(declared, proxiable, script_uses);
        let binding = bindings.push(BindingInfo {
            name: name.clone(),
            scope: root_scope,
            kind: state_kind_for_lowering(lowering),
            state: Some(StateClassification {
                declared,
                proxiable,
                uses: script_uses,
                lowering,
            }),
        });
        scopes.declare(root_scope, &name, binding);
        tracked.push(TrackedState {
            declared,
            proxiable,
            script_uses,
            binding,
        });
    }
    tracked
}

/// Attribute scope-resolved TEMPLATE writes to the tracked `$state` bindings and
/// finalize each binding's classification.
///
/// Walks every analyzed template expression. For each WRITE reference (a
/// reassignment or a deep mutation), it resolves the referenced name through the
/// scope graph at the expression's own scope: only when it resolves to the EXACT
/// tracked `$state` binding (not a shadowing each / `{@const}` / nested local of
/// the same name) is the write merged into that binding's use-set. The final
/// `StateClassification` + binding kind are then recomputed from the combined
/// script + template uses.
fn finalize_state_classifications(ctx: &mut LoweringCtx, tracked: &[TrackedState]) {
    if tracked.is_empty() {
        return;
    }
    // Index the tracked bindings by their root BindingId for O(1) resolution.
    let tracked_ids: rustc_hash::FxHashMap<BindingId, usize> = tracked
        .iter()
        .enumerate()
        .map(|(i, t)| (t.binding, i))
        .collect();

    // Start each binding's combined uses from its script-side observation.
    let mut combined: Vec<BindingUseSet> = tracked.iter().map(|t| t.script_uses).collect();

    for expr in ctx.expressions.all() {
        for r in &expr.references {
            let write = match r.kind {
                ExprRefKind::Reassign => Some(false),
                ExprRefKind::DeepMutate => Some(true),
                ExprRefKind::Read => None,
            };
            let Some(deep) = write else { continue };
            // Resolve the written name in the expression's own scope: only a write
            // that resolves to the EXACT tracked $state binding counts (a shadowing
            // local of the same name resolves elsewhere).
            let Some(resolved) = ctx.scopes.resolve(&ctx.bindings, expr.scope, &r.name) else {
                continue;
            };
            if let Some(&idx) = tracked_ids.get(&resolved) {
                if deep {
                    combined[idx].deep_mutated = true;
                } else {
                    combined[idx].reassigned = true;
                }
            }
        }
    }

    // A TWO-WAY `bind:` writes back to its bound target, so the bound `$state` is
    // observed as WRITTEN even though its expression is a syntactic READ — a
    // `bind:value={name}` makes `name` a reassigned signal (a bare-identifier
    // target is a reassignment; a member target `bind:value={o.x}` is a deep
    // mutation). This mirrors the official compiler treating a bind target as
    // mutated. The write attribution is scope-resolved, so a shadowing local is
    // never mis-attributed.
    attribute_bind_target_writes(ctx, &tracked_ids, &mut combined);

    for (t, uses) in tracked.iter().zip(combined) {
        let lowering = classify_state_lowering(t.declared, t.proxiable, uses);
        let info = ctx.bindings.get_mut(t.binding);
        info.kind = state_kind_for_lowering(lowering);
        info.state = Some(StateClassification {
            declared: t.declared,
            proxiable: t.proxiable,
            uses,
            lowering,
        });
    }
}

/// Attribute the WRITE-BACK of every two-way `bind:` directive to its bound
/// `$state` binding. Walks the IR nodes for an `AttrIr::Bind { target, expr }`
/// whose target is a two-way writable bind (anything except `this`, which is a
/// one-way element-ref write of the binding, also a reassignment), resolves the
/// bind expression's referenced binding scope-awarely, and marks it reassigned (a
/// bare-identifier target) or deep-mutated (a member target).
fn attribute_bind_target_writes(
    ctx: &LoweringCtx,
    tracked_ids: &rustc_hash::FxHashMap<BindingId, usize>,
    combined: &mut [BindingUseSet],
) {
    for node in &ctx.nodes {
        let attrs = match node {
            IrNode::Element(el) => &el.attrs,
            IrNode::Component(c) => &c.attrs,
            IrNode::Special(s) => &s.attrs,
            _ => continue,
        };
        for attr in attrs {
            let AttrIr::Bind {
                expr: Some(expr_id),
                ..
            } = attr
            else {
                continue;
            };
            let analyzed = ctx.expressions.get(*expr_id);
            // The bind expression's STRUCTURAL lvalue shape decides the write: a
            // bare-identifier target is a reassignment; a member target is a deep
            // mutation. Classify it from the parsed OXC node (NOT a `source` text
            // scan), so a member access that is not the target root cannot
            // mis-classify. A non-lvalue target (a literal / call) carries no
            // attributable write.
            let target_alloc = Allocator::default();
            let is_member = match expr::classify_bind_target(&target_alloc, analyzed.source) {
                Some(expr::BindTargetKind::Member) => true,
                Some(expr::BindTargetKind::Identifier) => false,
                // A non-lvalue bind target attributes no write.
                None => continue,
            };
            for r in &analyzed.references {
                let Some(resolved) = ctx.scopes.resolve(&ctx.bindings, analyzed.scope, &r.name)
                else {
                    continue;
                };
                if let Some(&idx) = tracked_ids.get(&resolved) {
                    if is_member {
                        combined[idx].deep_mutated = true;
                    } else {
                        combined[idx].reassigned = true;
                    }
                }
            }
        }
    }
}

/// Map a resolved `$state` lowering to its binding runtime kind.
fn state_kind_for_lowering(lowering: StateLowering) -> BindingRuntimeKind {
    match lowering {
        StateLowering::PlainLet => BindingRuntimeKind::PlainLocal,
        StateLowering::StateSignal => BindingRuntimeKind::StateSignal { raw: false },
        StateLowering::RawStateSignal => BindingRuntimeKind::StateSignal { raw: true },
        StateLowering::BareProxy => BindingRuntimeKind::BareProxy,
        StateLowering::StateProxy => BindingRuntimeKind::StateProxy,
    }
}

/// Lower one template node into the IR, returning its node id (or `None` for a
/// node that does not contribute a runtime node — e.g. whitespace handling is
/// preserved as text).
fn lower_node(ctx: &mut LoweringCtx, node: &SvelteNode, scope: ScopeId) -> Option<NodeId> {
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
    // The attribute host kind decides how an `on*` event lowers (the official
    // `metadata.delegated` parent-kind rule): a regular element delegates, a
    // component forwards the handler as a prop, a `<svelte:element>` runs it through
    // `$.attribute_effect`, and a window/body/document binds a direct global
    // listener. Compute it from the parser element kind BEFORE lowering attributes.
    let host = attr_host_for(&el.kind);
    let attrs = lower_attributes(ctx, &el.attributes, scope, host);
    let mut children = Vec::new();
    for child in &el.children {
        if let Some(id) = lower_node(ctx, child, scope) {
            children.push(id);
        }
    }
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
        })),
        SvelteElementKind::Special(special) => {
            let Some(kind) = lower_special_kind(*special) else {
                ctx.errors.push(
                    "svelte-runtime-unknown-special-element",
                    format!("unrecognised `<svelte:{}>` special element", el.name),
                    el.open_span,
                );
                return None;
            };
            // A `<svelte:element this={…}>` / `<svelte:component this={C}>` carries
            // its dynamic-tag / component selector in the `this` attribute. That is
            // NOT a DOM attribute — official reads `node.tag` / `node.expression` —
            // so split it out into the distinct `this_expr` fact and DROP it from the
            // generic attribute list (it must not surface as a `set_attribute` /
            // attribute slot). Only Element / Component specials consume `this`.
            let (this_expr, attrs) =
                if matches!(kind, SpecialKind::Element | SpecialKind::Component) {
                    extract_this_expr(attrs)
                } else {
                    (None, attrs)
                };
            ctx.push_node(IrNode::Special(SpecialElementIr {
                kind,
                span: el.open_span,
                attrs,
                this_expr,
                children,
                scope,
            }))
        }
    };
    Some(node)
}

/// Split the dynamic-tag / component `this` selector out of a special element's
/// attribute list: REMOVE the attribute named `this` from `attrs` and return its
/// reactive expression (`<svelte:element this={tag}>` → `Some(tag)`). A STATIC
/// `this="div"` (a literal tag) is still removed from `attrs` (it is not a DOM
/// attribute) but carries no [`ExprId`], so `this_expr` is `None`. Any non-`this`
/// attribute / directive stays in `attrs`.
fn extract_this_expr(attrs: Vec<AttrIr>) -> (Option<ExprId>, Vec<AttrIr>) {
    let mut this_expr = None;
    let mut kept = Vec::with_capacity(attrs.len());
    for attr in attrs {
        let is_this = match &attr {
            AttrIr::Static { name, .. }
            | AttrIr::Dynamic { name, .. }
            | AttrIr::Mixed { name, .. } => name == "this",
            _ => false,
        };
        if is_this {
            // Capture the reactive expression of a dynamic `this={…}`; a static
            // `this="div"` / mixed `this` carries no single ExprId (the tag literal /
            // concatenation is the emitting backend's concern at 5f).
            if let AttrIr::Dynamic { expr, .. } = &attr {
                this_expr = Some(*expr);
            }
            // Drop the `this` attribute from the generic list either way.
            continue;
        }
        kept.push(attr);
    }
    (this_expr, kept)
}

/// Map a parser element kind to the attribute host kind that decides how an `on*`
/// event lowers (the official `metadata.delegated` parent-kind rule). A regular
/// intrinsic element delegates; a component (incl. `<svelte:component>` /
/// `<svelte:self>`) forwards events as props; a `<svelte:element this={…}>` runs
/// them through `$.attribute_effect`; a window/body/document binds a direct global
/// listener; any other `<svelte:*>` falls through to the element-event path. An
/// unrecognised special (`Unknown`) records a diagnostic later, so the host here is
/// irrelevant (the node is dropped) — classify it as `OtherSpecial`.
fn attr_host_for(kind: &SvelteElementKind) -> AttrHost {
    match kind {
        SvelteElementKind::Intrinsic | SvelteElementKind::NestedStyle => AttrHost::Element,
        SvelteElementKind::Component => AttrHost::Component,
        SvelteElementKind::Special(special) => match special {
            SvelteSpecialKind::Element => AttrHost::DynamicElement,
            SvelteSpecialKind::Window | SvelteSpecialKind::Document | SvelteSpecialKind::Body => {
                AttrHost::GlobalSpecial
            }
            // `<svelte:component this={C}>` / `<svelte:self>` are component hosts —
            // an `on*` forwards as a prop, exactly like a `<Foo onclick>`.
            SvelteSpecialKind::Component | SvelteSpecialKind::SelfRef => AttrHost::Component,
            SvelteSpecialKind::Head
            | SvelteSpecialKind::Options
            | SvelteSpecialKind::Boundary
            | SvelteSpecialKind::Fragment
            | SvelteSpecialKind::Unknown => AttrHost::OtherSpecial,
        },
    }
}

/// Map a parser special-element kind to the IR special kind. An unrecognised
/// `<svelte:*>` (the parser's `Unknown`) yields `None` so the caller records a
/// diagnostic rather than coercing the element to a fragment.
fn lower_special_kind(kind: SvelteSpecialKind) -> Option<SpecialKind> {
    Some(match kind {
        SvelteSpecialKind::Head => SpecialKind::Head,
        SvelteSpecialKind::Window => SpecialKind::Window,
        SvelteSpecialKind::Document => SpecialKind::Document,
        SvelteSpecialKind::Body => SpecialKind::Body,
        SvelteSpecialKind::Element => SpecialKind::Element,
        SvelteSpecialKind::Boundary => SpecialKind::Boundary,
        SvelteSpecialKind::Options => SpecialKind::Options,
        SvelteSpecialKind::Component => SpecialKind::Component,
        SvelteSpecialKind::SelfRef => SpecialKind::SelfRef,
        SvelteSpecialKind::Fragment => SpecialKind::Fragment,
        SvelteSpecialKind::Unknown => return None,
    })
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

/// Lower an `{#each}` block. The item / index bindings are SIGNAL reads
/// (`EachSignal`), declared in the body scope so a same-name outer signal is
/// shadowed.
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
    // The body scope binds the item/index as signals.
    let body_scope = ctx.scopes.push_scope(Some(scope));
    let item_pat = item.map(|s| ctx.push_pattern(s, body_scope, BindingRuntimeKind::EachSignal));
    let index_pat = index.map(|s| ctx.push_pattern(s, body_scope, BindingRuntimeKind::EachSignal));
    let key_expr = key.map(|s| ctx.push_expr(s, body_scope));
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
fn lower_children_in_scope(
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
            }));
            ctx.pending_renders.push(PendingRender {
                node,
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
            lower_at_const(ctx, tag, scope)
        }
        SvelteTagKind::Const => lower_declaration_tag(ctx, tag, DeclKind::Const, scope),
        SvelteTagKind::Let => lower_declaration_tag(ctx, tag, DeclKind::Let, scope),
        SvelteTagKind::Debug => {
            let exprs = vec![ctx.push_expr(tag.inner, scope)];
            Some(ctx.push_node(IrNode::Tag(TagIr::Debug { exprs })))
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

/// Lower a `{@const … = expr}` tag into a binding pattern + an initializer
/// expression. The pattern's names + the initializer span both come from the
/// OXC-parsed declarator (no top-level-`=` text splitter), so a destructuring
/// `{@const {a, b} = obj}` declares one binding per name, NOT one collapsed
/// binding.
fn lower_at_const(ctx: &mut LoweringCtx, tag: &SvelteTag, scope: ScopeId) -> Option<NodeId> {
    let text = span_text(ctx.source, tag.inner);
    // `{@const}` always carries an initializer — wrap with `const`.
    let decls = match parse_declarators(text, DeclaratorKeyword::Const) {
        Ok(decls) => decls,
        Err(()) => {
            ctx.errors.push(
                "svelte-runtime-const-parse",
                format!("could not parse `{{@const}}` declaration `{text}`"),
                tag.span,
            );
            return None;
        }
    };
    // `{@const}` declares exactly one declarator with an initializer.
    let Some(decl) = decls.into_iter().next() else {
        ctx.errors.push(
            "svelte-runtime-const-empty",
            "`{@const}` requires a declarator".to_string(),
            tag.span,
        );
        return None;
    };
    let pattern =
        ctx.push_pattern_names(&decl.names, scope, BindingRuntimeKind::LegacyConstDerived);
    let Some((s, e)) = decl.init else {
        ctx.errors.push(
            "svelte-runtime-const-no-init",
            "`{@const}` requires an initializer".to_string(),
            tag.span,
        );
        return None;
    };
    let init_span = Span::new(tag.inner.start + s, tag.inner.start + e);
    let init = ctx.push_expr(init_span, scope);
    Some(ctx.push_node(IrNode::Tag(TagIr::LegacyConst { pattern, init })))
}

/// Lower a `{const …}` / `{let …}` declaration tag — INERT block-local
/// declarators (`TemplateDeclLocal`), DISTINCT from `{@const}`. Each declarator's
/// names + initializer span come from the OXC-parsed declaration (no `=`
/// splitter), and a destructuring declarator declares one binding per name.
fn lower_declaration_tag(
    ctx: &mut LoweringCtx,
    tag: &SvelteTag,
    kind: DeclKind,
    scope: ScopeId,
) -> Option<NodeId> {
    let text = span_text(ctx.source, tag.inner);
    // A `{let …}` tag may have NO initializer (`{let x}`), which is invalid under
    // a `const` wrapper — wrap with the matching keyword so `{let x}` parses.
    let keyword = match kind {
        DeclKind::Const => DeclaratorKeyword::Const,
        DeclKind::Let => DeclaratorKeyword::Let,
    };
    let parsed = match parse_declarators(text, keyword) {
        Ok(decls) => decls,
        Err(()) => {
            ctx.errors.push(
                "svelte-runtime-decl-parse",
                format!("could not parse declaration tag `{text}`"),
                tag.span,
            );
            return None;
        }
    };
    let mut declarators = Vec::with_capacity(parsed.len());
    for decl in parsed {
        let pattern =
            ctx.push_pattern_names(&decl.names, scope, BindingRuntimeKind::TemplateDeclLocal);
        let init = decl.init.map(|(s, e)| {
            let init_span = Span::new(tag.inner.start + s, tag.inner.start + e);
            ctx.push_expr(init_span, scope)
        });
        declarators.push(TemplateDeclarator { pattern, init });
    }
    Some(ctx.push_node(IrNode::Tag(TagIr::Declaration { kind, declarators })))
}

/// Resolve every pending `{@render}` callee now that the full scope graph exists.
///
/// A static-name call (`row(1)`) whose callee resolves to a `{#snippet}` NAME
/// binding becomes [`RenderCallee::Snippet`] with the parsed argument
/// expressions; an optional call (`getSnippet()?.()`), a non-identifier callee,
/// or an unresolved name stays [`RenderCallee::Dynamic`] (the whole inner
/// expression).
fn resolve_render_callees(ctx: &mut LoweringCtx) {
    let pending = std::mem::take(&mut ctx.pending_renders);
    for render in pending {
        let text = span_text(ctx.source, render.inner);
        let shape = match parse_render_call(text) {
            Ok(shape) => shape,
            Err(()) => {
                ctx.errors.push(
                    "svelte-runtime-render-parse",
                    format!("could not parse `{{@render}}` expression `{text}`"),
                    render.inner,
                );
                continue;
            }
        };
        let RenderCalleeShape::StaticName { name, args } = shape else {
            // Dynamic: the provisional `Dynamic(inner)` node is already correct.
            continue;
        };
        // A static-name callee is a snippet call only when it resolves to a
        // `{#snippet}` NAME binding in scope.
        let Some(binding) = ctx.scopes.resolve(&ctx.bindings, render.scope, &name) else {
            continue;
        };
        if ctx.bindings.get(binding).kind != BindingRuntimeKind::SnippetName {
            continue;
        }
        let arg_ids: Vec<ExprId> = args
            .into_iter()
            .map(|(s, e)| {
                let span = Span::new(render.inner.start + s, render.inner.start + e);
                ctx.push_expr(span, render.scope)
            })
            .collect();
        let node = render.node;
        if let IrNode::Tag(TagIr::Render { callee, args }) = &mut ctx.nodes[node.0 as usize] {
            *callee = RenderCallee::Snippet(binding);
            *args = arg_ids;
        }
    }
}

/// Plan the static templates, dynamic slots, and client-side node paths for a
/// component's runtime IR. (A thin re-export of [`html::plan_static_templates`]
/// at the module's public surface.)
#[must_use]
pub fn plan_static_templates(ir: &SvelteRuntimeIr) -> StaticTemplatePlan {
    html::plan_static_templates(ir)
}

/// The outcome of [`compile_client`] when the client module cannot be emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCompileError {
    /// The runtime lowering itself failed (a malformed construct) — carries the
    /// collected lowering diagnostics.
    Lowering(RuntimeLoweringErrors),
    /// The component uses a runtime surface this backend does not yet emit — fails
    /// closed with the typed reason (never a silent empty module).
    Unsupported(UnsupportedSvelteRuntimeSurface),
    /// The component is MALFORMED Svelte the official `svelte@5.56.3` compiler also
    /// COMPILE-ERRORS (a duplicate declaration, a `$`-prefixed binding, a duplicate /
    /// mis-`context`-ed `<script>`, an invalid HTML placement, a global `$foo`
    /// reference). Accepting it would change the observable contract from "compile
    /// error, no module" to "module exists", so it fails closed with the typed
    /// official-reject rejection (the rule class + its exact official code) — never a
    /// `Main`. The official-reject parity quadrant.
    OfficialReject(OfficialRejection),
}

/// Compile a parsed Svelte component into the `svelte/internal/client` JS module
/// (the carrier-facing entry).
///
/// Runs the full pipeline — runtime lowering → static-template planning →
/// client-topology planning → [`emit_client_module`] — and returns the emitted
/// module, or a typed [`ClientCompileError`] (a lowering failure, or an
/// unsupported surface that fails closed). `ssr` requests the server backend
/// (fails closed until the server backend lands).
pub fn compile_client<'a>(
    source: &'a str,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
    alloc: &'a Allocator,
    ssr: bool,
) -> Result<client::ClientModule, ClientCompileError> {
    // The REFUSE-BY-DEFAULT pipeline. Each stage is a choke point: an unsupported
    // surface fails closed BEFORE the next stage, so the narrow plan the emitter
    // consumes can ONLY describe a fully-supported component — emit-by-default is
    // structurally impossible.
    //
    // (0) SSR requests the server backend (fails closed until it lands).
    if ssr {
        return Err(ClientCompileError::Unsupported(
            UnsupportedSvelteRuntimeSurface::ServerGenerate {
                span: Span::new(0, 0),
            },
        ));
    }
    // (1) `official_reject_gate` — the OFFICIAL-REJECT parity gate. Refuse the
    // MALFORMED-input classes official ALSO compile-errors (a duplicate / mis-context
    // `<script>`, a `$`-prefixed binding, a duplicate accepted declaration, a global
    // `$foo` / `$$foo` reference, an invalid HTML placement) FIRST, so a genuinely
    // malformed component is rejected for being malformed — not later mis-attributed
    // to an unsupported feature, and never accepted as a divergent `Main`.
    if let Some(rejection) = official_reject::official_reject_gate(source, parsed) {
        return Err(ClientCompileError::OfficialReject(rejection));
    }
    // (2) `parse_domain_gate` — refuse the PARSE-DOMAIN surfaces the runtime IR
    // does not carry (a top-level `<style>` (5l), a `<svelte:options>` axis beyond
    // runes (5m / 5h customElement), a dev-mode codegen request (5k)) BEFORE
    // lowering, so a lossy lowering cannot hide them.
    if let Some(surface) = parse_domain_gate(source, parsed, opts) {
        return Err(ClientCompileError::Unsupported(surface));
    }
    // (2) Lower to the BROAD runtime IR (the shared substrate). The broad IR may
    // exist; it just never reaches emission.
    let ir = lower_parsed_svelte_to_ir(source, parsed, opts, alloc)
        .map_err(ClientCompileError::Lowering)?;
    // (3) `ClientSyntaxSurface::classify` — the DEFAULT-DENY classifier. It accepts
    // ONLY when every node / attr / script-item is in the supported allowlist; the
    // first unsupported surface fails closed (no wildcard accept arm).
    let classified = client_surface::ClientSyntaxSurface::classify(&ir)
        .map_err(ClientCompileError::Unsupported)?;
    // (4) `SupportedClientIr::build` — the semantic projection. It decides which
    // interpolations are ACTUALLY reactive (a non-reactive one fails closed),
    // validates lvalues, and rewrites every script item + op through the FALLIBLE
    // rewriter into the NARROW `ClientModulePlan`.
    let plan = client_plan::SupportedClientIr::build(&classified, &ir)
        .map_err(ClientCompileError::Unsupported)?;
    // (5) Plan the static templates + topology, then emit from the NARROW plan only.
    let html_plan = plan_static_templates(&ir);
    let topology = plan_client_topology(&ir, &html_plan);
    Ok(client::emit_client_module(&plan, &html_plan, &topology))
}
