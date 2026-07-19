use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use verter_type_runtime::traits::TypeProvider;

use super::lazy_managed::LazyManagedTypeProvider;
use super::mock::{MockCall, MockTypeProvider};

/// Preflight for the real lazy-recovery test: node, a resolvable tsserver.js,
/// the workspace esbuild binary, and the typescript-plugin dependencies must all
/// be present before the test's hard expectations are meaningful.
fn real_lazy_toolchain_available(repo_root: &std::path::Path) -> bool {
    if verter_type_runtime::find_node().is_none()
        || !repo_root.join("node_modules/esbuild/bin/esbuild").is_file()
        || !repo_root
            .join("packages")
            .join("typescript-plugin")
            .join("node_modules")
            .is_dir()
    {
        return false;
    }
    let root = repo_root.to_string_lossy();
    if verter_type_runtime::find_tsserver(None, Some(&root)).is_some() {
        return true;
    }
    let pnpm_store = repo_root.join("node_modules/.pnpm");
    std::fs::read_dir(&pnpm_store)
        .map(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                let name = entry.file_name();
                name.to_string_lossy().starts_with("typescript@")
                    && entry
                        .path()
                        .join("node_modules/typescript/lib/tsserver.js")
                        .is_file()
            })
        })
        .unwrap_or(false)
}

async fn build_source_plugin_probe(
    repo_root: &std::path::Path,
    fixture_root: &std::path::Path,
    node_path: &str,
) -> String {
    let plugin_probe = fixture_root.join("plugin-probe").join("node_modules");
    let plugin_package = plugin_probe.join("@verter").join("typescript-plugin");
    let plugin_entry = plugin_package.join("dist").join("index.js");
    assert!(
        !plugin_entry.exists(),
        "source-built lazy plugin fixture must start without dist"
    );
    std::fs::create_dir_all(plugin_entry.parent().expect("lazy plugin dist parent"))
        .expect("create source-built lazy plugin package");
    std::fs::write(
        plugin_package.join("package.json"),
        r#"{"name":"@verter/typescript-plugin","version":"0.0.0-test","type":"commonjs","main":"dist/index.js"}"#,
    )
    .expect("write source-built lazy plugin package.json");

    let dependency_link = plugin_package.join("node_modules");
    let workspace_dependencies = std::fs::canonicalize(
        repo_root
            .join("packages")
            .join("typescript-plugin")
            .join("node_modules"),
    )
    .expect("canonical lazy workspace plugin dependencies");
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&dependency_link)
            .arg(&workspace_dependencies)
            .output()
            .expect("create lazy plugin dependency junction");
        assert!(
            output.status.success(),
            "create lazy plugin dependency junction: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&workspace_dependencies, &dependency_link)
        .expect("create lazy plugin dependency symlink");

    let alias = format!(
        "--alias:@verter/language-shared={}",
        repo_root
            .join("packages/language-shared/src/index.ts")
            .to_string_lossy()
            .replace('\\', "/")
    );
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(node_path)
            .arg(repo_root.join("node_modules/esbuild/bin/esbuild"))
            .arg(repo_root.join("packages/typescript-plugin/src/index.ts"))
            .args([
                "--bundle",
                "--platform=node",
                "--format=cjs",
                "--target=node18",
            ])
            .arg(alias)
            .arg(format!("--outfile={}", plugin_entry.to_string_lossy()))
            .output(),
    )
    .await
    .expect("lazy source plugin build exceeded 30 seconds")
    .expect("run workspace esbuild for lazy plugin");
    assert!(
        output.status.success(),
        "build lazy production plugin source: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        plugin_entry.is_file(),
        "lazy source plugin emitted no entry"
    );
    plugin_probe.to_string_lossy().into_owned()
}

#[tokio::test]
async fn lifecycle_is_cached_without_activation_and_replayed_once_on_first_query() {
    let spawn_count = Arc::new(AtomicUsize::new(0));
    let managed = Arc::new(MockTypeProvider::new());
    let provider = LazyManagedTypeProvider::new({
        let spawn_count = Arc::clone(&spawn_count);
        let managed = Arc::clone(&managed);
        move || {
            let spawn_count = Arc::clone(&spawn_count);
            let managed = Arc::clone(&managed);
            async move {
                spawn_count.fetch_add(1, Ordering::SeqCst);
                Ok(managed as Arc<dyn TypeProvider>)
            }
        }
    });

    provider
        .open_file("/w/App.vue.tsx", "const before = 1")
        .await
        .unwrap();
    provider
        .update_file("/w/App.vue.tsx", "const current = 2")
        .await
        .unwrap();
    provider
        .load_file("/w/Closed.vue.tsx", "export const gone = true")
        .await
        .unwrap();
    provider.close_file("/w/Closed.vue.tsx").await.unwrap();
    provider
        .configure_paths("/w", serde_json::json!({ "@/*": ["src/*"] }))
        .await
        .unwrap();

    assert_eq!(
        spawn_count.load(Ordering::SeqCst),
        0,
        "file lifecycle must not start the managed fallback"
    );
    assert!(
        managed.calls().is_empty(),
        "nothing reaches managed tsgo before an observed fallback demand"
    );

    provider.get_hover("/w/App.vue.tsx", 8).await.unwrap();
    provider.get_hover("/w/App.vue.tsx", 8).await.unwrap();

    assert_eq!(
        spawn_count.load(Ordering::SeqCst),
        1,
        "repeated fallback demands reuse exactly one managed provider"
    );
    let calls = managed.calls();
    assert!(
        matches!(calls.first(), Some(MockCall::ConfigurePaths { .. })),
        "configuration must replay before file state: {calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, content }
                if path == "/w/App.vue.tsx" && content == "const current = 2"
        )),
        "activation must replay the latest desired open state before the query: {calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::LoadFile { path, .. }
                if path == "/w/Closed.vue.tsx"
        )),
        "a file closed before activation must not be replayed: {calls:?}"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, MockCall::GetHover { .. }))
            .count(),
        2
    );
}

#[tokio::test]
async fn concurrent_first_queries_singleflight_the_managed_factory() {
    let spawn_count = Arc::new(AtomicUsize::new(0));
    let managed = Arc::new(MockTypeProvider::new());
    let provider = Arc::new(LazyManagedTypeProvider::new({
        let spawn_count = Arc::clone(&spawn_count);
        let managed = Arc::clone(&managed);
        move || {
            let spawn_count = Arc::clone(&spawn_count);
            let managed = Arc::clone(&managed);
            async move {
                spawn_count.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                Ok(managed as Arc<dyn TypeProvider>)
            }
        }
    }));

    let (left, right) = tokio::join!(
        provider.get_hover("/w/App.vue.tsx", 1),
        provider.get_hover("/w/App.vue.tsx", 2)
    );
    left.unwrap();
    right.unwrap();
    assert_eq!(spawn_count.load(Ordering::SeqCst), 1);
}

/// D2: a transient activation failure is NOT latched for the session. Within the
/// cooldown the cached error is returned WITHOUT re-running the factory (storm
/// protection); after the cooldown the factory is retried and the provider
/// recovers, so the next fallback query answers.
#[tokio::test]
async fn failed_activation_retries_after_cooldown_and_recovers() {
    struct RecoveryCarrier {
        source_path: &'static str,
        companion_path: &'static str,
        content: &'static str,
        hover_offset: u32,
    }
    const CARRIERS: [RecoveryCarrier; 2] = [
        RecoveryCarrier {
            source_path: "/w/Recovery.vue",
            companion_path: "/w/Recovery.vue.tsx",
            content: "const vueRecoveryValue: string = 'vue';\nvueRecoveryValue;\nexport {};\n",
            hover_offset: 6,
        },
        RecoveryCarrier {
            source_path: "/w/Recovery.svelte",
            companion_path: "/w/Recovery.svelte.tsx",
            content: "const svelteRecoveryValue: number = 42;\nsvelteRecoveryValue;\nexport {};\n",
            hover_offset: 6,
        },
    ];

    let attempts = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let managed = Arc::new(MockTypeProvider::new());
    let provider = LazyManagedTypeProvider::new({
        let attempts = Arc::clone(&attempts);
        let fail = Arc::clone(&fail);
        let managed = Arc::clone(&managed);
        move || {
            let attempts = Arc::clone(&attempts);
            let fail = Arc::clone(&fail);
            let managed = Arc::clone(&managed);
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                if fail.load(Ordering::SeqCst) {
                    Err(verter_type_runtime::protocol::TypeProviderError::new(
                        "managed tsgo unavailable",
                    ))
                } else {
                    Ok(managed as Arc<dyn TypeProvider>)
                }
            }
        }
    });

    for fixture in &CARRIERS {
        provider
            .register_carrier_member(
                fixture.source_path,
                fixture.companion_path,
                fixture.content,
                "/w/tsconfig.json",
            )
            .await
            .unwrap();
    }
    // First activation fails (attempt 1).
    assert!(provider
        .get_hover(CARRIERS[0].companion_path, CARRIERS[0].hover_offset)
        .await
        .is_err());
    // Immediate retry: cached error, NO new factory run (storm protection).
    assert!(provider
        .get_hover(CARRIERS[1].companion_path, CARRIERS[1].hover_offset)
        .await
        .is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a failure within the cooldown is returned without re-running the factory"
    );
    provider.shutdown().await.unwrap();
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "shutdown after a failed activation must never retry or spawn the managed provider"
    );

    // After the cooldown the factory is retried; now it succeeds.
    fail.store(false, Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(
        super::lazy_managed::ACTIVATION_RETRY_COOLDOWN.as_millis() as u64 + 80,
    ))
    .await;
    for fixture in &CARRIERS {
        provider
            .get_hover(fixture.companion_path, fixture.hover_offset)
            .await
            .expect("a transient activation failure must recover after the cooldown");
    }
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "the factory is retried exactly once after the cooldown"
    );

    let calls = managed.calls();
    for fixture in &CARRIERS {
        let replay = calls
            .iter()
            .position(|call| matches!(
                call,
                MockCall::RegisterCarrierMember { source_path, companion_path, content, project_file_name }
                    if source_path == fixture.source_path
                        && companion_path == fixture.companion_path
                        && content == fixture.content
                        && project_file_name == "/w/tsconfig.json"
            ))
            .unwrap_or_else(|| panic!("{} carrier state was not replayed: {calls:?}", fixture.source_path));
        let typed_query = calls
            .iter()
            .position(|call| {
                matches!(
                    call,
                    MockCall::GetHover { path, offset }
                        if path == fixture.companion_path && *offset == fixture.hover_offset
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "{} carrier received no typed query: {calls:?}",
                    fixture.source_path
                )
            });
        assert!(
            replay < typed_query,
            "{} carrier replay must complete before its typed query: {calls:?}",
            fixture.source_path
        );
    }

    provider.shutdown().await.unwrap();
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "shutdown of the recovered provider must never spawn another managed provider"
    );
}

#[tokio::test]
async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries() {
    // verter_lsp real-provider convention: a missing workspace toolchain is an
    // honest skip-with-reason, never a silent pass — every assertion below is
    // unchanged when the toolchain is present.
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root above crates/");
    if !real_lazy_toolchain_available(repo_root) {
        eprintln!(
            "skipping (real lazy recovery): workspace node/tsserver/esbuild toolchain unavailable"
        );
        return;
    }

    struct Carrier {
        source_path: String,
        companion_path: String,
        content: &'static str,
        hover_offset: u32,
        expected_hover: &'static str,
    }

    let project = tempfile::tempdir().expect("create real lazy recovery project");
    std::fs::write(
        project.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"jsx":"preserve"},"include":["*.tsx"]}"#,
    )
    .expect("write real lazy recovery tsconfig");
    let fixture_rows = [
        (
            "Recovery.vue",
            "Recovery.vue.tsx",
            "export const vueRecoveryValue: string = 'vue';\nvueRecoveryValue;\n",
            "export const vueRecoveryValue = null;\nvueRecoveryValue;\n",
            "const vueRecoveryValue: string",
        ),
        (
            "Recovery.svelte",
            "Recovery.svelte.tsx",
            "export const svelteRecoveryValue: number = 42;\nsvelteRecoveryValue;\n",
            "export const svelteRecoveryValue = null;\nsvelteRecoveryValue;\n",
            "const svelteRecoveryValue: number",
        ),
    ];
    let carriers: Vec<_> = fixture_rows
        .into_iter()
        .map(
            |(source_name, companion_name, content, stale_disk_content, expected_hover)| {
                let source_path = project.path().join(source_name);
                let companion_path = project.path().join(companion_name);
                std::fs::write(&companion_path, stale_disk_content)
                    .expect("write stale lazy carrier bytes");
                Carrier {
                    source_path: source_path.to_string_lossy().replace('\\', "/"),
                    companion_path: companion_path.to_string_lossy().replace('\\', "/"),
                    content,
                    hover_offset: 13,
                    expected_hover,
                }
            },
        )
        .collect();

    let workspace_root = project.path().to_string_lossy().replace('\\', "/");
    let project_file_name = project
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let carrier_store_dir = project.path().join("carrier-store");
    let blobs_dir = carrier_store_dir.join("blobs");
    std::fs::create_dir_all(&blobs_dir).expect("create lazy carrier blob store");
    std::fs::create_dir_all(carrier_store_dir.join("maps")).expect("create lazy carrier map store");
    let mut owned_sources = Vec::new();
    let mut ready_files = serde_json::Map::new();
    for carrier in &carriers {
        let digest = blake3::hash(carrier.content.as_bytes());
        let content_hash = digest.as_bytes()[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let blob_name = format!("blake3-{content_hash}.tsx");
        std::fs::write(blobs_dir.join(&blob_name), carrier.content)
            .expect("publish lazy carrier blob");
        owned_sources.push(serde_json::json!({
            "source_uri": carrier.source_path,
            "provider_uri": carrier.companion_path,
            "role": "CarrierIde",
            "script_kind": "TSX",
        }));
        ready_files.insert(
            carrier.companion_path.clone(),
            serde_json::json!({
                "content_hash": content_hash,
                "version": 1,
                "script_kind": "TSX",
                "role": "CarrierIde",
                "map_hash": "00000000000000000000000000000000",
                "blob_rel": format!("blobs/{blob_name}"),
            }),
        );
    }
    let mut projects = serde_json::Map::new();
    projects.insert(
        project_file_name.clone(),
        serde_json::json!({
            "owned_sources": owned_sources,
            "ready_files": ready_files,
        }),
    );
    std::fs::write(
        carrier_store_dir.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "epoch": 1,
            "host_version": "real-lazy-recovery-test",
            "projects": projects,
        }))
        .expect("serialize lazy carrier manifest"),
    )
    .expect("publish lazy carrier manifest");
    let node_path = verter_type_runtime::find_node()
        .expect("real lazy recovery test requires the workspace Node.js runtime");
    let tsserver_path = {
        let root = repo_root.to_string_lossy();
        verter_type_runtime::find_tsserver(None, Some(&root)).unwrap_or_else(|| {
            let pnpm_store = repo_root.join("node_modules/.pnpm");
            let mut candidates = std::fs::read_dir(&pnpm_store)
                .unwrap_or_else(|error| panic!("read {}: {error}", pnpm_store.display()))
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("typescript@")
                })
                .map(|entry| entry.path().join("node_modules/typescript/lib/tsserver.js"))
                .filter(|path| path.is_file())
                .collect::<Vec<_>>();
            candidates.sort();
            candidates
                .pop()
                .expect("real lazy recovery test requires the workspace tsserver.js")
        })
    }
    .to_string_lossy()
    .into_owned();
    let plugin_path = build_source_plugin_probe(repo_root, project.path(), &node_path).await;
    let carrier_store_dir = carrier_store_dir.to_string_lossy().into_owned();

    let attempts = Arc::new(AtomicUsize::new(0));
    let provider = LazyManagedTypeProvider::new({
        let attempts = Arc::clone(&attempts);
        let node_path = node_path.clone();
        let tsserver_path = tsserver_path.clone();
        let workspace_root = workspace_root.clone();
        let plugin_path = plugin_path.clone();
        let carrier_store_dir = carrier_store_dir.clone();
        move || {
            let attempts = Arc::clone(&attempts);
            let node_path = node_path.clone();
            let tsserver_path = tsserver_path.clone();
            let workspace_root = workspace_root.clone();
            let plugin_path = plugin_path.clone();
            let carrier_store_dir = carrier_store_dir.clone();
            async move {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    return Err(verter_type_runtime::protocol::TypeProviderError::new(
                        "managed tsserver unavailable",
                    ));
                }
                let real = verter_type_runtime::tsserver::TsserverTypeProvider::spawn(
                    &node_path,
                    &tsserver_path,
                    &workspace_root,
                    Some(&plugin_path),
                    Some(&carrier_store_dir),
                    false,
                    None,
                )
                .await?;
                Ok(Arc::new(real) as Arc<dyn TypeProvider>)
            }
        }
    });

    for carrier in &carriers {
        // Deliberately do not cache an ordinary desired file. The replayed
        // contentless carrier registration creates membership; the plugin/store
        // is the only source of bytes observed by the TypeScript engine.
        provider
            .register_carrier_member(
                &carrier.source_path,
                &carrier.companion_path,
                carrier.content,
                &project_file_name,
            )
            .await
            .expect("cache real lazy carrier registration");
    }

    assert!(
        provider
            .get_hover(&carriers[0].companion_path, carriers[0].hover_offset)
            .await
            .is_err(),
        "first real managed activation is intentionally unavailable"
    );
    provider
        .shutdown()
        .await
        .expect("shutdown failed activation");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "shutdown must not retry or spawn the real provider"
    );

    tokio::time::sleep(
        super::lazy_managed::ACTIVATION_RETRY_COOLDOWN + std::time::Duration::from_millis(80),
    )
    .await;
    for carrier in &carriers {
        let mut last = None;
        for delay_ms in [0u64, 100, 250, 500, 1000] {
            if delay_ms != 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            last = provider
                .get_hover(&carrier.companion_path, carrier.hover_offset)
                .await
                .expect("real lazy typed query must reach tsserver");
            if last.as_ref().is_some_and(|hover| {
                hover.contents.contains(carrier.expected_hover) && !hover.contents.contains(": any")
            }) {
                break;
            }
        }
        let hover = last.unwrap_or_else(|| {
            panic!(
                "real tsserver returned no hover for lazy-replayed {}",
                carrier.source_path
            )
        });
        assert!(
            hover.contents.contains(carrier.expected_hover),
            "real tsserver must derive {} from lazy-replayed bytes, got {}",
            carrier.expected_hover,
            hover.contents
        );
        assert!(!hover.contents.contains(": any"));
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    provider
        .shutdown()
        .await
        .expect("shutdown real lazy provider");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "shutdown must never spawn a third real provider"
    );
}
