//! A resolve-symbol graph request issues exactly one declaration-root
//! reference under the one view it answered from; the reference is found
//! only by its whole tuple, admitted only while its basis holds, and
//! replays to the declaration it was issued for.

use std::sync::Arc;

use verter_protocol::typeinfo::graph::{self as wire, Operation, TypeInfoGraphRequest};
use verter_protocol::verter::v1::{
    graph_closure_policy, type_info_graph_request, type_info_graph_response, SemanticTypeGraph,
};
use verter_type_engine::resolver_core::fact_validation_port::FactValidationView;
use verter_workspace::{
    MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectRank, VfsProjectConfig,
};

use super::*;
use crate::resolver_store::CurrentHostStoreView;
use crate::types::{HostConfig, UpsertRequest};

const ROOT: &str = "/proj";
const ENTRY: &str = "/proj/a.ts";
const DEPENDENCY: &str = "/proj/b.ts";

type Ctx<'a> = crate::resolver_core::HostResolverContext<'a, verter_audit::observe::Uncaptured>;

fn write(workspace: &MemoryWorkspace, host: &VerterHost, canonical: &str, source: &str) {
    workspace.inject_file(canonical.to_string(), Arc::from(source));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert");
}

fn project_host() -> (Arc<MemoryWorkspace>, Arc<VerterHost>) {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: ROOT.to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some(format!("{ROOT}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![".ts".to_string()],
        workspace_root: ROOT.to_string(),
        workspace_aliases: vec![],
        compiler_options: Default::default(),
        references: vec![],
        membership: verter_workspace::configured_membership_match_all_under_root(
            &verter_workspace::CanonicalPath::new(ROOT),
        ),
    }]));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: false,
            ..HostConfig::default()
        },
        Arc::clone(&workspace) as Arc<dyn verter_workspace::WorkspaceAccess>,
    ));
    write(
        &workspace,
        &host,
        DEPENDENCY,
        "export type B = { x: number }\n",
    );
    write(
        &workspace,
        &host,
        ENTRY,
        "import type { B } from './b'\nexport type A = { b: B }\n",
    );
    (workspace, host)
}

fn current(host: &VerterHost) -> CurrentHostStoreView {
    crate::typeinfo::current_store_view_for_query(host).expect("a settled host has a current view")
}

/// A request context over `view`: the view plus a fresh completion
/// overlay.
fn request_ctx<'a>(host: &'a VerterHost, view: &'a CurrentHostStoreView) -> Ctx<'a> {
    Ctx::from_current_in(
        host,
        view,
        Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    )
}

fn envelope(name: &str) -> TypeInfoGraphRequest {
    TypeInfoGraphRequest {
        schema_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: Operation::ResolveSymbol as i32,
        payload: Some(type_info_graph_request::Payload::ResolveSymbol(
            wire::ResolveSymbolGraphRequest {
                canonical_id: ENTRY.to_string(),
                name: name.to_string(),
                context: Some(wire::ProjectionReductionContext {
                    mode: wire::ProjectionMode::Expanded as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(graph_closure_policy::Kind::OneLevel(
                        wire::ClosureOneLevel {},
                    )),
                }),
                display_policy: Some(wire::DisplayPolicy {
                    qualification: wire::DisplayQualification::Qualified as i32,
                    branding: wire::DisplayBranding::On as i32,
                    budgets: Some(wire::DisplayBudgets {
                        max_string_length: 4096,
                        max_depth: 16,
                    }),
                }),
                include_provenance: false,
                include_diagnostics: false,
                include_projection: vec![],
                include_degraded: false,
            },
        )),
    }
}

/// The graph the production executor answers for `name`.
fn answered_graph(host: &VerterHost, name: &str) -> SemanticTypeGraph {
    let (response, _) = host
        .resolve_symbol_graph_with_audit(envelope(name))
        .into_parts();
    match response.expect("the graph arm").kind {
        Some(type_info_graph_response::Kind::Graph(graph)) => graph,
        other => panic!("expected a graph answer, got {other:?}"),
    }
}

/// Run `f` as a request over a fresh capture of `host`: a request
/// context, a request port over the view, a dispatch and its construction
/// scope.
fn in_request<T>(
    host: &VerterHost,
    f: impl FnOnce(
        &CurrentHostStoreView,
        &Ctx<'_>,
        &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> T,
) -> T {
    let view = current(host);
    let ctx = request_ctx(host, &view);
    let request = crate::capture::root_entry_context(
        host,
        host.next_request_id(),
        Arc::from(ENTRY),
        || verter_audit::RequestKind::TypeResolution,
        false,
        false,
        None,
        verter_execution::cancellation::CancellationToken::new(),
    );
    let _request = verter_type_engine::request_context::RequestContextGuard::install(request);
    let dispatch = ProjectSemanticDispatch::new(&ctx);
    let _scope = dispatch.enter_construction_scope();
    f(&view, &ctx, &dispatch)
}

/// The graph a node replays to, exported with the executor's one-level
/// budgets.
fn exported(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    node: SemanticNodeId,
) -> SemanticTypeGraph {
    let budgets = verter_protocol::typeinfo::graph_export::GraphExportBudgets::new(
        crate::typeinfo::request_validation::MAX_EXPANSION_NODE_BUDGET,
        2,
    )
    .expect("in-range budgets");
    crate::typeinfo::raise::project_ordinal_to_semantic_type_graph(dispatch, node, &budgets)
        .expect("the replayed node exports")
}

/// The production executor answers and issues under one captured view:
/// its one declaration-root reference is admitted by a fresh capture of
/// the unchanged host, and replays to the very graph it answered. A
/// repeated request reissues nothing; a dependency edit stales the
/// reference, and the next answer issues a new one under the new basis.
#[test]
fn named_graph_issuance_uses_one_view() {
    let (workspace, host) = project_host();
    let mut answered = answered_graph(&host, "A");
    answered.query = None;
    let keys = issued_keys_for_tests(&host);
    assert_eq!(keys.len(), 1, "one declaration root is issued");
    let key = keys[0];
    assert_eq!(key.role, None, "a declaration root carries no role");

    let issued = lookup_issued(&host, &key).expect("the issued tuple is found");
    in_request(&host, |view, ctx, dispatch| {
        assert_eq!(
            admit_issued_in_view(view.view(), &FactValidationView::new(ctx), &issued),
            Ok(()),
            "an unchanged host admits the basis the answer was issued under"
        );
        let node = replay_issued_in_view(&host, ctx, dispatch, &issued)
            .expect("the admitted reference replays")
            .expect("to its declaration");
        assert_eq!(
            exported(dispatch, node),
            answered,
            "the replay reaches the exported declaration"
        );
    });

    let _ = answered_graph(&host, "A");
    assert_eq!(issued_keys_for_tests(&host), keys, "nothing new is issued");
    assert_eq!(issuance_counters_for_tests(&host).reissued, 1);

    write(
        &workspace,
        &host,
        DEPENDENCY,
        "export type B = { x: string }\n",
    );
    in_request(&host, |view, ctx, _| {
        assert!(
            admit_issued_in_view(view.view(), &FactValidationView::new(ctx), &issued).is_err(),
            "the dependency edit stales the issued basis"
        );
    });
    let _ = answered_graph(&host, "A");
    let keys = issued_keys_for_tests(&host);
    assert_eq!(keys.len(), 2);
    let reissued = keys[1];
    assert_eq!((reissued.query, reissued.slot), (key.query, key.slot));
    assert_ne!(reissued.basis, key.basis, "the new answer is a new basis");
}

/// A fact whose inputs move between its capture and its registration
/// registers nothing, though its trace committed under the captured view.
#[test]
fn capture_export_mutation_registers_nothing() {
    let (workspace, host) = project_host();
    let view = current(&host);
    let (locator, trace) = {
        let ctx = request_ctx(&host, &view);
        let request = crate::capture::root_entry_context(
            &host,
            host.next_request_id(),
            Arc::from(ENTRY),
            || verter_audit::RequestKind::TypeResolution,
            false,
            false,
            None,
            verter_execution::cancellation::CancellationToken::new(),
        );
        let _request = verter_type_engine::request_context::RequestContextGuard::install(request);
        let dispatch = ProjectSemanticDispatch::new(&ctx);
        let _scope = dispatch.enter_construction_scope();
        let (resolved, trace) =
            crate::typeinfo::native_basis::trace_native_fact(&ctx, &[ENTRY], || {
                crate::typeinfo::resolve_named_symbol::resolve_named_symbol_located(
                    &host,
                    &ctx,
                    &dispatch,
                    ENTRY,
                    "A",
                    &[],
                    ProjectionMode::Expanded,
                )
            });
        assert!(matches!(resolved.outcome, Ok(Some(_))));
        (resolved.locator.expect("A is addressable"), trace)
    };

    write(
        &workspace,
        &host,
        DEPENDENCY,
        "export type B = { x: string }\n",
    );
    let ctx = request_ctx(&host, &view);
    let refusal = issue_resolve_symbol(
        &host,
        view.view(),
        &FactValidationView::new(&ctx),
        trace,
        locator,
    )
    .expect_err("a moved input registers nothing");
    assert!(
        matches!(
            refusal,
            IssueRefusal::StaleAtRegistration | IssueRefusal::Basis(NativeBasisRefusal::NotVouched)
        ),
        "{refusal:?}"
    );
    assert!(issued_keys_for_tests(&host).is_empty());
    let counters = issuance_counters_for_tests(&host);
    assert_eq!(counters.registered, 0);
    assert_eq!(
        counters.stale_at_registration
            + counters.basis_refused[basis_refusal_slot(NativeBasisRefusal::NotVouched)],
        1,
        "the refusal is counted"
    );
}

/// A record for `name` with `entries` role-distinct addresses, under one
/// real committed basis.
fn record(basis: &NativeFactBasis, name: &str, entries: usize) -> (NativeFactRecord, IssuedRefKey) {
    let locator = DeclReplayLocator {
        entry_canonical: Arc::from(ENTRY),
        entry_owner: TopLevelOwnerId::ordinary_file(),
        requested_name: Arc::from(name),
        resolved_slot: ResolvedDeclSlotIdentity::type_slot(
            ENTRY,
            TopLevelOwnerId::ordinary_file(),
            name,
            0,
            [0; 16],
            [0; 16],
        ),
        effective_mode: ProjectionMode::Expanded,
        demand: ReductionDemand::Published,
    };
    let query = resolve_symbol_query_identity(&locator);
    let root = IssuedRefKey {
        query: *query.digest().as_bytes(),
        basis: *basis.basis().id().digest().as_bytes(),
        slot: *locator.resolved_slot.canonical_digest().as_bytes(),
        role: None,
    };
    let keys = (0..entries).map(|ordinal| IssuedRefKey {
        role: (ordinal > 0).then(|| {
            *verter_identity::encoding::CanonicalDigest::of_bytes(&ordinal.to_le_bytes()).as_bytes()
        }),
        ..root
    });
    let entries = keys
        .map(|key| (key, NativeReplayLocator::Declaration(locator.clone())))
        .collect();
    (
        NativeFactRecord::new(FactQuery::ResolveSymbol(query), basis.clone(), entries),
        root,
    )
}

/// The index finds an entry only by its whole tuple, evicts whole records
/// first in, first out under its record, entry and byte bounds, and
/// refuses an individually oversized record without issuing any of it.
#[test]
fn whole_tuple_lookup_and_fifo_bounds() {
    let (_workspace, host) = project_host();
    let _ = answered_graph(&host, "A");
    let issued = lookup_issued(&host, &issued_keys_for_tests(&host)[0]).expect("issued");
    let basis = &issued.record.basis;
    let mut index = IssuedReferenceIndex::default();

    let (first, key) = record(basis, "First", 1);
    index.register(first).expect("a bounded record registers");
    assert!(index.lookup(&key).is_some());
    let flip = |mut digest: [u8; 32]| {
        digest[0] ^= 1;
        digest
    };
    for (label, wrong) in [
        (
            "query",
            IssuedRefKey {
                query: flip(key.query),
                ..key
            },
        ),
        (
            "basis",
            IssuedRefKey {
                basis: flip(key.basis),
                ..key
            },
        ),
        (
            "slot",
            IssuedRefKey {
                slot: flip(key.slot),
                ..key
            },
        ),
        (
            "role",
            IssuedRefKey {
                role: Some([0; 32]),
                ..key
            },
        ),
    ] {
        assert!(
            index.lookup(&wrong).is_none(),
            "a substituted {label} names nothing"
        );
    }

    for ordinal in 1..MAX_ISSUED_RECORDS {
        index
            .register(record(basis, &format!("N{ordinal}"), 1).0)
            .expect("registers");
    }
    assert!(index.lookup(&key).is_some(), "the index is full, not over");
    index
        .register(record(basis, "Overflow", 1).0)
        .expect("registers");
    assert!(
        index.lookup(&key).is_none(),
        "the oldest record is evicted first"
    );
    assert_eq!(index.records.len(), MAX_ISSUED_RECORDS);
    assert_eq!(index.counters.evicted, 1);

    let (wide, wide_key) = record(basis, "Wide", MAX_ISSUED_ENTRIES + 1);
    assert_eq!(index.register(wide), Err(IssueRefusal::Oversized));
    assert!(
        index.lookup(&wide_key).is_none(),
        "an oversized record issues nothing"
    );

    let heavy = "H".repeat(MAX_ISSUED_RETAINED_BYTES / 5);
    let mut heavy_keys = Vec::new();
    for suffix in ["a", "b", "c", "d"] {
        let (heavy, heavy_key) = record(basis, &format!("{heavy}{suffix}"), 1);
        index.register(heavy).expect("a heavy record alone fits");
        heavy_keys.push(heavy_key);
        assert!(index.retained_bytes <= MAX_ISSUED_RETAINED_BYTES);
        assert!(index.entries <= MAX_ISSUED_ENTRIES);
    }
    assert!(
        index.lookup(&heavy_keys[0]).is_none() && index.lookup(&heavy_keys[3]).is_some(),
        "the byte bound evicts the oldest heavy record to admit the newest"
    );
}
