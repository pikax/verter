//! Unit tests for [`crate::tsserver::project_router`].
//!
//! Extracted from an inline `#[cfg(test)] mod tests` in `project_router.rs`:
//! the fixtures build real on-disk TypeScript installs with `std::fs`, and the
//! D14 / VFS-boundary architecture guards scan whole PRODUCTION source files
//! for `std::fs::`, so a test-only fixture helper living in the production file
//! reads to those guards as a disk-boundary bypass. Wired back as a
//! `#[cfg(test)] #[path = "project_router_tests.rs"] mod tests;` child of
//! `project_router`, so `use super::*` resolves to its items.

use super::*;
use crate::type_provider::mock::{MockCall, MockTypeProvider};
use std::path::PathBuf;
use verter_session::external_ts::EnvDims;
use verter_session::file_artifact_store::ProjectIdentity;
use verter_workspace::workspace_snapshot::{ProjectId, SnapshotGeneration};

struct BatchRouterFixture {
    _temp: tempfile::TempDir,
    router: ProjectTsserverProvider,
    workspace: Arc<verter_workspace::FilesystemWorkspace>,
    providers: [Arc<MockTypeProvider>; 2],
    members: Vec<CarrierActivation>,
}

/// An establisher that installs one pre-built engine.
struct PrebuiltEngine(Arc<dyn TypeProvider>);

impl crate::resilient_provider::ProviderEstablisher<dyn TypeProvider> for PrebuiltEngine {
    fn log_name(&self) -> &'static str {
        "prebuilt-tsserver"
    }

    fn user_label(&self) -> &'static str {
        "tsserver"
    }

    fn restarting_error(&self) -> &'static str {
        "prebuilt tsserver is restarting"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn establish<'a>(
        &'a self,
        _crash_signal: Arc<tokio::sync::Notify>,
    ) -> crate::resilient_provider::EstablishFuture<'a, dyn TypeProvider> {
        let engine = Arc::clone(&self.0);
        Box::pin(async move { Ok(engine) })
    }
}

/// Only engine discovery/spawning is substituted. Every operation still resolves
/// its source against the live, published configured-project ownership graph,
/// and every engine is established through its production provider hub.
async fn batch_router_fixture() -> BatchRouterFixture {
    batch_router_fixture_with_generated_membership(true).await
}

async fn batch_router_fixture_with_generated_membership(
    admit_generated: bool,
) -> BatchRouterFixture {
    use verter_semantic::resolver_core::{
        ConfiguredMembership, ModuleResolverCore, StaticMembershipSpec,
    };
    use verter_workspace::workspace_snapshot::{OwnershipProject, ProjectPayload};
    use verter_workspace::{CanonicalPath, PublishedRoot, WorkspaceSnapshot};

    let temp = tempfile::tempdir().unwrap();
    let root = ProjectTsserverProvider::normalized(&temp.path().to_string_lossy());
    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = Arc::new(VerterHost::new(
        verter_session::HostConfig::default(),
        workspace.clone(),
    ));
    let router = ProjectTsserverProvider {
        host,
        tsdk: None,
        plugin_path: None,
        node_path: "unused".to_string(),
        client: Arc::new(OnceCell::new()),
        witness_backend: TsserverEngineBackend::with_default_host_version(),
        engine_specs: DashMap::new(),
        providers: DashMap::new(),
        routes: DashMap::new(),
    };
    let providers = [
        Arc::new(MockTypeProvider::new()),
        Arc::new(MockTypeProvider::new()),
    ];
    let members: Vec<_> = [
        ("a", "First.vue", CarrierScriptKind::Tsx),
        ("b", "Middle.svelte", CarrierScriptKind::Ts),
        ("a", "Last.vue", CarrierScriptKind::Jsx),
    ]
    .into_iter()
    .map(|(project, file, script_kind)| CarrierActivation {
        source_path: format!("{root}/{project}/{file}"),
        companion_path: format!("{root}/{project}/{file}.verter.ts"),
        project_file_name: format!("{root}/{project}/tsconfig.json"),
        script_kind,
    })
    .collect();
    let mut projects = Vec::new();
    let mut configs = Vec::new();
    let mut cached_specs = Vec::new();
    for (index, (name, provider)) in ["a", "b"].into_iter().zip(&providers).enumerate() {
        let project_root = format!("{root}/{name}");
        let tsconfig = format!("{project_root}/tsconfig.json");
        let files: Vec<_> = members
            .iter()
            .filter(|member| member.project_file_name == tsconfig)
            .map(|member| CanonicalPath::new(&member.source_path))
            .collect();
        let admitted_files: Vec<_> = if admit_generated {
            members
                .iter()
                .filter(|member| member.project_file_name == tsconfig)
                .map(|member| CanonicalPath::new(&member.companion_path))
                .collect()
        } else {
            Vec::new()
        };
        projects.push(OwnershipProject {
            id: ProjectId(index as u32),
            root: CanonicalPath::new(&project_root),
            workspace_root: CanonicalPath::new(&root),
            payload: ProjectPayload::Configured {
                tsconfig_path: CanonicalPath::new(&tsconfig),
                membership: ConfiguredMembership {
                    spec: StaticMembershipSpec {
                        files: files.iter().cloned().chain(admitted_files).collect(),
                        include: Vec::new(),
                        exclude: Vec::new().into(),
                    },
                    materialized_files: files.into_iter().collect(),
                },
                compiler_options: Default::default(),
                references: Vec::new(),
                workspace_aliases: Vec::new(),
            },
        });
        configs.push(verter_workspace::ide_project_config(
            project_root,
            root.clone(),
            Some(tsconfig.clone()),
        ));
        let key = ProjectEngineKey {
            project: tsconfig.clone(),
            tsserver_path: format!("{root}/typescript/lib/tsserver.js"),
        };
        cached_specs.push((
            tsconfig,
            ProjectEngineSpec {
                key: key.clone(),
                workspace_root: root.clone(),
                default_lib_count: 1,
            },
        ));
        let provider: Arc<dyn TypeProvider> = provider.clone();
        let hub = ProviderHub::new(
            PrebuiltEngine(provider),
            Arc::new(verter_type_runtime::provider_hub::TracingNotifier),
            crate::resilient_provider::HubPolicy::explicit(3),
        );
        hub.establish().await.unwrap();
        router.providers.insert(key, Arc::new(hub));
    }
    workspace.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(WorkspaceSnapshot {
        owners_memo: Default::default(),
        projects,
        resolver: ModuleResolverCore::new(configs),
        generation: SnapshotGeneration(1),
    })));
    let ws_read = router.host.workspace_read();
    let basis = ResolvedPublication {
        published: ws_read.published_root().unwrap(),
        content_generation: ws_read.content_generation(),
        project_generation: router
            .host
            .project_type_store()
            .current_project_generation(),
    };
    for (project, spec) in cached_specs {
        router.engine_specs.insert(
            project,
            CachedEngineSpec {
                basis: basis.clone(),
                outcome: Ok(spec),
            },
        );
    }
    BatchRouterFixture {
        _temp: temp,
        router,
        workspace,
        providers,
        members,
    }
}

#[tokio::test]
async fn authored_owner_without_generated_membership_never_reaches_a_project_engine() {
    let fixture = batch_router_fixture_with_generated_membership(false).await;
    let result = fixture
        .router
        .activate_carrier_members(&fixture.members)
        .await;
    assert!(
        result.is_err(),
        "generated-unit exclusion must refuse the batch"
    );
    for provider in &fixture.providers {
        assert!(
            provider.calls().is_empty(),
            "refused units must not reach an engine"
        );
    }
}

#[tokio::test]
async fn refused_registration_does_not_poison_a_healthy_project_route() {
    let fixture = batch_router_fixture().await;
    let member = &fixture.members[0];
    let wrong_project = &fixture.members[1].project_file_name;
    assert!(fixture
        .router
        .register_carrier_member(
            &member.source_path,
            &member.companion_path,
            "export {};",
            wrong_project,
        )
        .await
        .is_err());
    assert!(fixture.router.routes.is_empty());
    fixture
        .router
        .register_carrier_member(
            &member.source_path,
            &member.companion_path,
            "export {};",
            &member.project_file_name,
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture.providers[0].calls().as_slice(),
        [MockCall::RegisterCarrierMember { .. }]
    ));
    assert!(fixture.providers[1].calls().is_empty());
}

#[tokio::test]
async fn fresh_registration_replaces_a_stale_project_route() {
    let fixture = batch_router_fixture().await;
    let member = &fixture.members[0];
    fixture.router.register_route(
        &member.source_path,
        &member.companion_path,
        &fixture.members[1].project_file_name,
    );
    fixture
        .router
        .register_carrier_member(
            &member.source_path,
            &member.companion_path,
            "export {};",
            &member.project_file_name,
        )
        .await
        .unwrap();
    let route = fixture.router.routes.get(&member.companion_path).unwrap();
    assert_eq!(
        route.project,
        ProjectTsserverProvider::normalized(&member.project_file_name)
    );
    assert!(matches!(
        fixture.providers[0].calls().as_slice(),
        [MockCall::RegisterCarrierMember { .. }]
    ));
    assert!(fixture.providers[1].calls().is_empty());
}

#[tokio::test]
async fn carrier_batches_preserve_each_provider_order_without_scalar_dispatch() {
    let fixture = batch_router_fixture().await;
    let members = &fixture.members;
    fixture
        .router
        .activate_carrier_members(members)
        .await
        .unwrap();
    for (provider, expected) in [
        (
            &fixture.providers[0],
            vec![members[0].clone(), members[2].clone()],
        ),
        (&fixture.providers[1], vec![members[1].clone()]),
    ] {
        let calls = provider.calls();
        assert!(
            matches!(calls.as_slice(), [MockCall::ActivateCarrierMembers { members }] if *members == expected),
            "each provider must receive one ordered activation batch, got {calls:?}"
        );
        provider.clear_calls();
    }

    let paths: Vec<_> = members
        .iter()
        .map(|member| member.companion_path.clone())
        .collect();
    fixture
        .router
        .notify_carriers_changed(&paths)
        .await
        .unwrap();
    for (provider, expected) in [
        (
            &fixture.providers[0],
            vec![paths[0].clone(), paths[2].clone()],
        ),
        (&fixture.providers[1], vec![paths[1].clone()]),
    ] {
        let calls = provider.calls();
        assert!(
            matches!(calls.as_slice(), [MockCall::NotifyCarriersChanged { companion_paths }] if *companion_paths == expected),
            "each provider must receive one ordered change batch, got {calls:?}"
        );
        provider.clear_calls();
    }

    let member = &members[0];
    fixture
        .router
        .activate_carrier_member(
            &member.source_path,
            &member.companion_path,
            &member.project_file_name,
            member.script_kind,
        )
        .await
        .unwrap();
    let calls = fixture.providers[0].calls();
    assert!(
        matches!(calls.as_slice(), [MockCall::ActivateCarrierMember { source_path, companion_path, project_file_name, script_kind }]
        if source_path == &member.source_path && companion_path == &member.companion_path
            && project_file_name == &member.project_file_name && script_kind == &member.script_kind)
    );
    assert!(fixture.providers[1].calls().is_empty());
}

#[tokio::test]
async fn carrier_batches_revalidate_live_ownership_after_routes_are_registered() {
    let fixture = batch_router_fixture().await;
    fixture
        .router
        .activate_carrier_members(&fixture.members)
        .await
        .unwrap();
    for provider in &fixture.providers {
        provider.clear_calls();
    }
    fixture
        .workspace
        .publish_snapshot(verter_workspace::PublishedRoot::new_vfs_only(Arc::new(
            verter_workspace::WorkspaceSnapshot {
                owners_memo: Default::default(),
                projects: Vec::new(),
                resolver: verter_semantic::resolver_core::ModuleResolverCore::new(Vec::new()),
                generation: SnapshotGeneration(2),
            },
        )));
    assert!(fixture
        .router
        .activate_carrier_members(&fixture.members)
        .await
        .is_err());
    let paths: Vec<_> = fixture
        .members
        .iter()
        .map(|member| member.companion_path.clone())
        .collect();
    assert!(fixture
        .router
        .notify_carriers_changed(&paths)
        .await
        .is_err());
    for provider in &fixture.providers {
        assert!(
            provider.calls().is_empty(),
            "withdrawn ownership must not reach a cached engine"
        );
    }
}

fn write_typescript(root: &Path, version: &str) -> PathBuf {
    let lib = root.join("node_modules/typescript/lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("tsserver.js"), "// tsserver").unwrap();
    std::fs::write(lib.join("lib.es5.d.ts"), "interface Array<T> {}").unwrap();
    std::fs::write(
        root.join("node_modules/typescript/package.json"),
        format!(r#"{{ "name": "typescript", "version": "{version}" }}"#),
    )
    .unwrap();
    lib.join("tsserver.js").canonicalize().unwrap()
}

/// A pnpm-shaped install: the package's `node_modules/typescript` is a
/// SYMLINK into a workspace-level `.pnpm` store, exactly as pnpm lays a
/// monorepo out. Returns the REAL (canonical) `tsserver.js`.
#[cfg(unix)]
fn link_pnpm_typescript(workspace: &Path, package: &Path, version: &str) -> PathBuf {
    use std::os::unix::fs::symlink;
    let store = workspace
        .join("node_modules/.pnpm")
        .join(format!("typescript@{version}"))
        .join("node_modules/typescript");
    let lib = store.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("tsserver.js"), "// tsserver").unwrap();
    std::fs::write(lib.join("lib.es5.d.ts"), "interface Array<T> {}").unwrap();
    std::fs::write(
        store.join("package.json"),
        format!(r#"{{ "name": "typescript", "version": "{version}" }}"#),
    )
    .unwrap();
    std::fs::create_dir_all(package.join("node_modules")).unwrap();
    symlink(&store, package.join("node_modules/typescript")).unwrap();
    lib.join("tsserver.js").canonicalize().unwrap()
}

/// Every caller is a `#[cfg(unix)]` test (the pnpm-symlink fixtures), so the
/// helper is gated to match — on Windows it would otherwise be dead code and
/// fail the `-D warnings` clippy gate.
#[cfg(unix)]
fn write_tsconfig(project: &Path) {
    std::fs::create_dir_all(project).unwrap();
    std::fs::write(project.join("tsconfig.json"), r#"{ "include": ["src"] }"#).unwrap();
}

fn binding(workspace: &Path, project: &Path, id: u32) -> ProjectBinding {
    ProjectBinding::new_for_test(
        workspace.to_string_lossy().into_owned(),
        project.join("tsconfig.json").to_string_lossy().into_owned(),
        "",
        EnvDims {
            parse_env_hash: [id as u8; 16],
            resolve_env_hash: [id as u8; 16],
            lib_env_hash: [id as u8; 16],
            project_identity: ProjectIdentity([id as u8; 16]),
        },
        Vec::new(),
        ProjectId(id),
        SnapshotGeneration(1),
    )
}

fn engine_spec(
    backend: &TsserverEngineBackend,
    binding: &ProjectBinding,
    tsdk: Option<&str>,
) -> Result<ProjectEngineSpec, String> {
    let bound = ensure_bound(backend, binding).expect("the witness mint is infallible");
    resolve_engine_spec(&bound, binding, tsdk)
}

/// @ai-generated - Pins distinct engine identity for different owning projects.
///
/// The whole point of the router: two packages in ONE workspace, pinned to
/// DIFFERENT TypeScript versions, resolve to two DIFFERENT `tsserver.js`
/// installs — so they can never share one process.
///
/// The fixture also plants a THIRD, unrelated TypeScript at the WORKSPACE
/// ROOT. Resolving from the workspace root (the behaviour this router
/// replaced) would hand BOTH packages that root install — the assertions
/// below fail in exactly that case, so this test discriminates the
/// per-project resolution from the workspace-level one.
#[cfg(unix)]
#[test]
fn different_projects_keep_their_own_typescript_engines() {
    let workspace = tempfile::tempdir().unwrap();
    let project_a = workspace.path().join("packages/a");
    let project_b = workspace.path().join("packages/b");
    std::fs::create_dir_all(&project_a).unwrap();
    std::fs::create_dir_all(&project_b).unwrap();
    let root_install = write_typescript(workspace.path(), "5.0.4");
    // pnpm layout: package symlinks into the workspace `.pnpm` store, so the
    // resolution must canonicalize to the REAL versioned install — tsserver
    // finds its `lib.*.d.ts` relative to its own script path.
    let expected_a = link_pnpm_typescript(workspace.path(), &project_a, "5.8.3");
    let expected_b = link_pnpm_typescript(workspace.path(), &project_b, "6.0.2");
    let backend = TsserverEngineBackend::with_default_host_version();

    let spec_a = engine_spec(&backend, &binding(workspace.path(), &project_a, 0), None).unwrap();
    let spec_b = engine_spec(&backend, &binding(workspace.path(), &project_b, 1), None).unwrap();

    assert_eq!(Path::new(&spec_a.key.tsserver_path), expected_a);
    assert_eq!(Path::new(&spec_b.key.tsserver_path), expected_b);
    assert_ne!(
        spec_a.key.tsserver_path, spec_b.key.tsserver_path,
        "two packages pinned to different TypeScript versions must not share an engine"
    );
    assert_ne!(spec_a.key, spec_b.key);
    for spec in [&spec_a, &spec_b] {
        assert_ne!(
            Path::new(&spec.key.tsserver_path),
            root_install,
            "a package must be served by its OWN install, never the workspace root's"
        );
    }
    assert!(spec_a.default_lib_count > 0 && spec_b.default_lib_count > 0);
}

/// @ai-generated - NEGATIVE CONTROL: a project with no resolvable TypeScript
/// fails closed with the actionable install message and is NEVER served by a
/// sibling project's engine.
#[cfg(unix)]
#[test]
fn project_without_typescript_fails_closed_and_never_borrows_a_sibling_engine() {
    let workspace = tempfile::tempdir().unwrap();
    let served = workspace.path().join("packages/served");
    let bare = workspace.path().join("packages/bare");
    std::fs::create_dir_all(&served).unwrap();
    std::fs::create_dir_all(&bare).unwrap();
    let served_tsserver = link_pnpm_typescript(workspace.path(), &served, "6.0.2");
    let backend = TsserverEngineBackend::with_default_host_version();

    let served_spec = engine_spec(&backend, &binding(workspace.path(), &served, 0), None).unwrap();
    assert_eq!(Path::new(&served_spec.key.tsserver_path), served_tsserver);

    // The bare package's ancestor walk escapes the tempdir, so the assertion
    // is conditional on the machine genuinely having no ambient TypeScript
    // above it; when one exists the meaningful invariant is still checked —
    // the refusal (or the resolution) is NEVER the sibling's engine.
    match engine_spec(&backend, &binding(workspace.path(), &bare, 1), None) {
        Err(message) => {
            assert!(
                message.contains("no usable TypeScript installation was found"),
                "the refusal names the missing install: {message}"
            );
            assert!(
                message.contains("npm install -D typescript"),
                "the refusal carries the actionable install command: {message}"
            );
            assert!(
                !message.contains(&served_spec.key.tsserver_path),
                "the refusal must not point at the sibling project's engine: {message}"
            );
        }
        Ok(spec) => assert_ne!(
            spec.key.tsserver_path, served_spec.key.tsserver_path,
            "a project must never be served by another project's resolved engine"
        ),
    }
}

/// @ai-generated - The route-selection probe reports the workspace as
/// servable when ANY configured project can obtain TypeScript, and computes
/// the advisory from the LOWEST serving version (not the first one found).
#[cfg(unix)]
#[test]
fn workspace_probe_serves_on_any_project_and_advises_on_the_lowest_version() {
    let workspace = tempfile::tempdir().unwrap();
    let legacy = workspace.path().join("packages/legacy");
    let current = workspace.path().join("packages/current");
    let bare = workspace.path().join("packages/bare");
    write_tsconfig(&legacy);
    write_tsconfig(&current);
    write_tsconfig(&bare);
    link_pnpm_typescript(workspace.path(), &legacy, "5.8.3");
    link_pnpm_typescript(workspace.path(), &current, "6.0.2");

    let probe = probe_workspace_tsserver(&workspace.path().to_string_lossy(), None);

    let servable = probe.servable.as_ref().expect("a servable project exists");
    assert!(
        servable.resolved.default_lib_count > 0,
        "a library-less install is never reported servable"
    );
    // `packages/bare` sorts first and cannot resolve locally; the probe must
    // keep walking rather than reporting the workspace unservable.
    assert_eq!(probe.lowest_servable_version, Some((5, 8)));
    let advisory = probe.advisory().expect("a 5.8 package is advised");
    assert!(
        advisory.contains("5.8"),
        "the advisory names 5.8: {advisory}"
    );
    assert!(probe.native_family_only.is_none());
}

/// @ai-generated - A workspace whose ONLY resolvable TypeScript is the TS7+
/// native family is never served over the Node tsserver protocol.
#[cfg(unix)]
#[test]
fn workspace_probe_reports_native_family_only() {
    let workspace = tempfile::tempdir().unwrap();
    let native = workspace.path().join("packages/native");
    write_tsconfig(&native);
    link_pnpm_typescript(workspace.path(), &native, "7.0.0");

    let probe = probe_project_dirs(&[native.to_string_lossy().into_owned()], None);

    assert!(probe.servable.is_none());
    assert_eq!(probe.native_family_only, Some(7));
}

/// @ai-generated - Guards the non-pnpm (plain `node_modules`) layout too.
#[test]
fn plain_node_modules_install_resolves_for_its_own_project() {
    let workspace = tempfile::tempdir().unwrap();
    let project = workspace.path().join("packages/plain");
    std::fs::create_dir_all(&project).unwrap();
    let expected = write_typescript(&project, "6.0.2");
    let backend = TsserverEngineBackend::with_default_host_version();

    let spec = engine_spec(&backend, &binding(workspace.path(), &project, 0), None).unwrap();

    assert_eq!(Path::new(&spec.key.tsserver_path), expected);
}

// ─── Lifecycle updates and teardown reach hubs still establishing ────────────

/// An establisher that parks on a gate before handing out its engine — a hub
/// whose FIRST establishment is still in flight (`has_served() == false`).
struct GatedEngine {
    engine: Arc<dyn TypeProvider>,
    gate: Arc<tokio::sync::Semaphore>,
    entered: Arc<std::sync::atomic::AtomicUsize>,
}

impl crate::resilient_provider::ProviderEstablisher<dyn TypeProvider> for GatedEngine {
    fn log_name(&self) -> &'static str {
        "gated-tsserver"
    }

    fn user_label(&self) -> &'static str {
        "tsserver"
    }

    fn restarting_error(&self) -> &'static str {
        "gated tsserver is restarting"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn establish<'a>(
        &'a self,
        _crash_signal: Arc<tokio::sync::Notify>,
    ) -> crate::resilient_provider::EstablishFuture<'a, dyn TypeProvider> {
        self.entered
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let engine = Arc::clone(&self.engine);
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            gate.acquire()
                .await
                .map_err(|_| TypeProviderError::new("gate closed"))?
                .forget();
            Ok(engine)
        })
    }
}

/// Spin (cooperatively) until `cond` holds, failing loudly instead of hanging.
async fn await_until(mut cond: impl FnMut() -> bool, what: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !cond() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("condition never held: {what}"));
}

/// Insert a hub whose first establishment parks on a gate, mirroring the
/// in-flight establishment `provider_for_binding` leaves behind when a cold
/// demand is still resolving. Returns the hub, its engine, the gate and the
/// in-flight demand's join handle.
async fn cold_establishing_hub(
    router: &ProjectTsserverProvider,
) -> (
    Arc<ProviderHub<dyn TypeProvider>>,
    Arc<MockTypeProvider>,
    Arc<tokio::sync::Semaphore>,
    tokio::task::JoinHandle<
        Result<verter_type_runtime::provider_hub::ProviderEpoch, TypeProviderError>,
    >,
) {
    let engine = Arc::new(MockTypeProvider::new());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let entered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hub = Arc::new(ProviderHub::new(
        GatedEngine {
            engine: engine.clone(),
            gate: Arc::clone(&gate),
            entered: Arc::clone(&entered),
        },
        Arc::new(verter_type_runtime::provider_hub::TracingNotifier),
        crate::resilient_provider::HubPolicy::explicit(3),
    ));
    router.providers.insert(
        ProjectEngineKey {
            project: "/cold/tsconfig.json".to_string(),
            tsserver_path: "/cold/tsserver.js".to_string(),
        },
        Arc::clone(&hub),
    );
    // The cold demand runs detached, exactly like the in-flight establishment
    // a `provider_for_binding` call leaves behind.
    let demand = tokio::spawn({
        let hub = Arc::clone(&hub);
        async move { hub.establish().await }
    });
    await_until(
        || entered.load(std::sync::atomic::Ordering::SeqCst) == 1,
        "the cold establishment started",
    )
    .await;
    (hub, engine, gate, demand)
}

/// Router shutdown must reach a hub whose first establishment is still in
/// flight: the shutdown abandons it (the install is rejected and the engine
/// torn down) instead of letting a fresh engine come live — orphaned — after
/// teardown already returned.
#[tokio::test]
async fn shutdown_reaches_a_hub_whose_first_establishment_is_in_flight() {
    let fixture = batch_router_fixture().await;
    let (hub, _engine, gate, demand) = cold_establishing_hub(&fixture.router).await;
    // The demand is still parked on the gate, exactly like a cold
    // `provider_for_binding` establishment that has not resolved yet.
    TypeProvider::shutdown(&fixture.router).await.unwrap();

    gate.add_permits(1);
    let outcome = demand
        .await
        .expect("the abandoned establishment must still settle");
    assert!(
        outcome.is_err(),
        "an establishment abandoned by router shutdown must fail, got {outcome:?}"
    );
    assert!(
        !hub.has_served(),
        "a hub shut down mid-establishment must not end up with a served engine"
    );
}

/// A workspace-folder update that lands while a hub is still establishing must
/// be recorded in that hub's desired state, so the engine it later installs is
/// replayed WITH the folder update instead of operating without its project
/// roots.
#[tokio::test]
async fn workspace_folder_updates_reach_a_hub_whose_first_establishment_is_in_flight() {
    let fixture = batch_router_fixture().await;
    let (_hub, engine, gate, demand) = cold_establishing_hub(&fixture.router).await;

    fixture
        .router
        .update_workspace_folders(vec![serde_json::json!({ "uri": "file:///ws" })], vec![])
        .await
        .unwrap();

    gate.add_permits(1);
    demand
        .await
        .expect("the establishment must still settle")
        .expect("the released establishment must install its engine");

    assert!(
        engine.calls().iter().any(
            |call| matches!(call, MockCall::UpdateWorkspaceFolders { added, .. }
                if added.iter().any(|folder| folder.get("uri") == Some(&serde_json::json!("file:///ws"))))
        ),
        "the folder update recorded while the hub was still establishing must reach its \
         engine through the replay, got {:?}",
        engine.calls()
    );
}
