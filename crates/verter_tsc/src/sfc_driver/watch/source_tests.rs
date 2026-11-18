use super::super::super::project_graph::{BuildMode, ProjectGraph};
use super::super::super::watch_binding::ProjectWatchProjectionBinding;
use super::*;
use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;
use verter_workspace::native_fs::NativeFs;

/// Guards against a hang only; every outcome is decided by received events.
const HANG_GUARD: Duration = Duration::from_secs(15);

/// Drain the source on its own thread, so the test blocks on the source's
/// events through a channel.
fn drain(mut source: FileSystemChangeSource) -> Receiver<(PathBuf, Instant)> {
    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        while let Some(change) = source.next() {
            if sender.send(change).is_err() {
                break;
            }
        }
    });
    receiver
}

/// Receive until every path in `expected` has arrived; return everything seen.
fn receive_all(changes: &Receiver<(PathBuf, Instant)>, expected: &[&PathBuf]) -> BTreeSet<PathBuf> {
    let mut seen = BTreeSet::new();
    while !expected.iter().all(|path| seen.contains(*path)) {
        match changes.recv_timeout(HANG_GUARD) {
            Ok((path, at)) => {
                assert!(at <= Instant::now());
                seen.insert(path);
            }
            Err(RecvTimeoutError::Timeout) => {
                panic!("source never reported {expected:?}; reported {seen:?}")
            }
            Err(RecvTimeoutError::Disconnected) => panic!("source closed"),
        }
    }
    seen
}

#[test]
fn watch_source_reports_canonical_changes_the_watched_set_names() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        (
            "app/tsconfig.json",
            r#"{"compilerOptions":{"outDir":"dist"},"exclude":["dist"]}"#,
        ),
        ("app/src/View.vue", "<template><div /></template>\n"),
        ("app/src/Old.ts", "export {};\n"),
        ("app/src/Before.ts", "export {};\n"),
        ("app/node_modules/pkg/package.json", "{}"),
        ("app/dist/.keep", ""),
        ("external/shared.ts", "export {};\n"),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let base = PathBuf::from(canonicalize_path(
        &fs.realpath(temp.path().to_str().unwrap()).unwrap(),
    ));
    let at = |name: &str| base.join(name);
    let write = |path: &PathBuf, text: &str| fs.write_file(path.to_str().unwrap(), text).unwrap();

    let config = at("app/tsconfig.json");
    let graph = ProjectGraph::load(BuildMode::Project, std::slice::from_ref(&config)).unwrap();
    let carrier = at("app/src/View.vue");
    let shared = at("external/shared.ts");
    let mut binding = ProjectWatchProjectionBinding::new(&graph);
    binding.record_read(&config, &carrier, std::slice::from_ref(&shared));

    let mut source = FileSystemChangeSource::new().unwrap();
    source.watch(&binding.watched_paths()).unwrap();
    let changes = drain(source);

    // Paths no read set names, under an excluded output directory and under
    // node_modules, plus a file no membership claims. Written first: events
    // arrive in order, so they would be seen before the reported changes.
    let ignored = [
        at("app/node_modules/pkg/index.d.ts"),
        at("app/dist/index.d.ts"),
        at("app/dist/index.js"),
        at("app/notes.txt"),
    ];
    for path in &ignored {
        write(path, "export {};\n");
    }

    let created = at("app/src/New.ts");
    let deleted = at("app/src/Old.ts");
    let renamed_from = at("app/src/Before.ts");
    let renamed_to = at("app/src/After.ts");
    write(&carrier, "<template><span /></template>\n");
    write(&created, "export {};\n");
    std::fs::remove_file(&deleted).unwrap();
    std::fs::rename(&renamed_from, &renamed_to).unwrap();
    // Outside every project root, named by the carrier's read set.
    write(&shared, "export const changed = 1;\n");

    let seen = receive_all(
        &changes,
        &[
            &carrier,
            &created,
            &deleted,
            &renamed_from,
            &renamed_to,
            &shared,
        ],
    );
    for path in &ignored {
        assert!(!seen.contains(path), "{path:?} was reported: {seen:?}");
    }
}

#[test]
fn watch_source_replaces_the_watched_set() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        ("app/tsconfig.json", r#"{"include":["*.vue"]}"#),
        ("app/View.vue", "<template><div /></template>\n"),
        ("app/Other.vue", "<template><div /></template>\n"),
        ("external/shared.ts", "export {};\n"),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let base = PathBuf::from(canonicalize_path(
        &fs.realpath(temp.path().to_str().unwrap()).unwrap(),
    ));
    let config = base.join("app/tsconfig.json");
    let carrier = base.join("app/View.vue");
    let other = base.join("app/Other.vue");
    let shared = base.join("external/shared.ts");
    let write = |path: &PathBuf, text: &str| fs.write_file(path.to_str().unwrap(), text).unwrap();
    let graph = ProjectGraph::load(BuildMode::Project, std::slice::from_ref(&config)).unwrap();
    let mut binding = ProjectWatchProjectionBinding::new(&graph);
    binding.record_read(&config, &carrier, std::slice::from_ref(&shared));

    let mut source = FileSystemChangeSource::new().unwrap();
    source.watch(&binding.watched_paths()).unwrap();
    // The carrier no longer reads the external file.
    binding.record_read(&config, &carrier, &[]);
    source.watch(&binding.watched_paths()).unwrap();
    let rewatcher = source.rewatcher();
    let changes = drain(source);

    write(&shared, "export const changed = 1;\n");
    write(&carrier, "<template><span /></template>\n");
    let seen = receive_all(&changes, &[&carrier]);
    assert!(!seen.contains(&shared), "{seen:?}");

    // Handed over while the source blocks on its own thread. The first write
    // of another carrier after it arrives only once the set was applied.
    binding.record_read(&config, &carrier, std::slice::from_ref(&shared));
    rewatcher.watch(binding.watched_paths());
    write(&other, "<template><span /></template>\n");
    receive_all(&changes, &[&other]);
    write(&shared, "export const again = 2;\n");
    receive_all(&changes, &[&shared]);
}

#[test]
fn watch_source_observes_a_missing_engine_probe_from_its_ancestor_without_its_subtree() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        ("app/tsconfig.json", r#"{"include":["*.vue"]}"#),
        ("app/View.vue", "<template><div /></template>\n"),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let base = PathBuf::from(canonicalize_path(
        &fs.realpath(temp.path().to_str().unwrap()).unwrap(),
    ));
    let config = base.join("app/tsconfig.json");
    let graph = ProjectGraph::load(BuildMode::Project, std::slice::from_ref(&config)).unwrap();
    let mut binding = ProjectWatchProjectionBinding::new(&graph);
    // An engine lookup probe outside the project, two directories below the
    // nearest one that exists, as probes up to a home directory or the
    // filesystem root are.
    let probe = base.join("outer/node_modules/pkg");
    binding.record_program_reads(&config, std::slice::from_ref(&probe));
    let watched = binding.watched_paths();

    let registered = registrations(&watched);
    assert_eq!(
        registered.get(&base).copied(),
        Some(Registration {
            mode: RecursiveMode::NonRecursive,
            required: true,
        })
    );
    let app = base.join("app");
    for (directory, registration) in &registered {
        assert!(
            registration.mode == RecursiveMode::NonRecursive || directory.starts_with(&app),
            "{directory:?} is watched with its subtree: {registered:?}"
        );
    }

    // The probed directory created with its missing parents at once.
    let mut source = FileSystemChangeSource::new().unwrap();
    source.watch(&watched).unwrap();
    let changes = drain(source);
    std::fs::create_dir_all(&probe).unwrap();
    receive_all(&changes, &[&probe]);
}

#[test]
fn watch_source_reports_lost_events_directory_arrivals_and_removals() {
    use notify::event::{AccessKind, CreateKind, Flag, RemoveKind};
    use notify::RecursiveMode;

    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        (
            "app/tsconfig.json",
            r#"{"include":["src/*.vue"],"references":[{"path":"../missing"}]}"#,
        ),
        ("app/src/View.vue", ""),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let base = PathBuf::from(canonicalize_path(
        &fs.realpath(temp.path().to_str().unwrap()).unwrap(),
    ));
    let config = base.join("app/tsconfig.json");
    let carrier = base.join("app/src/View.vue");
    let graph = ProjectGraph::load(BuildMode::Project, std::slice::from_ref(&config)).unwrap();
    let watched = ProjectWatchProjectionBinding::new(&graph).watched_paths();
    let mut source = FileSystemChangeSource::new().unwrap();
    source.watch(&watched).unwrap();
    let at = Instant::now();
    let take = |source: &mut FileSystemChangeSource| -> BTreeSet<PathBuf> {
        source.pending.drain(..).map(|(path, _)| path).collect()
    };

    source.accept(Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan)), at);
    let named: BTreeSet<PathBuf> = watched.named_paths().map(Path::to_path_buf).collect();
    assert!(named.contains(&config) && named.contains(&carrier));
    assert!(named.contains(&base.join("missing")));
    assert_eq!(take(&mut source), named);

    source.accept(Err(notify::Error::generic("lost")), at);
    assert_eq!(take(&mut source), named);

    // A directory's own timestamp change, an access and a file no membership
    // claims report nothing.
    let src = base.join("app/src");
    source.accept(
        Ok(Event::new(EventKind::Access(AccessKind::Any)).add_path(carrier.clone())),
        at,
    );
    source.accept(
        Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(src.clone())),
        at,
    );
    source.accept(
        Ok(Event::new(EventKind::Create(CreateKind::File)).add_path(base.join("app/notes.txt"))),
        at,
    );
    assert!(take(&mut source).is_empty());

    // A directory that arrives whole inside the member walk reports the
    // project's config, so the graph reload walks its files; one the walk
    // never enters reports nothing.
    for name in [
        "app/src/sub/Inner.vue",
        "app/src/node_modules/pkg/Inner.vue",
    ] {
        fs.write_file(base.join(name).to_str().unwrap(), "")
            .unwrap();
    }
    source.accept(
        Ok(Event::new(EventKind::Create(CreateKind::Folder)).add_path(base.join("app/src/sub"))),
        at,
    );
    assert_eq!(take(&mut source), BTreeSet::from([config.clone()]));
    source.accept(
        Ok(Event::new(EventKind::Create(CreateKind::Folder))
            .add_path(base.join("app/src/node_modules"))),
        at,
    );
    assert!(take(&mut source).is_empty());

    // A removed directory reports the named files it held, and its watch
    // moves to the nearest existing ancestor.
    std::fs::remove_dir_all(&src).unwrap();
    source.accept(
        Ok(Event::new(EventKind::Remove(RemoveKind::Any)).add_path(src.clone())),
        at,
    );
    assert_eq!(take(&mut source), BTreeSet::from([carrier]));
    assert!(source.failure().is_none());
    assert!(!source.registered.contains_key(&src));
    assert!(
        source.registered.iter().any(
            |(directory, mode)| *mode == RecursiveMode::Recursive && src.starts_with(directory)
        ),
        "{:?}",
        source.registered
    );
}

#[test]
fn watch_source_follows_directories_moved_in_out_and_recreated() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        ("app/tsconfig.json", r#"{"exclude":["dist"]}"#),
        ("app/dist/.keep", ""),
        ("app/src/View.vue", "<template><div /></template>\n"),
        ("outer/deep/shared.ts", "export {};\n"),
        ("staging/Moved/Inner.vue", "<template><div /></template>\n"),
        ("staging/out/index.d.ts", "export {};\n"),
        (
            "staging/outer/deep/shared.ts",
            "export const recreated = 1;\n",
        ),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let base = PathBuf::from(canonicalize_path(
        &fs.realpath(temp.path().to_str().unwrap()).unwrap(),
    ));
    let at = |name: &str| base.join(name);
    let write = |path: &PathBuf, text: &str| fs.write_file(path.to_str().unwrap(), text).unwrap();
    let config = at("app/tsconfig.json");
    let carrier = at("app/src/View.vue");
    let shared = at("outer/deep/shared.ts");
    let graph = ProjectGraph::load(BuildMode::Project, std::slice::from_ref(&config)).unwrap();
    let mut binding = ProjectWatchProjectionBinding::new(&graph);
    binding.record_read(&config, &carrier, std::slice::from_ref(&shared));
    let mut source = FileSystemChangeSource::new().unwrap();
    source.watch(&binding.watched_paths()).unwrap();
    let changes = drain(source);

    // Directories arriving where the member walk never goes: an excluded
    // output directory and a package. The carrier write after them is the
    // marker that every earlier event was seen.
    std::fs::rename(at("staging/out"), at("app/dist/out")).unwrap();
    std::fs::create_dir_all(at("app/node_modules/newpkg")).unwrap();
    write(&carrier, "<template><span /></template>\n");
    let seen = receive_all(&changes, &[&carrier]);
    assert!(!seen.contains(&config), "{seen:?}");

    // A populated directory moved into the walk: its files carry no events.
    std::fs::rename(at("staging/Moved"), at("app/src/Moved")).unwrap();
    receive_all(&changes, &[&config]);

    // Renaming away, outside every root, an ancestor two levels above a read:
    // only the ancestor's parent sees it.
    std::fs::rename(at("outer"), at("outer-old")).unwrap();
    receive_all(&changes, &[&shared]);
    write(&carrier, "<template><p /></template>\n");
    receive_all(&changes, &[&carrier]);

    // A tree of the same name moved back in, then edited: the new
    // tree is watched, not the one renamed away.
    std::fs::rename(at("staging/outer"), at("outer")).unwrap();
    receive_all(&changes, &[&shared]);
    write(&shared, "export const edited = 1;\n");
    receive_all(&changes, &[&shared]);
}

#[test]
fn watch_source_registers_needed_directories_and_every_ancestor_as_a_sentinel() {
    let temp = tempfile::tempdir().unwrap();
    let fs = NativeFs::new();
    for (name, text) in [
        ("app/tsconfig.json", r#"{"include":["src/*.vue"]}"#),
        ("app/src/View.vue", ""),
        ("outer/deep/shared.ts", ""),
    ] {
        fs.write_file(temp.path().join(name).to_str().unwrap(), text)
            .unwrap();
    }
    let base = PathBuf::from(canonicalize_path(
        &fs.realpath(temp.path().to_str().unwrap()).unwrap(),
    ));
    let config = base.join("app/tsconfig.json");
    let carrier = base.join("app/src/View.vue");
    let graph = ProjectGraph::load(BuildMode::Project, std::slice::from_ref(&config)).unwrap();
    let mut binding = ProjectWatchProjectionBinding::new(&graph);
    binding.record_read(&config, &carrier, &[base.join("outer/deep/shared.ts")]);
    let registrations = registrations(&binding.watched_paths());

    let required = |mode| {
        Some(Registration {
            mode,
            required: true,
        })
    };
    assert_eq!(
        registrations.get(&base.join("app/src")).copied(),
        required(RecursiveMode::Recursive)
    );
    assert_eq!(
        registrations.get(&base.join("app")).copied(),
        required(RecursiveMode::NonRecursive)
    );
    assert_eq!(
        registrations.get(&base.join("outer/deep")).copied(),
        required(RecursiveMode::NonRecursive)
    );
    // Every ancestor up to the filesystem root, without its subtree.
    for ancestor in base.join("outer").ancestors() {
        assert_eq!(
            registrations.get(ancestor).copied(),
            Some(Registration {
                mode: RecursiveMode::NonRecursive,
                required: false,
            }),
            "{ancestor:?}"
        );
    }
}
