//! The sealed type-info gateway of the compile transaction.
//!
//! [`CompileTypeInfo`] is concrete and sealed: it is constructed only by
//! [`super::CompileAttempt`](super::CompileAttempt) and exposes exactly
//! the six projection methods of the ratified C2 entry contract. Each
//! method drives the semantic kernel
//! (`verter_semantic::type_info::TypeInfoCore::attempt`) over the
//! observations staged on the transaction — it forwards nothing to a
//! host, a scheduler, a session callback, or any other authority
//! (`C2-GAP3-NO-FORWARDING-FACADE`).
//!
//! The request-local continuation is this module's private state and is
//! never exposed: no public continuation DTO exists, and nothing
//! outlives the transaction that minted it.

use std::collections::BTreeMap;
use std::sync::Arc;

use verter_macro_dto::RuntimePropType;
use verter_semantic::analysis::ScriptAnalysisSnapshot;
use verter_semantic::resolver_core::{AttemptOutcome, InputKey, ResolutionBasis};
use verter_semantic::type_info::{
    ExposeSurfaceProjection, ImportedComponentResolution, ImportedComponentSurface,
    NonFlowObservationKey, NonFlowObservationSnapshot, NonFlowOperation, NonFlowPayload,
    ObservedMacroSurface, RuntimeEmitsProjection, RuntimeModelProjection, RuntimePropsProjection,
    TypeInfoCore, VueMacroSemanticInput,
};

/// How a type-info route can fail. The proof id is the operation's
/// stable missing-input identifier (`C2-GAP3-MISSING-*`); the keys are
/// the kernel's own single derivation of what to load next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeInfoRouteFailure {
    /// The operation's inputs are not staged; the driver stages them and
    /// re-enters the same route.
    NeedInputs {
        /// The operation's stable missing-input proof id.
        proof_id: &'static str,
        /// The load set the kernel derived for the missing inputs.
        keys: Vec<InputKey>,
    },
    /// A re-entered operation demanded nothing new — the driver cannot
    /// make progress and the sealed output was discarded.
    NoProgress {
        /// The operation's stable missing-input proof id.
        proof_id: &'static str,
    },
    /// The kernel reported a terminal failure; the sealed output was
    /// discarded.
    Terminal,
}

/// The private request-local continuation identity
/// (`C2-AC-C1-A6-CONTINUATION-001`): binds one operation's
/// execution to the request that drove it, the originating observation
/// state, the resolution basis, the canonical frontier order, and every
/// consumed observation version. Sealed unpublished output rides here
/// and is discarded — never served — whenever any bound fact changes.
struct RequestContinuation {
    operation: NonFlowOperation,
    request_nonce: u64,
    originating: Vec<(NonFlowObservationKey, u64)>,
    basis: ResolutionBasis,
    consumed: Vec<(NonFlowObservationKey, u64)>,
    demanded: Vec<InputKey>,
    sealed: Option<NonFlowPayload>,
}

impl RequestContinuation {
    /// Whether the CURRENT observation state is exactly the originating
    /// one — same keys, same canonical frontier order, same versions —
    /// AND every consumed observation version still matches. Any changed,
    /// appeared, disappeared, reordered, or reconfigured input answers
    /// `false` and forces a whole-operation restart.
    fn current_matches_originating(&self, current: &[(NonFlowObservationKey, u64)]) -> bool {
        self.originating == current
            && self.consumed.iter().all(|(key, version)| {
                current.iter().any(|(current_key, current_version)| {
                    current_key == key && current_version == version
                })
            })
    }
}

/// The sealed type-info gateway. Constructed only by
/// [`super::CompileAttempt`](super::CompileAttempt).
pub struct CompileTypeInfo {
    snapshot: NonFlowObservationSnapshot,
    versions: BTreeMap<NonFlowObservationKey, u64>,
    request_nonce: u64,
    basis: ResolutionBasis,
    continuation: Option<RequestContinuation>,
}

impl std::fmt::Debug for CompileTypeInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The continuation identity (request nonce, sealed state) is
        // request-local and deliberately absent from the debug surface.
        f.debug_struct("CompileTypeInfo")
            .field("staged_observations", &self.versions.len())
            .finish()
    }
}

impl CompileTypeInfo {
    pub(super) fn new(basis: ResolutionBasis) -> Self {
        Self {
            snapshot: NonFlowObservationSnapshot::new(),
            versions: BTreeMap::new(),
            request_nonce: 0,
            basis,
            continuation: None,
        }
    }

    pub(super) fn bind_request_nonce(&mut self, request_nonce: u64) {
        self.request_nonce = request_nonce;
    }

    fn bump_version(&mut self, key: NonFlowObservationKey) {
        let next = self.versions.get(&key).map_or(1, |v| v.saturating_add(1));
        self.versions.insert(key, next);
    }

    pub(super) fn stage_script_analysis(
        &mut self,
        owner_canonical: Arc<str>,
        analysis: Arc<ScriptAnalysisSnapshot>,
    ) {
        self.snapshot
            .stage_script_analysis(owner_canonical.clone(), analysis);
        self.bump_version(NonFlowObservationKey::ScriptAnalysis { owner_canonical });
    }

    pub(super) fn stage_import_resolution(
        &mut self,
        owner_canonical: Arc<str>,
        resolution: ImportedComponentResolution,
    ) {
        let key = NonFlowObservationKey::ImportResolution {
            owner_canonical,
            specifier: Arc::clone(&resolution.specifier),
        };
        self.snapshot
            .stage_import_resolution(key.owner_canonical().clone(), resolution);
        self.bump_version(key);
    }

    pub(super) fn stage_macro_surface(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
        surface: ObservedMacroSurface,
    ) {
        self.snapshot
            .stage_macro_surface(Arc::clone(&owner_canonical), macro_index, surface);
        self.bump_version(NonFlowObservationKey::MacroSurface {
            owner_canonical,
            macro_index,
        });
    }

    pub(super) fn stage_model_value_type_shape(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
        value_type_shape: RuntimePropType,
    ) {
        self.snapshot.stage_model_value_type_shape(
            Arc::clone(&owner_canonical),
            macro_index,
            value_type_shape,
        );
        self.bump_version(NonFlowObservationKey::ModelValueTypeShape {
            owner_canonical,
            macro_index,
        });
    }

    pub(super) fn cancel(&mut self) {
        self.continuation = None;
    }

    /// The current observation state as the ordered (key, version) list
    /// the continuation binds — canonical frontier order with versions.
    fn current_state(&self) -> Vec<(NonFlowObservationKey, u64)> {
        self.snapshot
            .observation_frontier()
            .into_iter()
            .map(|key| {
                let version = self.versions.get(&key).copied().unwrap_or(0);
                (key, version)
            })
            .collect()
    }

    /// Drive one operation through the kernel under the request-local
    /// continuation contract.
    ///
    /// Round flow: the FIRST execution binds the continuation identity
    /// (operation, request nonce, originating observation state, basis)
    /// and keeps it across `NeedInputs` rounds while the driver stages
    /// inputs. A later round whose observation state differs from the
    /// originating one in ANY way — changed, appeared, disappeared,
    /// reordered, or reconfigured — discards the continuation (with any
    /// sealed output) and restarts the whole operation under the new
    /// state; only a byte-identical re-request resumes and serves the
    /// sealed output. No-progress (a round demanding nothing new),
    /// terminal failure, and cancellation all discard sealed output.
    fn execute(
        &mut self,
        operation: &NonFlowOperation,
    ) -> Result<NonFlowPayload, TypeInfoRouteFailure> {
        let mut continuation = self.continuation.take();
        let same_operation = continuation.as_ref().is_some_and(|continuation| {
            continuation.operation == *operation
                && continuation.request_nonce == self.request_nonce
                && continuation.basis == self.basis
        });
        if same_operation {
            let matches_originating = continuation
                .as_ref()
                .expect("same_operation implies a continuation")
                .current_matches_originating(&self.current_state());
            if matches_originating {
                let sealed = continuation
                    .as_ref()
                    .and_then(|continuation| continuation.sealed.clone());
                if let Some(sealed) = sealed {
                    self.continuation = continuation;
                    return Ok(sealed);
                }
                // First round had no sealed output; fall through and run.
            } else {
                // A changed, appeared, disappeared, reordered, or
                // reconfigured input forces a whole-operation restart:
                // the continuation (and any sealed output) is discarded.
                continuation = None;
            }
        } else {
            // A different operation or request: the previous
            // continuation is not resumable here.
            continuation = None;
        }

        let originating = self.current_state();
        let core =
            TypeInfoCore::from_observation_snapshot(Arc::new(self.snapshot.clone()), self.basis);
        let outcome = core.attempt(operation);
        match outcome {
            AttemptOutcome::Complete(payload) => {
                self.continuation = Some(RequestContinuation {
                    operation: operation.clone(),
                    request_nonce: self.request_nonce,
                    originating,
                    basis: self.basis,
                    consumed: self.current_state(),
                    demanded: Vec::new(),
                    sealed: Some(payload.clone()),
                });
                Ok(payload)
            }
            AttemptOutcome::NeedInputs(load_set) => {
                let proof_id = operation.missing_input_proof_id();
                let keys: Vec<InputKey> = load_set.keys().to_vec();
                let demanded = continuation
                    .as_ref()
                    .map_or_else(Vec::new, |continuation| continuation.demanded.clone());
                let any_new = keys.iter().any(|key| !demanded.contains(key));
                if !any_new {
                    // No-progress: discard the sealed output and the
                    // continuation, and refuse the route.
                    return Err(TypeInfoRouteFailure::NoProgress { proof_id });
                }
                let mut demanded = demanded;
                demanded.extend(keys.iter().cloned());
                self.continuation = Some(RequestContinuation {
                    operation: operation.clone(),
                    request_nonce: self.request_nonce,
                    originating: continuation
                        .map_or(originating, |continuation| continuation.originating),
                    basis: self.basis,
                    consumed: self.current_state(),
                    demanded,
                    sealed: None,
                });
                Err(TypeInfoRouteFailure::NeedInputs { proof_id, keys })
            }
            AttemptOutcome::Terminal(_) => {
                self.continuation = None;
                Err(TypeInfoRouteFailure::Terminal)
            }
        }
    }

    /// Derive one SFC's complete macro semantic input plan.
    pub fn project_vue_macro_semantics(
        &mut self,
        owner_canonical: Arc<str>,
    ) -> Result<VueMacroSemanticInput, TypeInfoRouteFailure> {
        let operation = NonFlowOperation::ProjectVueMacroSemantics { owner_canonical };
        match self.execute(&operation)? {
            NonFlowPayload::VueMacroSemanticInput(payload) => Ok(payload),
            _ => unreachable!("the kernel's payload follows its operation"),
        }
    }

    /// Resolve one authored type reference against the owner's observed
    /// import surface.
    pub fn resolve_imported_component_surface(
        &mut self,
        owner_canonical: Arc<str>,
        type_reference: Arc<str>,
        referenced_canonical: Option<Arc<str>>,
    ) -> Result<ImportedComponentSurface, TypeInfoRouteFailure> {
        let operation = NonFlowOperation::ResolveImportedComponentSurface {
            owner_canonical,
            type_reference,
            referenced_canonical,
        };
        match self.execute(&operation)? {
            NonFlowPayload::ImportedComponentSurface(payload) => Ok(payload),
            _ => unreachable!("the kernel's payload follows its operation"),
        }
    }

    /// Shape one `defineProps` macro's runtime prop rows over the
    /// observed member surface.
    pub fn project_runtime_props(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
    ) -> Result<RuntimePropsProjection, TypeInfoRouteFailure> {
        let operation = NonFlowOperation::ProjectRuntimeProps {
            owner_canonical,
            macro_index,
        };
        match self.execute(&operation)? {
            NonFlowPayload::RuntimePropsProjection(payload) => Ok(payload),
            _ => unreachable!("the kernel's payload follows its operation"),
        }
    }

    /// Shape one `defineEmits` macro's runtime emit rows over the
    /// observed event-name surface.
    pub fn project_runtime_emits(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
    ) -> Result<RuntimeEmitsProjection, TypeInfoRouteFailure> {
        let operation = NonFlowOperation::ProjectRuntimeEmits {
            owner_canonical,
            macro_index,
        };
        match self.execute(&operation)? {
            NonFlowPayload::RuntimeEmitsProjection(payload) => Ok(payload),
            _ => unreachable!("the kernel's payload follows its operation"),
        }
    }

    /// Synthesize one `defineModel` macro's runtime model shape.
    pub fn project_runtime_model(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
    ) -> Result<RuntimeModelProjection, TypeInfoRouteFailure> {
        let operation = NonFlowOperation::ProjectRuntimeModel {
            owner_canonical,
            macro_index,
        };
        match self.execute(&operation)? {
            NonFlowPayload::RuntimeModelProjection(payload) => Ok(payload),
            _ => unreachable!("the kernel's payload follows its operation"),
        }
    }

    /// Shape one runtime-object `defineExpose` macro's exposed member
    /// rows.
    pub fn project_expose_surface(
        &mut self,
        owner_canonical: Arc<str>,
        macro_index: usize,
    ) -> Result<ExposeSurfaceProjection, TypeInfoRouteFailure> {
        let operation = NonFlowOperation::ProjectExposeSurface {
            owner_canonical,
            macro_index,
        };
        match self.execute(&operation)? {
            NonFlowPayload::ExposeSurfaceProjection(payload) => Ok(payload),
            _ => unreachable!("the kernel's payload follows its operation"),
        }
    }
}
