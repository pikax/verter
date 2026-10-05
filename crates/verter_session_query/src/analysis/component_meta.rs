//! Owned component metadata records: props, events, slots, models, exposed members,
//! fallthrough and root reachability of a component.

use crate::analysis::types::{ImportBindingKind, JsdocTag};
use verter_type_expr::facts::SourcePosition;
use verter_type_expr::{TypeExprScope, TypePublication};

/// Analysis-domain component metadata. No serde — only used in Rust.
/// Converted to `FfiComponentMeta` at the NAPI/WASM boundary.
#[derive(Debug, Clone)]
pub struct ComponentMetaAnalysis {
    pub props: Vec<PropAnalysis>,
    pub events: Vec<EventAnalysis>,
    pub slots: Vec<SlotAnalysis>,
    pub models: Vec<ModelAnalysis>,
    pub exposed: Vec<ExposedAnalysis>,
    /// Host-populated public-instance sidecar derived from runtime-observable members.
    pub public_instance: Option<PublicInstanceAnalysis>,
    /// Host-populated, content-free structure projected from the registered artifact.
    pub ordered_sfc_structure: Option<OrderedSfcStructureAnalysis>,
    pub type_registry: Vec<ResolvedTypeAnalysis>,
    pub components: Vec<ComponentUsageAnalysis>,
    pub template_refs: Vec<TemplateRefAnalysis>,
    pub imports: Vec<ImportAnalysis>,
    pub bindings: Vec<BindingAnalysis>,
    pub vue_api_calls: Vec<VueApiCallAnalysis>,
    pub styles: Vec<StyleAnalysis>,
    pub flags: ComponentMetaFlags,
    /// Root reachability classification for fallthrough inheritance.
    /// Extracted from template facts only — host owns all inheritance semantics.
    pub root_reachability: RootReachability,
    /// Accepted props: declared props + inherited attrs (host-populated).
    pub accepted_props: Vec<AcceptedPropAnalysis>,
    /// Accepted events: declared emits + inherited listeners (host-populated).
    pub accepted_events: Vec<AcceptedEventAnalysis>,
    /// Whether the accepted surface is exact or a lower bound.
    pub accepted_surface_completeness: AcceptedSurfaceCompleteness,
    /// Branch-structured inherited surface (host-populated).
    pub fallthrough_surface: FallthroughSurface,
    /// Macro-wide expansion diagnostics that apply to the entire macro, not to a
    /// specific property. Lifted out of per-field `type_expansion.diagnostics` to
    /// avoid duplication across every prop/event/slot in the same macro.
    pub macro_expansion_diagnostics: Vec<MacroExpansionDiagnostics>,
    pub options_api: bool,
    pub file_path: String,
}

/// Which macro kind produced the expansion diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroExpansionKind {
    DefineProps,
    DefineEmits,
    DefineSlots,
}

/// Macro-wide expansion diagnostics that are not specific to any property.
/// Stored once per macro instead of duplicated on every field.
#[derive(Debug, Clone)]
pub struct MacroExpansionDiagnostics {
    pub macro_kind: MacroExpansionKind,
    pub macro_index: usize,
    pub diagnostics: Vec<crate::analysis::type_expand::ExpansionDiagnostic>,
    pub exactness: crate::analysis::type_expand::ExpansionExactness,
    pub execution_status: crate::analysis::type_expand::ExpansionExecutionStatus,
}

/// Analyzed prop from `defineProps` / Options API `props`.
#[derive(Debug, Clone)]
pub struct PropAnalysis {
    pub name: String,
    /// Typed callable role; display text never participates in classification.
    pub callable_role: verter_type_expr::PropCallableRole,
    /// The resolved type SOURCE POSITION: the evaluated source unless the
    /// expansion is incomplete and an authored payload exists (the symbolic
    /// fallback); a PROVEN schema absence when the position carries no
    /// annotation; a typed failure when the REQUIRED value position's source
    /// could not be constructed (fails output materialization).
    pub publication: TypePublication,
    /// Completeness and diagnostics from native expansion when available.
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    /// The author's own annotation is carried separately as bundled evidence
    /// inside `publication`.
    pub required: bool,
    pub has_default: bool,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
    /// True iff the SFC author explicitly wrote this prop name as a member
    /// of the `defineProps<T>()` type argument's own body. Propagates the
    /// per-prop provenance fact carried by [`AnalyzedPropField`] so the
    /// `verter_audit::PublishedSurfacePolicy::Refined` projection can
    /// distinguish author-declared names (Vue intrinsics like `class` /
    /// `style` and `on{Event}` shadows of declared emits the author *kept*
    /// on purpose) from names that arrived via heritage / utility-type
    /// expansion (HTMLAttributes inheritance, etc.).
    pub declared_in_macro_type_arg: bool,
}

/// Analyzed event from `defineEmits`.
#[derive(Debug, Clone)]
pub struct EventAnalysis {
    pub name: String,
    /// Legacy semantic source lane retained for accepted/fallthrough mechanics.
    /// Public contract consumers use `publication`.
    pub payload: SourcePosition,
    /// Producer-owned payload publication. This retains typed authority,
    /// exactness, diagnostics, and provenance through terminal materialization.
    pub publication: TypePublication,
    /// Producer-owned callable return publication. `None` denotes the
    /// property/event-map implicit `void` return.
    pub return_publication: Option<TypePublication>,
    /// Scope used to raise [`Self::return_publication`]'s selected source.
    pub return_publication_scope: Option<TypeExprScope>,
    pub payload_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub raw_signature: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
}

/// Analyzed slot from `defineSlots` / template.
#[derive(Debug, Clone)]
pub struct SlotAnalysis {
    pub name: String,
    pub is_scoped: bool,
    pub bindings: Vec<SlotBindingAnalysis>,
    pub is_required: bool,
    pub return_type: Option<String>,
    /// Producer-owned typed return publication. Source outcome, exactness,
    /// diagnostics, provenance, and authored evidence travel together to the
    /// terminal output sink.
    pub return_publication: Option<TypePublication>,
    /// Scope used to raise [`Self::return_publication`]'s selected source.
    pub return_publication_scope: Option<TypeExprScope>,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
    /// Producer fact: does this slot come from the component's own AUTHORED
    /// slots surface — a member of the resolved `defineSlots<T>()` macro
    /// surface (inline body, referenced interface, or its heritage) or a
    /// template `<slot>` element? `false` only for rows arriving purely
    /// through the evaluated type-expansion channel with no authored
    /// counterpart — the residual channel VNode-transport keys could leak
    /// through. Consumed by `@verter/component-meta/published-surface`'s
    /// `Compat` / `Refined` slot blocklist (an author-declared slot is
    /// never blocked, whatever its name).
    pub declared_in_macro_type_arg: bool,
}

/// A single binding property on a scoped slot.
#[derive(Debug, Clone)]
pub struct SlotBindingAnalysis {
    pub name: String,
    /// The resolved binding type SOURCE POSITION (`Absent` = display text
    /// only; the typed binding channel is host-raised).
    pub publication: TypePublication,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
}

/// Analyzed model from `defineModel`.
#[derive(Debug, Clone)]
pub struct ModelAnalysis {
    pub name: String,
    /// The model value's resolved type SOURCE POSITION (`Absent` = untyped
    /// model).
    pub type_source: SourcePosition,
}

/// Analyzed exposed member from `defineExpose`.
#[derive(Debug, Clone)]
pub struct ExposedAnalysis {
    pub name: String,
    /// The exposed member's resolved type SOURCE POSITION (`Absent` =
    /// untyped binding).
    pub type_source: SourcePosition,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub description: Option<String>,
    /// JSDoc tags from the exposed member's leading `/** ... */` block.
    pub tags: Vec<JsdocTag>,
}

/// Host-populated public-instance sidecar exposed by the official API.
#[derive(Debug, Clone)]
pub struct PublicInstanceAnalysis {
    pub members: Vec<PublicInstanceMemberAnalysis>,
    pub completeness: PublicInstanceCompleteness,
}

/// A single runtime-observable public-instance member.
#[derive(Debug, Clone)]
pub struct PublicInstanceMemberAnalysis {
    pub name: String,
    pub kind: PublicInstanceMemberKind,
    /// The member's resolved type SOURCE POSITION (`Absent` = untyped
    /// member).
    pub type_source: SourcePosition,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub raw_type: Option<String>,
    pub description: Option<String>,
    /// JSDoc tags carried onto the public member from its source surface.
    pub tags: Vec<JsdocTag>,
}

/// Content-free schema-8 structure authority. Token arrays are indexed only
/// by the corresponding canonical local IDs; public identity is the token.
#[derive(Debug, Clone)]
pub struct OrderedSfcStructureAnalysis {
    pub schema_version: u32,
    pub artifact_token: String,
    pub inventory:
        std::sync::Arc<verter_language::parse_artifact::carrier_inventory::CarrierBlockInventory>,
    pub source_space_tokens: std::sync::Arc<[String]>,
    pub block_tokens: std::sync::Arc<[String]>,
    pub markup_node_tokens: std::sync::Arc<[String]>,
    pub attribute_tokens: std::sync::Arc<[String]>,
}

/// What kind of member this public-instance entry represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicInstanceMemberKind {
    Prop,
    SlotContainer,
    Exposed,
}

/// Whether the host believes the surfaced public-instance contract is complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicInstanceCompleteness {
    Exact,
    Partial,
}

/// A named resolved type available for schema expansion.
#[derive(Debug, Clone)]
pub struct ResolvedTypeAnalysis {
    pub name: String,
    /// The registry entry's resolved type SOURCE POSITION (registry entries
    /// always resolve a present source; the position type keeps the lane
    /// uniform with every other materialized output lane).
    pub type_source: SourcePosition,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
}

/// A component usage discovered in the template.
#[derive(Debug, Clone)]
pub struct ComponentUsageAnalysis {
    pub name: String,
    pub import_source: Option<String>,
    pub is_dynamic: bool,
    pub props: Vec<ComponentPropUsageAnalysis>,
    pub has_spread: bool,
    pub slots_used: Vec<String>,
    pub static_classes: Vec<String>,
    pub has_dynamic_class: bool,
    pub v_models: Vec<String>,
    pub v_model_entries: Vec<ComponentVModelUsageAnalysis>,
    /// Framework-neutral two-way bindings (the Svelte `bind:` family). Empty for
    /// Vue.
    pub bindings: Vec<ComponentBindingUsageAnalysis>,
    /// Framework-neutral events (the legacy Svelte `on:` directive only — a
    /// plain `on*` attribute is a prop, never an event). Empty for Vue.
    pub events: Vec<ComponentEventUsageAnalysis>,
}

/// A two-way binding passed to a child component (the Svelte `bind:` family).
#[derive(Debug, Clone)]
pub struct ComponentBindingUsageAnalysis {
    /// The bound local member name (`value` in `bind:value`).
    pub name: String,
    /// The `|modifier` list, in source order.
    pub modifiers: Vec<String>,
}

/// An event listened on a child component via the legacy Svelte `on:`
/// directive. A plain `on*` attribute is a prop, never an event (the
/// props/events split is syntactic — the child component-meta, not a name
/// guess, decides which passed props are callback events).
#[derive(Debug, Clone)]
pub struct ComponentEventUsageAnalysis {
    /// The event name — the legacy directive local (`click` from `on:click`).
    pub name: String,
    /// The handler expression text, when present.
    pub handler_expression: Option<String>,
    /// Whether the handler is an inline function expression.
    pub is_inline: bool,
    /// The `|modifier` list, in source order.
    pub modifiers: Vec<String>,
}

/// A single prop passed to a child component in the template.
#[derive(Debug, Clone)]
pub struct ComponentPropUsageAnalysis {
    pub name: String,
    pub is_bound: bool,
    pub constness: crate::analysis::template::PropValueConstness,
    pub expression: Option<String>,
    pub referenced_bindings: Vec<String>,
    pub from_spread: bool,
    pub is_shorthand: bool,
}

/// A v-model directive used on a child component in the template.
#[derive(Debug, Clone)]
pub struct ComponentVModelUsageAnalysis {
    pub binding_name: String,
}

/// A template ref usage.
#[derive(Debug, Clone)]
pub struct TemplateRefAnalysis {
    pub name: String,
    pub is_dynamic: bool,
    pub target_tag: String,
}

/// A script import.
#[derive(Debug, Clone)]
pub struct ImportAnalysis {
    pub source: String,
    pub is_type_only: bool,
    pub bindings: Vec<ImportBindingAnalysis>,
}

/// A single imported binding.
#[derive(Debug, Clone)]
pub struct ImportBindingAnalysis {
    pub name: String,
    pub kind: ImportBindingKind,
    pub imported_name: Option<String>,
    pub is_type_only: bool,
}

/// Declaration kind for a script binding in the component-meta result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKindAnalysis {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    Class,
}

/// A script-level binding.
#[derive(Debug, Clone)]
pub struct BindingAnalysis {
    pub name: String,
    pub kind: BindingKindAnalysis,
    pub reactivity_kind: crate::analysis::types::ReactivityKind,
    /// The EXACTNESS carrier for a composable binding's whole-return type.
    ///
    /// [`Self::reactivity_kind`] is a collapsed decoration vocabulary with no
    /// degraded arm, so it cannot distinguish "proven `Ref`" from "proven not a
    /// Vue wrapper" from "could not be resolved, and here is why". This field
    /// carries that distinction:
    ///
    /// - `None` — no whole-return role was demanded for this binding (it is not
    ///   a whole-value composable call, or the value-space walk already decided
    ///   it). NOT a claim about reactivity.
    /// - `Some(Ref | ShallowRef | ComputedRef | ModelRef | Reactive |
    ///   ShallowReactive)` — the exact package-backed `vue` wrapper family the
    ///   callee's authored return annotation resolves to.
    /// - `Some(ReactiveWrapperRole::None)` — a COMPLETED proof that the return
    ///   type is not a Vue wrapper. This is not a proof of non-reactivity:
    ///   `reactive()` returns `UnwrapNestedRefs<T>`, not `Reactive<T>`, so it
    ///   never downgrades [`Self::reactivity_kind`].
    /// - `Some(ReactiveWrapperRole::Unresolved { reason })` — a typed
    ///   degradation with its exact reason.
    pub return_wrapper_role: Option<verter_type_expr::ReactiveWrapperRole>,
    pub type_annotation: Option<String>,
    pub used_in_template: bool,
    pub used_in_style: bool,
}

/// A Vue API call site.
#[derive(Debug, Clone)]
pub struct VueApiCallAnalysis {
    pub api: crate::analysis::types::VueApiClassification,
    pub arg_value: Option<String>,
}

/// Analysis of a single style block.
#[derive(Debug, Clone)]
pub struct StyleAnalysis {
    pub lang: crate::analysis::style::StyleAnalysisLang,
    pub scoped: bool,
    pub is_module: bool,
    pub module_name: Option<String>,
    /// Sealed artifact-bound block identity carried through from the style
    /// analysis; the wire boundary revalidates it against the ordered
    /// structure before minting a public block token.
    pub block_ref: Option<verter_language::parse_artifact::carrier_inventory::ArtifactBlockRef>,
    pub classes: Vec<String>,
    pub ids: Vec<String>,
    pub custom_properties: Vec<String>,
    pub v_binds: Vec<String>,
    pub selectors: Vec<SelectorAnalysis>,
}

/// A CSS selector plus specificity.
#[derive(Debug, Clone)]
pub struct SelectorAnalysis {
    pub text: String,
    pub specificity: (u32, u32, u32),
}

/// Capability flags derived from script analysis.
#[derive(Debug, Clone, Default)]
pub struct ComponentMetaFlags {
    pub async_setup: bool,
    pub has_reactive_state: bool,
    pub has_computed: bool,
    pub has_watchers: bool,
    pub has_lifecycle_hooks: bool,
    pub has_provide: bool,
    pub has_inject: bool,
    pub has_inherit_attrs_false: bool,
    pub has_store_usage: bool,
    /// D123 — marks a macro-impacting lowering failure (a
    /// `verter_session::owned_artifacts::eval_program::LoweringError`).
    /// Currently always `false`: `extract_flags` emits the default and
    /// no production path flips it; consumers detect macro-impacting
    /// failures via the structured `macro_expansion_diagnostics`
    /// entries instead. Per D117, the public `getComponentMeta` API
    /// still returns `Option<ComponentMetaPayload>` (NOT `Result`);
    /// macro-impacting lowering failures surface as a populated
    /// payload, so NAPI does not throw exceptions.
    pub has_macro_failure: bool,
}

/// Classification of a component's template root structure for fallthrough
/// inheritance resolution. Extracted from `TemplateAnalysisSnapshot` facts only.
///
/// The host-owned resolver uses this to determine whether and how fallthrough
/// inheritance applies. Analysis extracts facts; the host owns all semantics.
#[derive(Debug, Clone, PartialEq)]
pub enum RootReachability {
    /// No fallthrough inheritance is possible.
    NoFallthrough { reason: NoFallthroughReason },
    /// One or more conditional branches, each with exactly one root target.
    /// A single-element vec means an unconditional single root.
    Branches { branches: Vec<RootBranch> },
}

/// Why a component has no fallthrough surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoFallthroughReason {
    /// `defineOptions({ inheritAttrs: false })` or Options API `inheritAttrs: false`.
    InheritAttrsFalse,
    /// Multiple unconditional root elements (fragment).
    MultiRoot,
    /// A conditional branch does not resolve to exactly one root target.
    BranchNotSingleRoot,
    /// Root element has `v-for` (produces multiple DOM nodes).
    RootVFor,
    /// No `<template>` block in the SFC.
    NoTemplate,
    /// `<template>` exists but has no children.
    EmptyTemplate,
    /// Root children are only text/interpolation nodes (no element root).
    TextOrInterpolationRoot,
}

/// A single root render branch in a component's template.
#[derive(Debug, Clone, PartialEq)]
pub struct RootBranch {
    /// Branch index in normalized source order (after transparent `<template v-if>` expansion).
    pub branch_index: u16,
    /// Condition text for diagnostics and UI display only. Serialized to FFI/JSON for
    /// debugging purposes but never used for identity, hashing, equality, or cache keys.
    /// Semantic identity comes from `branch_index` / `branch_key`, not condition text.
    pub condition_text: Option<String>,
    /// What the root target is.
    pub target: RootTargetRef,
    /// Attrs/listeners explicitly consumed on the root element.
    pub consumed: ConsumedRootBindings,
    /// Whether `v-bind="obj"` spread (without argument) is used on the root.
    pub has_unknown_spread: bool,
}

/// The kind of root render target.
#[derive(Debug, Clone, PartialEq)]
pub enum RootTargetRef {
    /// Native HTML element (e.g., `<div>`, `<input>`).
    NativeElement {
        /// Index into `TemplateAnalysisSnapshot.elements`.
        element_index: u32,
        /// Tag name (lowercase).
        tag: String,
    },
    /// Dynamic `<component :is>` root with a stable link to `TemplateComponentUsage`.
    DynamicComponentUsage {
        /// Index into `TemplateAnalysisSnapshot.elements`.
        element_index: u32,
        /// Index into `TemplateAnalysisSnapshot.components`.
        usage_index: u32,
    },
    /// Resolved component with a stable link to `TemplateComponentUsage`.
    ComponentUsage {
        /// Index into `TemplateAnalysisSnapshot.elements`.
        element_index: u32,
        /// Index into `TemplateAnalysisSnapshot.components`.
        usage_index: u32,
        /// PascalCase component name.
        name: String,
        /// Import source path for cross-file resolution.
        import_source: Option<String>,
    },
    /// Dynamic, slot, built-in, or otherwise unresolvable root target.
    UnresolvedTarget {
        /// Index into `TemplateAnalysisSnapshot.elements`.
        element_index: u32,
        /// Tag name as written.
        tag: String,
        /// Why this target cannot be resolved.
        reason: UnresolvedRootTargetReason,
    },
}

/// Why a root target cannot be resolved for fallthrough inheritance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnresolvedRootTargetReason {
    /// `<component :is="...">` — dynamic component.
    DynamicComponentIs,
    /// `<slot>` outlet as root.
    SlotOutlet,
    /// Vue built-in with special render behavior (Teleport, Transition, etc.).
    UnsupportedBuiltin { tag: String },
    /// Component element without a matching `TemplateComponentUsage` entry.
    MissingUsageLink,
    /// Component whose import source could not be resolved.
    UnresolvedImport,
    /// Catch-all for unrecognized root target patterns.
    UnknownRootTarget,
}

/// Attrs/listeners explicitly bound on the root element (consumed, not inherited).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConsumedRootBindings {
    /// Static attr names consumed on the root (e.g., `disabled`, `placeholder`).
    /// Does NOT include `class` or `style` (Vue always merges those).
    pub attrs: Vec<String>,
    /// Canonical listener names consumed on the root (e.g., `click` from `@click` or `:onClick`).
    pub listeners: Vec<String>,
    /// Whether a computed/dynamic attr name is bound (e.g., `:[expr]`).
    /// When true, the branch is a lower bound — some consumed attrs are unknown.
    pub has_dynamic_attr_name: bool,
    /// Whether a computed/dynamic listener name is bound (e.g., `@[expr]`, `v-on="obj"`,
    /// or spread with unknown keys). When true, the branch is a lower bound.
    pub has_dynamic_listener_name: bool,
}

/// Why generic-root specialization could not resolve a concrete instantiation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GenericResolutionFailure {
    SpreadInput,
    DynamicKey,
    MissingType,
    UnsupportedExpression,
    MissingUsageLink,
    UnresolvedChildGenericSurface,
}

/// Known lower-bound causes for a partially resolved fallthrough branch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PartialBranchReason {
    DynamicAttrName,
    DynamicListenerName,
    UnknownSpread,
    GenericResolution { failure: GenericResolutionFailure },
}

/// Why a fallthrough branch could not be resolved at all.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnresolvedBranchReason {
    Cycle { canonical_id: String },
    DynamicComponentIs,
    ChildResolutionFailed,
    UnresolvedChildImport { import_source: Option<String> },
    RootTarget { reason: UnresolvedRootTargetReason },
    GenericResolution { failure: GenericResolutionFailure },
}

/// How a member arrived on the accepted surface.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberProvenance {
    /// Member is declared locally (defineProps / defineEmits / Options API).
    Declared,
    /// Member is inherited from one or more fallthrough sources.
    Inherited { sources: Vec<InheritedSource> },
}

/// A single inheritance source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum InheritedSource {
    /// Inherited from a native HTML element.
    NativeTag { tag: String },
    /// Inherited from a child component.
    Component { canonical_id: String },
}

/// Whether a member is always available or only in certain branches.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberAvailability {
    /// Available in all branches (unconditional single-root or all branches).
    Always,
    /// Available only in specific branches.
    Conditional { branch_keys: Vec<String> },
}

/// Kind of accepted prop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedPropKind {
    /// Locally declared prop.
    DeclaredProp,
    /// Inherited HTML attribute.
    Attr,
}

/// Kind of accepted event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedEventKind {
    /// Locally declared emit.
    DeclaredEmit,
    /// Inherited native listener.
    Listener,
}

/// Whether the accepted surface is exact or only a lower bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedSurfaceCompleteness {
    /// All accepted members are known exactly.
    Exact,
    /// Some members may be missing due to unresolved or partial branches.
    LowerBound,
}

/// An accepted prop on the computed call-site surface.
#[derive(Debug, Clone)]
pub struct AcceptedPropAnalysis {
    pub name: String,
    /// Typed callable role preserved through accepted-surface inheritance.
    pub callable_role: verter_type_expr::PropCallableRole,
    /// The accepted prop's resolved type SOURCE POSITION (`Absent` =
    /// untyped / cross-branch divergent).
    pub publication: TypePublication,
    /// The canonical file scope `type_source`'s SCOPE-RELATIVE names (bare
    /// `Ref` leaf spellings, producer-local anchors at any nesting depth)
    /// resolve under — the PRODUCING owner of an inherited source, carried
    /// positionally per the cross-owner effective-scope invariant. `None` =
    /// the analysis owner itself (own/declared rows, intrinsic attr rows).
    pub type_source_scope: Option<String>,
    pub required: bool,
    pub provenance: MemberProvenance,
    pub availability: MemberAvailability,
    pub kind: AcceptedPropKind,
}

/// An accepted event on the computed call-site surface.
#[derive(Debug, Clone)]
pub struct AcceptedEventAnalysis {
    pub name: String,
    /// The accepted event's resolved payload SOURCE POSITION (`Absent` =
    /// untyped / cross-branch divergent).
    pub payload: SourcePosition,
    /// The canonical file scope `payload`'s SCOPE-RELATIVE names resolve
    /// under — the PRODUCING owner of an inherited source, carried
    /// positionally (see [`AcceptedPropAnalysis::type_source_scope`]).
    /// `None` = the analysis owner itself.
    pub payload_scope: Option<String>,
    pub raw_signature: Option<String>,
    pub provenance: MemberProvenance,
    pub availability: MemberAvailability,
    pub kind: AcceptedEventKind,
}

/// The branch-structured inherited surface. Declared members do NOT appear here.
#[derive(Debug, Clone, PartialEq)]
pub enum FallthroughSurface {
    /// No fallthrough inheritance.
    None { reason: NoFallthroughReason },
    /// Branch-structured inherited props and events.
    Branches { branches: Vec<FallthroughBranch> },
}

/// An inherited prop entry in a fallthrough branch.
#[derive(Debug, Clone, PartialEq)]
pub struct FallthroughPropEntry {
    pub name: String,
    /// Typed callable role inherited from the producing prop.
    pub callable_role: verter_type_expr::PropCallableRole,
    /// The inherited prop's resolved type SOURCE POSITION (`Absent` =
    /// untyped).
    pub publication: TypePublication,
    /// The canonical file scope `type_source`'s SCOPE-RELATIVE names resolve
    /// under — the PRODUCING owner (the terminal origin of a multi-hop
    /// inheritance chain), carried positionally per the cross-owner
    /// effective-scope invariant. `None` = the intrinsic/native case with no
    /// producing file (the branch owner's scope applies).
    pub type_source_scope: Option<String>,
    pub sources: Vec<InheritedSource>,
}

/// An inherited event entry in a fallthrough branch.
#[derive(Debug, Clone, PartialEq)]
pub struct FallthroughEventEntry {
    pub name: String,
    /// The inherited event's resolved payload SOURCE POSITION (`Absent` =
    /// untyped).
    pub payload: SourcePosition,
    /// The canonical file scope `payload`'s SCOPE-RELATIVE names resolve
    /// under — the PRODUCING owner (the terminal origin of a multi-hop
    /// inheritance chain), carried positionally (see
    /// [`FallthroughPropEntry::type_source_scope`]).
    pub payload_scope: Option<String>,
    pub raw_signature: Option<String>,
    pub sources: Vec<InheritedSource>,
}

/// Status of a fallthrough branch.
#[derive(Debug, Clone, PartialEq)]
pub enum BranchStatus {
    /// All members in this branch are exactly known.
    Resolved,
    /// Some members are known but the branch may have additional unknown members.
    PartiallyUnresolved { reasons: Vec<PartialBranchReason> },
    /// This branch could not be resolved at all.
    Unresolved { reason: UnresolvedBranchReason },
}

/// A single step in the root resolution chain.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedRootStep {
    /// Native HTML element target.
    NativeTag { tag: String },
    /// Resolved child component target.
    Component {
        canonical_id: String,
        component_name: String,
    },
    /// Unresolved root target.
    Unresolved {
        tag: String,
        reason: UnresolvedBranchReason,
    },
}

/// A single branch in the fallthrough surface.
#[derive(Debug, Clone, PartialEq)]
pub struct FallthroughBranch {
    /// Deterministic branch key (e.g., "0", "0.1", "2.0.3").
    pub branch_key: String,
    /// Condition text for diagnostics only.
    pub condition_text: Option<String>,
    /// Inherited props in this branch (after subtraction).
    pub props: Vec<FallthroughPropEntry>,
    /// Inherited events in this branch (after subtraction).
    pub events: Vec<FallthroughEventEntry>,
    /// Chain of root steps traversed to produce this branch.
    pub root_chain: Vec<ResolvedRootStep>,
    /// Resolution status of this branch.
    pub status: BranchStatus,
}
