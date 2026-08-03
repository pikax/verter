//! Hermetic fixture staging and type-only package materialization for
//! real-provider fixtures.
//!
//! A fixture's authored tree is git-tracked source; its `node_modules` is
//! gitignored, shared with the VS Code E2E suite, and populated by whatever ran
//! last on the machine. Materializing INTO that directory made the resolved
//! dependency surface the union of every revision that ever wrote there, so a
//! four-month-old package could decide a test outcome while remaining invisible
//! to `git status`.
//!
//! Fixtures are therefore STAGED: the authored tree is copied into a
//! per-process directory that EXCLUDES `node_modules`, and dependencies are
//! materialized into the copy. Pre-existing content cannot reach a run, because
//! a run never reads the directory that accumulates it.

/// Resolve an E2E fixture workspace root as a canonical path.
///
/// The returned root is a per-process STAGED copy of the authored fixture, not
/// the authored tree itself. Everything the harness writes for a fixture —
/// dependency materialization, vendored packages, files a test edits — lands in
/// the copy, so no run inherits state from a previous one and the authored tree
/// is never mutated.
pub(crate) fn fixture_workspace_root(name: &str) -> String {
    crate::test_utils::canonical_test_path(&staged_fixture_root(name))
}

/// The git-tracked fixture source. Read-only: staging copies OUT of it.
fn authored_fixture_root(name: &str) -> std::path::PathBuf {
    std::fs::canonicalize(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../packages/vue-vscode/e2e/fixtures/{name}")),
    )
    .unwrap_or_else(|error| panic!("canonicalize authored fixture `{name}`: {error}"))
}

/// Stage a fixture once per process, memoized so every consumer of a fixture
/// name inside one test process agrees on the same root.
fn staged_fixture_root(name: &str) -> std::path::PathBuf {
    static STAGED: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, std::path::PathBuf>>,
    > = std::sync::OnceLock::new();
    let mut staged = STAGED
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(existing) = staged.get(name) {
        return existing.clone();
    }
    let destination = stage_area().join(name);
    copy_authored_tree(&authored_fixture_root(name), &destination);
    let destination = std::fs::canonicalize(&destination).unwrap_or_else(|error| {
        panic!(
            "canonicalize staged fixture {}: {error}",
            destination.display()
        )
    });
    staged.insert(name.to_string(), destination.clone());
    destination
}

/// A per-process staging area. Process identity varies the path for the same
/// reason the carrier-store test roots do: under one-test-per-process execution
/// a shared directory is shared mutable state between concurrent runs.
fn stage_area() -> std::path::PathBuf {
    static AREA: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    AREA.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let root = std::env::temp_dir().join("verter-fixture-stage");
        let pending_cutoff = std::time::SystemTime::now()
            .checked_sub(STAGE_PENDING_TTL)
            .unwrap_or(std::time::UNIX_EPOCH);
        reap_unowned_stage_areas(&root, pending_cutoff);
        claim_stage_area(&root, &format!("{}-{nanos}", std::process::id()))
    })
    .clone()
}

/// Prefix of the lease file naming a stage area. Its lock — not its contents —
/// is the liveness signal; the recorded pid is for a human reading the directory.
///
/// The lease sits BESIDE the area it names, never inside it, because a lease
/// inside cannot coexist with publishing by rename: Windows refuses to rename a
/// directory while any handle anywhere in its subtree is open. That is not a
/// share-mode question — `FILE_SHARE_DELETE` is refused identically, as is an
/// unlocked handle and one nested several levels down — so no way of opening the
/// file makes an inside lease renameable, and only moving it out does.
///
/// One lease spans an area's whole life: it is keyed to the id, which the
/// construction-to-published rename does not change.
const STAGE_LEASE_PREFIX: &str = "lease-";

/// Lease name carried by areas that keep their lease INSIDE themselves.
///
/// Never written. Retained for the same reason [`STAGE_LEGACY`] is: such areas
/// are already on disk, and a reaper that cannot read their lease cannot judge
/// them — it would treat every one as owned forever and orphan it.
const STAGE_LEASE: &str = ".verter-stage-lease";

/// Prefix of a PUBLISHED stage area: locked, in use, and scanned by the reaper.
const STAGE_PUBLISHED: &str = "area-";

/// Prefix used before areas were published by rename. Retained only so those
/// areas can still be reclaimed rather than orphaned.
const STAGE_LEGACY: &str = "test-";

/// Prefix of an area still being constructed. Distinct from the published
/// prefix so that publication can be an atomic rename of an ALREADY-LOCKED
/// area: nothing is ever visible under the published name unlocked. The reaper
/// does scan this prefix, and honours a held lease here exactly as it does for
/// a published area — only an unowned AND aged pending area is debris.
const STAGE_PENDING: &str = "pending-";

/// Create this process's stage area and publish it ALREADY LOCKED.
///
/// The lock must exist before the area is discoverable, or reclamation races
/// creation: a reaper that finds the area between `create_dir_all` and the lock
/// sees no held lease, deletes it, and on POSIX the creator then locks a file
/// that is already unlinked — it goes on to rebuild its workspace with no
/// visible lease, and a later reaper removes the LIVE area. Windows closes the
/// window no better.
///
/// So the lease is taken FIRST, at a path outside every directory this function
/// creates, and the area is then built under the pending prefix and RENAMED into
/// the published name. The reaper scans pending areas too — the safety rail is
/// the already-held lease that marks them owned, not invisibility — and the
/// rename only publishes an id that lease already owns. The lease is keyed to
/// the id rather than to a location, so it names the area across that rename
/// without living in it:
/// the published name is owned from before it exists, and there is no interval in
/// which a published area is unlocked.
///
/// The lease must stay OUT of the renamed directory. A rename target holding any
/// open handle is refused on Windows — see [`STAGE_LEASE_PREFIX`] — so a lease
/// inside the area would make publication fail on one supported platform while
/// the ordering above stayed correct on the other two.
fn claim_stage_area(root: &std::path::Path, id: &str) -> std::path::PathBuf {
    // EVERY claim retains its own lease. A single-slot `OnceLock` silently
    // dropped the second claim's file — releasing its lock and leaving a live
    // area advertised as unowned, reapable by any other process.
    static LEASES: std::sync::Mutex<Vec<std::fs::File>> = std::sync::Mutex::new(Vec::new());
    let pending = root.join(format!("{STAGE_PENDING}{id}"));
    let published = root.join(format!("{STAGE_PUBLISHED}{id}"));
    let lease_path = stage_lease_path(root, id);
    // The lease is a sibling of the area, so the root has to exist before it can
    // be created — creating the area no longer brings the root along with it.
    std::fs::create_dir_all(root)
        .unwrap_or_else(|error| panic!("create staging root {}: {error}", root.display()));
    let lease = std::fs::File::create(&lease_path)
        .unwrap_or_else(|error| panic!("create stage lease {}: {error}", lease_path.display()));
    lease
        .lock()
        .unwrap_or_else(|error| panic!("lock stage lease {}: {error}", lease_path.display()));
    use std::io::Write;
    let _ = (&lease).write_all(std::process::id().to_string().as_bytes());
    // Retained BEFORE anything is visible, so the retention window strictly
    // contains the visibility window. Held for the process lifetime: statics are
    // never dropped, which is exactly the lifetime these locks need.
    LEASES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(lease);
    std::fs::create_dir_all(&pending)
        .unwrap_or_else(|error| panic!("create stage area {}: {error}", pending.display()));
    std::fs::rename(&pending, &published)
        .unwrap_or_else(|error| panic!("publish stage area {}: {error}", published.display()));
    published
}

/// Where the lease naming area `id` under `root` lives.
fn stage_lease_path(root: &std::path::Path, id: &str) -> std::path::PathBuf {
    root.join(format!("{STAGE_LEASE_PREFIX}{id}"))
}

/// The sibling lease naming `area`, when its directory name carries an id.
fn sibling_lease_for(area: &std::path::Path) -> Option<std::path::PathBuf> {
    let root = area.parent()?;
    let name = area.file_name()?.to_string_lossy();
    let id = [STAGE_PUBLISHED, STAGE_PENDING, STAGE_LEGACY]
        .into_iter()
        .find_map(|prefix| name.strip_prefix(prefix))?;
    Some(stage_lease_path(root, id))
}

/// How long ABANDONED construction debris may sit before it is reclaimed.
///
/// Applies to the construction prefix, which lives for microseconds, and to a
/// lease whose area is gone. A published area is NEVER judged by age.
const STAGE_PENDING_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Reclaim stage areas whose owning process has exited.
///
/// One area per test process is created and nothing else deletes them, so
/// without this the staging root grows without bound — a slower version of the
/// accumulation this module exists to prevent.
///
/// Ownership, not age, decides for a published area: it is reclaimed only when
/// its lease lock can be TAKEN, which the kernel permits only once the owner is
/// gone. A long-running process is therefore safe even while idle, and a
/// wall-clock jump cannot reclaim live work — neither of which an mtime cutoff
/// can promise, because writes nested inside an area do not refresh the area's
/// own mtime.
///
/// Best-effort by construction: concurrent processes reap the same root, so a
/// removal losing a race is normal and never fails a test.
fn reap_unowned_stage_areas(root: &std::path::Path, pending_cutoff: std::time::SystemTime) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // A lease is a FILE beside the areas, judged by its own rule: nothing
        // below may treat it as a directory to remove wholesale.
        if name.starts_with(STAGE_LEASE_PREFIX) {
            if lease_is_orphaned(&entry.path(), root, pending_cutoff) {
                let _ = std::fs::remove_file(entry.path());
            }
            continue;
        }
        let reclaimable = if name.starts_with(STAGE_PUBLISHED) {
            published_area_is_unowned(&entry.path())
        } else if name.starts_with(STAGE_PENDING) {
            // Ownership FIRST here too. A creator holds its pending lease while
            // it works, so age alone would delete a live tree out from under a
            // process that is merely slow — or one the clock jumped past.
            published_area_is_unowned(&entry.path()) && aged_out(&entry.path(), pending_cutoff)
        } else if name.starts_with(STAGE_LEGACY) {
            // Areas from the implementation that named them `test-*`. They carry
            // a lease, so ownership still decides; without this arm they would be
            // invisible to every future run and orphaned forever.
            published_area_is_unowned(&entry.path())
        } else {
            false
        };
        if reclaimable {
            reclaim_stage_area(&entry.path());
        }
    }
}

/// Remove an unowned area and the lease that named it.
///
/// The AREA goes first. A lease removed ahead of its area leaves the area
/// leaseless in between, and a leaseless area is kept forever by design — so a
/// crash in that order converts reclaimable debris into a permanent orphan. The
/// completed order leaves at worst a lease naming nothing, which
/// [`lease_is_orphaned`] reclaims on the next pass.
///
/// The lease goes ONLY once the area is actually gone. `remove_dir_all` can
/// fail routinely — a concurrent reaper racing the same tree, a read-only
/// attribute, a still-open handle — and a surviving area whose lease was
/// removed anyway is leaseless, so kept forever. Keeping the lease keeps the
/// pair reclaimable on the next pass instead.
fn reclaim_stage_area(area: &std::path::Path) {
    let _ = std::fs::remove_dir_all(area);
    if area.exists() {
        return;
    }
    if let Some(lease) = sibling_lease_for(area) {
        let _ = std::fs::remove_file(lease);
    }
}

/// Whether a lease file is debris.
///
/// Three conditions, every one required. The area it names must be GONE — a lease
/// belonging to a live or merely idle area is not debris, and this is what keeps
/// the sweep from racing the areas it sits beside. Its lock must be takeable, so
/// a creator that has locked its lease but not yet created its area keeps it. And
/// it must be old, which covers the sliver between creating the file and locking
/// it: inside that window the lock IS takeable and the area does not exist yet,
/// and unlinking there would leave the creator holding a lock on an unlinked file
/// with nothing left to advertise its ownership.
fn lease_is_orphaned(
    lease: &std::path::Path,
    root: &std::path::Path,
    cutoff: std::time::SystemTime,
) -> bool {
    let Some(name) = lease.file_name() else {
        return false;
    };
    let name = name.to_string_lossy();
    let Some(id) = name.strip_prefix(STAGE_LEASE_PREFIX) else {
        return false;
    };
    let names_an_existing_area = [STAGE_PUBLISHED, STAGE_PENDING, STAGE_LEGACY]
        .into_iter()
        .any(|prefix| root.join(format!("{prefix}{id}")).exists());
    !names_an_existing_area && lease_is_unheld(lease) == Some(true) && aged_out(lease, cutoff)
}

/// Whether a PUBLISHED area's owner has exited.
///
/// Two places can hold an area's lease — beside it, and inside it — so both are
/// consulted and ANY held lease means owned. Only the sibling is ever written;
/// the inside location is read so that areas already carrying one there remain
/// judgeable rather than orphaned.
///
/// Fails CLOSED everywhere. A published area always has a locked lease at the
/// instant it becomes visible, so finding no readable lease at all is an anomaly,
/// not the ordinary case — and it is treated as owned FOREVER rather than
/// reclaimed on a timer, because wrongly keeping an area costs disk while wrongly
/// deleting one destroys a running test's workspace.
fn published_area_is_unowned(area: &std::path::Path) -> bool {
    let mut released = false;
    for lease in sibling_lease_for(area)
        .into_iter()
        .chain(std::iter::once(area.join(STAGE_LEASE)))
    {
        match lease_is_unheld(&lease) {
            // Absent, or unreadable: no signal here. An area with no signal
            // anywhere leaves `released` false and so reads as owned.
            None => continue,
            // Acquired: whoever held this lease released it by exiting.
            Some(true) => released = true,
            // Still held, or unknowable. Leave the area alone.
            Some(false) => return false,
        }
    }
    released
}

/// Whether a lease's lock is free — `None` when the file cannot be read at all.
///
/// An unreadable lease is reported as no signal rather than as free, so every
/// caller composing this into a removal decision fails closed by default.
fn lease_is_unheld(lease: &std::path::Path) -> Option<bool> {
    let file = std::fs::File::open(lease).ok()?;
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            Some(true)
        }
        Err(std::fs::TryLockError::WouldBlock) => Some(false),
        Err(std::fs::TryLockError::Error(_)) => Some(false),
    }
}

/// Whether a path's own mtime predates `cutoff`.
///
/// Only ever consulted AFTER ownership says the creator is gone, so age never
/// decides the fate of live work — it separates a crashed creator's leftovers
/// from one that is merely between `create_dir_all` and `rename`.
fn aged_out(path: &std::path::Path, cutoff: std::time::SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified < cutoff)
}

/// Copy a fixture's AUTHORED tree — the files git tracks, and nothing else.
///
/// Tracked-ness is the definition of "authored", so the copy carries exactly
/// what a fresh clone would have. Excluding `node_modules` by name would only be
/// true of the residue we happened to think of: a gitignored `src/dist/*.d.ts`
/// sits inside a fixture's own `"include": ["src"]` and would join the staged
/// program, making a local build artefact part of a test's meaning. Asking git
/// makes the whole class impossible rather than the one member we named, and
/// `node_modules` is excluded as a CONSEQUENCE — it is never tracked.
///
/// Fails loudly if git cannot answer: a silent fall back to copying everything
/// would restore the residue exactly when the guarantee is least observable.
fn copy_authored_tree(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination)
        .unwrap_or_else(|error| panic!("create staged fixture {}: {error}", destination.display()));
    for relative in tracked_fixture_files(source) {
        let authored = source.join(&relative);
        // A tracked path deleted in the working tree is listed but absent. Skip
        // it: the fixture is mid-edit, and a clone would not have the file
        // either.
        if !authored.is_file() {
            continue;
        }
        let staged = destination.join(&relative);
        if let Some(parent) = staged.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!("create staged fixture dir {}: {error}", parent.display())
            });
        }
        std::fs::copy(&authored, &staged)
            .unwrap_or_else(|error| panic!("stage fixture file {}: {error}", authored.display()));
    }
}

/// The repo-tracked files under a fixture root, relative to it.
fn tracked_fixture_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .unwrap_or_else(|error| panic!("list tracked files in {}: {error}", root.display()));
    assert!(
        output.status.success(),
        "git ls-files failed in {}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let listed: Vec<std::path::PathBuf> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| std::path::PathBuf::from(String::from_utf8_lossy(entry).into_owned()))
        .collect();
    assert!(
        !listed.is_empty(),
        "fixture {} has no tracked files — staging it would produce an empty \
         workspace and every assertion over it would be vacuous",
        root.display()
    );
    listed
}

/// Materialize the type-only dependencies a fixture DECLARES, so a staged root
/// carries the surface a real install would have produced.
///
/// The fixture's own `package.json` is the declaration — there is no per-fixture
/// allowlist to keep in step. A fixture-name `match` with a `_ => {}` arm silently
/// gave every unlisted fixture an EMPTY dependency surface: its sources' imports
/// resolved to nothing, the provider answered from a degraded program, and any
/// test guarding on that degraded answer reported a pass without running its
/// assertions. Deriving from the manifest makes a new fixture correct by default.
///
/// A declared dependency this harness cannot supply is passed over, NOT an error
/// — see [`supplier_for`] for why relevance is not decided here.
pub(crate) fn materialize_real_provider_framework_types(fixture: &str) {
    static MATERIALIZE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = MATERIALIZE_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture_root = std::path::PathBuf::from(fixture_workspace_root(fixture));
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let node_modules = fixture_root.join("node_modules");

    for dependency in declared_dependencies(&fixture_root) {
        let Some(supplier) = supplier_for(&dependency) else {
            continue;
        };
        supplier(&repo_root, &node_modules);
    }
}

/// A routine that materializes one dependency into a staged `node_modules`.
type DependencySupplier = fn(&std::path::Path, &std::path::Path);

/// The supplier this harness has for a dependency, if any.
///
/// This answers ONE question — "can this harness produce that package?" — and
/// deliberately not "does anything depend on it?". The second question cannot be
/// answered from a manifest: `vue` contributes a provider-visible type surface
/// and `eslint` does not, with no rule separating them. Encoding an answer here
/// produced a second maintained list on the build-tooling axis, where a fixture
/// adding `@vitejs/plugin-vue` to its devDependencies aborted provider startup
/// for a package that contributes nothing.
///
/// So a dependency with no supplier is simply not staged, and the degradation
/// that used to hide behind that surfaces at the assertion instead. That is
/// COMPLETE for the manually-spawned provider tests, whose discriminators panic
/// when a fixture's surface is missing. It is PARTIAL for the macro-based tests:
/// they gate on `require_or_skip_ready`, which panics under `VERTER_REQUIRE_*`
/// but soft-skips otherwise, so in a default local run a genuinely missing
/// surface can still skip. That residual is the pre-existing readiness policy,
/// not something this table introduces, and it does not reach canonical CI,
/// whose required leg fails closed.
///
/// `typescript` is absent from this table ON PURPOSE rather than by omission:
/// the harness pins its tsserver to the configured tsdk tier precisely so no
/// directory near a staged root can decide which compiler runs, and installing
/// one here would reintroduce exactly that hazard.
fn supplier_for(dependency: &str) -> Option<DependencySupplier> {
    match dependency {
        "vue" => Some(|repo_root, node_modules| {
            copy_type_package_with_dependencies(
                &repo_root.join("packages/types/node_modules/vue"),
                node_modules,
            )
        }),
        "svelte" => Some(|repo_root, node_modules| {
            copy_type_package_atomically(
                &repo_root.join("packages/svelte-jsx/node_modules/svelte"),
                &node_modules.join("svelte"),
            );
            copy_type_package_atomically(
                &repo_root.join("packages/svelte-jsx"),
                &node_modules.join("@verter/svelte-jsx"),
            );
        }),
        "@types/react" => Some(|_repo_root, node_modules| {
            write_react_jsx_ambient_atomically(&node_modules.join("@types/react"))
        }),
        _ => None,
    }
}

/// The dependency names a fixture's own `package.json` declares.
///
/// The line this draws is INTERPRETABILITY, not presence. Two reviewers split on
/// it once, so it is stated at the reader rather than left to be re-derived:
///
/// - **No manifest at all** — declares nothing. Valid; several fixtures exist to
///   exercise config-less resolution.
/// - **Key absent** (`{"name":"f"}`) — declares nothing. Valid.
/// - **Key present and empty** (`{"dependencies":{}}`) — declares NONE. Valid,
///   and a shape npm and pnpm both emit routinely. "No dependencies" is a real
///   answer, so treating it as malformed would reject a correct manifest the
///   moment a fixture adopted one.
/// - **Key present but UNINTERPRETABLE** (`null`, a scalar, an array), or a
///   manifest whose ROOT cannot carry declarations — malformed, so ABORT. No
///   real tool emits these; they signal corruption or a generator bug, and
///   reading them as "declares nothing" is how a fixture that declared its needs
///   correctly still stages with an empty surface.
///
/// An unreadable manifest aborts for the same reason: it cannot be distinguished
/// from "declares nothing" and must not be silently treated as it.
/// `optionalDependencies` participates — it is a real declaration, and an
/// install would honour it.
fn declared_dependencies(fixture_root: &std::path::Path) -> Vec<String> {
    let manifest_path = fixture_root.join("package.json");
    let bytes = match std::fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => panic!(
            "read fixture manifest {}: {error} — refusing to treat an unreadable \
             manifest as an empty declaration",
            manifest_path.display()
        ),
    };
    let manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "parse fixture manifest {}: {error}",
            manifest_path.display()
        )
    });
    let serde_json::Value::Object(root) = &manifest else {
        panic!(
            "fixture manifest {} has a non-object root: {manifest} — a manifest \
             that cannot carry declarations must not be read as declaring nothing",
            manifest_path.display()
        )
    };
    let mut declared = Vec::new();
    for field in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        // Ask whether the key EXISTS: indexing a `Value` yields `Null` for an
        // absent key AND for an explicit `null`, collapsing a missing
        // declaration into a corrupt one. An empty object is NOT in that class —
        // it is a valid declaration of none and passes through as such.
        match root.get(field) {
            None => {}
            Some(serde_json::Value::Object(entries)) => declared.extend(entries.keys().cloned()),
            Some(other) => panic!(
                "fixture manifest {} has a non-object `{field}`: {other} — a \
                 malformed declaration must not be read as an empty one",
                manifest_path.display()
            ),
        }
    }
    declared.sort();
    declared.dedup();
    declared
}

fn copy_type_package_with_dependencies(source: &std::path::Path, node_modules: &std::path::Path) {
    fn visit(
        source: &std::path::Path,
        node_modules: &std::path::Path,
        visited: &mut std::collections::HashSet<String>,
    ) {
        let manifest_path = source.join("package.json");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap_or_else(|error| {
                panic!(
                    "read real-provider fixture dependency manifest {}: {error}",
                    manifest_path.display()
                )
            }))
            .unwrap_or_else(|error| {
                panic!(
                    "parse real-provider fixture dependency manifest {}: {error}",
                    manifest_path.display()
                )
            });
        let name = manifest["name"].as_str().unwrap_or_else(|| {
            panic!(
                "real-provider fixture dependency has no package name: {}",
                manifest_path.display()
            )
        });
        if !visited.insert(name.to_string()) {
            return;
        }

        copy_type_package_atomically(source, &node_modules.join(name));
        let Some(dependencies) = manifest["dependencies"].as_object() else {
            return;
        };
        for dependency in dependencies.keys() {
            let dependency_source = resolve_dependency_source(source, dependency)
                .unwrap_or_else(|| {
                    panic!(
                        "real-provider fixture transitive dependency `{dependency}` must resolve from {}",
                        source.display()
                    )
                });
            visit(&dependency_source, node_modules, visited);
        }
    }

    let source = std::fs::canonicalize(source).unwrap_or_else(|error| {
        panic!(
            "canonicalize real-provider fixture dependency {}: {error}",
            source.display()
        )
    });
    visit(&source, node_modules, &mut std::collections::HashSet::new());
}

fn resolve_dependency_source(
    package_root: &std::path::Path,
    dependency: &str,
) -> Option<std::path::PathBuf> {
    let mut current = Some(package_root);
    while let Some(directory) = current {
        let candidate = directory.join("node_modules").join(dependency);
        if candidate.is_dir() {
            return Some(candidate);
        }
        let pnpm_hoist = directory
            .join("node_modules/.pnpm/node_modules")
            .join(dependency);
        if pnpm_hoist.is_dir() {
            return Some(pnpm_hoist);
        }
        current = directory.parent();
    }
    None
}

fn copy_type_package_atomically(source: &std::path::Path, destination: &std::path::Path) {
    assert!(
        source.is_dir(),
        "real-provider fixture dependency must exist after the workspace package install: {}",
        source.display()
    );
    copy_type_package_directory(source, destination, true);
}

fn copy_type_package_directory(
    source: &std::path::Path,
    destination: &std::path::Path,
    is_package_root: bool,
) {
    std::fs::create_dir_all(destination).unwrap_or_else(|error| {
        panic!(
            "create real-provider fixture dependency directory {}: {error}",
            destination.display()
        )
    });
    let mut package_json = None;
    for entry in std::fs::read_dir(source)
        .unwrap_or_else(|error| panic!("read fixture dependency {}: {error}", source.display()))
    {
        let entry = entry.expect("read fixture dependency entry");
        let file_type = entry
            .file_type()
            .expect("read fixture dependency file type");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if file_type.is_dir() {
            if entry.file_name() != "node_modules" {
                copy_type_package_directory(&source_path, &destination_path, false);
            }
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "package.json" {
            package_json = Some((source_path, destination_path));
        } else if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
            copy_file_atomically(&source_path, &destination_path);
        }
    }
    if let Some((source_path, destination_path)) = package_json {
        copy_file_atomically(&source_path, &destination_path);
    } else if is_package_root {
        panic!(
            "real-provider fixture dependency has no package.json: {}",
            source.display()
        );
    }
}

fn copy_file_atomically(source: &std::path::Path, destination: &std::path::Path) {
    let bytes = std::fs::read(source)
        .unwrap_or_else(|error| panic!("read fixture dependency {}: {error}", source.display()));
    if std::fs::read(destination).is_ok_and(|existing| existing == bytes) {
        return;
    }
    write_file_atomically(destination, &bytes);
}

fn write_file_atomically(destination: &std::path::Path, bytes: &[u8]) {
    use std::io::Write;
    let parent = destination
        .parent()
        .expect("fixture dependency file has a parent");
    std::fs::create_dir_all(parent).unwrap_or_else(|error| {
        panic!(
            "create fixture dependency parent {}: {error}",
            parent.display()
        )
    });
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .unwrap_or_else(|error| panic!("create atomic fixture dependency file: {error}"));
    temporary
        .write_all(bytes)
        .unwrap_or_else(|error| panic!("write atomic fixture dependency file: {error}"));
    temporary.persist(destination).unwrap_or_else(|error| {
        panic!(
            "publish fixture dependency {}: {error}",
            destination.display()
        )
    });
}

fn write_react_jsx_ambient_atomically(package_root: &std::path::Path) {
    const REACT_AMBIENT: &str = r#"declare namespace React {
  interface HTMLAttributes<T> { className?: string }
}
declare namespace JSX {
  interface Element {}
  interface IntrinsicElements { div: React.HTMLAttributes<HTMLDivElement> }
}
export = React;
export as namespace React;
"#;
    write_file_atomically(&package_root.join("index.d.ts"), REACT_AMBIENT.as_bytes());
    write_file_atomically(
        &package_root.join("package.json"),
        br#"{"name":"@types/react","types":"index.d.ts"}"#,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A planted directory that removes itself even if the test unwinds.
    ///
    /// The plant lives in a directory shared with the E2E suite, so a panic
    /// between planting and cleanup would otherwise leave a stray package there
    /// — the exact residue this module exists to stop. Removes ONLY the plant:
    /// the enclosing scope holds real packages this test never owned.
    struct Plant {
        path: std::path::PathBuf,
    }

    impl Plant {
        fn new(path: std::path::PathBuf) -> Self {
            Self { path }
        }
    }

    impl Drop for Plant {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// Plant a shadowing package in the AUTHORED fixture's `node_modules` and
    /// prove the production seam — `fixture_workspace_root`, the one function
    /// every consumer resolves a fixture through — hands back a root that does
    /// not carry it.
    ///
    /// This is the regression that a four-month-old `@verter/types` beat: it
    /// shadowed the virtually served `verter_types_stub.d.ts` and emptied three
    /// provider surfaces. The assertion runs THROUGH the production entry point
    /// rather than a helper, so removing staging from it fails this test.
    #[test]
    fn staged_fixture_root_never_carries_authored_node_modules() {
        let authored = authored_fixture_root("single-project");
        // The probe name is unique per process: this writes into a directory the
        // E2E suite and other test processes share, so a fixed name could collide
        // with an overlapping run. It names no package any fixture imports, so it
        // cannot shadow something a concurrent test resolves for real.
        let planted = Plant::new(
            authored
                .join("node_modules")
                .join("@verter")
                .join(format!("types-stage-probe-{}", std::process::id())),
        );
        assert!(
            !planted.path.exists(),
            "probe must be NEW, not a pre-existing directory: {}",
            planted.path.display()
        );
        std::fs::create_dir_all(&planted.path).expect("plant shadow package");
        std::fs::write(
            planted.path.join("package.json"),
            br#"{"name":"@verter/types-stage-probe","types":"index.d.ts"}"#,
        )
        .expect("plant shadow manifest");
        // Prove the plant LANDED at the exact path staging reads. A plant that
        // silently failed to apply would otherwise report a clean pass.
        assert!(
            planted.path.join("package.json").is_file(),
            "plant must be present before the staging seam runs: {}",
            planted.path.display()
        );

        let staged = std::path::PathBuf::from(fixture_workspace_root("single-project"));
        let leaked = staged.join("node_modules").join("@verter").join(
            planted
                .path
                .file_name()
                .expect("probe directory has a name"),
        );
        let leaked_present = leaked.exists();
        let authored_sources_staged = staged.join("src").join("App.vue").is_file();
        let staged_outside_authored = !staged.starts_with(&authored);

        assert!(
            !leaked_present,
            "staging must not carry authored `node_modules` into the fixture root: {}",
            leaked.display()
        );
        assert!(
            staged_outside_authored,
            "the staged root must not be the authored tree itself: {}",
            staged.display()
        );
        assert!(
            authored_sources_staged,
            "staging must still carry the authored sources: {}",
            staged.join("src").join("App.vue").display()
        );
    }

    /// Materialization must land in the staged root, so a dependency surface is
    /// built fresh per process instead of inheriting the shared directory.
    #[test]
    fn materialization_targets_the_staged_root_not_the_authored_tree() {
        let authored = authored_fixture_root("single-project");
        let before = authored.join("node_modules").join("vue").exists();

        materialize_real_provider_framework_types("single-project");

        let staged = std::path::PathBuf::from(fixture_workspace_root("single-project"));
        // The authored tree already carries a `vue` on most machines, so an
        // unchanged-existence check alone cannot tell "materialized into the
        // staged copy" from "materialized into the authored tree". Pin the
        // target root first: if staging is removed the two roots coincide and
        // this fails before the weaker checks can pass vacuously.
        assert!(
            !staged.starts_with(&authored),
            "materialization must target a root outside the authored tree, got: {}",
            staged.display()
        );
        assert!(
            staged
                .join("node_modules")
                .join("vue")
                .join("package.json")
                .is_file(),
            "materialization must populate the staged root: {}",
            staged.join("node_modules").join("vue").display()
        );
        assert_eq!(
            before,
            authored.join("node_modules").join("vue").exists(),
            "materialization must not write into the authored tree: {}",
            authored.join("node_modules").display()
        );
    }

    /// Reclamation must never delete an area a running process owns, and must
    /// reclaim one whose owner has exited. Ownership is proven by the lease
    /// lock, so a live area is safe even while idle — an mtime cutoff cannot
    /// promise that, because writes nested inside an area do not refresh the
    /// area's own mtime.
    #[test]
    fn reaping_reclaims_unowned_areas_and_never_touches_owned_or_unpublished_ones() {
        let temporary = tempfile::tempdir().expect("create reap scratch root");
        let root = temporary.path().join("verter-fixture-stage");

        // Owned: a lease held for the duration of this test, exactly as a live
        // test process holds its own.
        let owned = root.join(format!("{STAGE_PUBLISHED}owned"));
        std::fs::create_dir_all(owned.join("single-project")).expect("plant owned area");
        let held = std::fs::File::create(owned.join(STAGE_LEASE)).expect("create owned lease");
        held.lock().expect("hold owned lease");

        // Unowned: a lease nobody holds, as an exited process leaves behind.
        let unowned = root.join(format!("{STAGE_PUBLISHED}unowned"));
        std::fs::create_dir_all(unowned.join("single-project")).expect("plant unowned area");
        drop(std::fs::File::create(unowned.join(STAGE_LEASE)).expect("create unowned lease"));

        // Published but leaseless: an anomaly, kept FOREVER rather than reclaimed
        // on a timer. Deleting a live workspace is worse than keeping debris.
        let leaseless = root.join(format!("{STAGE_PUBLISHED}leaseless"));
        std::fs::create_dir_all(leaseless.join("single-project")).expect("plant leaseless area");

        // Mid-construction and OWNED: a creator holding its lease while it works.
        // The reaper scans this prefix, so only ownership can spare it — age
        // alone would delete a live tree from under a slow or suspended creator.
        let pending = root.join(format!("{STAGE_PENDING}under-construction"));
        std::fs::create_dir_all(&pending).expect("plant pending area");
        let pending_held =
            std::fs::File::create(pending.join(STAGE_LEASE)).expect("create pending lease");
        pending_held.lock().expect("hold pending lease");

        // An abandoned creator's leftovers: pending, unlocked, and old.
        let pending_debris = root.join(format!("{STAGE_PENDING}abandoned"));
        std::fs::create_dir_all(&pending_debris).expect("plant pending debris");
        drop(std::fs::File::create(pending_debris.join(STAGE_LEASE)).expect("create debris lease"));

        // Prove the plants landed AND that the lock is genuinely observable
        // across handles, or the owned case below would pass for the wrong reason.
        assert!(
            owned.join(STAGE_LEASE).is_file()
                && unowned.join(STAGE_LEASE).is_file()
                && pending.join(STAGE_LEASE).is_file()
                && !leaseless.join(STAGE_LEASE).exists(),
            "the four areas must be planted in their intended states"
        );
        assert!(
            !published_area_is_unowned(&owned),
            "a HELD lease must read as owned: {}",
            owned.display()
        );
        assert!(
            published_area_is_unowned(&unowned),
            "an unheld lease must read as reclaimable: {}",
            unowned.display()
        );
        assert!(
            !published_area_is_unowned(&pending),
            "the pending plant must read as OWNED, so its survival below is \
             attributable to ownership rather than to its age: {}",
            pending.display()
        );
        assert!(
            published_area_is_unowned(&pending_debris),
            "the debris plant must read as unowned: {}",
            pending_debris.display()
        );

        // A cutoff in the FUTURE makes every pending area look abandoned by age.
        // Only ownership can spare one, so this drives the reaper's real path
        // instead of passing because a plant was younger than the TTL.
        let everything_looks_old =
            std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        reap_unowned_stage_areas(&root, everything_looks_old);

        assert!(
            owned.exists(),
            "reaping must spare an area whose owner still holds its lease: {}",
            owned.display()
        );
        assert!(
            !unowned.exists(),
            "reaping must reclaim an area whose owner has exited: {}",
            unowned.display()
        );
        assert!(
            leaseless.exists(),
            "a published area with no lease must be kept, not reclaimed on a timer: {}",
            leaseless.display()
        );
        assert!(
            pending.exists(),
            "a pending area whose creator still holds its lease must survive even \
             when age says it is abandoned: {}",
            pending.display()
        );
        assert!(
            !pending_debris.exists(),
            "an abandoned pending area must still be reclaimed: {}",
            pending_debris.display()
        );

        drop(held);
        drop(pending_held);
    }

    /// Ownership read from a SIBLING lease must decide an area's fate exactly as
    /// an inside one does, reclaiming a lease along with the area it named, and a
    /// lease naming no area must be swept — but only once it is old.
    ///
    /// The age condition is what keeps the sweep off a creator that has taken its
    /// lease and not yet built its area, so it is driven in BOTH directions here:
    /// a cutoff in the past must spare the orphan, and only a cutoff past its
    /// mtime may take it.
    #[test]
    fn reaping_judges_sibling_leases_and_sweeps_only_an_aged_lease_naming_no_area() {
        let temporary = tempfile::tempdir().expect("create sibling-lease scratch root");
        let root = temporary.path().join("verter-fixture-stage");
        std::fs::create_dir_all(&root).expect("create staging root");

        // Owned: a sibling lease held for the duration of this test, exactly as a
        // live test process holds its own.
        let owned = root.join(format!("{STAGE_PUBLISHED}sibling-owned"));
        std::fs::create_dir_all(owned.join("single-project")).expect("plant owned area");
        let owned_lease = root.join(format!("{STAGE_LEASE_PREFIX}sibling-owned"));
        let held = std::fs::File::create(&owned_lease).expect("create owned sibling lease");
        held.lock().expect("hold owned sibling lease");

        // Unowned: a sibling lease nobody holds, as an exited process leaves.
        let unowned = root.join(format!("{STAGE_PUBLISHED}sibling-unowned"));
        std::fs::create_dir_all(unowned.join("single-project")).expect("plant unowned area");
        let unowned_lease = root.join(format!("{STAGE_LEASE_PREFIX}sibling-unowned"));
        drop(std::fs::File::create(&unowned_lease).expect("create unowned sibling lease"));

        // A lease naming NO area: what a creator leaves by dying between taking
        // its lease and building its area.
        let orphan = root.join(format!("{STAGE_LEASE_PREFIX}no-such-area"));
        drop(std::fs::File::create(&orphan).expect("create orphan lease"));

        // A lease naming no area whose lock is HELD: a creator inside the window
        // between locking its lease and building its area. Heldness — not the
        // missing area, not age — is what must spare it once it looks old.
        let held_no_area = root.join(format!("{STAGE_LEASE_PREFIX}held-no-area"));
        let held_no_area_lock =
            std::fs::File::create(&held_no_area).expect("create held no-area lease");
        held_no_area_lock.lock().expect("hold the no-area lease");

        // An area owned through an INSIDE lease (the pre-sibling format) whose
        // sibling lease is unheld and will look aged. The area arm spares the
        // area (owned), so only the lease arm's existing-area check protects the
        // sibling — in every directory-iteration order.
        let inside_owned = root.join(format!("{STAGE_PUBLISHED}inside-owned"));
        std::fs::create_dir_all(&inside_owned).expect("plant inside-owned area");
        let inside_lease_lock =
            std::fs::File::create(inside_owned.join(STAGE_LEASE)).expect("create inside lease");
        inside_lease_lock.lock().expect("hold the inside lease");
        let inside_owned_sibling = root.join(format!("{STAGE_LEASE_PREFIX}inside-owned"));
        drop(std::fs::File::create(&inside_owned_sibling).expect("create unheld sibling"));

        // Prove the plants read as intended BEFORE any reaping, or the outcomes
        // below are not attributable to the rule under test.
        assert!(
            owned_lease.is_file() && unowned_lease.is_file() && orphan.is_file(),
            "the three leases must be planted beside the areas, not inside them"
        );
        assert!(
            !published_area_is_unowned(&owned),
            "a HELD sibling lease must read as owned: {}",
            owned.display()
        );
        assert!(
            published_area_is_unowned(&unowned),
            "an unheld sibling lease must read as reclaimable: {}",
            unowned.display()
        );

        // A cutoff in the PAST: nothing is old. A published area is never judged
        // by age, so the unowned one still goes — but the orphan lease must stay.
        reap_unowned_stage_areas(&root, std::time::UNIX_EPOCH);
        assert!(
            orphan.is_file(),
            "a lease younger than the cutoff must be spared — this is the window \
             in which a creator has its lease but not yet its area: {}",
            orphan.display()
        );
        assert!(
            !unowned.exists(),
            "an unowned published area is reclaimed on ownership, not age: {}",
            unowned.display()
        );
        assert!(
            !unowned_lease.exists(),
            "reclaiming an area must take the lease that named it: {}",
            unowned_lease.display()
        );

        // A cutoff in the FUTURE makes every lease look abandoned by age. Only
        // ownership and a missing area may spare one.
        let everything_looks_old =
            std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        reap_unowned_stage_areas(&root, everything_looks_old);
        assert!(
            !orphan.exists(),
            "an aged lease naming no area must be swept: {}",
            orphan.display()
        );
        assert!(
            owned.exists(),
            "reaping must spare an area whose owner still holds its sibling lease: {}",
            owned.display()
        );
        assert!(
            owned_lease.is_file(),
            "a held lease naming a live area must survive both passes: {}",
            owned_lease.display()
        );
        assert!(
            held_no_area.is_file(),
            "an aged lease naming no area must be spared while its lock is HELD — \
             heldness alone protects the lock-taken-area-not-yet-built window: {}",
            held_no_area.display()
        );
        assert!(
            inside_owned_sibling.is_file(),
            "an aged unheld sibling lease naming a LIVE (inside-owned) area must \
             never be swept by the lease arm — the existing-area check alone \
             protects it, since the area arm spares the owned area: {}",
            inside_owned_sibling.display()
        );
        assert!(
            inside_owned.exists(),
            "an area owned through its inside lease must survive: {}",
            inside_owned.display()
        );

        drop(held);
        drop(held_no_area_lock);
        drop(inside_lease_lock);
    }

    /// A failed area removal must keep the sibling lease. A surviving area whose
    /// lease was removed is leaseless, and a leaseless area is kept forever by
    /// design — so dropping the lease after a FAILED `remove_dir_all` converts
    /// retryable debris into a permanent orphan. Keeping it leaves the pair
    /// reclaimable on the next pass, which the second reap below proves.
    #[test]
    fn a_failed_area_removal_keeps_the_lease_so_the_area_stays_reclaimable() {
        let temporary = tempfile::tempdir().expect("create failed-reclaim scratch root");
        let root = temporary.path().join("verter-fixture-stage");
        std::fs::create_dir_all(&root).expect("create staging root");

        // An unowned area with a removal blocker inside it, and the unheld
        // sibling lease its dead owner left behind.
        let area = root.join(format!("{STAGE_PUBLISHED}stuck-reclaim"));
        let blocker_dir = area.join("blocker");
        std::fs::create_dir_all(&blocker_dir).expect("plant blocker dir");
        let pinned = blocker_dir.join("pinned.txt");
        std::fs::write(&pinned, b"pinned").expect("plant pinned file");
        // Windows blocker: a handle opened WITHOUT `FILE_SHARE_DELETE`. Rust's
        // default open grants all three share flags, and `remove_dir_all` sets
        // `IGNORE_READONLY_ATTRIBUTE`, so neither a plain handle nor a readonly
        // attribute blocks removal — a no-share-delete handle is the one thing
        // its POSIX-semantics deletion cannot override.
        #[cfg(windows)]
        let pin_handle = {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_SHARE_READ_WRITE_ONLY: u32 = 0x1 | 0x2;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ_WRITE_ONLY)
                .open(&pinned)
                .expect("pin the file with a no-share-delete handle")
        };
        // POSIX blocker: a read-only DIRECTORY refuses unlinking its children
        // (open handles do not gate unlink there; the parent's mode does).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&blocker_dir, std::fs::Permissions::from_mode(0o555))
                .expect("pin the blocker dir read-only");
        }
        let lease = root.join(format!("{STAGE_LEASE_PREFIX}stuck-reclaim"));
        drop(std::fs::File::create(&lease).expect("create unheld sibling lease"));

        reap_unowned_stage_areas(&root, std::time::UNIX_EPOCH);

        assert!(
            area.exists(),
            "the removal blocker must have armed — a reap that fully removed the \
             area proves nothing about the failed-removal path: {}",
            area.display()
        );
        assert!(
            lease.is_file(),
            "a lease must survive a FAILED area removal, or the surviving area \
             becomes leaseless and is kept forever: {}",
            lease.display()
        );
        assert!(
            published_area_is_unowned(&area),
            "the surviving pair must still read as reclaimable, not orphaned: {}",
            area.display()
        );

        // Clear the blockers: the next pass must heal the debris completely.
        #[cfg(windows)]
        drop(pin_handle);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&blocker_dir, std::fs::Permissions::from_mode(0o755))
                .expect("unpin the blocker dir");
        }

        reap_unowned_stage_areas(&root, std::time::UNIX_EPOCH);
        assert!(
            !area.exists() && !lease.exists(),
            "with the blocker cleared the pair must reclaim together: {} / {}",
            area.display(),
            lease.display()
        );
    }

    /// The published name must never exist unlocked: publication is a rename of
    /// an already-locked area, so no reaper can observe the window in which an
    /// area is visible but unowned.
    #[test]
    fn a_published_stage_area_is_locked_from_the_instant_it_is_visible() {
        let temporary = tempfile::tempdir().expect("create publication scratch root");
        let root = temporary.path().join("verter-fixture-stage");
        std::fs::create_dir_all(&root).expect("create staging root");

        let published = claim_stage_area(&root, "publication-probe");

        assert!(
            published
                .file_name()
                .expect("published area has a name")
                .to_string_lossy()
                .starts_with(STAGE_PUBLISHED),
            "the returned area must carry the published prefix: {}",
            published.display()
        );
        assert!(
            !published_area_is_unowned(&published),
            "the area must already be owned at the moment it becomes visible: {}",
            published.display()
        );
        assert!(
            !root
                .join(format!("{STAGE_PENDING}publication-probe"))
                .exists(),
            "the construction name must not survive publication"
        );
    }

    /// Publication must SUCCEED while the area's lease is held, and the lease
    /// must not live inside the directory publication renames.
    ///
    /// Those are one requirement seen from both ends. A rename target holding an
    /// open handle anywhere in its subtree is refused on Windows — measured to be
    /// independent of share mode, of whether the handle is locked, and of nesting
    /// depth — so a lease inside the area makes every claim fail on every Windows
    /// machine by construction while staying invisible on POSIX. Asserting only
    /// that the publish happened would pass on POSIX for a layout that cannot
    /// work; asserting only the layout would pass without proving publication.
    #[test]
    fn a_claimed_area_publishes_while_its_lease_is_held() {
        let temporary = tempfile::tempdir().expect("create lease-layout scratch root");
        let root = temporary.path().join("verter-fixture-stage");
        // Deliberately NOT pre-created: the lease is a sibling of the area, so
        // claiming has to bring the root along before it can take the lease.
        assert!(!root.exists(), "the staging root must not pre-exist");

        let published = claim_stage_area(&root, "held-lease-probe");

        let sibling = root.join(format!("{STAGE_LEASE_PREFIX}held-lease-probe"));
        assert!(
            published.is_dir(),
            "publication must have produced the area: {}",
            published.display()
        );
        assert!(
            !published.join(STAGE_LEASE).exists(),
            "the renamed directory must contain no lease file — an open handle \
             under a rename target is exactly what publication cannot survive: {}",
            published.join(STAGE_LEASE).display()
        );
        assert!(
            sibling.is_file(),
            "the lease must exist beside the area, not inside it: {}",
            sibling.display()
        );
        assert!(
            !root
                .join(format!("{STAGE_PENDING}held-lease-probe"))
                .exists(),
            "the construction name must not survive publication"
        );

        // Moving the lease must not cost the ownership signal it carries: the
        // area is owned the instant it is visible, and stays owned under a reaper
        // whose every age test says otherwise.
        assert!(
            !published_area_is_unowned(&published),
            "the area must read as OWNED through its sibling lease: {}",
            published.display()
        );
        let everything_looks_old =
            std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        reap_unowned_stage_areas(&root, everything_looks_old);
        assert!(
            published.is_dir(),
            "reaping must spare an area whose lease is held: {}",
            published.display()
        );
        assert!(
            sibling.is_file(),
            "reaping must not sweep the lease of a live area: {}",
            sibling.display()
        );
    }

    /// A staged fixture lives under the system temp directory, whose ancestry
    /// this harness does not own. `resolve_tsserver` ranks every
    /// `node_modules/typescript` on the owning project's ancestor walk ABOVE the
    /// configured tsdk, so resolving with a workspace root would make the
    /// TypeScript under every tsserver test a property of where `TMPDIR` points.
    ///
    /// The first assertion proves that exposure is REAL rather than theoretical;
    /// the second proves the harness seam is immune to it. Without the first,
    /// the second could pass simply because nothing was planted.
    #[test]
    fn harness_tsserver_resolution_ignores_ambient_ancestor_typescript() {
        let temporary = tempfile::tempdir().expect("create toolchain scratch root");
        // An ambient TypeScript ABOVE the staged root, exactly as a `TMPDIR`
        // nested under any tree with a `node_modules` would supply.
        let ambient = temporary.path().join("node_modules/typescript/lib");
        plant_tsserver_install(&ambient);
        // The tsdk the harness names, somewhere the ancestor walk never reaches.
        let tsdk_dir = temporary.path().join("configured/typescript/lib");
        plant_tsserver_install(&tsdk_dir);
        let staged_root = temporary
            .path()
            .join("verter-fixture-stage/test-0/single-project");
        std::fs::create_dir_all(&staged_root).expect("create staged root");
        let tsdk = tsdk_dir.to_string_lossy().replace('\\', "/");

        // Prove both plants are present and DISTINCT, so a resolution landing on
        // one cannot be mistaken for the other.
        assert!(
            ambient.join("tsserver.js").is_file(),
            "ambient plant missing"
        );
        assert!(tsdk_dir.join("tsserver.js").is_file(), "tsdk plant missing");
        assert_ne!(
            ambient, tsdk_dir,
            "the two installs must be distinguishable"
        );

        // Both sides of every comparison below go through the canonical-id
        // normalizer. A resolved tsserver path is canonicalized, which on Windows
        // carries the `\\?\` verbatim prefix, and `Path::starts_with` matches whole
        // COMPONENTS — `\\?\C:` and `C:` are different components, so a resolution
        // that landed exactly where this test wanted still read as a miss. The
        // trailing `/` keeps the string comparison on a component boundary —
        // which `Path::starts_with` gave for free — so `node_modules_backup`
        // can never satisfy a `node_modules` prefix.
        let under = |directory: &str| {
            format!(
                "{}/",
                crate::test_utils::canonical_test_path(&temporary.path().join(directory))
            )
        };
        let ambient_tree = under("node_modules");
        let configured_tree = under("configured");
        assert_ne!(
            ambient_tree, configured_tree,
            "the two trees must be distinguishable after normalization too"
        );

        let with_root = crate::tsserver::find_tsserver(
            Some(&tsdk),
            Some(&staged_root.to_string_lossy().replace('\\', "/")),
        )
        .expect("a workspace-rooted resolution must find something");
        assert!(
            crate::test_utils::canonical_test_path(&with_root).starts_with(&ambient_tree),
            "the ancestor walk must outrank the configured tsdk — otherwise this \
             test cannot discriminate, got: {}",
            with_root.display()
        );

        let harness = crate::test_harness::harness_tsserver_path(&tsdk)
            .expect("the harness seam must resolve the configured tsdk");
        assert!(
            crate::test_utils::canonical_test_path(&harness).starts_with(&configured_tree),
            "the harness must run the TypeScript it names, not one found above \
             the staged root, got: {}",
            harness.display()
        );
    }

    /// Plant a structurally valid TypeScript install: `resolve_tsserver` accepts
    /// a candidate whose `lib` directory carries at least one default library.
    fn plant_tsserver_install(lib_dir: &std::path::Path) {
        std::fs::create_dir_all(lib_dir)
            .unwrap_or_else(|error| panic!("create {}: {error}", lib_dir.display()));
        std::fs::write(lib_dir.join("tsserver.js"), b"// test tsserver\n")
            .expect("plant tsserver.js");
        std::fs::write(lib_dir.join("lib.es5.d.ts"), b"// test default library\n")
            .expect("plant default library");
    }

    /// Removing the ancestor walk is not the same as pinning the toolchain: the
    /// ambient `npm root -g` tier survives it, so a missing or rejected tsdk
    /// would let the harness run the GLOBAL compiler and report nothing unusual.
    /// The tier itself must be checked. Split from the resolver so the refusal
    /// is testable without a global npm install.
    #[test]
    fn only_the_configured_tsdk_is_accepted_as_the_harness_toolchain() {
        use verter_type_runtime::discovery::{ResolvedTsserver, TsserverSource};
        let resolved = |source| ResolvedTsserver {
            path: std::path::PathBuf::from("/somewhere/typescript/lib/tsserver.js"),
            source,
            default_lib_count: 1,
            skipped: Vec::new(),
        };

        assert_eq!(
            crate::test_harness::accept_only_configured_tsdk(resolved(
                TsserverSource::ConfiguredTsdk
            )),
            std::path::PathBuf::from("/somewhere/typescript/lib/tsserver.js"),
            "the configured tsdk is the one tier the harness may run"
        );

        for substituted in [TsserverSource::Global, TsserverSource::ProjectLocal] {
            let refused = std::panic::catch_unwind(|| {
                crate::test_harness::accept_only_configured_tsdk(resolved(substituted))
            });
            assert!(
                refused.is_err(),
                "a {substituted:?} install must be REFUSED, never substituted for the \
                 TypeScript the harness names"
            );
        }
    }

    /// A second claim must not release the first's lock. A single-slot static
    /// silently dropped the second lease, leaving a LIVE area advertised as
    /// unowned and reapable by any other process.
    #[test]
    fn every_stage_area_claim_retains_its_own_lease() {
        let temporary = tempfile::tempdir().expect("create multi-claim scratch root");
        let root = temporary.path().join("verter-fixture-stage");
        std::fs::create_dir_all(&root).expect("create staging root");

        let first = claim_stage_area(&root, "claim-one");
        let second = claim_stage_area(&root, "claim-two");

        assert_ne!(first, second, "distinct claims must produce distinct areas");
        assert!(
            !published_area_is_unowned(&first),
            "the FIRST claim must stay owned after a second claim: {}",
            first.display()
        );
        assert!(
            !published_area_is_unowned(&second),
            "the SECOND claim must be owned, not silently unlocked: {}",
            second.display()
        );
    }

    /// Build a fixture-shaped root carrying `manifest` as its `package.json`.
    fn fixture_with_manifest(manifest: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("create manifest scratch root");
        std::fs::write(root.path().join("package.json"), manifest.as_bytes())
            .expect("write fixture manifest");
        root
    }

    /// An unreadable or malformed manifest must ABORT, never read as "declares
    /// nothing". Both collapse a fixture that declared its needs correctly into
    /// an empty staged surface — the failure the manifest rule exists to end.
    #[test]
    fn an_unreadable_or_malformed_manifest_aborts_instead_of_declaring_nothing() {
        // A manifest that is a DIRECTORY: a read error that is NOT NotFound, and
        // one every platform produces without depending on file permissions.
        let unreadable = tempfile::tempdir().expect("create unreadable scratch root");
        std::fs::create_dir(unreadable.path().join("package.json"))
            .expect("plant a directory where the manifest belongs");
        assert!(
            unreadable.path().join("package.json").is_dir(),
            "the plant must be a directory for the read to fail as something \
             other than NotFound"
        );
        assert!(
            std::panic::catch_unwind(|| declared_dependencies(unreadable.path())).is_err(),
            "an unreadable manifest must abort"
        );

        let malformed = fixture_with_manifest(r#"{"dependencies": "vue"}"#);
        assert!(
            std::panic::catch_unwind(|| declared_dependencies(malformed.path())).is_err(),
            "a non-object dependency field must abort"
        );

        let unparsable = fixture_with_manifest("{ not json");
        assert!(
            std::panic::catch_unwind(|| declared_dependencies(unparsable.path())).is_err(),
            "an unparsable manifest must abort"
        );

        // An explicit `null` is a DECLARATION that says nothing, not an absent
        // key. Indexing a `Value` renders the two identical, which is how a
        // fixture that imports Vue could still stage an empty surface.
        let explicit_null = fixture_with_manifest(r#"{"dependencies": null}"#);
        assert!(
            std::panic::catch_unwind(|| declared_dependencies(explicit_null.path())).is_err(),
            "an explicitly null dependency field must abort, not read as absent"
        );

        // A root that cannot carry declarations at all.
        let array_root = fixture_with_manifest("[]");
        assert!(
            std::panic::catch_unwind(|| declared_dependencies(array_root.path())).is_err(),
            "a non-object manifest root must abort"
        );

        // The BENIGN cases, so the aborts above are attributable to malformity
        // rather than to any manifest reaching this function failing.
        let absent_field = fixture_with_manifest(r#"{"name":"fixture"}"#);
        assert!(
            declared_dependencies(absent_field.path()).is_empty(),
            "an ABSENT declaration field is benign and must not abort"
        );

        // An EMPTY object declares NO dependencies — a real answer npm and pnpm
        // both emit, not a corrupt one. Pinned because the line above it aborts
        // on shapes that merely look similar: without this, tightening the
        // malformity check to reject `{}` would break every fixture that
        // legitimately declares none, and nothing would fail.
        let empty_declaration = fixture_with_manifest(r#"{"dependencies":{}}"#);
        assert!(
            declared_dependencies(empty_declaration.path()).is_empty(),
            "an EMPTY declaration object is a valid declaration of none and must \
             not abort"
        );
    }

    /// A fixture with no manifest declares nothing — a real state, not an error.
    #[test]
    fn an_absent_manifest_declares_nothing_without_aborting() {
        let empty = tempfile::tempdir().expect("create empty scratch root");
        assert!(
            !empty.path().join("package.json").exists(),
            "the probe root must have NO manifest"
        );
        assert!(declared_dependencies(empty.path()).is_empty());
    }

    /// Every declaration field participates, `optionalDependencies` included: an
    /// install would honour it, so a staged root that ignores it is not the
    /// workspace the fixture asked for.
    #[test]
    fn every_declaration_field_participates_including_optional() {
        let root = fixture_with_manifest(
            r#"{"dependencies":{"vue":"^3"},"devDependencies":{"typescript":"^5"},
                "peerDependencies":{"svelte":"^5"},"optionalDependencies":{"@types/react":"^18"}}"#,
        );

        let declared = declared_dependencies(root.path());

        // A DECOY the manifest does not declare. Without it these assertions
        // cannot fail against an implementation that returns a superset — every
        // positive membership check would still pass.
        assert!(
            !declared.contains(&"eslint".to_string()),
            "a package the manifest never declared must not appear, got: {declared:?}"
        );
        assert_eq!(
            declared.len(),
            4,
            "exactly the four declared packages, no more, got: {declared:?}"
        );
        for expected in ["vue", "typescript", "svelte", "@types/react"] {
            assert!(
                declared.contains(&expected.to_string()),
                "`{expected}` must be declared, got: {declared:?}"
            );
        }
    }

    /// The supplier table answers "can this harness produce it?" and nothing
    /// else. A dependency it cannot produce must be passed over silently rather
    /// than aborting: build tooling a fixture legitimately declares contributes
    /// no provider-visible surface, and aborting on it makes a devDependency a
    /// tripwire.
    #[test]
    fn the_supplier_table_is_a_capability_not_a_verdict_on_relevance() {
        assert!(
            supplier_for("vue").is_some() && supplier_for("svelte").is_some(),
            "the harness must supply the surfaces it has"
        );
        for irrelevant in [
            "eslint",
            "@vitejs/plugin-vue",
            "vite",
            "verter",
            "typescript",
        ] {
            assert!(
                supplier_for(irrelevant).is_none(),
                "`{irrelevant}` has no supplier, and that must be a pass-over \
                 rather than a verdict that the fixture is invalid"
            );
        }
    }

    /// Two fixtures must not collide, and one fixture must resolve to one root
    /// within a process — every consumer of a fixture name has to agree.
    #[test]
    fn staging_is_stable_per_fixture_and_distinct_across_fixtures() {
        let first = fixture_workspace_root("single-project");
        let again = fixture_workspace_root("single-project");
        let other = fixture_workspace_root("vue-parity");

        assert_eq!(
            first, again,
            "one fixture must stage to one root within a process"
        );
        assert_ne!(
            first, other,
            "distinct fixtures must stage to distinct roots"
        );
    }
}
