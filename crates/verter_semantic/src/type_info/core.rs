//! The C2 non-flow semantic gateway: [`TypeInfoCore`].
//!
//! `TypeInfoCore` is a pure kernel over one immutable
//! [`NonFlowObservationSnapshot`]. Its single semantic entry
//! ([`TypeInfoCore::attempt`]) dispatches a [`NonFlowOperation`]
//! exhaustively — no wildcard arm — and answers from staged observations
//! only. Anything the snapshot cannot answer is
//! [`NonFlowOutcome::NeedInputs`] naming the exact observation slots the
//! route found missing; the kernel never guesses, never falls back, and
//! never performs I/O.
//!
//! Sealing: [`NonFlowObservation`] is a sealed trait with exactly one
//! implementation in this crate. Foreign observation sources, injected
//! semantic traits, and construction of payload values from outside the
//! crate are structurally rejected (compile-fail rails
//! `C2-GAP3-FOREIGN-IMPL`, `C2-GAP3-PRIVATE-FIELDS`,
//! `C2-GAP3-ALTERNATE-ENTRY`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use verter_macro_dto::{
    MacroAnchor, ModelRuntimeShape, PropsDefaultsAssociation, RuntimeEmit, RuntimeProp,
    RuntimePropType, SynthesizedRowKind,
};

use crate::analysis::{
    AnalyzedMacro, AnalyzedMacroKind, MacroTypeDepUsage, ScriptAnalysisSnapshot,
};
use crate::resolver_core::ResolutionBasis;

use super::non_flow::{
    authored_emit_order, containing_with_defaults_index, emit_member_anchor, expose_member_anchor,
    is_codegen_macro, macro_index, prop_member_anchor, top_level_syntax_index,
    ExposeSurfaceProjection, ImportedComponentSurface, MacroSemanticLane, NonFlowOperation,
    NonFlowOutcome, NonFlowPayload, ProjectedExposeRow, ProjectedRuntimePropRow,
    RuntimeEmitsProjection, RuntimeModelProjection, RuntimePropsProjection, VueMacroMissingRoot,
    VueMacroSemanticDemand, VueMacroSemanticInput,
};

pub(crate) mod sealed {
    pub trait Sealed {}
}

/// The immutable observation contract the non-flow kernel reads. Sealed:
/// the only implementation is [`NonFlowObservationSnapshot`] in this
/// crate, so a foreign crate cannot inject its own observation source (or,
/// through one, a callback) into the gateway.
pub trait NonFlowObservation: sealed::Sealed + Send + Sync {
    /// The immutable script analysis observation for one canonical owner.
    fn script_analysis(&self, owner_canonical: &str) -> Option<&Arc<ScriptAnalysisSnapshot>>;

    /// The observed import resolution for one authored specifier: the
    /// canonical id the host resolved it to, when it resolved.
    fn import_resolution(&self, owner_canonical: &str, specifier: &str) -> Option<&Arc<str>>;

    /// The observed macro surface for one authored macro.
    fn macro_surface(
        &self,
        owner_canonical: &str,
        macro_index: usize,
    ) -> Option<&ObservedMacroSurface>;

    /// The observed broad-runtime value classification for one
    /// `defineModel` macro's value type.
    fn model_value_type_shape(
        &self,
        owner_canonical: &str,
        macro_index: usize,
    ) -> Option<&RuntimePropType>;
}

/// One key of the observation frontier: the canonical identity of one
/// staged observation, ordered canonically so a driver can bind and
/// revalidate frontier order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum NonFlowObservationKey {
    /// Script analysis of one canonical owner.
    ScriptAnalysis { owner_canonical: Arc<str> },
    /// One resolved import specifier of one owner.
    ImportResolution {
        owner_canonical: Arc<str>,
        specifier: Arc<str>,
    },
    /// One observed macro surface.
    MacroSurface {
        owner_canonical: Arc<str>,
        macro_index: usize,
    },
    /// One observed model value classification.
    ModelValueTypeShape {
        owner_canonical: Arc<str>,
        macro_index: usize,
    },
}

impl NonFlowObservationKey {
    /// The owner this key's observation is keyed by.
    pub fn owner_canonical(&self) -> &Arc<str> {
        match self {
            Self::ScriptAnalysis { owner_canonical }
            | Self::ImportResolution {
                owner_canonical, ..
            }
            | Self::MacroSurface {
                owner_canonical, ..
            }
            | Self::ModelValueTypeShape {
                owner_canonical, ..
            } => owner_canonical,
        }
    }
}

/// One observed member of a macro's resolved surface — the immutable
/// shape of what the live shallow-surface walk produced. Staging is
/// public to the driving session, but every projection decision made
/// over it is the kernel's.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObservedSurfaceMember {
    /// The member's published runtime name; `None` members never project.
    pub published_name: Option<String>,
    /// Whether the member is optional.
    pub optional: bool,
    /// Whether the member is public (private members never project).
    pub is_public: bool,
    /// The member's referenced type annotation name, when it names one.
    pub referenced_type_name: Option<String>,
}

/// The observed surface of one authored macro: its members and its
/// call-signature event names, in observation order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObservedMacroSurface {
    /// The surface's members, in walk order.
    pub members: Vec<ObservedSurfaceMember>,
    /// Event names the surface's call signatures yield, in walk order.
    pub call_signature_event_names: Vec<String>,
}

/// One resolved import specifier observation staged by the driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedComponentResolution {
    /// The authored import specifier.
    pub specifier: Arc<str>,
    /// The canonical id the host resolved the specifier to.
    pub resolved_canonical: Arc<str>,
}

/// The immutable observation store the kernel reads. Population happens
/// through the staging methods before the snapshot is shared with a
/// [`TypeInfoCore`]; afterwards it is never mutated.
#[derive(Debug, Clone, Default)]
pub struct NonFlowObservationSnapshot {
    script_analyses: BTreeMap<Arc<str>, Arc<ScriptAnalysisSnapshot>>,
    import_resolutions: BTreeMap<(Arc<str>, Arc<str>), Arc<str>>,
    macro_surfaces: BTreeMap<(Arc<str>, usize), ObservedMacroSurface>,
    model_value_type_shapes: BTreeMap<(Arc<str>, usize), RuntimePropType>,
}

impl NonFlowObservationSnapshot {
    /// An empty snapshot: every operation reports `NeedInputs`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stage the script analysis observation for one owner. Returns the
    /// previously staged observation, when one existed.
    pub fn stage_script_analysis(
        &mut self,
        owner_canonical: Arc<str>,
        analysis: Arc<ScriptAnalysisSnapshot>,
    ) -> Option<Arc<ScriptAnalysisSnapshot>> {
        self.script_analyses.insert(owner_canonical, analysis)
    }

    /// Stage one resolved import specifier observation.
    pub fn stage_import_resolution(
        &mut self,
        owner_canonical: Arc<str>,
        resolution: ImportedComponentResolution,
    ) {
        self.import_resolutions.insert(
            (owner_canonical, resolution.specifier),
            resolution.resolved_canonical,
        );
    }

    /// Stage one observed macro surface.
    pub fn stage_macro_surface(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
        surface: ObservedMacroSurface,
    ) {
        self.macro_surfaces
            .insert((owner_canonical, macro_index), surface);
    }

    /// Stage one observed model value classification.
    pub fn stage_model_value_type_shape(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
        value_type_shape: RuntimePropType,
    ) {
        self.model_value_type_shapes
            .insert((owner_canonical, macro_index), value_type_shape);
    }

    /// The canonically ordered frontier of every staged observation —
    /// the identity a request-local continuation binds and revalidates.
    pub fn observation_frontier(&self) -> Vec<NonFlowObservationKey> {
        let mut frontier: Vec<NonFlowObservationKey> = self
            .script_analyses
            .keys()
            .map(|owner_canonical| NonFlowObservationKey::ScriptAnalysis {
                owner_canonical: Arc::clone(owner_canonical),
            })
            .collect();
        frontier.extend(
            self.import_resolutions
                .keys()
                .map(
                    |(owner_canonical, specifier)| NonFlowObservationKey::ImportResolution {
                        owner_canonical: Arc::clone(owner_canonical),
                        specifier: Arc::clone(specifier),
                    },
                ),
        );
        frontier.extend(
            self.macro_surfaces
                .keys()
                .map(
                    |(owner_canonical, macro_index)| NonFlowObservationKey::MacroSurface {
                        owner_canonical: Arc::clone(owner_canonical),
                        macro_index: *macro_index,
                    },
                ),
        );
        frontier.extend(self.model_value_type_shapes.keys().map(
            |(owner_canonical, macro_index)| NonFlowObservationKey::ModelValueTypeShape {
                owner_canonical: Arc::clone(owner_canonical),
                macro_index: *macro_index,
            },
        ));
        frontier.sort();
        frontier
    }
}

impl sealed::Sealed for NonFlowObservationSnapshot {}

impl NonFlowObservation for NonFlowObservationSnapshot {
    fn script_analysis(&self, owner_canonical: &str) -> Option<&Arc<ScriptAnalysisSnapshot>> {
        self.script_analyses.get(owner_canonical)
    }

    fn import_resolution(&self, owner_canonical: &str, specifier: &str) -> Option<&Arc<str>> {
        self.import_resolutions
            .get(&(Arc::from(owner_canonical), Arc::from(specifier)))
    }

    fn macro_surface(
        &self,
        owner_canonical: &str,
        macro_index: usize,
    ) -> Option<&ObservedMacroSurface> {
        self.macro_surfaces
            .get(&(Arc::from(owner_canonical), macro_index))
    }

    fn model_value_type_shape(
        &self,
        owner_canonical: &str,
        macro_index: usize,
    ) -> Option<&RuntimePropType> {
        self.model_value_type_shapes
            .get(&(Arc::from(owner_canonical), macro_index))
    }
}

/// The C2 non-flow semantic gateway. Constructed from one immutable
/// observation snapshot; the single semantic entry is [`Self::attempt`].
#[derive(Debug, Clone)]
pub struct TypeInfoCore {
    snapshot: Arc<NonFlowObservationSnapshot>,
    basis: ResolutionBasis,
}

impl TypeInfoCore {
    /// Bind the kernel to one immutable snapshot under one resolution
    /// basis. The basis flows into every `NeedInputs` load set this
    /// kernel reports.
    pub fn from_observation_snapshot(
        snapshot: Arc<NonFlowObservationSnapshot>,
        basis: ResolutionBasis,
    ) -> Self {
        Self { snapshot, basis }
    }

    /// The immutable snapshot the kernel reads.
    pub fn snapshot(&self) -> &NonFlowObservationSnapshot {
        &self.snapshot
    }

    /// The resolution basis every `NeedInputs` outcome carries.
    pub const fn basis(&self) -> ResolutionBasis {
        self.basis
    }

    /// The only C2-accessible non-flow semantic gateway. Dispatches
    /// `operation` exhaustively — there is no wildcard arm, so adding a
    /// [`NonFlowOperation`] variant without extending this match (and the
    /// proof table) is a compile error, not a silent route.
    pub fn attempt(&self, operation: &NonFlowOperation) -> NonFlowOutcome {
        match operation {
            NonFlowOperation::ProjectVueMacroSemantics { owner_canonical } => {
                self.project_vue_macro_semantics(owner_canonical)
            }
            NonFlowOperation::ResolveImportedComponentSurface {
                owner_canonical,
                type_reference,
                referenced_canonical,
            } => self.resolve_imported_component_surface(
                owner_canonical,
                type_reference,
                referenced_canonical,
            ),
            NonFlowOperation::ProjectRuntimeProps {
                owner_canonical,
                macro_index,
            } => self.project_runtime_props(owner_canonical, *macro_index),
            NonFlowOperation::ProjectRuntimeEmits {
                owner_canonical,
                macro_index,
            } => self.project_runtime_emits(owner_canonical, *macro_index),
            NonFlowOperation::ProjectRuntimeModel {
                owner_canonical,
                macro_index,
            } => self.project_runtime_model(owner_canonical, *macro_index),
            NonFlowOperation::ProjectExposeSurface {
                owner_canonical,
                macro_index,
            } => self.project_expose_surface(owner_canonical, *macro_index),
        }
    }

    fn need_inputs(&self, operation: &NonFlowOperation) -> NonFlowOutcome {
        NonFlowOutcome::NeedInputs(operation.missing_input_load_set(self.basis))
    }

    fn script_analysis(&self, owner_canonical: &Arc<str>) -> Option<&Arc<ScriptAnalysisSnapshot>> {
        self.snapshot.script_analysis(owner_canonical)
    }

    fn macro_row<'a>(
        &self,
        analysis: &'a ScriptAnalysisSnapshot,
        macro_index: usize,
    ) -> Option<&'a AnalyzedMacro> {
        analysis.macros.get(macro_index)
    }

    fn project_vue_macro_semantics(&self, owner_canonical: &Arc<str>) -> NonFlowOutcome {
        let operation = NonFlowOperation::ProjectVueMacroSemantics {
            owner_canonical: Arc::clone(owner_canonical),
        };
        let Some(analysis) = self.script_analysis(owner_canonical) else {
            return self.need_inputs(&operation);
        };
        let macros = &analysis.macros;
        let mut demands = Vec::new();
        for (payload_index, mac) in macros.iter().enumerate() {
            if mac.kind == AnalyzedMacroKind::DefineExpose
                && !mac.is_type_based
                && !mac.expose_fields.is_empty()
            {
                demands.push(VueMacroSemanticDemand {
                    macro_index: payload_index,
                    effective_index: payload_index,
                    syntax_index: top_level_syntax_index(macros, payload_index),
                    kind: mac.kind,
                    lane: MacroSemanticLane::ExposeRuntimeObject,
                    has_type_argument: false,
                    binding_name: mac.binding_name.clone(),
                    model_name: None,
                    defaults_macro_index: None,
                    surface_dependency_failures: Vec::new(),
                });
                continue;
            }
            if mac.kind == AnalyzedMacroKind::WithDefaults || !mac.is_type_based {
                continue;
            }
            if !is_codegen_macro(mac.kind) {
                continue;
            }
            let defaults_index = (mac.kind == AnalyzedMacroKind::DefineProps)
                .then(|| containing_with_defaults_index(macros, payload_index))
                .flatten();
            let effective_index = defaults_index.unwrap_or(payload_index);
            demands.push(VueMacroSemanticDemand {
                macro_index: payload_index,
                effective_index,
                syntax_index: top_level_syntax_index(macros, effective_index),
                kind: mac.kind,
                lane: MacroSemanticLane::CodegenPayload,
                has_type_argument: mac.parsed_type_argument.is_some(),
                binding_name: mac.binding_name.clone(),
                model_name: mac.model_name.clone(),
                defaults_macro_index: defaults_index.map(macro_index),
                surface_dependency_failures: surface_dependency_failures(
                    analysis,
                    payload_index,
                    mac,
                ),
            });
        }
        NonFlowOutcome::Complete(NonFlowPayload::VueMacroSemanticInput(
            VueMacroSemanticInput {
                owner_canonical: Arc::clone(owner_canonical),
                demands,
            },
        ))
    }

    fn resolve_imported_component_surface(
        &self,
        owner_canonical: &Arc<str>,
        type_reference: &Arc<str>,
        referenced_canonical: &Option<Arc<str>>,
    ) -> NonFlowOutcome {
        let operation = NonFlowOperation::ResolveImportedComponentSurface {
            owner_canonical: Arc::clone(owner_canonical),
            type_reference: Arc::clone(type_reference),
            referenced_canonical: referenced_canonical.clone(),
        };
        let Some(analysis) = self.script_analysis(owner_canonical) else {
            return self.need_inputs(&operation);
        };
        // A directly-imported named binding is retained by the scope
        // requirements, so its bare name already resolves.
        let bare_name_is_imported = analysis
            .imports
            .iter()
            .flat_map(|import| import.bindings.iter())
            .any(|binding| binding.name.as_str() == type_reference.as_ref());
        // The authored specifier whose resolution reaches the referenced
        // declaration — reused verbatim so the sibling testing surface
        // resolves the same target the component's own import does. A
        // reference that resolved to no canonical (or to the owner
        // itself) is not a cross-file import.
        let import_specifier = referenced_canonical.as_ref().and_then(|canonical| {
            // A reference that resolved to the owner itself is local,
            // never a cross-file import.
            if canonical.as_ref() == owner_canonical.as_ref() {
                return None;
            }
            analysis.imports.iter().find_map(|import| {
                (self
                    .snapshot
                    .import_resolution(owner_canonical, &import.source)
                    .is_some_and(|resolved| resolved.as_ref() == canonical.as_ref()))
                .then(|| import.source.clone())
            })
        });
        NonFlowOutcome::Complete(NonFlowPayload::ImportedComponentSurface(
            ImportedComponentSurface {
                owner_canonical: Arc::clone(owner_canonical),
                type_reference: Arc::clone(type_reference),
                bare_name_is_imported,
                import_specifier,
            },
        ))
    }

    fn project_runtime_props(
        &self,
        owner_canonical: &Arc<str>,
        macro_index_value: usize,
    ) -> NonFlowOutcome {
        let operation = NonFlowOperation::ProjectRuntimeProps {
            owner_canonical: Arc::clone(owner_canonical),
            macro_index: macro_index_value,
        };
        let Some(analysis) = self.script_analysis(owner_canonical) else {
            return self.need_inputs(&operation);
        };
        let Some(mac) = self.macro_row(analysis, macro_index_value) else {
            return self.need_inputs(&operation);
        };
        if mac.kind != AnalyzedMacroKind::DefineProps {
            return self.need_inputs(&operation);
        }
        let Some(surface) = self
            .snapshot
            .macro_surface(owner_canonical, macro_index_value)
        else {
            return self.need_inputs(&operation);
        };
        let member_dependency_names = member_dependency_names(analysis, macro_index_value, mac);
        let mut rows = Vec::new();
        for member in surface.members.iter().filter(|member| member.is_public) {
            let Some(name) = member.published_name.as_deref() else {
                continue;
            };
            rows.push(ProjectedRuntimePropRow {
                name: name.to_owned(),
                optional: member.optional,
                anchor: prop_member_anchor(mac, macro_index_value, name),
                member_dependency: member
                    .referenced_type_name
                    .as_deref()
                    .is_some_and(|name| member_dependency_names.contains(name)),
                referenced_type_name: member.referenced_type_name.clone(),
            });
        }
        let defaults_macro_index =
            containing_with_defaults_index(&analysis.macros, macro_index_value);
        let defaults_association =
            defaults_macro_index.map_or(PropsDefaultsAssociation::None, |index| {
                PropsDefaultsAssociation::WithDefaults {
                    payload_macro_index: macro_index(macro_index_value),
                    defaults_macro_index: macro_index(index),
                }
            });
        NonFlowOutcome::Complete(NonFlowPayload::RuntimePropsProjection(
            RuntimePropsProjection {
                owner_canonical: Arc::clone(owner_canonical),
                macro_index: macro_index_value,
                rows,
                defaults_association,
                member_dependency_names: {
                    let mut names: Vec<String> = member_dependency_names
                        .into_iter()
                        .map(str::to_owned)
                        .collect();
                    names.sort();
                    names.dedup();
                    names
                },
            },
        ))
    }

    fn project_runtime_emits(
        &self,
        owner_canonical: &Arc<str>,
        macro_index_value: usize,
    ) -> NonFlowOutcome {
        let operation = NonFlowOperation::ProjectRuntimeEmits {
            owner_canonical: Arc::clone(owner_canonical),
            macro_index: macro_index_value,
        };
        let Some(analysis) = self.script_analysis(owner_canonical) else {
            return self.need_inputs(&operation);
        };
        let Some(mac) = self.macro_row(analysis, macro_index_value) else {
            return self.need_inputs(&operation);
        };
        if mac.kind != AnalyzedMacroKind::DefineEmits {
            return self.need_inputs(&operation);
        }
        let Some(surface) = self
            .snapshot
            .macro_surface(owner_canonical, macro_index_value)
        else {
            return self.need_inputs(&operation);
        };
        let effective_index = containing_with_defaults_index(&analysis.macros, macro_index_value)
            .unwrap_or(macro_index_value);
        // The filtered semantic surface is the sole event-membership
        // authority; authored fields only anchor names it admitted, and
        // one name is admitted once, on first observation.
        let mut emits = Vec::new();
        let push_emit = |rows: &mut Vec<RuntimeEmit>, name: &str, anchor: MacroAnchor| {
            if rows.iter().any(|row| row.name == name) {
                return;
            }
            rows.push(RuntimeEmit {
                name: name.to_owned(),
                anchor,
            });
        };
        for name in &surface.call_signature_event_names {
            push_emit(
                &mut emits,
                name,
                emit_member_anchor(mac, macro_index_value, effective_index, name),
            );
        }
        for member in surface.members.iter().filter(|member| member.is_public) {
            let Some(name) = member.published_name.as_deref() else {
                continue;
            };
            push_emit(
                &mut emits,
                name,
                emit_member_anchor(mac, macro_index_value, effective_index, name),
            );
        }
        emits.sort_by_key(|row| authored_emit_order(row.anchor));
        NonFlowOutcome::Complete(NonFlowPayload::RuntimeEmitsProjection(
            RuntimeEmitsProjection {
                owner_canonical: Arc::clone(owner_canonical),
                macro_index: macro_index_value,
                emits,
            },
        ))
    }

    fn project_runtime_model(
        &self,
        owner_canonical: &Arc<str>,
        macro_index_value: usize,
    ) -> NonFlowOutcome {
        let operation = NonFlowOperation::ProjectRuntimeModel {
            owner_canonical: Arc::clone(owner_canonical),
            macro_index: macro_index_value,
        };
        let Some(analysis) = self.script_analysis(owner_canonical) else {
            return self.need_inputs(&operation);
        };
        let Some(mac) = self.macro_row(analysis, macro_index_value) else {
            return self.need_inputs(&operation);
        };
        if mac.kind != AnalyzedMacroKind::DefineModel {
            return self.need_inputs(&operation);
        }
        let Some(value_type_shape) = self
            .snapshot
            .model_value_type_shape(owner_canonical, macro_index_value)
        else {
            return self.need_inputs(&operation);
        };
        let effective_index = containing_with_defaults_index(&analysis.macros, macro_index_value)
            .unwrap_or(macro_index_value);
        let dto_macro_index = macro_index(effective_index);
        let name = mac.model_name.as_deref().unwrap_or("modelValue");
        let modifiers_name = if name == "modelValue" {
            "modelModifiers".to_owned()
        } else {
            format!("{name}Modifiers")
        };
        let optional = mac
            .prop_fields
            .first()
            .is_none_or(|field| field.is_optional);
        NonFlowOutcome::Complete(NonFlowPayload::RuntimeModelProjection(
            RuntimeModelProjection {
                owner_canonical: Arc::clone(owner_canonical),
                macro_index: macro_index_value,
                shape: ModelRuntimeShape {
                    prop: RuntimeProp {
                        name: name.to_owned(),
                        optional,
                        type_shape: value_type_shape.clone(),
                        anchor: MacroAnchor::Synthesized {
                            macro_index: dto_macro_index,
                            row: SynthesizedRowKind::ModelProp,
                        },
                    },
                    update_event: RuntimeEmit {
                        name: format!("update:{name}"),
                        anchor: MacroAnchor::Synthesized {
                            macro_index: dto_macro_index,
                            row: SynthesizedRowKind::ModelUpdateEvent,
                        },
                    },
                    modifiers_prop: RuntimeProp {
                        name: modifiers_name,
                        optional: true,
                        type_shape: RuntimePropType::Resolved {
                            constructors: Default::default(),
                            skip_check: false,
                        },
                        anchor: MacroAnchor::Synthesized {
                            macro_index: dto_macro_index,
                            row: SynthesizedRowKind::ModelModifiersProp,
                        },
                    },
                },
            },
        ))
    }

    fn project_expose_surface(
        &self,
        owner_canonical: &Arc<str>,
        macro_index_value: usize,
    ) -> NonFlowOutcome {
        let operation = NonFlowOperation::ProjectExposeSurface {
            owner_canonical: Arc::clone(owner_canonical),
            macro_index: macro_index_value,
        };
        let Some(analysis) = self.script_analysis(owner_canonical) else {
            return self.need_inputs(&operation);
        };
        let Some(mac) = self.macro_row(analysis, macro_index_value) else {
            return self.need_inputs(&operation);
        };
        if mac.kind != AnalyzedMacroKind::DefineExpose {
            return self.need_inputs(&operation);
        }
        let rows = mac
            .expose_fields
            .iter()
            .enumerate()
            .map(|(field_index, field)| ProjectedExposeRow {
                name: field.name.clone(),
                referenced_binding: field.referenced_binding.clone(),
                anchor: expose_member_anchor(mac, macro_index_value, field_index),
            })
            .collect();
        NonFlowOutcome::Complete(NonFlowPayload::ExposeSurfaceProjection(
            ExposeSurfaceProjection {
                owner_canonical: Arc::clone(owner_canonical),
                macro_index: macro_index_value,
                rows,
            },
        ))
    }
}

/// The member-tier dependency names one props macro's members degrade on.
fn member_dependency_names<'a>(
    analysis: &'a ScriptAnalysisSnapshot,
    payload_index: usize,
    mac: &AnalyzedMacro,
) -> BTreeSet<&'a str> {
    analysis
        .macro_type_deps
        .iter()
        .filter(|dependency| {
            dependency.macro_index == payload_index && dependency.macro_span == mac.span
        })
        .filter(|dependency| {
            matches!(
                dependency.usage,
                MacroTypeDepUsage::Member | MacroTypeDepUsage::ValueQueryMember
            )
        })
        .map(|dependency| dependency.type_name.as_str())
        .collect()
}

/// The surface-tier missing-root rows one macro's analysis recorded.
fn surface_dependency_failures(
    analysis: &ScriptAnalysisSnapshot,
    payload_index: usize,
    mac: &AnalyzedMacro,
) -> Vec<VueMacroMissingRoot> {
    analysis
        .macro_type_deps
        .iter()
        .filter(|dependency| {
            dependency.macro_index == payload_index
                && dependency.macro_span == mac.span
                && dependency.usage.is_surface()
        })
        .map(|dependency| VueMacroMissingRoot {
            macro_index: payload_index,
            import_source: dependency.import_source.clone(),
            type_name: dependency.type_name.clone(),
            macro_span: (dependency.macro_span.start, dependency.macro_span.end),
        })
        .collect()
}
