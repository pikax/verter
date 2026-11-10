//! A native fact basis commits exactly what one traced resolution
//! observed under one captured view, and a later view admits it only while
//! every committed observation still holds.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_session_query::resolution::IdeProjectCompilerOptions;
use verter_type_engine::resolver_core::fact_validation_port::FactValidationView;
use verter_type_engine::semantic_query::ProjectionMode;
use verter_workspace::{
    MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectRank, VfsProjectConfig,
};

use super::*;
use crate::resolver_store::CurrentHostStoreView;
use crate::types::{HostConfig, UpsertRequest};

const ROOT: &str = "/proj";
const ENTRY: &str = "/proj/a.ts";
const DEPENDENCY: &str = "/proj/b.ts";
const MISSING: &str = "/proj/c.ts";
const ENTRY_SOURCE: &str = "import type { B } from './b'\n\
                            import type { C } from './c'\n\
                            export type A = { b: B; c: C }\n";
const DEPENDENCY_SOURCE: &str = "export type B = { x: number }\n";

fn project(paths: Vec<(String, Vec<String>)>) -> VfsProjectConfig {
    VfsProjectConfig {
        root: ROOT.to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some(format!("{ROOT}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![".ts".to_string()],
        workspace_root: ROOT.to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions {
            base_url: Some(ROOT.to_string()),
            paths,
            ..Default::default()
        },
        references: vec![],
        membership: verter_workspace::configured_membership_match_all_under_root(
            &verter_workspace::CanonicalPath::new(ROOT),
        ),
    }
}

fn publish_graph(workspace: &MemoryWorkspace, paths: Vec<(String, Vec<String>)>) {
    workspace.set_project_graph(ProjectGraph::from_configs(vec![project(paths)]));
}

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
    publish_graph(
        &workspace,
        vec![("@a/*".to_string(), vec!["./*".to_string()])],
    );
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: false,
            ..HostConfig::default()
        },
        Arc::clone(&workspace) as Arc<dyn verter_workspace::WorkspaceAccess>,
    ));
    write(&workspace, &host, DEPENDENCY, DEPENDENCY_SOURCE);
    write(&workspace, &host, ENTRY, ENTRY_SOURCE);
    (workspace, host)
}

fn current(host: &VerterHost) -> CurrentHostStoreView {
    crate::typeinfo::current_store_view_for_query(host).expect("a settled host has a current view")
}

type Ctx<'a> = crate::resolver_core::HostResolverContext<'a, verter_audit::observe::Uncaptured>;

/// Run `f` with a request context over `view`, built the way a request
/// builds it: the view plus a fresh request-completion overlay.
fn with_request<T>(
    host: &VerterHost,
    view: &CurrentHostStoreView,
    f: impl FnOnce(&Ctx<'_>) -> T,
) -> T {
    let ctx = Ctx::from_current_in(
        host,
        view,
        Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
    f(&ctx)
}

fn port<'a>(ctx: &'a Ctx<'_>) -> FactValidationView<'a> {
    FactValidationView::new(ctx)
}

/// Resolve `A` under `ctx`'s one captured view with the resolution traced.
fn traced_resolution(host: &VerterHost, ctx: &Ctx<'_>) -> NativeTrace {
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
    let dispatch = verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let _scope = dispatch.enter_construction_scope();
    let ((outcome, _), trace) = trace_native_fact(ctx, &[ENTRY], || {
        crate::typeinfo::resolve_named_symbol::resolve_named_symbol_in_current_view(
            host,
            ctx,
            &dispatch,
            ENTRY,
            "A",
            &[],
            ProjectionMode::Expanded,
        )
    });
    assert!(
        matches!(outcome, Ok(Some(_))),
        "A must resolve: {outcome:?}"
    );
    trace
}

/// Resolve, trace and commit under one captured view.
fn committed(host: &VerterHost) -> NativeFactBasis {
    let view = current(host);
    with_request(host, &view, |ctx| {
        let trace = traced_resolution(host, ctx);
        let traced: Vec<FactVersionRef> = match &trace.finalise {
            FactReadSetFinalise::Ok(facts) | FactReadSetFinalise::NonCacheable(facts) => {
                facts.iter().cloned().collect()
            }
            FactReadSetFinalise::MutationUnstable => Vec::new(),
        };
        commit_native_fact_basis(host, view.view(), &port(ctx), trace, &[ENTRY]).unwrap_or_else(
            |refusal| {
                panic!(
                    "commit refused ({refusal:?}); rejected by the base view: {:?}",
                    view.view().invalid_fact_details(&traced, 8)
                )
            },
        )
    })
}

/// Admission of `basis` against a fresh capture of `host`.
fn admits(host: &VerterHost, basis: &NativeFactBasis) -> Result<(), NativeBasisStale> {
    let view = current(host);
    with_request(host, &view, |ctx| {
        validate_native_fact_basis(view.view(), &port(ctx), basis)
    })
}

fn has_row(basis: &NativeFactBasis, canonical: &str) -> bool {
    basis.basis().observe(canonical).is_ok()
}

/// The committed environment rows a fresh capture of `host` reads
/// differently, for failure messages.
fn changed_rows(host: &VerterHost, basis: &NativeFactBasis) -> Vec<String> {
    let view = current(host);
    environment_rows(view.view(), &basis.scopes)
        .into_iter()
        .filter(|row| basis.basis().observe(row.canonical()) != Ok(row))
        .map(|row| row.canonical().to_string())
        .collect()
}

/// Every committed input class is load-bearing: a dependency edit, the
/// appearance of a negatively probed path, a configuration republish and
/// an overlay over the captured inputs each reject the basis that a fresh
/// capture of the unchanged host still admits.
#[test]
fn dependency_negative_config_overlay_changes_reject_admission() {
    type Mutation = fn(&MemoryWorkspace, &VerterHost);
    let mutations: [(&str, Mutation); 3] = [
        ("dependency edit", |workspace, host| {
            write(
                workspace,
                host,
                DEPENDENCY,
                "export type B = { x: string }\n",
            );
        }),
        ("negative probe appears", |workspace, host| {
            write(workspace, host, MISSING, "export type C = { y: number }\n");
        }),
        ("configuration republish", |workspace, _| {
            publish_graph(
                workspace,
                vec![("@moved/*".to_string(), vec!["./moved/*".to_string()])],
            );
        }),
    ];
    for (label, mutate) in mutations {
        let (workspace, host) = project_host();
        let basis = committed(&host);
        assert!(has_row(&basis, &format!("env:{DEPENDENCY}")), "{label}");
        assert!(has_row(&basis, &format!("env:{ENTRY}")), "{label}");
        assert_eq!(
            admits(&host, &basis),
            Ok(()),
            "{label}: an unchanged host admits its own basis; changed rows: {:?}",
            changed_rows(&host, &basis)
        );
        mutate(&workspace, &host);
        assert!(
            admits(&host, &basis).is_err(),
            "{label}: the changed input must reject the committed basis"
        );
        let recommitted = committed(&host);
        assert!(
            basis.fence().admit(recommitted.basis()).is_err(),
            "{label}: the changed input commits a different basis the old fence refuses"
        );
    }

    // An overlay that shadows a committed dependency: the session view
    // serves other bytes for it, so the basis is not this view's inputs.
    let (_workspace, host) = project_host();
    let basis = committed(&host);
    let source = "export type B = { z: boolean }\n";
    let mut sources: FxHashMap<String, Arc<str>> = FxHashMap::default();
    sources.insert(DEPENDENCY.to_string(), Arc::from(source));
    let mut hashes = FxHashMap::default();
    hashes.insert(
        DEPENDENCY.to_string(),
        verter_semantic_source::source_hash::hash_16(source.as_bytes()),
    );
    let deleted = std::collections::HashSet::new();
    let overlay = crate::session_view::OverlaidViewRef::new(&host, &sources, &hashes, &deleted);
    let overlaid = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &overlay);
    let session =
        crate::resolver_core::SessionResolverContext::<'_, verter_audit::observe::Uncaptured>::new(
            &host,
            &overlay,
            &overlaid,
            Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
    let session_port = FactValidationView::new(&session);
    assert!(
        validate_native_fact_basis(&overlaid, &session_port, &basis).is_err(),
        "an overlay over a committed input must reject the basis"
    );
    assert_eq!(
        admits(&host, &basis),
        Ok(()),
        "the base view still admits it"
    );
}

/// A warm request consumes its dependencies' evidence through receipts;
/// its basis must still commit those dependencies, and equal the cold
/// request's basis, because the same inputs were observed.
#[test]
fn warm_receipt_dependencies_are_committed() {
    let (_workspace, host) = project_host();
    let cold = committed(&host);
    let warm = committed(&host);
    for basis in [&cold, &warm] {
        assert!(has_row(basis, &format!("env:{DEPENDENCY}")));
        assert!(
            basis
                .basis()
                .observations()
                .any(|row| row.value_bytes().is_some_and(|bytes| bytes
                    .windows(DEPENDENCY.len())
                    .any(|window| window == DEPENDENCY.as_bytes()))
                    && row.canonical().starts_with("fact:")),
            "a fact naming the dependency is committed"
        );
    }
    assert_eq!(
        cold.basis().id(),
        warm.basis().id(),
        "warm evidence commits the same observations as the cold trace"
    );
    assert!(warm.retained_bytes() > 0);
}

/// A trace the captured view cannot vouch for commits nothing: here the
/// dependency moved between resolution and commit.
#[test]
fn a_view_that_cannot_vouch_for_the_trace_commits_nothing() {
    let (workspace, host) = project_host();
    let resolved_under = current(&host);
    let trace = with_request(&host, &resolved_under, |ctx| traced_resolution(&host, ctx));
    write(
        &workspace,
        &host,
        DEPENDENCY,
        "export type B = { x: string }\n",
    );
    let view = current(&host);
    let refusal = with_request(&host, &view, |ctx| {
        commit_native_fact_basis(&host, view.view(), &port(ctx), trace, &[ENTRY]).err()
    });
    assert_eq!(refusal, Some(NativeBasisRefusal::NotVouched));
}

/// An owner whose source cannot be observed leaves the trace uncommittable,
/// whatever the traced work itself read.
#[test]
fn an_unobservable_owner_commits_nothing() {
    let (_workspace, host) = project_host();
    let view = current(&host);
    let refusal = with_request(&host, &view, |ctx| {
        let ((), trace) = trace_native_fact(ctx, &[ENTRY, "/proj/absent.ts"], || ());
        commit_native_fact_basis(&host, view.view(), &port(ctx), trace, &[ENTRY]).err()
    });
    assert_eq!(refusal, Some(NativeBasisRefusal::OwnerUnobserved));
}
