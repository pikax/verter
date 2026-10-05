//! Request-local flow artifact execution. Shared stores expose claims and
//! immutable products; this driver alone requests owned source lowering.
use verter_session_query::flow::bundle::FlowGraphBundle;
use verter_session_query::flow::bundle::FlowSliceFunctionKey;

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
use verter_session_query::flow::flow_ir::FlowSliceIR;
use verter_session_query::flow::hashing::compute_flow_slice_hash;
use verter_session_query::flow::lower::lower_slice_plan;
use verter_session_query::flow::peeker::{ReturnPathPeeker, SliceDemand};
use verter_session_query::flow::{binding::FlowBindingMapError, skeleton::FunctionBodySkeleton};

/// Fixture acquisition belongs to the test driver, never shared storage.
#[cfg(test)]
pub(crate) trait FlowBodySkeletonSource {
    fn build_bundle(
        &self,
        key: &FlowSliceFunctionKey,
    ) -> Result<Option<FlowGraphBundle>, FlowBindingMapError>;
}

pub(crate) struct FlowSliceDriver<'a> {
    stores: &'a FlowSliceStores,
    lowering: &'a dyn OwnedLowering,
    facts: &'a dyn FactValidation,
    #[cfg(test)]
    fixture: Option<&'a dyn FlowBodySkeletonSource>,
}

impl<'a> FlowSliceDriver<'a> {
    pub(crate) fn new<C: crate::resolver_core::ResolverCapabilities>(
        stores: &'a FlowSliceStores,
        ctx: &'a dyn ResolverContext<C>,
    ) -> Self {
        Self::from_ports(stores, ctx, ctx)
    }
    pub(crate) fn from_ports(
        stores: &'a FlowSliceStores,
        lowering: &'a dyn OwnedLowering,
        facts: &'a dyn FactValidation,
    ) -> Self {
        Self {
            stores,
            lowering,
            facts,
            #[cfg(test)]
            fixture: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_fixture(mut self, fixture: &'a dyn FlowBodySkeletonSource) -> Self {
        self.fixture = Some(fixture);
        self
    }

    pub(crate) fn graph_bundle(
        &self,
        key: &FlowSliceFunctionKey,
    ) -> Result<Option<Arc<FlowGraphBundle>>, FlowBindingMapError> {
        match self.stores.graphs.claim(key) {
            GraphClaim::Read(bundle) => Ok(Some(bundle)),
            GraphClaim::Produce(lease) => {
                #[cfg(test)]
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

    pub(crate) fn skeleton_for(
        &self,
        key: &FlowSliceFunctionKey,
    ) -> Option<Arc<FunctionBodySkeleton>> {
        self.graph_bundle(key)
            .ok()
            .flatten()
            .map(|bundle| Arc::clone(bundle.skeleton()))
    }

    pub(crate) fn lookup_hash(&self, key: FlowSliceHashKey) -> Option<FlowSliceHashOutcome> {
        crate::cache_runtime::lookup(&FlowSliceHashNodeDemand { driver: self }, key, self.facts)
    }

    pub(crate) fn lookup_lowered(&self, key: FlowSliceLoweredKey) -> Option<Arc<FlowSliceIR>> {
        crate::cache_runtime::lookup(
            &FlowSliceLoweredBodyNodeDemand { driver: self },
            key,
            self.facts,
        )
    }
}

struct FlowSliceHashNodeDemand<'a> {
    driver: &'a FlowSliceDriver<'a>,
}
struct FlowSliceLoweredBodyNodeDemand<'a> {
    driver: &'a FlowSliceDriver<'a>,
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
                    value: FlowSliceHashOutcome::Planned(Arc::new(PlannedFlowSlice::new(
                        slice_hash, plan,
                    ))),
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
    type Value = Arc<FlowSliceIR>;

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
        // Lower EXACTLY the plan the hash node planned and retained on its
        // published outcome: planning runs once per cold demand, so this
        // node never re-plans and never computes a slice hash. An absent
        // or evicted retained plan is a torn view — a typed miss, never a
        // re-plan under a different demand.
        let Some(planned) = self
            .driver
            .stores
            .hash_node()
            .retained_plan(&key.hash_key, key.slice_hash)
        else {
            return CacheAdmission::Failed {
                reason: NonAdmissionReason::ComputeFailed,
            };
        };
        CacheAdmission::Cacheable {
            value: Arc::new(lower_slice_plan(
                planned.selection(),
                bundle.graph(),
                bundle.skeleton(),
            )),
            signature: ReadSetSignature::empty(),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            validated_at_generation: cx.generation(),
        }
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
