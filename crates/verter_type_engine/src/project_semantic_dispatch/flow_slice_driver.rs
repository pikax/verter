//! Request-local flow artifact execution. Shared stores expose claims and
//! immutable products; this driver alone requests owned source lowering.
use verter_session_query::flow::bundle::FlowSliceFunctionKey;
use verter_session_query::flow::bundle::{FlowGraphBundle, FlowGraphBundleRef};

use crate::cache_runtime::flow_slice_node::*;
use crate::cache_runtime::node::{ArtifactNode, ComputeCtx, QueryFlightKey};
use crate::cache_runtime::singleflight::InflightTable;
use crate::cache_runtime::{CacheAdmission, CacheEntry, NonAdmissionReason};
use crate::resolver_core::fact_validation_port::FactValidation;
use crate::resolver_core::request_ports::OwnedLowering;
use crate::resolver_core::ResolverContext;
use dashmap::DashMap;
use std::sync::Arc;
use verter_session_query::facts::fact_cache::ReadSetSignature;
use verter_session_query::flow::binding::FlowBindingMapError;
use verter_session_query::flow::flow_ir::SharedFlowSliceIR;
use verter_session_query::flow::hashing::compute_flow_slice_hash;
use verter_session_query::flow::lower::lower_slice_plan;
use verter_session_query::flow::peeker::{ReturnPathPeeker, SliceDemand};

/// Fixture acquisition belongs to the test driver, never shared storage.
#[cfg(any(test, feature = "test-support"))]
pub trait FlowBodySkeletonSource {
    fn build_bundle(
        &self,
        key: &FlowSliceFunctionKey,
    ) -> Result<Option<FlowGraphBundle>, FlowBindingMapError>;
}

pub struct FlowSliceDriver<'a> {
    stores: &'a FlowSliceStores,
    lowering: &'a dyn OwnedLowering,
    facts: &'a dyn FactValidation,
    #[cfg(any(test, feature = "test-support"))]
    fixture: Option<&'a dyn FlowBodySkeletonSource>,
    flags: &'a crate::resolver_core::resolver_context::RequestFlags,
}

impl<'a> FlowSliceDriver<'a> {
    pub fn new<C: crate::resolver_core::ResolverCapabilities>(
        stores: &'a FlowSliceStores,
        ctx: &'a dyn ResolverContext<C>,
        flags: &'a crate::resolver_core::resolver_context::RequestFlags,
    ) -> Self {
        Self::from_ports(stores, ctx, ctx, flags)
    }
    pub(crate) fn from_ports(
        stores: &'a FlowSliceStores,
        lowering: &'a dyn OwnedLowering,
        facts: &'a dyn FactValidation,
        flags: &'a crate::resolver_core::resolver_context::RequestFlags,
    ) -> Self {
        Self {
            stores,
            lowering,
            facts,
            flags,
            #[cfg(any(test, feature = "test-support"))]
            fixture: None,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn with_fixture(mut self, fixture: &'a dyn FlowBodySkeletonSource) -> Self {
        self.fixture = Some(fixture);
        self
    }

    pub fn graph_bundle(
        &self,
        key: &FlowSliceFunctionKey,
    ) -> Result<Option<FlowGraphBundleRef>, FlowBindingMapError> {
        if self.flags.is_cancelled() {
            crate::request_context::observe_execution_abort(
                crate::semantic_query::ExecutionAbort::Cancelled,
            );
            return Ok(None);
        }
        match self.stores.graphs.claim(key) {
            GraphClaim::Read(bundle) => Ok(Some(bundle)),
            GraphClaim::Produce(lease) => {
                #[cfg(any(test, feature = "test-support"))]
                if let Some(fixture) = self.fixture {
                    return Ok(fixture
                        .build_bundle(key)?
                        .and_then(|bundle| lease.publish(bundle)));
                }
                let Some(structure) = self.lowering.prepare_function_structure(key)? else {
                    return Ok(None);
                };
                Ok(lease.publish(FlowGraphBundle::build(structure)))
            }
        }
    }

    pub fn skeleton_for(
        &self,
        key: &FlowSliceFunctionKey,
    ) -> Option<verter_session_query::flow::bundle::SkeletonRef> {
        self.graph_bundle(key)
            .ok()
            .flatten()
            .map(|bundle| verter_session_query::flow::bundle::SkeletonRef::of(&bundle))
    }

    pub fn lookup_hash(&self, key: FlowSliceHashKey) -> Option<FlowSliceHashOutcome> {
        crate::cache_runtime::lookup(
            &FlowSliceHashNodeDemand { driver: self },
            key,
            self.facts,
            self.flags,
        )
    }

    /// The lowered slice of `planned`, the plan the hash node delivered
    /// under `key`'s slice hash.
    pub fn lookup_lowered(
        &self,
        key: FlowSliceLoweredKey,
        planned: &verter_session_query::retention::heap_size::OwnedTree<PlannedFlowSlice>,
    ) -> Option<SharedFlowSliceIR> {
        crate::cache_runtime::lookup(
            &FlowSliceLoweredBodyNodeDemand {
                driver: self,
                planned,
            },
            key,
            self.facts,
            self.flags,
        )
    }
}

struct FlowSliceHashNodeDemand<'a> {
    driver: &'a FlowSliceDriver<'a>,
}
struct FlowSliceLoweredBodyNodeDemand<'a> {
    driver: &'a FlowSliceDriver<'a>,
    /// The plan the hash node delivered for this slice: the one cold
    /// planning run, shared with the hash node's entry when it retained it.
    planned: &'a verter_session_query::retention::heap_size::OwnedTree<PlannedFlowSlice>,
}

impl ArtifactNode for FlowSliceHashNodeDemand<'_> {
    type Key = FlowSliceHashKey;
    type Value = FlowSliceHashOutcome;

    fn entries(&self) -> &DashMap<Self::Key, Arc<CacheEntry<Self::Value>>> {
        &self.driver.stores.hash_node().entries
    }

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.driver.stores.hash_node().inflight
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        let Ok(Some(bundle)) = self.driver.graph_bundle(&key.function) else {
            return CacheAdmission::Failed {
                reason: NonAdmissionReason::ComputeFailed,
            };
        };
        let demand =
            SliceDemand::for_return_projection(bundle.skeleton(), &key.demand.projection_path);
        let peeker = ReturnPathPeeker::new(bundle.graph());
        let budget = *self.driver.stores.hash_node().budget.read();
        match peeker.plan(&demand, &budget) {
            Err(exceeded) => CacheAdmission::ReturnOnly {
                value: FlowSliceHashOutcome::BudgetExceeded(exceeded),
                reason: NonAdmissionReason::BudgetExceeded,
            },
            Ok(plan) => {
                let slice_hash = compute_flow_slice_hash(&plan, bundle.graph(), bundle.skeleton());
                CacheAdmission::Cacheable {
                    value: FlowSliceHashOutcome::Planned(
                        verter_session_query::retention::heap_size::owned_tree(
                            PlannedFlowSlice::new(slice_hash, plan),
                        ),
                    ),
                    // Content-addressed: the key pins every input, so the
                    // fact rail stays EMPTY — no slice identity ever
                    // enters `ReadSetSignature.facts`.
                    signature: ReadSetSignature::empty(),
                    self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
                    validated_at_generation: cx.generation(),
                }
            }
        }
    }

    fn entry_footprint(
        &self,
        key: &Self::Key,
        value: &Self::Value,
    ) -> verter_session_query::retention::Footprint {
        verter_session_query::retention::heap_size::footprint(&(key, value))
    }

    fn retention_account(&self) -> Arc<verter_session_query::retention::SemanticRetentionAccount> {
        self.driver.stores.retention_account()
    }

    /// Content-addressed warm validity: the key pins the canonical, the
    /// function identity, the body content hash, the parse env, the
    /// exact parse identity and file language row, and the demand — key
    /// identity IS validity, so a
    /// published entry serves across generations (like every
    /// content-addressed artifact family).
    fn validate(
        &self,
        _key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        _cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        Some(entry.value.clone())
    }
}

impl ArtifactNode for FlowSliceLoweredBodyNodeDemand<'_> {
    type Key = FlowSliceLoweredKey;
    type Value = SharedFlowSliceIR;

    fn entries(&self) -> &DashMap<Self::Key, Arc<CacheEntry<Self::Value>>> {
        &self.driver.stores.lowered_node().entries
    }

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.driver.stores.lowered_node().inflight
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        let Ok(Some(bundle)) = self.driver.graph_bundle(&key.hash_key.function) else {
            return CacheAdmission::Failed {
                reason: NonAdmissionReason::ComputeFailed,
            };
        };
        // Lower EXACTLY the plan the hash node delivered: planning runs
        // once per cold demand, so this node never re-plans and never
        // computes a slice hash. The plan reaches it whether or not the hash
        // node could retain it — a plan for another slice is a torn view,
        // a typed miss, never a re-plan under a different demand.
        let planned = self.planned;
        if planned.hash() != key.slice_hash {
            return CacheAdmission::Failed {
                reason: NonAdmissionReason::ComputeFailed,
            };
        }
        CacheAdmission::Cacheable {
            value: SharedFlowSliceIR::new(lower_slice_plan(
                planned.selection(),
                bundle.graph(),
                bundle.skeleton(),
            )),
            signature: ReadSetSignature::empty(),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            validated_at_generation: cx.generation(),
        }
    }

    fn entry_footprint(
        &self,
        key: &Self::Key,
        value: &Self::Value,
    ) -> verter_session_query::retention::Footprint {
        verter_session_query::retention::heap_size::footprint(&(key, value))
    }

    fn retention_account(&self) -> Arc<verter_session_query::retention::SemanticRetentionAccount> {
        self.driver.stores.retention_account()
    }

    /// Content-addressed warm validity — see
    /// [`FlowSliceHashNode::validate`].
    fn validate(
        &self,
        _key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        _cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        Some(entry.value.clone())
    }
}
