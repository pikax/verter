//! Small enum-to-string and per-field conversion helpers shared between the
//! component-meta, fallthrough, input, and output modules.

use verter_session as host;

use crate::types::*;

pub(super) fn macro_expansion_kind_to_string(
    kind: verter_semantic::analysis::component_meta::MacroExpansionKind,
) -> String {
    match kind {
        verter_semantic::analysis::component_meta::MacroExpansionKind::DefineProps => {
            "defineProps".to_string()
        }
        verter_semantic::analysis::component_meta::MacroExpansionKind::DefineEmits => {
            "defineEmits".to_string()
        }
        verter_semantic::analysis::component_meta::MacroExpansionKind::DefineSlots => {
            "defineSlots".to_string()
        }
    }
}
pub(super) fn jsdoc_to_ffi(tag: verter_semantic::analysis::types::JsdocTag) -> FfiJsdocTag {
    FfiJsdocTag {
        name: tag.name,
        text: tag.text,
    }
}
pub(super) fn expansion_metadata_to_ffi(
    metadata: verter_semantic::analysis::type_expand::ExpansionMetadata,
) -> FfiExpansionMetadata {
    FfiExpansionMetadata {
        exactness: expansion_exactness_to_string(metadata.exactness),
        execution_status: expansion_execution_status_to_string(metadata.execution_status),
        diagnostics: metadata
            .diagnostics
            .into_iter()
            .map(|diagnostic| FfiExpansionDiagnostic {
                reason: expansion_stop_reason_to_string(diagnostic.reason),
                context: diagnostic.context,
                property_name: diagnostic.property_name,
            })
            .collect(),
    }
}
pub(super) fn expansion_exactness_to_string(
    exactness: verter_semantic::analysis::type_expand::ExpansionExactness,
) -> String {
    match exactness {
        verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete => {
            "exactConcrete".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic => {
            "exactSymbolic".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExactness::Incomplete => {
            "incomplete".to_string()
        }
    }
}

pub(super) fn expansion_execution_status_to_string(
    status: verter_semantic::analysis::type_expand::ExpansionExecutionStatus,
) -> String {
    match status {
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed => {
            "completed".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Cancelled => {
            "cancelled".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Interrupted => {
            "interrupted".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::HardStop => {
            "hardStop".to_string()
        }
    }
}

pub(super) fn expansion_stop_reason_to_string(
    reason: verter_semantic::analysis::type_expand::ExpansionStopReason,
) -> String {
    match reason {
        verter_semantic::analysis::type_expand::ExpansionStopReason::BudgetExceeded => {
            "budgetExceeded".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::MappedDepthExceeded => {
            "mappedDepthExceeded".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::UnresolvedReference => {
            "unresolvedReference".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::IndeterminateConditional => {
            "indeterminateConditional".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::InfiniteKeySpace => {
            "infiniteKeySpace".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::UnsupportedOperator => {
            "unsupportedOperator".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::ConditionalContextTruncated => {
            "conditionalContextTruncated".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::IdempotentArm => {
            "idempotentArm".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::CyclicReference => {
            "cyclicReference".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::CyclicInstantiation => {
            "cyclicInstantiation".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::InstantiationError => {
            "instantiationError".to_string()
        }
        verter_semantic::analysis::type_expand::ExpansionStopReason::EmptyUnionArm => {
            "emptyUnionArm".to_string()
        }
    }
}
pub(super) fn accepted_prop_to_ffi(
    prop: verter_semantic::analysis::component_meta::AcceptedPropAnalysis,
    r#type: verter_type_expr::TypeExpr,
) -> FfiAcceptedPropMeta {
    FfiAcceptedPropMeta {
        name: prop.name,
        r#type,
        raw_type: prop.raw_type,
        required: prop.required,
        provenance: member_provenance_to_ffi(prop.provenance),
        availability: member_availability_to_ffi(prop.availability),
        kind: accepted_prop_kind_to_ffi(prop.kind),
    }
}

pub(super) fn accepted_event_to_ffi(
    event: verter_semantic::analysis::component_meta::AcceptedEventAnalysis,
    payload: verter_type_expr::TypeExpr,
) -> FfiAcceptedEventMeta {
    FfiAcceptedEventMeta {
        name: event.name,
        payload,
        raw_signature: event.raw_signature,
        provenance: member_provenance_to_ffi(event.provenance),
        availability: member_availability_to_ffi(event.availability),
        kind: accepted_event_kind_to_ffi(event.kind),
    }
}

pub(super) fn member_provenance_to_ffi(
    provenance: verter_semantic::analysis::component_meta::MemberProvenance,
) -> FfiMemberProvenance {
    match provenance {
        verter_semantic::analysis::component_meta::MemberProvenance::Declared => {
            FfiMemberProvenance::Declared
        }
        verter_semantic::analysis::component_meta::MemberProvenance::Inherited { sources } => {
            FfiMemberProvenance::Inherited {
                sources: sources.into_iter().map(inherited_source_to_ffi).collect(),
            }
        }
    }
}

pub(super) fn inherited_source_to_ffi(
    source: verter_semantic::analysis::component_meta::InheritedSource,
) -> FfiInheritedSource {
    match source {
        verter_semantic::analysis::component_meta::InheritedSource::NativeTag { tag } => {
            FfiInheritedSource::NativeTag { tag }
        }
        verter_semantic::analysis::component_meta::InheritedSource::Component { canonical_id } => {
            FfiInheritedSource::Component { canonical_id }
        }
    }
}

pub(super) fn member_availability_to_ffi(
    availability: verter_semantic::analysis::component_meta::MemberAvailability,
) -> FfiMemberAvailability {
    match availability {
        verter_semantic::analysis::component_meta::MemberAvailability::Always => {
            FfiMemberAvailability::Always
        }
        verter_semantic::analysis::component_meta::MemberAvailability::Conditional {
            branch_keys,
        } => FfiMemberAvailability::Conditional { branch_keys },
    }
}

pub(super) fn accepted_prop_kind_to_ffi(
    kind: verter_semantic::analysis::component_meta::AcceptedPropKind,
) -> FfiAcceptedPropKind {
    match kind {
        verter_semantic::analysis::component_meta::AcceptedPropKind::DeclaredProp => {
            FfiAcceptedPropKind::DeclaredProp
        }
        verter_semantic::analysis::component_meta::AcceptedPropKind::Attr => {
            FfiAcceptedPropKind::Attr
        }
    }
}

pub(super) fn accepted_event_kind_to_ffi(
    kind: verter_semantic::analysis::component_meta::AcceptedEventKind,
) -> FfiAcceptedEventKind {
    match kind {
        verter_semantic::analysis::component_meta::AcceptedEventKind::DeclaredEmit => {
            FfiAcceptedEventKind::DeclaredEmit
        }
        verter_semantic::analysis::component_meta::AcceptedEventKind::Listener => {
            FfiAcceptedEventKind::Listener
        }
    }
}

pub(super) fn accepted_surface_completeness_to_ffi(
    completeness: verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness,
) -> FfiAcceptedSurfaceCompleteness {
    match completeness {
        verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact => {
            FfiAcceptedSurfaceCompleteness::Exact
        }
        verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound => {
            FfiAcceptedSurfaceCompleteness::LowerBound
        }
    }
}
pub(super) fn resolved_jsdoc_tag_to_ffi(
    tag: &host::meta_resolve::ResolvedJsdocTag,
) -> FfiResolvedJsdocTag {
    FfiResolvedJsdocTag {
        name: tag.name.clone(),
        text: tag.text.clone(),
        raw_type: tag.raw_type.clone(),
        subject_name: tag.subject_name.clone(),
        resolved_type: tag.resolved_type.clone(),
    }
}

pub(super) fn component_prop_constness_to_string(
    constness: verter_semantic::analysis::template::PropValueConstness,
) -> String {
    match constness {
        verter_semantic::analysis::template::PropValueConstness::Const => "const".to_string(),
        verter_semantic::analysis::template::PropValueConstness::Dynamic => "dynamic".to_string(),
        verter_semantic::analysis::template::PropValueConstness::Unknown => "unknown".to_string(),
    }
}

pub(super) fn binding_kind_to_string(
    kind: verter_semantic::analysis::component_meta::BindingKindAnalysis,
) -> String {
    match kind {
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Const => {
            "const".to_string()
        }
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Let => "let".to_string(),
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Var => "var".to_string(),
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Function => {
            "function".to_string()
        }
        verter_semantic::analysis::component_meta::BindingKindAnalysis::AsyncFunction => {
            "asyncFunction".to_string()
        }
        verter_semantic::analysis::component_meta::BindingKindAnalysis::Class => {
            "class".to_string()
        }
    }
}

pub(super) fn reactivity_kind_to_string(
    kind: verter_semantic::analysis::types::ReactivityKind,
) -> String {
    match kind {
        verter_semantic::analysis::types::ReactivityKind::None => "none".to_string(),
        verter_semantic::analysis::types::ReactivityKind::Ref => "ref".to_string(),
        verter_semantic::analysis::types::ReactivityKind::Computed => "computed".to_string(),
        verter_semantic::analysis::types::ReactivityKind::Reactive => "reactive".to_string(),
        verter_semantic::analysis::types::ReactivityKind::MaybeRef => "maybeRef".to_string(),
        verter_semantic::analysis::types::ReactivityKind::Mutable => "mutable".to_string(),
    }
}

pub(super) fn vue_api_to_string(
    api: verter_semantic::analysis::types::VueApiClassification,
) -> String {
    format!("{api:?}")
}

pub(super) fn style_lang_to_string(
    lang: verter_semantic::analysis::style::StyleAnalysisLang,
) -> String {
    format!("{lang:?}")
}

pub(super) fn projection_mode_to_string(mode: host::ProjectionMode) -> String {
    match mode {
        host::ProjectionMode::Identity => "identity".to_string(),
        host::ProjectionMode::Navigate => "navigate".to_string(),
        host::ProjectionMode::Shallow => "shallow".to_string(),
        host::ProjectionMode::Expanded => "expanded".to_string(),
        host::ProjectionMode::Skeleton => "skeleton".to_string(),
    }
}

pub(super) fn macro_kind_to_string(kind: verter_semantic::analysis::AnalyzedMacroKind) -> String {
    match kind {
        verter_semantic::analysis::AnalyzedMacroKind::DefineProps => "defineProps".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::WithDefaults => "withDefaults".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => "defineEmits".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => "defineSlots".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineModel => "defineModel".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineExpose => "defineExpose".to_string(),
        verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => "defineOptions".to_string(),
    }
}

pub(super) fn resolved_declaration_kind_to_string(
    kind: host::meta_resolve::ResolvedDeclarationKind,
) -> String {
    match kind {
        host::meta_resolve::ResolvedDeclarationKind::Interface => "interface".to_string(),
        host::meta_resolve::ResolvedDeclarationKind::TypeAlias => "typeAlias".to_string(),
        host::meta_resolve::ResolvedDeclarationKind::Class => "class".to_string(),
        host::meta_resolve::ResolvedDeclarationKind::Unknown => "unknown".to_string(),
    }
}

pub(super) fn public_instance_completeness_to_string(
    completeness: verter_semantic::analysis::component_meta::PublicInstanceCompleteness,
) -> String {
    match completeness {
        verter_semantic::analysis::component_meta::PublicInstanceCompleteness::Exact => {
            "exact".to_string()
        }
        verter_semantic::analysis::component_meta::PublicInstanceCompleteness::Partial => {
            "partial".to_string()
        }
    }
}

pub(super) fn public_instance_member_kind_to_string(
    kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind,
) -> String {
    match kind {
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Prop => {
            "prop".to_string()
        }
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::SlotContainer => {
            "slotContainer".to_string()
        }
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed => {
            "exposed".to_string()
        }
    }
}
pub(super) fn member_visibility_to_string(
    visibility: verter_type_expr::MemberVisibility,
) -> String {
    match visibility {
        verter_type_expr::MemberVisibility::Public => "public".to_string(),
        verter_type_expr::MemberVisibility::Protected => "protected".to_string(),
        verter_type_expr::MemberVisibility::Private => "private".to_string(),
    }
}
pub(super) fn host_block_type_to_string(bt: host::PreprocessorBlockType) -> String {
    match bt {
        host::PreprocessorBlockType::Template => "template".to_string(),
        host::PreprocessorBlockType::Script => "script".to_string(),
        host::PreprocessorBlockType::Style => "style".to_string(),
        host::PreprocessorBlockType::Custom => "custom".to_string(),
    }
}
pub(super) fn host_module_reference_syntax_to_string(syntax: impl std::fmt::Debug) -> String {
    match format!("{syntax:?}").as_str() {
        "StaticImport" => "staticImport".to_string(),
        "ExportFrom" => "exportFrom".to_string(),
        "DynamicImport" => "dynamicImport".to_string(),
        "RequireCall" => "requireCall".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn host_module_reference_semantics_to_string(semantics: impl std::fmt::Debug) -> String {
    match format!("{semantics:?}").as_str() {
        "Import" => "import".to_string(),
        "Require" => "require".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn host_module_reference_analyzability_to_string(
    analyzability: impl std::fmt::Debug,
) -> String {
    match format!("{analyzability:?}").as_str() {
        "Exact" => "exact".to_string(),
        "FiniteSet" => "finiteSet".to_string(),
        "UnknownDynamic" => "unknownDynamic".to_string(),
        other => other.to_string(),
    }
}
