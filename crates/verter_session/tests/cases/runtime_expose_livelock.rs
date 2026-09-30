//! Livelock reproduction: the Vue macro codegen's runtime-object
//! `defineExpose` projection never finishes on a `ref(0)` binding.
//!
//! `<script setup>` with `const count = ref(0)` (`ref` from the real `vue`
//! package) and `defineExpose({ count })` spins one worker at full CPU inside
//! the expose projection (`project_expose_runtime_object` →
//! `reduce_output_node_with_context` → the query frames → the locator-view
//! worklist / `drain_projection` / canonical intersection algebra), and the
//! `verter-tsc` run that demands it never ends. The same component with a
//! DECLARED `Ref<number>`, with `shallowRef(0)`, with a function or method
//! member instead of the ref, or with the ref used but not exposed, finishes
//! at once.
//!
//! The smallest trigger found is generic inference through the real `vue`
//! `UnwrapRef`: `declare function r<T>(value: T): UnwrapRef<T>` with
//! `const count = r(0)` exposed spins, while `UnwrapRef<number>` written out,
//! or `r<T>(value: T): Ref<T>`, does not. A local copy of `Ref`, `UnwrapRef` /
//! `UnwrapRefSimple`, both `ref` overloads and the `RefUnwrapBailTypes`
//! members (the runtime-dom `DomType<Node | Window>`, the runtime-core
//! `VNode | { $: ComponentInternalInstance }`) declared in ONE module does not
//! spin; the remaining difference is `vue`'s re-export chain and the
//! cross-module augmentation of `RefUnwrapBailTypes`.
//!
//! The reproduction drives the public-API projection (the TSC macro demand
//! whose runtime-object expose lane spins), as `verter-tsc` does, on a
//! worker thread under a hard deadline, so it FAILS naming the livelock
//! instead of hanging the suite. It needs the workspace's installed
//! `node_modules` (the `vue` package), or `VERTER_TEST_NODE_MODULES`.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

/// Far above the milliseconds the component takes once the loop is fixed;
/// only a livelocked projection reaches it.
const DEADLINE: Duration = Duration::from_secs(30);

const EXPOSE_A_REF: &str = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
defineExpose({ count })
</script>
"#;

const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "strict": true,
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "skipLibCheck": true
  },
  "include": ["src"]
}
"#;

/// The installed packages: the workspace's `node_modules`, or the directory
/// `VERTER_TEST_NODE_MODULES` names (a worktree without its own install).
fn workspace_node_modules() -> PathBuf {
    let node_modules = std::env::var_os("VERTER_TEST_NODE_MODULES").map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("crate is <workspace>/crates/verter_session")
                .join("node_modules")
        },
        PathBuf::from,
    );
    assert!(
        node_modules.join("vue").exists(),
        "this reproduction needs the installed `vue` package under {} (run pnpm install)",
        node_modules.display()
    );
    node_modules
}

#[cfg(windows)]
fn windows_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    text.strip_prefix(r"\\?\")
        .map(str::to_string)
        .unwrap_or(text)
}

#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(windows_path(link))
        .arg(windows_path(target))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("mklink runs");
    assert!(
        status.success(),
        "junction {} -> {}",
        link.display(),
        target.display()
    );
}

#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("symlink node_modules");
}

fn slash_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("//?/")
        .map(str::to_string)
        .unwrap_or(text)
}

/// Drive the macro codegen the way `verter-tsc` does: a filesystem
/// workspace over the project, the batch-typecheck host, the carrier
/// upserted, then its public API demanded (the TSC macro demand).
#[test]
fn exposing_a_ref_finishes_the_runtime_object_expose_projection() {
    let node_modules = workspace_node_modules();
    let project = tempfile::tempdir().expect("temp project");
    let root = project.path().canonicalize().expect("canonical temp root");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("tsconfig.json"), TSCONFIG).unwrap();
    let carrier = root.join("src").join("Expose.vue");
    std::fs::write(&carrier, EXPOSE_A_REF).unwrap();
    let link = root.join("node_modules");
    link_dir(&node_modules, &link);

    let host = Arc::new(VerterHost::new_standalone(HostConfig::batch_typecheck()));
    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let snapshot = verter_workspace::build_selected_project_snapshot(
        workspace.as_ref(),
        &slash_path(&root.join("tsconfig.json")),
        &slash_path(&root),
        verter_workspace::workspace_snapshot::SnapshotGeneration(1),
    );
    workspace.publish_snapshot(
        verter_workspace::published_state::PublishedRoot::new_vfs_only(Arc::new(snapshot)),
    );
    host.set_workspace(workspace as Arc<dyn verter_workspace::WorkspaceAccess>);
    let canonical = verter_span::path::canonicalize_path(&slash_path(&carrier));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.clone()),
            input_id: canonical.clone(),
            source: Arc::from(EXPOSE_A_REF),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("the carrier upserts");

    let (done_tx, done_rx) = mpsc::channel();
    let worker_host = Arc::clone(&host);
    let worker_canonical = canonical.clone();
    std::thread::spawn(move || {
        let responses = worker_host.get_public_api_batch(&[worker_canonical.as_str()]);
        let _ = done_tx.send(responses.iter().all(Result::is_ok));
    });
    let finished = done_rx.recv_timeout(DEADLINE);

    // Unlink the junction before the temp directory is removed, so removal
    // never walks into the workspace's `node_modules`.
    #[cfg(windows)]
    let _ = std::fs::remove_dir(&link);
    #[cfg(unix)]
    let _ = std::fs::remove_file(&link);

    match finished {
        Ok(produced) => assert!(produced, "the macro semantics are produced"),
        Err(_) => panic!(
            "livelock: the runtime-object expose projection of `defineExpose({{ count }})` \
             over `const count = ref(0)` did not finish within {DEADLINE:?}"
        ),
    }
}
