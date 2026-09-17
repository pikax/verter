//! The closed non-flow operation vocabulary and its payload contracts.
//!
//! [`NonFlowOperation`] is the sealed request vocabulary of the C2
//! gateway. It is closed (no `#[non_exhaustive]`), and every match over
//! it — inside this crate or out — must enumerate all six variants
//! without a `_` arm: the missing-input proof table below is the
//! exhaustive contract, and a wildcard arm would silently strand a route
//! outside it. The proof ids are stable identifiers carried in test and
//! route evidence; they are not diagnostics and never render to users.

use std::sync::Arc;

use verter_macro_dto::{
    AuthoredMemberOrdinal, MacroAnchor, ModelRuntimeShape, PropsDefaultsAssociation, RuntimeEmit,
};
use verter_span::Span;
use verter_type_expr::TopLevelOwnerId;

use crate::analysis::{AnalyzedMacro, AnalyzedMacroKind};
use crate::resolver_core::{InputKey, LoadSet, ResolutionBasis};
use verter_type_expr::DeclBindingKey;

/// Missing-input proof id for [`NonFlowOperation::ProjectVueMacroSemantics`].
pub const MISSING_PROOF_VUE_MACRO: &str = "C2-GAP3-MISSING-VUE-MACRO";
/// Missing-input proof id for
/// [`NonFlowOperation::ResolveImportedComponentSurface`].
pub const MISSING_PROOF_IMPORTED_COMPONENT: &str = "C2-GAP3-MISSING-IMPORTED-COMPONENT";
/// Missing-input proof id for [`NonFlowOperation::ProjectRuntimeProps`].
pub const MISSING_PROOF_PROPS: &str = "C2-GAP3-MISSING-PROPS";
/// Missing-input proof id for [`NonFlowOperation::ProjectRuntimeEmits`].
pub const MISSING_PROOF_EMITS: &str = "C2-GAP3-MISSING-EMITS";
/// Missing-input proof id for [`NonFlowOperation::ProjectRuntimeModel`].
pub const MISSING_PROOF_MODEL: &str = "C2-GAP3-MISSING-MODEL";
/// Missing-input proof id for [`NonFlowOperation::ProjectExposeSurface`].
pub const MISSING_PROOF_EXPOSE: &str = "C2-GAP3-MISSING-EXPOSE";

/// One non-flow semantic operation, request-bound: every variant carries
/// the canonical owner it asks about plus the per-operation request axes
/// that pin its result identity (authored macro position, or the authored
/// type reference and the declaration it resolved to), so no two
/// distinct requests can alias one answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonFlowOperation {
    /// Derive one SFC's complete macro semantic input plan: which authored
    /// macros carry codegen demands, in authored order, with their lane,
    /// index derivations, and surface dependency failures.
    ProjectVueMacroSemantics { owner_canonical: Arc<str> },
    /// Resolve one authored type reference against the owner's observed
    /// import surface (bare-name retention and cross-file import lookup).
    /// `referenced_canonical` is the declaration the reference's resolved
    /// carrier names; `None` while the caller has not resolved one.
    ResolveImportedComponentSurface {
        owner_canonical: Arc<str>,
        type_reference: Arc<str>,
        referenced_canonical: Option<Arc<str>>,
    },
    /// Shape one `defineProps` macro's runtime prop rows over the observed
    /// member surface.
    ProjectRuntimeProps {
        owner_canonical: Arc<str>,
        macro_index: usize,
    },
    /// Shape one `defineEmits` macro's runtime emit rows over the observed
    /// event-name surface, in authored order.
    ProjectRuntimeEmits {
        owner_canonical: Arc<str>,
        macro_index: usize,
    },
    /// Synthesize one `defineModel` macro's runtime model shape from the
    /// macro row and the observed value classification.
    ProjectRuntimeModel {
        owner_canonical: Arc<str>,
        macro_index: usize,
    },
    /// Shape one runtime-object `defineExpose` macro's exposed member
    /// rows, anchored by authored source order.
    ProjectExposeSurface {
        owner_canonical: Arc<str>,
        macro_index: usize,
    },
}

impl NonFlowOperation {
    /// The stable missing-input proof id this operation's
    /// `NeedInputs` outcome is evidenced by.
    ///
    /// Exhaustive by construction: adding a variant without extending the
    /// proof table is a compile error, and this match carries no `_` arm.
    pub const fn missing_input_proof_id(&self) -> &'static str {
        match self {
            Self::ProjectVueMacroSemantics { .. } => MISSING_PROOF_VUE_MACRO,
            Self::ResolveImportedComponentSurface { .. } => MISSING_PROOF_IMPORTED_COMPONENT,
            Self::ProjectRuntimeProps { .. } => MISSING_PROOF_PROPS,
            Self::ProjectRuntimeEmits { .. } => MISSING_PROOF_EMITS,
            Self::ProjectRuntimeModel { .. } => MISSING_PROOF_MODEL,
            Self::ProjectExposeSurface { .. } => MISSING_PROOF_EXPOSE,
        }
    }

    /// The single derivation of the load set this operation reports when
    /// its inputs are missing: the authored owner's content plus, for
    /// per-macro operations, the demanded macro row's declaration body.
    pub fn missing_input_load_set(&self, basis: ResolutionBasis) -> LoadSet {
        let macro_row = match self {
            Self::ProjectVueMacroSemantics { .. }
            | Self::ResolveImportedComponentSurface { .. } => None,
            Self::ProjectRuntimeProps {
                owner_canonical,
                macro_index,
            }
            | Self::ProjectRuntimeEmits {
                owner_canonical,
                macro_index,
            }
            | Self::ProjectRuntimeModel {
                owner_canonical,
                macro_index,
            }
            | Self::ProjectExposeSurface {
                owner_canonical,
                macro_index,
            } => Some((Arc::clone(owner_canonical), *macro_index)),
        };
        let mut keys = vec![InputKey::FileContent {
            canonical: Arc::clone(self.owner_canonical()),
        }];
        if let Some((canonical, macro_index)) = macro_row {
            keys.push(InputKey::DeclBody {
                canonical,
                owner: TopLevelOwnerId::default(),
                name: Arc::from(format!("__vue_macro_{macro_index}")),
                space: crate::resolver_core::DeclarationSpace::Type,
            });
        }
        LoadSet::new(keys, basis)
    }

    /// The canonical owner every observation this operation reads is
    /// keyed by — the operation/request binding the driver revalidates.
    pub fn owner_canonical(&self) -> &Arc<str> {
        match self {
            Self::ProjectVueMacroSemantics { owner_canonical }
            | Self::ResolveImportedComponentSurface {
                owner_canonical, ..
            }
            | Self::ProjectRuntimeProps {
                owner_canonical, ..
            }
            | Self::ProjectRuntimeEmits {
                owner_canonical, ..
            }
            | Self::ProjectRuntimeModel {
                owner_canonical, ..
            }
            | Self::ProjectExposeSurface {
                owner_canonical, ..
            } => owner_canonical,
        }
    }
}

/// Which lane one authored macro's semantic demand serves — derived once,
/// here, so route policy cannot disagree with inventory structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroSemanticLane {
    /// Runtime-object `defineExpose({ ... })`: a dedicated TSC-only lane
    /// with no macro type argument and no runtime shape.
    ExposeRuntimeObject,
    /// A codegen macro (`defineProps` / `defineEmits` / `defineModel`)
    /// with an authored type argument: runtime and TSC demands both read
    /// its resolved payload.
    CodegenPayload,
}

/// One macro row of a [`VueMacroSemanticInput`] plan.
///
/// Fields are private: the plan is minted only by
/// [`TypeInfoCore::attempt`](super::core::TypeInfoCore::attempt), and its
/// identity (indices, lane, failures) is the kernel's decision, not the
/// caller's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueMacroSemanticDemand {
    pub(super) macro_index: usize,
    pub(super) effective_index: usize,
    pub(super) syntax_index: u32,
    pub(super) kind: AnalyzedMacroKind,
    pub(super) lane: MacroSemanticLane,
    pub(super) has_type_argument: bool,
    pub(super) binding_name: Option<String>,
    pub(super) model_name: Option<String>,
    pub(super) defaults_macro_index: Option<u32>,
    pub(super) surface_dependency_failures: Vec<VueMacroMissingRoot>,
}

impl VueMacroSemanticDemand {
    /// Authored position of the macro in the analysis snapshot.
    pub const fn macro_index(&self) -> usize {
        self.macro_index
    }
    /// The macro whose declaration site names this demand for syntax
    /// purposes (`withDefaults` wraps its inner `defineProps`).
    pub const fn effective_index(&self) -> usize {
        self.effective_index
    }
    /// Top-level syntax position among the file's top-level macros.
    pub const fn syntax_index(&self) -> u32 {
        self.syntax_index
    }
    /// The authored macro kind.
    pub const fn kind(&self) -> AnalyzedMacroKind {
        self.kind
    }
    /// Which lane serves this demand.
    pub const fn lane(&self) -> MacroSemanticLane {
        self.lane
    }
    /// Whether the macro carries an authored type argument to resolve a
    /// payload from.
    pub const fn has_type_argument(&self) -> bool {
        self.has_type_argument
    }
    /// The runtime binding the macro declares, when it declares one.
    pub fn binding_name(&self) -> Option<&str> {
        self.binding_name.as_deref()
    }
    /// The model name a `defineModel` macro declares.
    pub fn model_name(&self) -> Option<&str> {
        self.model_name.as_deref()
    }
    /// The `withDefaults` macro index wrapping this `defineProps`, when
    /// one does.
    pub const fn defaults_macro_index(&self) -> Option<u32> {
        self.defaults_macro_index
    }
    /// Authored surface-tier dependency failures bound to this macro —
    /// one row per unresolvable surface reference the analysis recorded.
    pub fn surface_dependency_failures(&self) -> &[VueMacroMissingRoot] {
        &self.surface_dependency_failures
    }
}

/// One surface-tier dependency the analysis recorded as unresolvable for
/// a macro — the missing-root evidence a payload-less macro reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueMacroMissingRoot {
    pub(super) macro_index: usize,
    pub(super) import_source: String,
    pub(super) type_name: String,
    pub(super) macro_span: (u32, u32),
}

impl VueMacroMissingRoot {
    /// Authored position of the macro that referenced the missing root.
    pub const fn macro_index(&self) -> usize {
        self.macro_index
    }
    /// The import source the missing root was referenced through.
    pub fn import_source(&self) -> &str {
        &self.import_source
    }
    /// The referenced type name.
    pub fn type_name(&self) -> &str {
        &self.type_name
    }
    /// The referencing macro's byte span.
    pub const fn macro_span(&self) -> (u32, u32) {
        self.macro_span
    }
}

/// The complete macro semantic input plan for one SFC — the payload of
/// [`NonFlowOperation::ProjectVueMacroSemantics`]. Rows appear in
/// authored macro order; skipped macros (non-codegen, `withDefaults`
/// wrappers, non-type-based forms) contribute no row, exactly as the
/// lane policy in [`MacroSemanticLane`] defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueMacroSemanticInput {
    pub(super) owner_canonical: Arc<str>,
    pub(super) demands: Vec<VueMacroSemanticDemand>,
}

impl VueMacroSemanticInput {
    /// The owner the plan was derived for.
    pub fn owner_canonical(&self) -> &str {
        &self.owner_canonical
    }
    /// The macro demands, in authored macro order.
    pub fn demands(&self) -> &[VueMacroSemanticDemand] {
        &self.demands
    }
}

/// The resolved import surface of one authored type reference — the
/// payload of [`NonFlowOperation::ResolveImportedComponentSurface`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedComponentSurface {
    pub(super) owner_canonical: Arc<str>,
    pub(super) type_reference: Arc<str>,
    pub(super) bare_name_is_imported: bool,
    pub(super) import_specifier: Option<String>,
}

impl ImportedComponentSurface {
    /// The owner whose import surface was consulted.
    pub fn owner_canonical(&self) -> &str {
        &self.owner_canonical
    }
    /// The authored reference that was resolved.
    pub fn type_reference(&self) -> &str {
        &self.type_reference
    }
    /// Whether the reference's bare name is a directly-imported binding —
    /// such names are retained by scope requirements and never take the
    /// cross-file qualified form.
    pub const fn bare_name_is_imported(&self) -> bool {
        self.bare_name_is_imported
    }
    /// The authored specifier of the import whose resolution reaches the
    /// referenced declaration, when the reference crosses files.
    pub fn import_specifier(&self) -> Option<&str> {
        self.import_specifier.as_deref()
    }
    /// The sibling-testing surface's qualified name
    /// (`import("specifier").Name`) for a cross-file reference; `None`
    /// for a local or directly-imported one.
    pub fn qualified_testing_name(&self) -> Option<String> {
        self.import_specifier
            .as_ref()
            .map(|specifier| format!("import(\"{specifier}\").{}", self.type_reference))
    }
}

/// One shaped runtime prop row — the payload rows of
/// [`NonFlowOperation::ProjectRuntimeProps`]. The live constructor
/// classification (`type_shape`) is an I/O fact the driver supplies when
/// it renders the final DTO; identity, order, optionality, anchoring, and
/// the member-dependency tier are decided here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedRuntimePropRow {
    pub(super) name: String,
    pub(super) optional: bool,
    pub(super) anchor: MacroAnchor,
    pub(super) member_dependency: bool,
    pub(super) referenced_type_name: Option<String>,
}

impl ProjectedRuntimePropRow {
    /// The prop's published runtime name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Whether the prop is optional.
    pub const fn optional(&self) -> bool {
        self.optional
    }
    /// The row's identity anchor (authored member ordinal, or the macro
    /// argument when the member is not authored).
    pub const fn anchor(&self) -> MacroAnchor {
        self.anchor
    }
    /// Whether this member's type annotation names a member-tier
    /// dependency — the tier that degrades one member instead of
    /// faulting the macro.
    pub const fn member_dependency(&self) -> bool {
        self.member_dependency
    }
    /// The member's referenced type name, when its annotation names one.
    pub fn referenced_type_name(&self) -> Option<&str> {
        self.referenced_type_name.as_deref()
    }
}

/// The shaped runtime props projection of one `defineProps` macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePropsProjection {
    pub(super) owner_canonical: Arc<str>,
    pub(super) macro_index: usize,
    pub(super) rows: Vec<ProjectedRuntimePropRow>,
    pub(super) defaults_association: PropsDefaultsAssociation,
    pub(super) member_dependency_names: Vec<String>,
}

impl RuntimePropsProjection {
    /// The owner the projection was derived for.
    pub fn owner_canonical(&self) -> &str {
        &self.owner_canonical
    }
    /// The authored macro position the projection shapes.
    pub const fn macro_index(&self) -> usize {
        self.macro_index
    }
    /// The shaped rows, in observed surface order.
    pub fn rows(&self) -> &[ProjectedRuntimePropRow] {
        &self.rows
    }
    /// The `withDefaults` association this macro carries.
    pub const fn defaults_association(&self) -> PropsDefaultsAssociation {
        self.defaults_association
    }
    /// The macro's member-tier dependency names, sorted and deduplicated —
    /// the degradation tier that degrades one member instead of faulting
    /// the macro, carried for the driver's live missing-dependency walk.
    pub fn member_dependency_names(&self) -> &[String] {
        &self.member_dependency_names
    }
}

/// The shaped runtime emits projection of one `defineEmits` macro —
/// deduplicated by name on first admission, ordered by authored emit
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEmitsProjection {
    pub(super) owner_canonical: Arc<str>,
    pub(super) macro_index: usize,
    pub(super) emits: Vec<RuntimeEmit>,
}

impl RuntimeEmitsProjection {
    /// The owner the projection was derived for.
    pub fn owner_canonical(&self) -> &str {
        &self.owner_canonical
    }
    /// The authored macro position the projection shapes.
    pub const fn macro_index(&self) -> usize {
        self.macro_index
    }
    /// The shaped emit rows, in authored order.
    pub fn emits(&self) -> &[RuntimeEmit] {
        &self.emits
    }
}

/// The synthesized runtime model shape of one `defineModel` macro — the
/// payload of [`NonFlowOperation::ProjectRuntimeModel`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeModelProjection {
    pub(super) owner_canonical: Arc<str>,
    pub(super) macro_index: usize,
    pub(super) shape: ModelRuntimeShape,
}

impl RuntimeModelProjection {
    /// The owner the projection was derived for.
    pub fn owner_canonical(&self) -> &str {
        &self.owner_canonical
    }
    /// The authored macro position the projection shapes.
    pub const fn macro_index(&self) -> usize {
        self.macro_index
    }
    /// The synthesized model shape (prop row, update event, modifiers
    /// prop) with its identity anchors.
    pub const fn shape(&self) -> &ModelRuntimeShape {
        &self.shape
    }
}

/// One shaped exposed member row of a runtime-object `defineExpose`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedExposeRow {
    pub(super) name: String,
    pub(super) referenced_binding: Option<DeclBindingKey>,
    pub(super) anchor: MacroAnchor,
}

impl ProjectedExposeRow {
    /// The exposed member's field name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// The resolved `(owner, name)` key of the local value binding the
    /// member's value expression names, when it names one.
    pub fn referenced_binding(&self) -> Option<&DeclBindingKey> {
        self.referenced_binding.as_ref()
    }
    /// The row's identity anchor, keyed by authored source-order position
    /// among the macro's own expose fields.
    pub const fn anchor(&self) -> MacroAnchor {
        self.anchor
    }
}

/// The shaped expose projection of one runtime-object `defineExpose`
/// macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposeSurfaceProjection {
    pub(super) owner_canonical: Arc<str>,
    pub(super) macro_index: usize,
    pub(super) rows: Vec<ProjectedExposeRow>,
}

impl ExposeSurfaceProjection {
    /// The owner the projection was derived for.
    pub fn owner_canonical(&self) -> &str {
        &self.owner_canonical
    }
    /// The authored macro position the projection shapes.
    pub const fn macro_index(&self) -> usize {
        self.macro_index
    }
    /// The shaped rows, in authored field order.
    pub fn rows(&self) -> &[ProjectedExposeRow] {
        &self.rows
    }
}

/// The payload of one completed [`NonFlowOperation`] — one variant per
/// operation, exhaustive and closed like the operation vocabulary itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonFlowPayload {
    /// Completed [`NonFlowOperation::ProjectVueMacroSemantics`].
    VueMacroSemanticInput(VueMacroSemanticInput),
    /// Completed [`NonFlowOperation::ResolveImportedComponentSurface`].
    ImportedComponentSurface(ImportedComponentSurface),
    /// Completed [`NonFlowOperation::ProjectRuntimeProps`].
    RuntimePropsProjection(RuntimePropsProjection),
    /// Completed [`NonFlowOperation::ProjectRuntimeEmits`].
    RuntimeEmitsProjection(RuntimeEmitsProjection),
    /// Completed [`NonFlowOperation::ProjectRuntimeModel`].
    RuntimeModelProjection(RuntimeModelProjection),
    /// Completed [`NonFlowOperation::ProjectExposeSurface`].
    ExposeSurfaceProjection(ExposeSurfaceProjection),
}

// ── Authored-order anchor derivations (the kernel's identity policy) ──

/// Anchor for one `defineProps` member, keyed by its authored
/// source-order position among the macro's own prop fields.
pub(super) fn prop_member_anchor(
    mac: &AnalyzedMacro,
    payload_index: usize,
    name: &str,
) -> MacroAnchor {
    let Some(ordinal) = mac
        .prop_fields
        .iter()
        .filter(|field| span_is_owned_by_macro(field.span, mac.span))
        .position(|field| field.name == name)
    else {
        return MacroAnchor::MacroArgument {
            macro_index: macro_index(payload_index),
        };
    };
    MacroAnchor::Authored {
        macro_index: macro_index(payload_index),
        member_ordinal: AuthoredMemberOrdinal::new(member_ordinal(ordinal)),
    }
}

/// Anchor for one `defineEmits` event, keyed by its authored source-order
/// position among the macro's own emit fields.
pub(super) fn emit_member_anchor(
    mac: &AnalyzedMacro,
    payload_index: usize,
    effective_index: usize,
    name: &str,
) -> MacroAnchor {
    let Some(ordinal) = mac
        .emit_fields
        .iter()
        .filter(|field| span_is_owned_by_macro(field.span, mac.span))
        .position(|field| field.name == name)
    else {
        return MacroAnchor::MacroArgument {
            macro_index: macro_index(payload_index),
        };
    };
    MacroAnchor::Authored {
        macro_index: macro_index(effective_index),
        member_ordinal: AuthoredMemberOrdinal::new(member_ordinal(ordinal)),
    }
}

/// Anchor for one runtime-object `defineExpose` member, keyed by its
/// authored source-order position among the macro's own expose fields —
/// by POSITION, not name, because two authored members may legally share
/// a name.
pub(super) fn expose_member_anchor(
    mac: &AnalyzedMacro,
    payload_index: usize,
    field_index: usize,
) -> MacroAnchor {
    let is_owned = mac.expose_fields[field_index]
        .span
        .is_some_and(|span| span_is_owned_by_macro(span, mac.span));
    if !is_owned {
        return MacroAnchor::MacroArgument {
            macro_index: macro_index(payload_index),
        };
    }
    let ordinal = mac.expose_fields[..=field_index]
        .iter()
        .filter(|field| {
            field
                .span
                .is_some_and(|span| span_is_owned_by_macro(span, mac.span))
        })
        .count()
        - 1;
    MacroAnchor::Authored {
        macro_index: macro_index(payload_index),
        member_ordinal: AuthoredMemberOrdinal::new(member_ordinal(ordinal)),
    }
}

/// Sort key reproducing authored emit order: authored members before
/// synthesized ones, by ordinal.
pub(super) fn authored_emit_order(anchor: MacroAnchor) -> (u8, u32) {
    match anchor {
        MacroAnchor::Authored { member_ordinal, .. } => (0, member_ordinal.get()),
        MacroAnchor::MacroArgument { .. } | MacroAnchor::Synthesized { .. } => (1, 0),
    }
}

pub(super) fn span_is_owned_by_macro(member: Span, mac: Span) -> bool {
    member.start >= mac.start && member.end <= mac.end
}

pub(super) fn macro_index(index: usize) -> u32 {
    u32::try_from(index).expect("Vue macro inventory exceeds the DTO identity space")
}

fn member_ordinal(index: usize) -> u32 {
    u32::try_from(index).expect("Vue macro member inventory exceeds the DTO identity space")
}

/// Whether a macro kind renders runtime option objects (the codegen
/// filter the inventory derives lane policy from).
pub(super) fn is_codegen_macro(kind: AnalyzedMacroKind) -> bool {
    matches!(
        kind,
        AnalyzedMacroKind::DefineProps
            | AnalyzedMacroKind::DefineEmits
            | AnalyzedMacroKind::DefineModel
    )
}

/// The closest enclosing `withDefaults` macro index for a `defineProps`
/// payload, when one encloses it.
pub(super) fn containing_with_defaults_index(
    macros: &[AnalyzedMacro],
    inner_index: usize,
) -> Option<usize> {
    let inner = &macros[inner_index];
    macros
        .iter()
        .enumerate()
        .filter(|(_, outer)| outer.kind == AnalyzedMacroKind::WithDefaults)
        .filter(|(_, outer)| outer.span.start < inner.span.start && inner.span.end < outer.span.end)
        .min_by_key(|(_, outer)| outer.span.end.saturating_sub(outer.span.start))
        .map(|(index, _)| index)
}

/// Top-level syntax position among the file's top-level macros.
pub(super) fn top_level_syntax_index(macros: &[AnalyzedMacro], effective_index: usize) -> u32 {
    let effective = &macros[effective_index];
    let preceding = macros
        .iter()
        .enumerate()
        .filter(|(index, _)| is_top_level_macro(macros, *index))
        .filter(|(index, mac)| {
            (mac.span.start, mac.span.end, *index)
                < (effective.span.start, effective.span.end, effective_index)
        })
        .count();
    u32::try_from(preceding).unwrap_or(u32::MAX)
}

fn is_top_level_macro(macros: &[AnalyzedMacro], candidate_index: usize) -> bool {
    let candidate = &macros[candidate_index];
    !macros.iter().enumerate().any(|(index, outer)| {
        index != candidate_index
            && outer.span.start < candidate.span.start
            && candidate.span.end < outer.span.end
    })
}
