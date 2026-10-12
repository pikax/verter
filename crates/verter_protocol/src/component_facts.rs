//! Context-free publication of independent component facts.
use crate::types::*;
use serde::Serialize;

/// A nonempty set of open machine cause codes.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct FactCauses(Vec<u32>);
impl FactCauses {
    pub fn new(causes: Vec<u32>) -> Option<Self> {
        (!causes.is_empty() && causes.iter().all(|cause| *cause != 0)).then_some(Self(causes))
    }
    pub fn as_slice(&self) -> &[u32] {
        &self.0
    }
}
/// Value and guarantee are one output record. Unavailability owns no value.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiFact<T> {
    Complete {
        value: T,
        open: bool,
    },
    Approximate {
        value: T,
        causes: FactCauses,
        open: bool,
    },
    Unavailable {
        causes: FactCauses,
    },
}
impl<T> FfiFact<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Complete { value, .. } | Self::Approximate { value, .. } => Some(value),
            Self::Unavailable { .. } => None,
        }
    }
    pub fn causes(&self) -> &[u32] {
        match self {
            Self::Complete { .. } => &[],
            Self::Approximate { causes, .. } | Self::Unavailable { causes } => causes.as_slice(),
        }
    }
    pub fn kind(&self) -> i32 {
        match self {
            Self::Complete { .. } => 1,
            Self::Approximate { .. } => 2,
            Self::Unavailable { .. } => 3,
        }
    }
    pub fn open(&self) -> bool {
        match self {
            Self::Complete { open, .. } | Self::Approximate { open, .. } => *open,
            Self::Unavailable { .. } => true,
        }
    }
}
fn domain<T>(
    value: T,
    guarantee: &verter_session_query::analysis::component_meta::MetadataFactGuarantee,
) -> FfiFact<T> {
    match guarantee.kind {
        1 if guarantee.causes.is_empty() => FfiFact::Complete {
            value,
            open: guarantee.open,
        },
        2 => FactCauses::new(guarantee.causes.clone()).map_or_else(
            || unavailable(104),
            |causes| FfiFact::Approximate {
                value,
                causes,
                open: true,
            },
        ),
        3 => FfiFact::Unavailable {
            causes: FactCauses::new(guarantee.causes.clone()).unwrap_or(FactCauses(vec![104])),
        },
        _ => unavailable(104),
    }
}
fn unavailable<T>(cause: u32) -> FfiFact<T> {
    FfiFact::Unavailable {
        causes: FactCauses(vec![cause]),
    }
}
fn complete<T>(value: T) -> FfiFact<T> {
    FfiFact::Complete { value, open: false }
}
fn lower_bound<T>(value: T) -> FfiFact<T> {
    FfiFact::Approximate {
        value,
        causes: FactCauses(vec![103]),
        open: true,
    }
}

/// Borrowed terminal projection: no parallel member arrays or retained IR copies.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentFacts<'a> {
    pub props: FfiFact<&'a Vec<FfiPropMeta>>,
    pub events: FfiFact<&'a Vec<FfiEventMeta>>,
    pub slots: FfiFact<&'a Vec<FfiSlotMeta>>,
    pub models: FfiFact<&'a Vec<FfiModelMeta>>,
    pub exposed: FfiFact<&'a Vec<FfiExposedMeta>>,
    pub accepted_props: FfiFact<&'a Vec<FfiAcceptedPropMeta>>,
    pub accepted_events: FfiFact<&'a Vec<FfiAcceptedEventMeta>>,
    pub root_reachability: FfiFact<&'a FfiRootReachability>,
    pub fallthrough_surface: FfiFact<&'a FfiFallthroughSurface>,
    pub public_instance: FfiFact<&'a Option<FfiPublicInstanceMeta>>,
    pub component_public_contract: FfiFact<&'a FfiComponentPublicContract>,
    pub type_registry: FfiFact<&'a Vec<FfiResolvedTypeMeta>>,
    pub delivery: FfiFact<()>,
}
impl FfiComponentMeta {
    pub fn facts(&self) -> FfiComponentFacts<'_> {
        let accepted_exact = matches!(
            self.accepted_surface_completeness,
            FfiAcceptedSurfaceCompleteness::Exact
        );
        FfiComponentFacts {
            props: domain(&self.props, &self.domain_guarantees.props),
            events: domain(&self.events, &self.domain_guarantees.events),
            slots: domain(&self.slots, &self.domain_guarantees.slots),
            models: complete(&self.models),
            exposed: complete(&self.exposed),
            accepted_props: if accepted_exact {
                complete(&self.accepted_props)
            } else {
                lower_bound(&self.accepted_props)
            },
            accepted_events: if accepted_exact {
                complete(&self.accepted_events)
            } else {
                lower_bound(&self.accepted_events)
            },
            root_reachability: complete(&self.root_reachability),
            fallthrough_surface: if accepted_exact {
                complete(&self.fallthrough_surface)
            } else {
                lower_bound(&self.fallthrough_surface)
            },
            public_instance: match self.public_instance.as_ref() {
                Some(instance) if instance.completeness == "partial" => {
                    lower_bound(&self.public_instance)
                }
                _ => complete(&self.public_instance),
            },
            component_public_contract: match &self.component_public_contract {
                FfiComponentContractAvailability::Supported { contract } => {
                    if matches!(contract.exactness, FfiContractExactness::Exact) {
                        complete(contract)
                    } else {
                        lower_bound(contract)
                    }
                }
                FfiComponentContractAvailability::Unsupported { unsupported } => {
                    FfiFact::Unavailable {
                        causes: contract_causes(&unsupported.reason),
                    }
                }
            },
            type_registry: complete(&self.type_registry),
            delivery: match &self.result_completeness {
                FfiResultCompleteness::Complete => complete(()),
                FfiResultCompleteness::Partial { reasons } => FfiFact::Unavailable {
                    causes: FactCauses::new(
                        reasons
                            .iter()
                            .map(|reason| match reason {
                                FfiSurfacePartialReason::UndecidedConditional => 18,
                                FfiSurfacePartialReason::ConnectedMemoryLimit => 19,
                                FfiSurfacePartialReason::OperationBudget => 20,
                                _ => {
                                    crate::component_meta::surface_partial_reason_to_proto(*reason)
                                        as u32
                                }
                            })
                            .collect(),
                    )
                    .unwrap_or(FactCauses(vec![104])),
                },
            },
        }
    }
}

impl Serialize for FfiComponentMeta {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let facts = self.facts();
        let mut output = serializer.serialize_struct("ComponentMeta", 29)?;
        output.serialize_field("facts", &facts)?;
        output.serialize_field("componentPublicContract", &self.component_public_contract)?;
        output.serialize_field(
            "props",
            &facts
                .props
                .value()
                .map(|value| value.as_slice())
                .unwrap_or(&[]),
        )?;
        output.serialize_field(
            "events",
            &facts
                .events
                .value()
                .map(|value| value.as_slice())
                .unwrap_or(&[]),
        )?;
        output.serialize_field(
            "slots",
            &facts
                .slots
                .value()
                .map(|value| value.as_slice())
                .unwrap_or(&[]),
        )?;
        output.serialize_field(
            "models",
            &facts
                .models
                .value()
                .map(|value| value.as_slice())
                .unwrap_or(&[]),
        )?;
        output.serialize_field(
            "exposed",
            &facts
                .exposed
                .value()
                .map(|value| value.as_slice())
                .unwrap_or(&[]),
        )?;
        if self.public_instance.is_some() {
            output.serialize_field("publicInstance", &self.public_instance)?;
        }
        output.serialize_field("orderedSfcStructure", &self.ordered_sfc_structure)?;
        output.serialize_field(
            "typeRegistry",
            &facts
                .type_registry
                .value()
                .map(|value| value.as_slice())
                .unwrap_or(&[]),
        )?;
        output.serialize_field("components", &self.components)?;
        output.serialize_field("templateRefs", &self.template_refs)?;
        output.serialize_field("imports", &self.imports)?;
        output.serialize_field("bindings", &self.bindings)?;
        output.serialize_field("vueApiCalls", &self.vue_api_calls)?;
        output.serialize_field("styles", &self.styles)?;
        output.serialize_field("flags", &self.flags)?;
        output.serialize_field(
            "acceptedProps",
            &facts
                .accepted_props
                .value()
                .map(|value| value.as_slice())
                .unwrap_or(&[]),
        )?;
        output.serialize_field(
            "acceptedEvents",
            &facts
                .accepted_events
                .value()
                .map(|value| value.as_slice())
                .unwrap_or(&[]),
        )?;
        output.serialize_field(
            "acceptedSurfaceCompleteness",
            &self.accepted_surface_completeness,
        )?;
        output.serialize_field("rootInfo", &self.root_info)?;
        output.serialize_field("rootReachability", &self.root_reachability)?;
        output.serialize_field("fallthroughSurface", &self.fallthrough_surface)?;
        if !self.macro_expansion_diagnostics.is_empty() {
            output.serialize_field(
                "macroExpansionDiagnostics",
                &self.macro_expansion_diagnostics,
            )?;
        }
        output.serialize_field("optionsApi", &self.options_api)?;
        output.serialize_field("filePath", &self.file_path)?;
        if self.resolution.is_some() {
            output.serialize_field("resolution", &self.resolution)?;
        }
        output.serialize_field("resolutionStatus", &self.resolution_status)?;
        if !self.origin.edges.is_empty() {
            output.serialize_field("origin", &self.origin)?;
        }
        output.serialize_field("resultCompleteness", &self.derived_completeness())?;
        output.end()
    }
}

fn publication_fact<'a>(
    publication: &FfiTypePublication,
    value: Option<&'a verter_type_expr::TypeExpr>,
) -> FfiFact<Option<&'a verter_type_expr::TypeExpr>> {
    match publication {
        FfiTypePublication::Failed { .. } => unavailable(102),
        FfiTypePublication::Absent { .. } => complete(None),
        FfiTypePublication::Published {
            exactness: FfiPublicationExactness::Incomplete,
            ..
        } if value.is_some() => FfiFact::Approximate {
            value,
            causes: FactCauses(vec![103]),
            open: false,
        },
        FfiTypePublication::Published { .. } if value.is_some() => complete(value),
        _ => unavailable(104),
    }
}

impl Serialize for FfiPropMeta {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let type_fact = publication_fact(&self.publication, self.r#type.as_ref());
        let mut output = serializer.serialize_struct("FfiPropMeta", 12)?;
        output.serialize_field("typeFact", &type_fact)?;
        if !matches!(type_fact, FfiFact::Unavailable { .. }) {
            if let Some(value) = &self.r#type {
                output.serialize_field("type", value)?;
            }
        }
        output.serialize_field("requiredFact", &complete(self.required))?;
        output.serialize_field("hasDefaultFact", &complete(self.has_default))?;
        output.serialize_field("name", &self.name)?;
        output.serialize_field("publication", &self.publication)?;
        output.serialize_field("terminalDisplay", &self.terminal_display)?;
        if let Some(value) = &self.type_expansion {
            output.serialize_field("typeExpansion", value)?;
        }
        output.serialize_field("required", &self.required)?;
        output.serialize_field("hasDefault", &self.has_default)?;
        if let Some(value) = &self.default_value {
            output.serialize_field("defaultValue", value)?;
        }
        if let Some(value) = &self.description {
            output.serialize_field("description", value)?;
        }
        if !self.tags.is_empty() {
            output.serialize_field("tags", &self.tags)?;
        }
        output.serialize_field("declaredInMacroTypeArg", &self.declared_in_macro_type_arg)?;
        output.end()
    }
}

impl Serialize for FfiEventMeta {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let type_fact = publication_fact(&self.publication, self.payload.as_ref());
        let mut output = serializer.serialize_struct("FfiEventMeta", 9)?;
        output.serialize_field("typeFact", &type_fact)?;
        if !matches!(type_fact, FfiFact::Unavailable { .. }) {
            if let Some(value) = &self.payload {
                output.serialize_field("payload", value)?;
            }
        }
        output.serialize_field("name", &self.name)?;
        output.serialize_field("publication", &self.publication)?;
        output.serialize_field("terminalDisplay", &self.terminal_display)?;
        if let Some(value) = &self.payload_expansion {
            output.serialize_field("payloadExpansion", value)?;
        }
        if let Some(value) = &self.raw_signature {
            output.serialize_field("rawSignature", value)?;
        }
        if let Some(value) = &self.description {
            output.serialize_field("description", value)?;
        }
        if !self.tags.is_empty() {
            output.serialize_field("tags", &self.tags)?;
        }
        output.end()
    }
}

impl Serialize for FfiSlotMeta {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let return_fact = self
            .return_publication
            .as_ref()
            .map(|publication| publication_fact(publication, self.return_value.as_ref()));
        let omit_return_value = matches!(return_fact, Some(FfiFact::Unavailable { .. }));
        let mut output = serializer.serialize_struct("FfiSlotMeta", 12)?;
        output.serialize_field("name", &self.name)?;
        output.serialize_field("isScoped", &self.is_scoped)?;
        output.serialize_field("bindings", &self.bindings)?;
        output.serialize_field("isRequired", &self.is_required)?;
        if let Some(value) = &self.return_type {
            output.serialize_field("returnType", value)?;
        }
        if let Some(fact) = &return_fact {
            output.serialize_field("returnFact", fact)?;
        }
        if !omit_return_value {
            if let Some(value) = &self.return_value {
                output.serialize_field("returnValue", value)?;
            }
        }
        if let Some(value) = &self.return_publication {
            output.serialize_field("returnPublication", value)?;
        }
        if let Some(value) = &self.return_terminal_display {
            output.serialize_field("returnTerminalDisplay", value)?;
        }
        if let Some(value) = &self.description {
            output.serialize_field("description", value)?;
        }
        if !self.tags.is_empty() {
            output.serialize_field("tags", &self.tags)?;
        }
        output.serialize_field("declaredInMacroTypeArg", &self.declared_in_macro_type_arg)?;
        output.end()
    }
}

impl Serialize for FfiSlotBindingMeta {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let type_fact = publication_fact(&self.publication, self.r#type.as_ref());
        let mut output = serializer.serialize_struct("FfiSlotBindingMeta", 6)?;
        output.serialize_field("typeFact", &type_fact)?;
        if !matches!(type_fact, FfiFact::Unavailable { .. }) {
            if let Some(value) = &self.r#type {
                output.serialize_field("type", value)?;
            }
        }
        output.serialize_field("name", &self.name)?;
        output.serialize_field("publication", &self.publication)?;
        output.serialize_field("terminalDisplay", &self.terminal_display)?;
        if let Some(value) = &self.type_expansion {
            output.serialize_field("typeExpansion", value)?;
        }
        output.end()
    }
}

impl FfiComponentMeta {
    /// Compatibility projection, never authority for an individual fact.
    pub fn derived_completeness(&self) -> FfiResultCompleteness {
        let facts = self.facts();
        let mut causes = Vec::new();
        let mut add = |codes: &[u32]| {
            for code in codes {
                if !causes.contains(code) {
                    causes.push(*code);
                }
            }
        };
        add(facts.props.causes());
        add(facts.events.causes());
        add(facts.slots.causes());
        add(facts.models.causes());
        add(facts.exposed.causes());
        add(facts.accepted_props.causes());
        add(facts.accepted_events.causes());
        add(facts.root_reachability.causes());
        add(facts.fallthrough_surface.causes());
        add(facts.public_instance.causes());
        add(facts.component_public_contract.causes());
        add(facts.type_registry.causes());
        add(facts.delivery.causes());
        let mut publication = |value: &FfiTypePublication| match value {
            FfiTypePublication::Failed { .. } => add(&[102]),
            FfiTypePublication::Published {
                exactness: FfiPublicationExactness::Incomplete,
                ..
            } => add(&[103]),
            _ => {}
        };
        for prop in &self.props {
            publication(&prop.publication);
        }
        for event in &self.events {
            publication(&event.publication);
        }
        for slot in &self.slots {
            for binding in &slot.bindings {
                publication(&binding.publication);
            }
            if let Some(value) = &slot.return_publication {
                publication(value);
            }
        }
        let mut reasons = Vec::new();
        for code in causes {
            let reason = match code {
                1 => FfiSurfacePartialReason::BudgetExceeded,
                2 => FfiSurfacePartialReason::Cancelled,
                3 => FfiSurfacePartialReason::SupersededGeneration,
                4 => FfiSurfacePartialReason::UnstableState,
                5 => FfiSurfacePartialReason::SamePathRecursion,
                6 => FfiSurfacePartialReason::WalkerFatal,
                7 => FfiSurfacePartialReason::Propagated,
                8 => FfiSurfacePartialReason::DeferredEvaluationLimit,
                9 => FfiSurfacePartialReason::StructuralFactDemandLimit,
                10 => FfiSurfacePartialReason::SemanticQueryFault,
                11 => FfiSurfacePartialReason::MissingSemanticNodeData,
                12 => FfiSurfacePartialReason::ProjectionWorkLimit,
                13 => FfiSurfacePartialReason::ConnectedQueryDepthLimit,
                14 => FfiSurfacePartialReason::MissingDependency,
                15 => FfiSurfacePartialReason::FlowReturnUninferred,
                16 => FfiSurfacePartialReason::FlowReturnUnverified,
                17 => FfiSurfacePartialReason::FlowReturnNoSurface,
                18 => FfiSurfacePartialReason::UndecidedConditional,
                19 => FfiSurfacePartialReason::ConnectedMemoryLimit,
                20 => FfiSurfacePartialReason::OperationBudget,
                _ => FfiSurfacePartialReason::Propagated,
            };
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
        if reasons.is_empty() {
            FfiResultCompleteness::Complete
        } else {
            FfiResultCompleteness::Partial { reasons }
        }
    }
}

fn contract_causes(reason: &FfiComponentContractUnsupportedReason) -> FactCauses {
    let codes = match reason {
        FfiComponentContractUnsupportedReason::AdapterUnavailable => vec![106],
        FfiComponentContractUnsupportedReason::ComponentMetaUnavailable => vec![107],
        FfiComponentContractUnsupportedReason::PublicationFailed { failure, .. } => {
            vec![match failure {
                FfiTypePublicationFailure::UnrepresentableRequiredMemberValue => 102,
                FfiTypePublicationFailure::UnrepresentableRequiredPayload => 105,
            }]
        }
        FfiComponentContractUnsupportedReason::OutputMaterializationFailed { failure, .. } => {
            match failure {
                FfiComponentMetaOutputFailure::UnraisableSource => vec![110],
                FfiComponentMetaOutputFailure::RequiredSourceUnavailable {
                    failure: publication_failure,
                } => vec![
                    111,
                    match publication_failure {
                        FfiTypePublicationFailure::UnrepresentableRequiredMemberValue => 102,
                        FfiTypePublicationFailure::UnrepresentableRequiredPayload => 105,
                    },
                ],
                FfiComponentMetaOutputFailure::InteriorSourceMiss => vec![112],
                FfiComponentMetaOutputFailure::ShellMaterializationMiss => vec![113],
                FfiComponentMetaOutputFailure::UnknownMaterializingSourceInterior => vec![114],
            }
        }
    };
    FactCauses(codes)
}
