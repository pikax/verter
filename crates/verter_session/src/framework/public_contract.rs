//! Framework-neutral public component contract projected exactly once from
//! component-meta analysis and its terminally materialized type lanes.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
use verter_type_expr::{
    PrimitiveName, PublicationResult, ResolutionDiagnostic, ResolutionExactness,
    ResolutionProvenance, TypeExpr, TypedResolutionFailure,
};

use crate::framework::FrameworkAdapterId;
use crate::meta_resolve::{
    ComponentMetaOutputError, ComponentMetaOutputFailure, ComponentMetaOutputLane,
    MaterializedComponentMetaTypeLanes, MaterializedTypePublication,
};

/// Closed availability carried by every produced component declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentContractAvailability {
    /// The semantic contract was projected successfully.
    Supported(Arc<ComponentPublicContract>),
    /// Projection failed closed with typed diagnostics.
    Unsupported(ComponentContractUnsupported),
}

/// A typed unsupported contract result.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentContractUnsupported {
    /// Framework adapter selected for the component.
    pub adapter_id: FrameworkAdapterId,
    /// Closed failure reason.
    pub reason: ComponentContractUnsupportedReason,
    /// Producer-owned resolution diagnostics, when present.
    pub diagnostics: Arc<[ResolutionDiagnostic]>,
}

/// Closed reasons a component contract cannot be published.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentContractUnsupportedReason {
    /// The output does not belong to a registered framework carrier.
    AdapterUnavailable,
    /// Component-meta output was unavailable under the declaration view.
    ComponentMetaUnavailable,
    /// A terminal output lane could not be materialized.
    OutputMaterializationFailed {
        /// Failed lane.
        lane: ComponentMetaOutputLane,
        /// Outer positional index.
        index: usize,
        /// Optional nested positional index.
        inner_index: Option<usize>,
        /// Typed sink failure.
        failure: ComponentMetaOutputFailure,
    },
    /// A public member carried a failed A1 publication.
    PublicationFailed {
        /// Exact public surface position.
        surface: ContractSurface,
        /// Typed source/publication failure.
        failure: TypedResolutionFailure,
        /// Producer-owned resolution provenance.
        provenance: ResolutionProvenance,
    },
}

/// Aggregate or member exactness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractExactness {
    /// Every publication is exact.
    Exact,
    /// At least one publication is absent or incomplete.
    Degraded,
}

/// Contract projection provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractProvenance {
    /// Projected from one component-meta output envelope.
    ComponentMetaOutput,
}

/// A typed location inside the public contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractSurface {
    /// A public prop.
    Prop { name: Arc<str> },
    /// One event overload.
    Event {
        name: Arc<str>,
        overload_index: usize,
    },
    /// One scoped-slot binding.
    SlotBinding { slot: Arc<str>, binding: Arc<str> },
    /// A slot return.
    SlotReturn { slot: Arc<str> },
}

/// Why an otherwise supported contract is degraded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractDegradationReason {
    /// The schema proves that no semantic type was authored.
    Absent,
    /// The producer resolved only an incomplete semantic type.
    Incomplete,
}

/// One typed degradation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractDegradation {
    /// Affected contract position.
    pub surface: ContractSurface,
    /// Closed degradation reason.
    pub reason: ContractDegradationReason,
    /// Producer-owned typed diagnostics.
    pub diagnostics: Arc<[ResolutionDiagnostic]>,
}

/// Framework-neutral public component contract.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentPublicContract {
    /// Framework adapter identity.
    pub adapter_id: FrameworkAdapterId,
    /// Aggregate exactness.
    pub exactness: ContractExactness,
    /// Aggregate typed degradations.
    pub degradation: Arc<[ContractDegradation]>,
    /// Projection provenance.
    pub provenance: ContractProvenance,
    /// Public props in source order.
    pub props: Arc<[PublicProp]>,
    /// Public events in first-name order, with source-ordered overloads.
    pub events: Arc<[PublicEvent]>,
    /// Public slots in source order.
    pub slots: Arc<[PublicSlot]>,
}

/// One typed public type reference.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicTypeReference {
    /// A1 publication, materialized descriptor, and separately branded display.
    pub publication: MaterializedTypePublication,
}

/// One public prop.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicProp {
    /// Public name.
    pub name: Arc<str>,
    /// Whether callers may omit the prop.
    pub optional: bool,
    /// Whether an authored runtime default exists.
    pub has_default: bool,
    /// Typed source/descriptor publication.
    pub ty: PublicTypeReference,
    /// Member exactness.
    pub exactness: ContractExactness,
    /// Member degradations.
    pub degradation: Arc<[ContractDegradation]>,
    /// Member provenance.
    pub provenance: ContractProvenance,
}

/// One public event, grouping duplicate semantic rows as overloads.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicEvent {
    /// Event name.
    pub name: Arc<str>,
    /// Source-ordered overloads.
    pub overloads: Arc<[PublicCallSignature]>,
    /// Structured listener-handler shape derived from the overloads.
    pub derived_handler: PublicDerivedHandlerShape,
    /// Member exactness.
    pub exactness: ContractExactness,
    /// Member degradations.
    pub degradation: Arc<[ContractDegradation]>,
    /// Member provenance.
    pub provenance: ContractProvenance,
}

/// One structured public call signature.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicCallSignature {
    /// Original typed publication for the payload/callable descriptor.
    pub source: PublicTypeReference,
    /// Structured parameters.
    pub parameters: Arc<[PublicParameter]>,
    /// Structured return type.
    pub return_type: TypeExpr,
}

/// One public parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicParameter {
    /// Authored parameter/tuple label, when present.
    pub name: Option<Arc<str>>,
    /// Whether the parameter is optional.
    pub optional: bool,
    /// Whether the parameter is a rest parameter.
    pub rest: bool,
    /// Structured parameter type.
    pub ty: TypeExpr,
}

/// Structured listener-handler overload shape.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicDerivedHandlerShape {
    /// Handler overloads corresponding positionally to event overloads.
    pub overloads: Arc<[PublicHandlerSignature]>,
}

/// One structured listener-handler signature.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicHandlerSignature {
    /// Structured handler parameters.
    pub parameters: Arc<[PublicParameter]>,
    /// Structured handler return.
    pub return_type: TypeExpr,
}

/// One public slot.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicSlot {
    /// Slot name.
    pub name: Arc<str>,
    /// Whether callers may omit the slot.
    pub optional: bool,
    /// Structured scoped-slot input.
    pub input: PublicSlotInput,
    /// Meaningful typed slot return, when present.
    pub return_type: Option<PublicTypeReference>,
    /// Member exactness.
    pub exactness: ContractExactness,
    /// Member degradations.
    pub degradation: Arc<[ContractDegradation]>,
    /// Member provenance.
    pub provenance: ContractProvenance,
}

/// Structured slot input.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicSlotInput {
    /// Scoped binding rows in source order.
    pub bindings: Arc<[PublicSlotBinding]>,
}

/// One structured scoped-slot input binding.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicSlotBinding {
    /// Binding name.
    pub name: Arc<str>,
    /// Typed source/descriptor publication.
    pub ty: PublicTypeReference,
}

/// Convert a terminal output error into a closed unsupported contract.
pub(crate) fn unsupported_from_output_error(
    adapter_id: FrameworkAdapterId,
    error: &ComponentMetaOutputError,
) -> ComponentContractAvailability {
    ComponentContractAvailability::Unsupported(ComponentContractUnsupported {
        adapter_id,
        reason: ComponentContractUnsupportedReason::OutputMaterializationFailed {
            lane: error.lane,
            index: error.index,
            inner_index: error.inner_index,
            failure: error.failure.clone(),
        },
        diagnostics: Arc::from([]),
    })
}

/// The sole analysis/materialized-lanes to public-contract projector.
pub(crate) fn project_component_public_contract(
    adapter_id: FrameworkAdapterId,
    analysis: &ComponentMetaAnalysis,
    lanes: &MaterializedComponentMetaTypeLanes,
) -> ComponentContractAvailability {
    match project_supported(adapter_id.clone(), analysis, lanes) {
        Ok(contract) => ComponentContractAvailability::Supported(Arc::new(contract)),
        Err((reason, diagnostics)) => {
            ComponentContractAvailability::Unsupported(ComponentContractUnsupported {
                adapter_id,
                reason,
                diagnostics,
            })
        }
    }
}

type ProjectionFailure = (
    ComponentContractUnsupportedReason,
    Arc<[ResolutionDiagnostic]>,
);

fn project_supported(
    adapter_id: FrameworkAdapterId,
    analysis: &ComponentMetaAnalysis,
    lanes: &MaterializedComponentMetaTypeLanes,
) -> Result<ComponentPublicContract, ProjectionFailure> {
    let mut degradation = Vec::new();
    let mut props = Vec::with_capacity(analysis.props.len());
    for (prop, lane) in analysis.props.iter().zip(&lanes.props) {
        let surface = ContractSurface::Prop {
            name: Arc::from(prop.name.as_str()),
        };
        let member_degradation = publication_degradation(lane, surface.clone())?;
        degradation.extend(member_degradation.iter().cloned());
        props.push(PublicProp {
            name: Arc::from(prop.name.as_str()),
            optional: !prop.required || prop.has_default,
            has_default: prop.has_default,
            ty: PublicTypeReference {
                publication: lane.clone(),
            },
            exactness: exactness(&member_degradation),
            degradation: member_degradation.into(),
            provenance: ContractProvenance::ComponentMetaOutput,
        });
    }

    let mut events: Vec<PublicEvent> = Vec::new();
    for (event_index, (event, lane)) in analysis
        .events
        .iter()
        .zip(&lanes.event_publications)
        .enumerate()
    {
        let existing = events
            .iter()
            .position(|candidate| candidate.name.as_ref() == event.name);
        let overload_index = existing
            .map(|index| events[index].overloads.len())
            .unwrap_or(0);
        let surface = ContractSurface::Event {
            name: Arc::from(event.name.as_str()),
            overload_index,
        };
        let member_degradation = publication_degradation(lane, surface)?;
        let signature = signature_from_publication(lane);
        let handler = PublicHandlerSignature {
            parameters: Arc::clone(&signature.parameters),
            return_type: signature.return_type.clone(),
        };
        degradation.extend(member_degradation.iter().cloned());
        if let Some(index) = existing {
            let row = &mut events[index];
            let mut overloads = row.overloads.to_vec();
            overloads.push(signature);
            row.overloads = overloads.into();
            let mut handlers = row.derived_handler.overloads.to_vec();
            handlers.push(handler);
            row.derived_handler.overloads = handlers.into();
            let mut row_degradation = row.degradation.to_vec();
            row_degradation.extend(member_degradation);
            row.exactness = exactness(&row_degradation);
            row.degradation = row_degradation.into();
        } else {
            events.push(PublicEvent {
                name: Arc::from(event.name.as_str()),
                overloads: Arc::from([signature]),
                derived_handler: PublicDerivedHandlerShape {
                    overloads: Arc::from([handler]),
                },
                exactness: exactness(&member_degradation),
                degradation: member_degradation.into(),
                provenance: ContractProvenance::ComponentMetaOutput,
            });
        }
        let _ = event_index;
    }

    let mut slots = Vec::with_capacity(analysis.slots.len());
    for (slot_index, slot) in analysis.slots.iter().enumerate() {
        let binding_lanes = lanes
            .slot_bindings
            .get(slot_index)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut member_degradation = Vec::new();
        let mut bindings = Vec::with_capacity(slot.bindings.len());
        for (binding, lane) in slot.bindings.iter().zip(binding_lanes) {
            let row_degradation = publication_degradation(
                lane,
                ContractSurface::SlotBinding {
                    slot: Arc::from(slot.name.as_str()),
                    binding: Arc::from(binding.name.as_str()),
                },
            )?;
            member_degradation.extend(row_degradation);
            bindings.push(PublicSlotBinding {
                name: Arc::from(binding.name.as_str()),
                ty: PublicTypeReference {
                    publication: lane.clone(),
                },
            });
        }
        let return_type = lanes
            .slot_returns
            .get(slot_index)
            .and_then(Option::as_ref)
            .map(|lane| {
                let row_degradation = publication_degradation(
                    lane,
                    ContractSurface::SlotReturn {
                        slot: Arc::from(slot.name.as_str()),
                    },
                )?;
                member_degradation.extend(row_degradation);
                Ok(PublicTypeReference {
                    publication: lane.clone(),
                })
            })
            .transpose()?;
        degradation.extend(member_degradation.iter().cloned());
        slots.push(PublicSlot {
            name: Arc::from(slot.name.as_str()),
            optional: !slot.is_required,
            input: PublicSlotInput {
                bindings: bindings.into(),
            },
            return_type,
            exactness: exactness(&member_degradation),
            degradation: member_degradation.into(),
            provenance: ContractProvenance::ComponentMetaOutput,
        });
    }

    Ok(ComponentPublicContract {
        adapter_id,
        exactness: exactness(&degradation),
        degradation: degradation.into(),
        provenance: ContractProvenance::ComponentMetaOutput,
        props: props.into(),
        events: events.into(),
        slots: slots.into(),
    })
}

fn publication_degradation(
    publication: &MaterializedTypePublication,
    surface: ContractSurface,
) -> Result<Vec<ContractDegradation>, ProjectionFailure> {
    match publication.publication() {
        PublicationResult::Failed {
            failure,
            provenance,
        } => Err((
            ComponentContractUnsupportedReason::PublicationFailed {
                surface,
                failure: *failure,
                provenance: *provenance,
            },
            publication.diagnostics().to_vec().into(),
        )),
        PublicationResult::Absent { .. } => Ok(vec![ContractDegradation {
            surface,
            reason: ContractDegradationReason::Absent,
            diagnostics: publication.diagnostics().to_vec().into(),
        }]),
        PublicationResult::Published {
            exactness: ResolutionExactness::Incomplete,
            ..
        } => Ok(vec![ContractDegradation {
            surface,
            reason: ContractDegradationReason::Incomplete,
            diagnostics: publication.diagnostics().to_vec().into(),
        }]),
        PublicationResult::Published { .. } => Ok(Vec::new()),
    }
}

fn exactness(degradation: &[ContractDegradation]) -> ContractExactness {
    if degradation.is_empty() {
        ContractExactness::Exact
    } else {
        ContractExactness::Degraded
    }
}

fn signature_from_publication(publication: &MaterializedTypePublication) -> PublicCallSignature {
    let (parameters, return_type) = match publication.materialized_type() {
        Some(TypeExpr::Tuple { elements, .. }) => (
            elements
                .iter()
                .map(|element| PublicParameter {
                    name: element.label.as_deref().map(Arc::from),
                    optional: element.optional,
                    rest: element.rest,
                    ty: element.ty.clone(),
                })
                .collect::<Vec<_>>(),
            TypeExpr::Primitive(PrimitiveName::Void),
        ),
        Some(TypeExpr::Function(function)) => (
            function
                .parameters
                .iter()
                .map(|parameter| PublicParameter {
                    name: parameter.name.as_deref().map(Arc::from),
                    optional: parameter.optional,
                    rest: parameter.rest,
                    ty: parameter.ty.clone(),
                })
                .collect(),
            function
                .return_type
                .as_deref()
                .cloned()
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
        ),
        Some(other) => (
            vec![PublicParameter {
                name: Some(Arc::from("payload")),
                optional: false,
                rest: false,
                ty: other.clone(),
            }],
            TypeExpr::Primitive(PrimitiveName::Void),
        ),
        None => (Vec::new(), TypeExpr::Primitive(PrimitiveName::Void)),
    };
    PublicCallSignature {
        source: PublicTypeReference {
            publication: publication.clone(),
        },
        parameters: parameters.into(),
        return_type,
    }
}
