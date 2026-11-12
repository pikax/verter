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

/// The reference `graph` publishes on its first root, if any.
fn root_node_ref(graph: &SemanticTypeGraph) -> Option<wire::NodeRef> {
    let root = *graph.root_ids.first()?;
    graph
        .node_id_map
        .iter()
        .find(|entry| entry.node_id == root)
        .and_then(|entry| entry.node_ref.clone())
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
    assert_eq!(
        root_node_ref(&answered),
        Some(key.to_wire()),
        "the answer publishes its issued reference on its root"
    );
    answered.node_id_map.clear();

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

const SFC: &str = "/comp.vue";
const SFC_SOURCE: &str = r#"<script setup lang="ts">
interface Item { id: number }
defineProps<{ item: Item; count: number }>();
</script>
<template><div></div></template>
"#;

fn upsert_standalone(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

fn vue_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_standalone(&host, SFC, SFC_SOURCE);
    host
}

/// A framework-surface envelope for `SFC` under the advertised Vue profile,
/// demanding `kinds`.
fn framework_envelope(
    host: &VerterHost,
    kinds: &[verter_protocol::typeinfo::graph::FrameworkSurfaceKind],
) -> TypeInfoGraphRequest {
    framework_envelope_for(host, SFC, "vue", kinds)
}

/// A framework-surface envelope for `canonical` under the advertised
/// `family` profile, demanding `kinds`.
fn framework_envelope_for(
    host: &VerterHost,
    canonical: &str,
    family: &str,
    kinds: &[verter_protocol::typeinfo::graph::FrameworkSurfaceKind],
) -> TypeInfoGraphRequest {
    let profile = host
        .typeinfo_capabilities(&wire::TypeInfoCapabilityHandshakeRequest {
            client_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        })
        .framework_profiles
        .into_iter()
        .find(|advertised| advertised.family == family)
        .and_then(|advertised| advertised.profile)
        .unwrap_or_else(|| panic!("the host advertises a {family} profile"));
    TypeInfoGraphRequest {
        schema_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: Operation::FrameworkSurfaces as i32,
        payload: Some(type_info_graph_request::Payload::FrameworkSurface(
            wire::FrameworkSurfaceRequest {
                selector: Some(wire::ComponentSelector {
                    canonical_id: canonical.to_string(),
                    export_name: String::new(),
                    has_export_name: false,
                    framework_profile: Some(profile),
                }),
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
                schema_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
                requested_kinds: kinds.iter().map(|kind| *kind as i32).collect(),
            },
        )),
    }
}

/// The framework-member locator an issued key names.
fn member_locator(issued: &IssuedRef) -> &FrameworkMemberLocator {
    match issued.locator() {
        NativeReplayLocator::FrameworkMember(locator) => locator,
        other => panic!("expected a framework member, got {other:?}"),
    }
}

/// A framework-surface request addresses exactly its exported members whose
/// source replays: the reference-typed prop is issued under its producing
/// macro with the very source the props row published, and replays to the
/// member type the surface exported; the closed-leaf prop is an answer, not
/// an address, and receives none. An edit of the component stales it.
#[test]
fn framework_props_issue_replayable_member_references() {
    use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
    use verter_session_query::analysis::types::AnalyzedMacroKind;
    let host = vue_host();
    let (response, _) = host
        .resolve_framework_surface_with_audit(framework_envelope(
            &host,
            &[FrameworkSurfaceKind::Props],
        ))
        .into_parts();
    assert!(matches!(
        response.expect("the framework arm").kind,
        Some(type_info_graph_response::Kind::FrameworkSurface(_))
    ));
    let keys = issued_keys_for_tests(&host);
    assert_eq!(
        keys.len(),
        1,
        "only the replayable prop is addressed: {keys:?}; {:?}",
        issuance_counters_for_tests(&host)
    );
    assert!(keys[0].role.is_some(), "a member carries its role");
    let issued = lookup_issued(&host, &keys[0]).expect("the issued member is found");
    let locator = member_locator(&issued).clone();
    assert_eq!(&*locator.member_name, "item");

    // The row the surface published, read through the macro's own bundle.
    let indexed = host.ensure_indexed_ready(SFC).expect("indexed");
    let macro_index = indexed
        .snapshot
        .macros
        .iter()
        .position(|mac| mac.kind == AnalyzedMacroKind::DefineProps)
        .expect("a defineProps macro");
    assert_eq!(
        locator.producer,
        MemberProducer::VueMacro {
            kind: AnalyzedMacroKind::DefineProps,
            index: u32::try_from(macro_index).expect("a small macro index"),
        }
    );
    assert_eq!(
        locator.scope_owner, indexed.snapshot.macros[macro_index].owner,
        "the source replays under its producing macro owner"
    );
    let dtos = host
        .vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
            owner_canonical: Arc::from(SFC),
            macro_index,
            macro_kind: AnalyzedMacroKind::DefineProps,
            root_identity: indexed.whole_hash,
            level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
        })
        .expect("vue admits the component");
    let row = dtos
        .prop_fields()
        .iter()
        .find(|row| row.analysis.name == "item")
        .expect("the item prop row");
    assert_eq!(row.authority.source(), Some(&locator.source));

    in_request(&host, |view, ctx, dispatch| {
        assert_eq!(
            admit_issued_in_view(view.view(), &FactValidationView::new(ctx), &issued),
            Ok(())
        );
        let node = replay_issued_in_view(&host, ctx, dispatch, &issued)
            .expect("the admitted member replays")
            .expect("to its value");
        let replayed =
            verter_type_engine::project_semantic_dispatch::raise::node_shallow_member_output_with_dispatch(
                dispatch, node,
            )
            .map(crate::typeinfo::framework_surface::results::NamedTypeMemberOutput::from_raised_shallow);
        assert_eq!(
            replayed, row.member_type,
            "the replay reaches the exported member type"
        );
    });

    upsert_standalone(&host, SFC, &SFC_SOURCE.replace("id: number", "id: string"));
    in_request(&host, |view, ctx, _| {
        assert!(
            admit_issued_in_view(view.view(), &FactValidationView::new(ctx), &issued).is_err(),
            "an edit of the component stales its member references"
        );
    });
}

/// Two requests for different facets of one component are different
/// questions: each issues under its own query identity, and the member keeps
/// the same basis, anchor slot and role whatever sibling facets were
/// demanded alongside it.
#[test]
fn framework_member_role_is_independent_of_sibling_facets() {
    use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
    let host = vue_host();
    let _ = host.resolve_framework_surface_with_audit(framework_envelope(
        &host,
        &[FrameworkSurfaceKind::Props],
    ));
    let _ = host.resolve_framework_surface_with_audit(framework_envelope(
        &host,
        &[FrameworkSurfaceKind::Props, FrameworkSurfaceKind::Emits],
    ));
    let keys = issued_keys_for_tests(&host);
    assert_eq!(
        keys.len(),
        2,
        "{keys:?}; {:?}",
        issuance_counters_for_tests(&host)
    );
    assert_ne!(
        keys[0].query, keys[1].query,
        "different demands are different questions"
    );
    assert_eq!(
        (keys[0].basis, keys[0].slot, keys[0].role),
        (keys[1].basis, keys[1].slot, keys[1].role),
        "the same member under the same basis has the same address components"
    );
}

/// The member names every issued framework-member key addresses.
fn issued_member_names(host: &VerterHost) -> Vec<String> {
    issued_keys_for_tests(host)
        .iter()
        .filter_map(|key| lookup_issued(host, key))
        .filter_map(|issued| match issued.locator() {
            NativeReplayLocator::FrameworkMember(locator) => Some(locator.member_name.to_string()),
            NativeReplayLocator::Declaration(_) => None,
        })
        .collect()
}

/// The issued reference addressing `name`.
fn issued_member(host: &VerterHost, name: &str) -> IssuedRef {
    issued_keys_for_tests(host)
        .iter()
        .filter_map(|key| lookup_issued(host, key))
        .find(|issued| {
            matches!(issued.locator(), NativeReplayLocator::FrameworkMember(locator)
                if &*locator.member_name == name)
        })
        .unwrap_or_else(|| panic!("`{name}` is addressed: {:?}", issued_member_names(host)))
}

/// The shallow member type `issued` replays to under a fresh admitted view.
fn replayed_member_type(
    host: &VerterHost,
    issued: &IssuedRef,
) -> Option<crate::typeinfo::framework_surface::results::NamedTypeMemberOutput> {
    in_request(host, |view, ctx, dispatch| {
        assert_eq!(
            admit_issued_in_view(view.view(), &FactValidationView::new(ctx), issued),
            Ok(())
        );
        let node = replay_issued_in_view(host, ctx, dispatch, issued)
            .expect("the admitted member replays")
            .expect("to its value");
        verter_type_engine::project_semantic_dispatch::raise::node_shallow_member_output_with_dispatch(
            dispatch, node,
        )
        .map(crate::typeinfo::framework_surface::results::NamedTypeMemberOutput::from_raised_shallow)
    })
}

/// The DTO bundle `canonical`'s first macro of `kind` resolves to.
fn macro_dtos(
    host: &VerterHost,
    canonical: &str,
    kind: verter_session_query::analysis::types::AnalyzedMacroKind,
) -> Arc<crate::typeinfo::framework_surface::results::MacroSurfaceDtos> {
    let indexed = host.ensure_indexed_ready(canonical).expect("indexed");
    let macro_index = indexed
        .snapshot
        .macros
        .iter()
        .position(|mac| mac.kind == kind)
        .expect("the macro exists");
    host.vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: Arc::from(canonical),
        macro_index,
        macro_kind: kind,
        root_identity: indexed.whole_hash,
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    })
    .expect("vue admits the component")
}

const SLOTS_SFC: &str = "/slots.vue";
const SLOTS_SFC_SOURCE: &str = r#"<script setup lang="ts">
interface Row { id: number }
defineOptions<{ config: { row: Row } }>();
defineSlots<{ default(props: { row: Row }): unknown }>();
</script>
<template><div></div></template>
"#;

/// Options members and slots are addressed by their own value sources: an
/// options member replays to the member type the options surface exported,
/// and a slot replays through its realized callable root to the slot type
/// the slots surface exported.
#[test]
fn framework_options_and_slots_issue_replayable_member_references() {
    use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
    use verter_session_query::analysis::types::AnalyzedMacroKind;
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_standalone(&host, SLOTS_SFC, SLOTS_SFC_SOURCE);
    let _ = host.resolve_framework_surface_with_audit(framework_envelope_for(
        &host,
        SLOTS_SFC,
        "vue",
        &[FrameworkSurfaceKind::Options, FrameworkSurfaceKind::Slots],
    ));

    let options = macro_dtos(&host, SLOTS_SFC, AnalyzedMacroKind::DefineOptions);
    let config = options
        .options
        .as_ref()
        .and_then(|surface| surface.members.iter().find(|m| m.name == "config"))
        .expect("the config option");
    let issued = issued_member(&host, "config");
    assert_eq!(member_locator(&issued).role, MemberRole::OptionsMember);
    assert_eq!(replayed_member_type(&host, &issued), config.value);

    let slots = macro_dtos(&host, SLOTS_SFC, AnalyzedMacroKind::DefineSlots);
    let slot = slots
        .slot_member_types
        .iter()
        .find(|m| m.name == "default")
        .expect("the default slot");
    let issued = issued_member(&host, "default");
    assert_eq!(member_locator(&issued).role, MemberRole::Slot);
    assert_eq!(replayed_member_type(&host, &issued), slot.value);
}

const MODEL_SFC: &str = "/model.vue";
const MODEL_SFC_SOURCE: &str = r#"<script setup lang="ts">
interface Item { id: number }
defineProps<{ modelValue: Item; other: Item }>();
defineModel<Item>();
</script>
<template><div></div></template>
"#;

/// A member name two producers contribute (a declared `modelValue` prop and
/// `defineModel`'s synthesized one) is ambiguous: neither is addressed,
/// while an unambiguous sibling prop is.
#[test]
fn ambiguous_member_names_are_not_addressed() {
    use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_standalone(&host, MODEL_SFC, MODEL_SFC_SOURCE);
    let _ = host.resolve_framework_surface_with_audit(framework_envelope_for(
        &host,
        MODEL_SFC,
        "vue",
        &[FrameworkSurfaceKind::Props],
    ));
    let names = issued_member_names(&host);
    assert!(names.iter().any(|name| name == "other"), "{names:?}");
    assert!(!names.iter().any(|name| name == "modelValue"), "{names:?}");
}

const SVELTE: &str = "/comp.svelte";
const SVELTE_SOURCE: &str = r#"<script lang="ts">
  interface Item { id: number }
  let { item }: { item: Item } = $props();
</script>
"#;

/// A Svelte component's typed `$props()` members are addressed under the
/// runes-props source family, replay under the owner their authored anchor
/// is written in, and reach the member type the props surface exported.
#[test]
fn svelte_props_issue_replayable_member_references() {
    use crate::typeinfo::framework_surface::plan::SvelteSurfaceSource;
    use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_standalone(&host, SVELTE, SVELTE_SOURCE);
    let _ = host.resolve_framework_surface_with_audit(framework_envelope_for(
        &host,
        SVELTE,
        "svelte",
        &[FrameworkSurfaceKind::Props],
    ));
    let issued = issued_member(&host, "item");
    let locator = member_locator(&issued).clone();
    assert_eq!(
        locator.producer,
        MemberProducer::Svelte(SvelteSurfaceSource::RunesProps)
    );
    // The named member type publishes as a shallow reference to `Item`,
    // replayed in the declaring owner's scope.
    assert!(
        matches!(
            &locator.source,
            verter_type_expr::facts::SemanticTypeSource::Synthesized(
                verter_type_expr::facts::ResolvedLocalShape::Ref(symbol)
            ) if symbol.anchor.symbol.as_ref() == "Item"
        ),
        "{:?}",
        locator.source
    );
    assert_eq!(
        Some(locator.scope_owner),
        source_scope_owner(&locator.source)
    );
    let exported = in_request(&host, |_, ctx, dispatch| {
        match crate::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface(
            ctx,
            dispatch,
            SVELTE,
            SvelteSurfaceSource::RunesProps,
        ) {
            crate::typeinfo::framework_surface::results::ResolvedOutcome::Resolved(dtos) => dtos
                .prop_fields()
                .iter()
                .find(|row| row.analysis.name == "item")
                .and_then(|row| row.member_type.clone()),
            other => panic!("the runes props resolve: {other:?}"),
        }
    });
    assert!(exported.is_some(), "the item prop exports a member type");
    assert_eq!(replayed_member_type(&host, &issued), exported);
}

/// An expand-around envelope presenting `node_ref` under `mode` and a
/// one-level closure.
fn expand_envelope(node_ref: wire::NodeRef, mode: wire::ProjectionMode) -> TypeInfoGraphRequest {
    TypeInfoGraphRequest {
        schema_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: Operation::ExpandAround as i32,
        payload: Some(type_info_graph_request::Payload::ExpandAround(
            wire::ExpandGraphAroundRequest {
                node_ref: Some(node_ref),
                context: Some(wire::ProjectionReductionContext {
                    mode: mode as i32,
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
            },
        )),
    }
}

/// What the production graph entry answers for an expand-around of
/// `node_ref` under `mode`: the graph, or the typed refusal.
fn expand_in(
    host: &VerterHost,
    node_ref: wire::NodeRef,
    mode: wire::ProjectionMode,
) -> Result<SemanticTypeGraph, verter_protocol::verter::v1::type_info_request_error::Kind> {
    let (response, _) = host
        .resolve_symbol_graph_with_audit(expand_envelope(node_ref, mode))
        .into_parts();
    let refused = |error: wire::TypeInfoRequestError| Err(error.kind.expect("a typed error"));
    match response {
        Ok(wire::TypeInfoGraphResponse {
            kind: Some(type_info_graph_response::Kind::Graph(graph)),
        }) => Ok(graph),
        Ok(wire::TypeInfoGraphResponse {
            kind: Some(type_info_graph_response::Kind::Error(error)),
        })
        | Err(error) => refused(error),
        Ok(other) => panic!("expected a graph or a typed error, got {other:?}"),
    }
}

/// The expanded graph under the resolve-symbol envelope's `Expanded`
/// context, or a panic naming the refusal.
fn expanded_graph(host: &VerterHost, node_ref: wire::NodeRef) -> SemanticTypeGraph {
    expand_in(host, node_ref, wire::ProjectionMode::Expanded)
        .unwrap_or_else(|error| panic!("expected a graph answer, got {error:?}"))
}

/// The typed refusal an expand-around of `node_ref` answers under the
/// resolve-symbol envelope's context.
fn expand_error(
    host: &VerterHost,
    node_ref: wire::NodeRef,
) -> verter_protocol::verter::v1::type_info_request_error::Kind {
    match expand_in(host, node_ref, wire::ProjectionMode::Expanded) {
        Err(error) => error,
        Ok(graph) => panic!("expected a typed error, got {graph:?}"),
    }
}

/// An issued declaration root expands to the very graph its answer
/// exported, and the expansion carries the same reference on its root. An
/// expansion issues nothing.
#[test]
fn expand_around_replays_an_issued_declaration_root() {
    let (_workspace, host) = project_host();
    let mut answered = answered_graph(&host, "A");
    let node_ref = root_node_ref(&answered).expect("the answer publishes its root reference");

    let expanded = expanded_graph(&host, node_ref.clone());
    assert_eq!(root_node_ref(&expanded), Some(node_ref));
    answered.query = None;
    assert_eq!(
        expanded, answered,
        "the expansion is the answer's own root graph"
    );
    assert_eq!(
        issued_keys_for_tests(&host).len(),
        1,
        "an expansion issues nothing"
    );
}

/// A reference names its position under the mode it was issued under:
/// asking for another mode is refused, never answered under the issued one.
/// The reduction demand selects no different answer.
#[test]
fn expand_around_refuses_another_projection_context() {
    use verter_protocol::verter::v1::type_info_request_error::Kind;
    let (_workspace, host) = project_host();
    let node_ref = root_node_ref(&answered_graph(&host, "A"))
        .expect("the answer publishes its root reference");
    assert!(
        matches!(
            expand_in(&host, node_ref.clone(), wire::ProjectionMode::Shallow),
            Err(Kind::MalformedPayload(_))
        ),
        "a Shallow expansion of an Expanded answer is a different question"
    );
    let mut transit = expand_envelope(node_ref, wire::ProjectionMode::Expanded);
    if let Some(type_info_graph_request::Payload::ExpandAround(expand)) = transit.payload.as_mut() {
        if let Some(context) = expand.context.as_mut() {
            context.demand = wire::ReductionDemand::StructuralTransit as i32;
        }
    }
    let (response, _) = host.resolve_symbol_graph_with_audit(transit).into_parts();
    assert!(
        matches!(
            response.map(|response| response.kind),
            Ok(Some(type_info_graph_response::Kind::Graph(_)))
        ),
        "the issued mode under another demand expands"
    );
}

/// A tuple the host never issued is unknown, whichever part is changed: a
/// forged basis, a role a declaration root does not carry, or a reference
/// another host issued.
#[test]
fn expand_around_refuses_a_reference_the_host_never_issued() {
    use verter_protocol::verter::v1::type_info_request_error::Kind;
    let (_workspace, host) = project_host();
    let node_ref = root_node_ref(&answered_graph(&host, "A"))
        .expect("the answer publishes its root reference");

    let mut forged_basis = node_ref.clone();
    forged_basis.input_basis_digest[0] ^= 1;
    let mut forged_role = node_ref.clone();
    forged_role.role_digest = vec![0; wire::NODE_REF_DIGEST_LEN];
    for forged in [forged_basis, forged_role] {
        match expand_error(&host, forged.clone()) {
            Kind::UnknownNodeRef(error) => assert_eq!(error.node_ref, Some(forged)),
            other => panic!("expected UnknownNodeRef, got {other:?}"),
        }
    }

    let (_other_workspace, other_host) = project_host();
    assert!(
        matches!(expand_error(&other_host, node_ref), Kind::UnknownNodeRef(_)),
        "a reference is held only by the host that issued it"
    );
}

/// An input edit stales an issued reference: its expansion is refused as
/// `StaleBasis`, never re-answered under the new inputs. The next answer
/// issues a new reference, which expands.
#[test]
fn expand_around_refuses_a_reference_an_edit_staled() {
    use verter_protocol::verter::v1::type_info_request_error::Kind;
    let (workspace, host) = project_host();
    let node_ref = root_node_ref(&answered_graph(&host, "A"))
        .expect("the answer publishes its root reference");

    write(
        &workspace,
        &host,
        DEPENDENCY,
        "export type B = { x: string }\n",
    );
    match expand_error(&host, node_ref.clone()) {
        Kind::StaleBasis(error) => assert_eq!(error.node_ref, Some(node_ref.clone())),
        other => panic!("expected StaleBasis, got {other:?}"),
    }

    let fresh =
        root_node_ref(&answered_graph(&host, "A")).expect("the new answer publishes its reference");
    assert_ne!(fresh, node_ref, "the new answer is a new basis");
    assert_eq!(
        root_node_ref(&expanded_graph(&host, fresh.clone())),
        Some(fresh)
    );
}

/// A framework-surface answer publishes each addressed member's reference
/// on the member's own type node, and only there: the replayable prop is
/// addressed, the closed leaf is not. The member reference expands to the
/// member type the surface exported.
#[test]
fn expand_around_replays_a_framework_member() {
    use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
    let host = vue_host();
    let (response, _) = host
        .resolve_framework_surface_with_audit(framework_envelope(
            &host,
            &[FrameworkSurfaceKind::Props],
        ))
        .into_parts();
    let payload = match response.expect("the framework arm").kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(payload)) => payload,
        other => panic!("expected a framework surface, got {other:?}"),
    };
    let graph = payload.graph.as_ref().expect("a surface graph");
    let strings = graph
        .strings
        .as_ref()
        .map(|table| table.entries.as_slice())
        .unwrap_or_default();
    let member = |name: &str| {
        payload
            .surfaces
            .iter()
            .flat_map(|surface| surface.members.iter())
            .find(|member| strings.get(member.name_id as usize).map(String::as_str) == Some(name))
            .unwrap_or_else(|| panic!("the {name} prop"))
    };
    let published = |node_id: u32| {
        graph
            .node_id_map
            .iter()
            .find(|entry| entry.node_id == node_id)
            .and_then(|entry| entry.node_ref.clone())
    };
    assert_eq!(
        published(member("count").type_node_id),
        None,
        "a closed leaf is an answer, not an address"
    );
    let item = member("item");
    let node_ref = published(item.type_node_id).expect("the replayable prop is addressed");
    assert_eq!(node_ref, issued_member(&host, "item").key().to_wire());

    // A member's type is read from the navigable published view.
    let expanded = expand_in(&host, node_ref.clone(), wire::ProjectionMode::Navigate)
        .unwrap_or_else(|error| panic!("the member expands, got {error:?}"));
    assert_eq!(root_node_ref(&expanded), Some(node_ref.clone()));
    assert!(
        expand_in(&host, node_ref, wire::ProjectionMode::Expanded).is_err(),
        "a member reference is issued under the navigable view only"
    );
    let kind_of = |graph: &SemanticTypeGraph, node_id: u32| {
        graph
            .nodes
            .get(node_id as usize)
            .and_then(|node| node.kind.as_ref())
            .map(std::mem::discriminant)
    };
    let member_kind = kind_of(graph, item.type_node_id);
    assert!(member_kind.is_some(), "the member type is exported");
    assert_eq!(
        kind_of(&expanded, expanded.root_ids[0]),
        member_kind,
        "the expansion reaches the member type the surface exported"
    );
}
