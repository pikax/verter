//! Session-composed suites for the semantic graph store.
//!
//! Each suite drives the store through a live session host or
//! session-owned fixtures, so it lives with the session rather than beside
//! the store.

use crate::semantic_query::demand::{MaterializedPoint, MaterializedSet, ProjectionPath};
use crate::semantic_query::{
    DepSignature, NodeScopeId, OriginEdgeKind, QueryError, QueryResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey, SemanticQueryValue,
};
use crate::semantic_query::{PathSegment, ProjectionMode, SemanticGraphStats};
use crate::semantic_query_memo::arena::{shard_index_for, NUM_SHARDS};
use crate::semantic_query_memo::family::{carrier_facts_reference_canonical, FamilyKey, ModeSlot};
use crate::semantic_query_memo::inflight::FlightCell;
use crate::semantic_query_memo::inflight::MAX_INFLIGHT_RETRIES;
use crate::semantic_query_memo::interner::DepSignatureInterner;
use crate::semantic_query_memo::interner::SWEEP_INTERVAL;
use crate::semantic_query_memo::scc_publish::{
    PendingFlowReturnMember, PendingRelationMember, SccRootWitness,
};
use crate::semantic_query_memo::test_support::test_trigger_inflight_abort;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[allow(unused_imports)]
use crate::semantic_query_memo::*;

mod cancellation_tests;
mod object_spread_projection_tests;
mod producer_tests;
mod scc_publish_tests;
mod tasks_tests;
pub(crate) mod tests;
