use std::path::Path;

use super::*;
use crate::sfc_driver::project_program::tests::{fake_engine, Fixture};

/// The identity the graph gives a fixture file.
fn canonical(f: &Fixture, name: &str) -> PathBuf {
    PathBuf::from(canonicalize_path(
        &f.fs.realpath(f.path(name).to_str().unwrap()).unwrap(),
    ))
}

/// A switch file beside the fake engine copy.
fn beside(engine: &Path, suffix: &str) -> PathBuf {
    let mut name = engine.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Make the fake record a pid file for every engine process it starts.
fn record_engine_processes(f: &Fixture, engine: &Path) {
    f.fs.create_dir_all(beside(engine, ".pids").to_str().unwrap())
        .unwrap();
}

/// `--api` engine processes started so far.
fn engine_processes(f: &Fixture, engine: &Path) -> usize {
    f.fs.read_dir(beside(engine, ".pids").to_str().unwrap())
        .unwrap()
        .iter()
        .filter(|entry| {
            Path::new(&entry.path)
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("api-"))
        })
        .count()
}

/// The serving ordinal of the engine session that answered a project.
fn session_ordinal(check: &ProjectCheck) -> u64 {
    check
        .binding
        .as_ref()
        .expect("the project was certified")
        .engine
        .serving_identity()
        .editor_session_generation
}

/// A project's config, carriers, rendered diagnostics and engine failure.
type Reported = (Arc<str>, Vec<Arc<str>>, String, Option<String>);

/// What a run reports per project.
fn reported(outcome: &RunOutcome) -> Vec<Reported> {
    outcome
        .projects
        .iter()
        .map(|project| {
            (
                project.config.clone(),
                project.carriers.iter().map(|c| c.carrier.clone()).collect(),
                format!("{:?} {:?}", project.attributed, project.diagnostics),
                project.engine_failure.clone(),
            )
        })
        .collect()
}

fn vue(text: &str) -> String {
    format!(
        r#"<script setup lang="ts">const value = {text};</script><template>{{{{ value }}}}</template>"#
    )
}

#[test]
fn watch_session_carrier_edit_composes_one_carrier_and_starts_no_engine() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"include":["*.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    for index in 0..10 {
        f.write(&format!("View{index}.vue"), &vue(&index.to_string()));
    }
    let engine = fake_engine(&f);
    record_engine_processes(&f, &engine);
    let (mut session, first) = WatchSession::open(&f.request(engine.clone())).unwrap();
    let before = &first.projects[0];
    assert!(
        before.engine_failure.is_none(),
        "{:?}",
        before.engine_failure
    );
    assert_eq!(first.carriers_checked, 10);
    let compositions = session.driver.mapper.composition_count();
    let processes = engine_processes(&f, &engine);
    assert!(processes > 0);

    let edited = canonical(&f, "View3.vue");
    f.write("View3.vue", &vue("'edited'"));
    let recheck = session.recheck_for(std::slice::from_ref(&edited));
    assert!(!recheck.reload_graph);
    assert_eq!(recheck.carriers.len(), 1);
    let next = session.recheck(&recheck);

    assert_eq!(session.driver.mapper.composition_count() - compositions, 1);
    assert_eq!(engine_processes(&f, &engine), processes);
    let after = &next.projects[0];
    assert!(after.engine_failure.is_none(), "{:?}", after.engine_failure);
    assert_eq!(after.carriers.len(), 10);
    for (old, new) in before.carriers.iter().zip(&after.carriers) {
        assert_eq!(old.carrier, new.carrier);
        if *new.carrier == *edited.to_str().unwrap() {
            assert!(new.source.contains("'edited'"));
            assert!(new.result.text.contains("'edited'"));
        } else {
            assert!(Arc::ptr_eq(&old.result, &new.result));
        }
    }
    // The same engine session answers under a new admission generation.
    assert_eq!(session_ordinal(after), 1);
    assert!(
        after.binding.as_ref().unwrap().generation > before.binding.as_ref().unwrap().generation
    );
}

#[test]
fn watch_session_reopens_a_failed_engine_session_on_the_next_recheck() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"include":["*.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write("View.vue", &vue("1"));
    let engine = fake_engine(&f);
    record_engine_processes(&f, &engine);
    let hollow = beside(&engine, ".hollow");
    f.fs.write_file(
        hollow.to_str().unwrap(),
        "tsconfig.json
",
    )
    .unwrap();
    let (mut session, first) = WatchSession::open(&f.request(engine.clone())).unwrap();
    let failure = first.projects[0]
        .engine_failure
        .as_deref()
        .expect("the hollow engine opens no program");
    assert!(
        failure.contains("did not open the configured project"),
        "{failure}"
    );
    assert!(first.projects[0].binding.is_none());
    let processes = engine_processes(&f, &engine);

    f.fs.delete_file(hollow.to_str().unwrap()).unwrap();
    let next = session.recheck(&Recheck::default());
    let project = &next.projects[0];
    assert!(
        project.engine_failure.is_none(),
        "{:?}",
        project.engine_failure
    );
    assert_eq!(project.carriers.len(), 1);
    // A failed open serves nothing, so the first certified session is the reopened one.
    assert_eq!(session_ordinal(project), 1);
    assert_eq!(engine_processes(&f, &engine), processes + 1);
}

#[test]
fn watch_session_config_edit_reopens_only_projects_whose_basis_moved() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./first.json"},{"path":"./second.json"}]}"#,
    );
    f.write(
        "first.json",
        r#"{"files":["First.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write(
        "second.json",
        r#"{"files":["Second.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write("First.vue", &vue("1"));
    f.write("Second.vue", &vue("2"));
    let engine = fake_engine(&f);
    record_engine_processes(&f, &engine);
    let (mut session, first) = WatchSession::open(&f.request(engine.clone())).unwrap();
    assert_eq!(first.projects.len(), 3);
    for project in &first.projects[..2] {
        assert!(
            project.engine_failure.is_none(),
            "{:?}",
            project.engine_failure
        );
        assert_eq!(session_ordinal(project), 1);
    }
    let processes = engine_processes(&f, &engine);

    f.write(
        "first.json",
        r#"{"files":["First.vue"],"compilerOptions":{"noEmit":true,"strict":true}}"#,
    );
    let recheck = session.recheck_for(&[canonical(&f, "first.json")]);
    assert!(recheck.reload_graph);
    let next = session.recheck(&recheck);
    assert_eq!(reported(&next).len(), 3);
    let (changed, kept) = (&next.projects[0], &next.projects[1]);
    assert!(changed.config.ends_with("first.json"));
    assert!(
        changed.engine_failure.is_none(),
        "{:?}",
        changed.engine_failure
    );
    assert!(kept.engine_failure.is_none(), "{:?}", kept.engine_failure);
    assert_eq!(session_ordinal(changed), 2);
    assert_eq!(session_ordinal(kept), 1);
    assert_eq!(engine_processes(&f, &engine), processes + 1);
}

#[test]
fn watch_session_added_and_deleted_carriers_match_a_fresh_run() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"include":["*.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write("View.vue", &vue("1"));
    let engine = fake_engine(&f);
    let request = f.request(engine);
    let fresh = || {
        SfcTypeScriptDriverAdapter::new()
            .run(&request, None)
            .unwrap()
    };
    let (mut session, first) = WatchSession::open(&request).unwrap();
    assert_eq!(reported(&first), reported(&fresh()));

    f.write("Added.vue", &vue("2"));
    let added = canonical(&f, "Added.vue");
    let recheck = session.recheck_for(std::slice::from_ref(&added));
    assert!(recheck.reload_graph);
    let next = session.recheck(&recheck);
    assert_eq!(next.carriers_checked, 2);
    assert_eq!(reported(&next), reported(&fresh()));

    f.fs.delete_file(added.to_str().unwrap()).unwrap();
    let recheck = session.recheck_for(std::slice::from_ref(&added));
    assert_eq!(recheck.carriers.len(), 1);
    let next = session.recheck(&recheck);
    assert_eq!(next.carriers_checked, 1);
    assert!(next.projects[0].carriers.iter().all(|c| !c.unreadable));
    assert_eq!(reported(&next), reported(&fresh()));
}

#[test]
fn watch_session_watches_member_roots_outside_the_config_directory() {
    let f = Fixture::new();
    f.write(
        ".nuxt/tsconfig.json",
        r#"{"include":["../app/**/*.vue"],"compilerOptions":{"noEmit":true}}"#,
    );
    f.write("app/View.vue", &vue("1"));
    let mut request = f.request(fake_engine(&f));
    request.configs = vec![f.path(".nuxt/tsconfig.json")];
    let (session, _) = WatchSession::open(&request).unwrap();
    let watched = session.watched_paths();
    for expected in [
        canonical(&f, ".nuxt/tsconfig.json"),
        canonical(&f, "app/View.vue"),
        // Not created yet: claimed by the include spec.
        canonical(&f, "app").join("Later.vue"),
    ] {
        assert!(watched.reports(&expected), "{expected:?} in {watched:?}");
    }
    let directories = watched
        .directories()
        .map(|(directory, recursive)| (directory.to_path_buf(), recursive))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(directories.get(&canonical(&f, "app")), Some(&true));
    assert!(directories.contains_key(&canonical(&f, ".nuxt")));
}

#[test]
fn watch_session_keeps_changes_a_failed_reload_did_not_consume() {
    let f = Fixture::new();
    let config = r#"{"include":["*.vue"],"compilerOptions":{"noEmit":true}}"#;
    f.write("tsconfig.json", config);
    f.write("View.vue", &vue("1"));
    let (mut session, _) = WatchSession::open(&f.request(fake_engine(&f))).unwrap();
    let carrier = canonical(&f, "View.vue");
    let root = canonical(&f, "tsconfig.json");

    f.write("View.vue", &vue("'edited'"));
    f.write("tsconfig.json", "{ not json");
    let broken = session.recheck(&session.recheck_for(&[carrier.clone(), root.clone()]));
    assert!(broken.projects.is_empty());
    assert_eq!(broken.graph_problems.len(), 1);

    f.write("tsconfig.json", config);
    let recheck = session.recheck_for(std::slice::from_ref(&root));
    assert!(recheck.carriers.is_empty());
    let restored = session.recheck(&recheck);
    let project = &restored.projects[0];
    assert!(
        project.engine_failure.is_none(),
        "{:?}",
        project.engine_failure
    );
    assert!(project.carriers[0].source.contains("'edited'"));
}
