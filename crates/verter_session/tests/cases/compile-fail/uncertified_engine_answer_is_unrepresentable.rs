//! Compile-fail fixture: a `BoundProject` witness is NOT a route to an engine
//! answer.
//!
//! The production-result ops (`publish_snapshot` / `query` / `diagnostics`)
//! take a `CertifiedTypeEngineBinding`. Passing the bare bound-project witness
//! — which certifies nothing about an observed capability interpretation, the
//! serving session, or the input basis — is the uncertified route the dual-plane
//! contract removes. If the op signatures were widened back to `&BoundProject`,
//! this fixture would COMPILE and trybuild would turn red.

use std::sync::Arc;

use verter_session_query::analysis::types::Hash16;
use verter_session::external_ts::{
    BoundProject, EngineBackend, EngineCapabilities, EngineError, OpenState, PublishSnapshot,
    Query, QueryFeature, QueryOutcome, ScriptKind, SnapshotFile, SnapshotRole,
};

/// A backend whose production-result ops are the seam under test.
struct Seam;

impl EngineBackend for Seam {
    fn ensure_project(
        &self,
        _request: verter_session::external_ts::EnsureProject,
    ) -> Result<BoundProject, EngineError> {
        unreachable!("the fixture never reaches ensure_project")
    }

    fn publish_snapshot(
        &self,
        _project: &BoundProject,
        _snapshot: PublishSnapshot,
    ) -> Result<(), EngineError> {
        unreachable!("the uncertified publish never compiles")
    }

    fn query(&self, _project: &BoundProject, _query: Query) -> Result<QueryOutcome, EngineError> {
        unreachable!("the uncertified query never compiles")
    }

    fn diagnostics(
        &self,
        _project: &BoundProject,
        _request: verter_session::external_ts::Diagnostics,
    ) -> Result<verter_session::external_ts::DiagnosticsOutcome, EngineError> {
        unreachable!("the uncertified diagnostics never compiles")
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities::default()
    }
}

fn carrier_file() -> SnapshotFile {
    SnapshotFile {
        source_uri: Arc::from("file:///ws/src/Foo.vue"),
        provider_uri: Arc::from("file:///ws/src/Foo.vue.tsx"),
        role: SnapshotRole::CarrierIde,
        script_kind: ScriptKind::Tsx,
        content: Arc::from("const a = 1;"),
        content_hash: Hash16::from([1_u8; 16]),
        map_hash: Hash16::from([2_u8; 16]),
        map_json: None,
        structure: None,
        version: 7,
        open_state: OpenState::Closed,
    }
}

/// The seal is crate-internal, so the fixture names the witness as a parameter
/// rather than minting one — the same technique the struct-literal forge uses.
fn publish_uncertified(bound: &BoundProject) -> Result<(), EngineError> {
    Seam.publish_snapshot(
        bound,
        PublishSnapshot {
            project: Arc::from("file:///ws/tsconfig.json"),
            files: vec![carrier_file()],
            resolution_map_version: 0,
            fs_generation: 0,
        },
    )
}

/// And the query lane is closed the same way.
fn query_uncertified(bound: &BoundProject, feature: QueryFeature) -> Result<QueryOutcome, EngineError> {
    Seam.query(
        bound,
        Query {
            project: Arc::from("file:///ws/tsconfig.json"),
            provider_uri: Arc::from("file:///ws/src/Foo.vue.tsx"),
            carrier_offset: 0,
            feature,
            content_hash: Hash16::from([1_u8; 16]),
            map_hash: Hash16::from([2_u8; 16]),
            required_version: 7,
        },
    )
}

fn main() {
    let _ = publish_uncertified;
    let _ = query_uncertified;
}
