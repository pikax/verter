//! Session-composed suites for the semantic graph store.
//!
//! Each suite drives the store through a live session host or
//! session-owned fixtures, so it lives with the session rather than beside
//! the store.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use verter_type_engine::semantic_query::demand::{
    MaterializedPoint, MaterializedSet, ProjectionPath,
};
use verter_type_engine::semantic_query::{
    DepSignature, NodeScopeId, QueryError, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SemanticQueryValue,
};
use verter_type_engine::semantic_query::{PathSegment, ProjectionMode};
use verter_type_engine::semantic_query_memo::family::{FamilyKey, ModeSlot};
use verter_type_engine::semantic_query_memo::inflight::FlightCell;
use verter_type_engine::semantic_query_memo::inflight::MAX_INFLIGHT_RETRIES;
use verter_type_engine::semantic_query_memo::scc_publish::{
    PendingFlowReturnMember, PendingRelationMember, SccRootWitness,
};
use verter_type_engine::semantic_query_memo::test_support::test_trigger_inflight_abort;

#[allow(unused_imports)]
use verter_type_engine::semantic_query_memo::*;

mod cancellation_tests;
mod object_spread_projection_tests;
mod producer_tests;
mod scc_publish_tests;
mod tasks_tests;
pub(crate) mod tests;
