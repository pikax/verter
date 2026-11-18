//! The loop over a scripted change source and a fixed clock, with the fake
//! engine.

use std::io::{self, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use verter_span::path::canonicalize_path;

use super::run_watch;
pub(crate) use super::CHANGE_LINE;
use crate::position::ColumnEncoding;
use crate::sfc_driver::project_program::tests::{fake_engine, Fixture};
use crate::sfc_driver::watch::{ChangeSource, Rewatcher, WatchError};
use crate::sfc_driver::{
    exit_status, outcome_diagnostics, outcome_renderer, render_outcome, DriverRequest, ExitStatus,
    SfcTypeScriptDriverAdapter, WatchedPaths,
};

/// Guards against a hang only; every outcome is decided by the script.
const HANG_GUARD: Duration = Duration::from_secs(15);

enum Step {
    Change(PathBuf, Instant),
    Fail(WatchError),
}

/// What the scripted source tells the test.
enum Seen {
    Watched(WatchedPaths),
    /// The reader asks for the next change: it handled the one before.
    Asked,
}

/// Reports the scripted changes in turn and closes when the script ends.
struct ScriptedSource {
    steps: Receiver<Step>,
    seen: Sender<Seen>,
    failure: Option<WatchError>,
}

impl ChangeSource for ScriptedSource {
    fn watch(&mut self, paths: &WatchedPaths) -> Result<(), WatchError> {
        let _ = self.seen.send(Seen::Watched(paths.clone()));
        Ok(())
    }

    fn next(&mut self) -> Option<(PathBuf, Instant)> {
        let _ = self.seen.send(Seen::Asked);
        match self.steps.recv() {
            Ok(Step::Change(path, at)) => Some((path, at)),
            Ok(Step::Fail(failure)) => {
                self.failure = Some(failure);
                None
            }
            Err(_) => None,
        }
    }

    fn failure(&self) -> Option<&WatchError> {
        self.failure.as_ref()
    }

    fn rewatcher(&self) -> Rewatcher {
        let seen = self.seen.clone();
        Rewatcher::new(move |paths| {
            let _ = seen.send(Seen::Watched(paths));
        })
    }
}

struct SharedOut(Arc<Mutex<Vec<u8>>>);

impl Write for SharedOut {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A loop on its own thread over a scripted source. Its clock stands still,
/// so a batch is released by a later change or by the source closing.
pub(crate) struct Watching {
    steps: Option<Sender<Step>>,
    seen: Receiver<Seen>,
    out: Arc<Mutex<Vec<u8>>>,
    looping: JoinHandle<ExitStatus>,
    clock: Instant,
    changes: u32,
    asks: u32,
    watched: Vec<WatchedPaths>,
}

impl Watching {
    pub(crate) fn start(request: DriverRequest) -> Self {
        let (steps, script) = mpsc::channel();
        let (told, seen) = mpsc::channel();
        let out = Arc::new(Mutex::new(Vec::new()));
        let clock = Instant::now();
        let printed = Arc::clone(&out);
        let looping = std::thread::spawn(move || {
            let mut source = ScriptedSource {
                steps: script,
                seen: told,
                failure: None,
            };
            run_watch(&request, &mut source, &|| clock, &mut SharedOut(printed))
        });
        Self {
            steps: Some(steps),
            seen,
            out,
            looping,
            clock,
            changes: 0,
            asks: 0,
            watched: Vec::new(),
        }
    }

    /// Wait until the reader handled every scripted change; before any, until
    /// the first run finished and the reader started.
    pub(crate) fn caught_up(&mut self) {
        while self.asks <= self.changes {
            match self.seen.recv_timeout(HANG_GUARD) {
                Ok(Seen::Asked) => self.asks += 1,
                Ok(Seen::Watched(paths)) => self.watched.push(paths),
                Err(error) => panic!("the loop stopped reading changes: {error:?}"),
            }
        }
    }

    /// Wait for the next watched set the loop gives the source.
    fn next_watched(&mut self) -> WatchedPaths {
        loop {
            match self.seen.recv_timeout(HANG_GUARD) {
                Ok(Seen::Asked) => self.asks += 1,
                Ok(Seen::Watched(paths)) => {
                    self.watched.push(paths.clone());
                    return paths;
                }
                Err(error) => panic!("the loop gave the source no watched set: {error:?}"),
            }
        }
    }

    /// Script a change of `path`, a quiet window after the one before, so it
    /// releases the batch before it.
    pub(crate) fn change(&mut self, path: &Path) {
        self.changes += 1;
        let at = self.clock + Duration::from_secs(u64::from(self.changes));
        self.steps
            .as_ref()
            .unwrap()
            .send(Step::Change(path.to_path_buf(), at))
            .unwrap();
    }

    /// Close the source, failing when `failure` is given; return the loop's
    /// status, its output lines and every watched set it gave the source.
    pub(crate) fn close(
        mut self,
        failure: Option<WatchError>,
    ) -> (ExitStatus, Vec<String>, Vec<WatchedPaths>) {
        if let Some(failure) = failure {
            self.steps
                .as_ref()
                .unwrap()
                .send(Step::Fail(failure))
                .unwrap();
        }
        drop(self.steps.take());
        let status = self.looping.join().unwrap();
        while let Ok(seen) = self.seen.try_recv() {
            if let Seen::Watched(paths) = seen {
                self.watched.push(paths);
            }
        }
        let out = String::from_utf8(self.out.lock().unwrap().clone()).unwrap();
        (
            status,
            out.lines().map(str::to_owned).collect(),
            self.watched,
        )
    }
}

/// The diagnostic lines a fresh run prints, and its error count.
pub(crate) fn fresh(request: &DriverRequest) -> (Vec<String>, usize) {
    let outcome = SfcTypeScriptDriverAdapter::new()
        .run(request, None)
        .unwrap();
    let mut out = Vec::new();
    render_outcome(
        &outcome,
        &outcome_renderer(&outcome, ColumnEncoding::default()),
        &mut out,
    )
    .unwrap();
    let lines = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    (lines, crate::error_count(&outcome_diagnostics(&outcome)))
}

/// tsc's line after a watched run.
pub(crate) fn found(errors: usize) -> String {
    match errors {
        1 => "Found 1 error. Watching for file changes.".to_owned(),
        errors => format!("Found {errors} errors. Watching for file changes."),
    }
}

/// The identity the graph gives the fixture directory.
fn base(f: &Fixture) -> PathBuf {
    PathBuf::from(canonicalize_path(
        &f.fs.realpath(f.dir.path().to_str().unwrap()).unwrap(),
    ))
}

/// The identity the graph gives a fixture file.
fn canonical(f: &Fixture, name: &str) -> PathBuf {
    PathBuf::from(canonicalize_path(
        &f.fs.realpath(f.path(name).to_str().unwrap()).unwrap(),
    ))
}

/// A path no project reads or contains.
fn elsewhere(f: &Fixture) -> PathBuf {
    base(f).parent().unwrap().join("not-in-any-project.txt")
}

/// A switch file beside the fake engine copy.
fn beside(engine: &Path, suffix: &str) -> PathBuf {
    let mut name = engine.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Make the next engine session that answers semantic diagnostics wait at a
/// loopback gate; the receiver yields the connection once it waits there.
fn hold_next_check(f: &Fixture, engine: &Path) -> Receiver<TcpStream> {
    let gate = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = gate.local_addr().unwrap().port();
    f.fs.write_file(beside(engine, ".gate").to_str().unwrap(), &port.to_string())
        .unwrap();
    let (opened, waiting) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = gate.accept() {
            let _ = opened.send(stream);
        }
    });
    waiting
}

/// The held check's connection: one byte on it lets the check go on.
fn held(gate: &Receiver<TcpStream>) -> TcpStream {
    gate.recv_timeout(HANG_GUARD)
        .expect("the check reaches the engine's diagnostics")
}

/// Make the fake record its processes and diagnostics answers.
fn record_engine(f: &Fixture, engine: &Path) {
    f.fs.create_dir_all(beside(engine, ".pids").to_str().unwrap())
        .unwrap();
}

/// The fake's records whose names start with `prefix`.
fn records(f: &Fixture, engine: &Path, prefix: &str) -> Vec<PathBuf> {
    f.fs.read_dir(beside(engine, ".pids").to_str().unwrap())
        .unwrap()
        .iter()
        .map(|entry| PathBuf::from(&entry.path))
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(prefix))
        })
        .collect()
}

/// Engine processes started in `mode` (`api` or `build`).
fn processes(f: &Fixture, engine: &Path, mode: &str) -> usize {
    records(f, engine, &format!("{mode}-")).len()
}

/// Semantic-diagnostics answers the engine sessions served: one per check of
/// a project program.
fn checks_served(f: &Fixture, engine: &Path) -> usize {
    records(f, engine, "diagnostics-")
        .iter()
        .map(|path| std::fs::read_to_string(path).unwrap().lines().count())
        .sum()
}

fn vue(text: &str) -> String {
    format!(
        r#"<script setup lang="ts">const value = {text};</script><template>{{{{ value }}}}</template>"#
    )
}

const PROJECT: &str = r#"{"include":["*.vue"],"compilerOptions":{"noEmit":true}}"#;

#[test]
fn watch_loop_prints_each_run_as_a_fresh_one_and_a_fixing_edit_reports_no_error() {
    let f = Fixture::new();
    f.write("tsconfig.json", PROJECT);
    f.write(
        "View.vue",
        "<script setup lang=\"ts\">const a = </script>\n",
    );
    f.write("Clean.vue", &vue("1"));
    let request = f.request(fake_engine(&f));
    let (broken, errors) = fresh(&request);
    assert!(errors > 0, "{broken:?}");
    let mut watching = Watching::start(request.clone());
    watching.caught_up();

    f.write("View.vue", &vue("2"));
    let (fixed, no_errors) = fresh(&request);
    assert_eq!((fixed.len(), no_errors), (0, 0), "{fixed:?}");
    watching.change(&canonical(&f, "View.vue"));
    let (status, output, watched) = watching.close(None);

    // The run with errors did not end the loop.
    let mut expected = broken;
    expected.push(found(errors));
    expected.push(CHANGE_LINE.to_owned());
    expected.push(found(0));
    assert_eq!(output, expected);
    assert_eq!(status, ExitStatus::Clean);
    // The graph's set before the first run, then each run's.
    assert_eq!(watched.len(), 3);
    for set in &watched {
        assert!(set.reports(&canonical(&f, "View.vue")));
    }
}

#[test]
fn watch_loop_a_change_during_a_check_is_checked_once_more_and_the_overtaken_check_prints_nothing()
{
    let f = Fixture::new();
    f.write("tsconfig.json", PROJECT);
    f.write("View.vue", &vue("1"));
    let engine = fake_engine(&f);
    record_engine(&f, &engine);
    let request = f.request(engine.clone());
    let mut watching = Watching::start(request.clone());
    watching.caught_up();
    assert_eq!(processes(&f, &engine, "api"), 1);
    assert_eq!(checks_served(&f, &engine), 1);

    // The next check waits in the engine after its binding, before it
    // publishes, until the gate opens.
    let gate = hold_next_check(&f, &engine);
    let view = canonical(&f, "View.vue");
    f.write("View.vue", &vue("2"));
    watching.change(&view);
    // A window later, a change nothing reads releases the edit's batch.
    watching.change(&elsewhere(&f));
    let mut held = held(&gate);

    // A newer edit arrives while that check waits.
    f.write("View.vue", &vue("3"));
    watching.change(&view);
    watching.caught_up();
    held.write_all(&[1]).unwrap();
    drop(held);
    let (status, output, _) = watching.close(None);
    // The first check, the overtaken one, and exactly one more, all in the
    // first engine process: the overtaken check kept its program.
    assert_eq!(checks_served(&f, &engine), 3);
    assert_eq!(processes(&f, &engine, "api"), 1);

    let (lines, errors) = fresh(&request);
    let mut expected = vec![found(0), CHANGE_LINE.to_owned()];
    expected.extend(lines);
    expected.push(found(errors));
    assert_eq!(output, expected);
    assert_eq!(status, ExitStatus::Clean);
}

#[test]
fn watch_loop_watches_the_graph_before_the_first_check_and_rechecks_an_edit_made_during_it() {
    let f = Fixture::new();
    f.write("tsconfig.json", PROJECT);
    f.write("View.vue", &vue("1"));
    let engine = fake_engine(&f);
    // The first check waits in the engine, after its carriers were projected.
    let gate = hold_next_check(&f, &engine);
    let request = f.request(engine);
    let mut watching = Watching::start(request.clone());
    let mut held = held(&gate);
    let view = canonical(&f, "View.vue");
    assert!(watching.next_watched().reports(&view));

    // An edit while the first check runs, reported by the watch already set.
    f.write(
        "View.vue",
        "<script setup lang=\"ts\">const a = </script>\n",
    );
    watching.change(&view);
    held.write_all(&[1]).unwrap();
    drop(held);
    let (status, output, _) = watching.close(None);

    let (lines, errors) = fresh(&request);
    assert!(errors > 0, "{lines:?}");
    let mut expected = vec![found(0), CHANGE_LINE.to_owned()];
    expected.extend(lines);
    expected.push(found(errors));
    assert_eq!(output, expected);
    assert_eq!(
        status,
        exit_status(
            &SfcTypeScriptDriverAdapter::new()
                .run(&request, None)
                .unwrap()
        )
    );
}

#[test]
fn watch_loop_rechecks_a_project_when_a_directory_its_engine_probed_is_created() {
    let f = Fixture::new();
    f.write("app/tsconfig.json", PROJECT);
    f.write("app/View.vue", &vue("1"));
    let engine = fake_engine(&f);
    record_engine(&f, &engine);
    // Outside the project, below a directory that does not exist either.
    let probe = base(&f).join("outside").join("lib");
    f.fs.write_file(
        beside(&engine, ".probes").to_str().unwrap(),
        probe.to_str().unwrap(),
    )
    .unwrap();
    let mut request = f.request(engine.clone());
    request.configs = vec![f.path("app/tsconfig.json")];
    let mut watching = Watching::start(request);
    watching.caught_up();
    // The graph names nothing outside the project; the set given after the
    // first check adds what its engine read.
    assert_eq!(watching.watched.len(), 2);
    assert!(!watching.watched[0].reports(&probe));
    assert!(
        watching.watched[1].reports(&probe),
        "{:?}",
        watching.watched
    );

    watching.change(&elsewhere(&f));
    std::fs::create_dir_all(&probe).unwrap();
    watching.change(&probe);
    let failure = WatchError {
        directory: base(&f),
        detail: "the directory was removed".to_owned(),
    };
    let (status, output, _) = watching.close(Some(failure.clone()));

    // The unrelated change checked nothing; the created directory rechecked
    // the project, and the failed source then ended the loop.
    assert_eq!(
        output,
        [
            found(0),
            CHANGE_LINE.to_owned(),
            found(0),
            failure.to_string()
        ]
    );
    assert_eq!(status, ExitStatus::Failed);
    assert_eq!(checks_served(&f, &engine), 2);
}

#[test]
fn watch_loop_a_package_edit_reruns_the_engine_build_then_rechecks_the_dependent_project() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"./pkg"},{"path":"./app"}]}"#,
    );
    f.write(
        "pkg/tsconfig.json",
        r#"{"compilerOptions":{"composite":true,"outDir":"dist"},"include":["src"]}"#,
    );
    f.write("pkg/src/index.ts", "export const value = 1;\n");
    f.write(
        "app/tsconfig.json",
        r#"{"include":["*.vue"],"compilerOptions":{"noEmit":true},"references":[{"path":"../pkg"}]}"#,
    );
    f.write("app/View.vue", &vue("1"));
    let engine = fake_engine(&f);
    record_engine(&f, &engine);
    let mut watching = Watching::start(f.request(engine.clone()));
    watching.caught_up();
    // The package is built by the engine; only the app is a program.
    assert_eq!(processes(&f, &engine, "build"), 1);
    assert_eq!(processes(&f, &engine, "api"), 1);
    assert_eq!(checks_served(&f, &engine), 1);

    f.write("pkg/src/index.ts", "export const value = 2;\n");
    watching.change(&canonical(&f, "pkg/src/index.ts"));
    let (status, output, _) = watching.close(None);

    assert_eq!(output, [found(0), CHANGE_LINE.to_owned(), found(0)]);
    assert_eq!(status, ExitStatus::Clean);
    assert_eq!(processes(&f, &engine, "build"), 2);
    assert_eq!(processes(&f, &engine, "api"), 1);
    assert_eq!(checks_served(&f, &engine), 2);
}

#[test]
fn watch_loop_ends_with_exit_2_when_the_engine_is_unusable() {
    let f = Fixture::new();
    f.write("tsconfig.json", PROJECT);
    f.write("View.vue", &vue("1"));
    let watching = Watching::start(f.request(f.path("no-engine-here")));
    let (status, output, watched) = watching.close(None);
    assert_eq!(status, ExitStatus::Failed);
    assert!(!output.is_empty());
    assert!(
        output.iter().all(|line| !line.contains("Watching")),
        "{output:?}"
    );
    // Only the graph's paths, given before the check failed.
    assert_eq!(watched.len(), 1);
}
