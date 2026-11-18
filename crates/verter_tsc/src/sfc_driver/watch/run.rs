//! The watch loop.
//!
//! A reader thread takes the change source's paths and hands them to the
//! loop, which debounces them into batches and checks each batch through the
//! [`WatchSession`]. For each change, the reader first hands it over, then
//! restarts the admission generation of every project the change makes
//! stale, so a check of such a project still in flight is refused at
//! admission. Before it prints a run, the loop takes the changes that
//! arrived meanwhile; when there are any, the run prints nothing, and the
//! batch of those changes checks again and prints the outcome.

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use verter_session::sfc_projection_mapper::MapperProjectHandle;
use verter_session::sfc_projection_observations::ProjectionPublicationAdmission;

use super::super::{
    exit_status, outcome_diagnostics, outcome_renderer, render_outcome, DriverRequest, ExitStatus,
    ProjectWatchProjectionBinding, Recheck, RunOutcome,
};
use super::source::WatchError;
use super::{ChangeSource, Debouncer, WatchSession};
use crate::api_check::TypecheckError;
use crate::position::ColumnEncoding;

/// How long the latest change must be quiet before its batch is checked, as
/// tsc waits.
const QUIET: Duration = Duration::from_millis(250);

/// Printed when a batch starts a check. The run that completes the batch
/// prints its outcome under it, also after an overtaken run printed nothing.
pub(crate) const CHANGE_LINE: &str = "File change detected. Starting incremental compilation...";

/// Run, then recheck on every batch until the source closes. Returns the
/// exit status of the last run, or `Failed` when the engine became unusable.
///
/// Diagnostics never end the loop. A run that a newer change overtook prints
/// nothing, and the newer change's batch is checked exactly once more. Batches
/// still pending when the source closes are checked before it returns; a
/// source that closed because it failed then returns `Failed`.
pub fn run_watch(
    request: &DriverRequest,
    source: &mut (dyn ChangeSource + Send),
    now: &dyn Fn() -> Instant,
    out: &mut dyn Write,
) -> ExitStatus {
    // The graph's paths are watched before the first check, so a change
    // while it runs is reported once the reader starts.
    let first = WatchSession::load(request).and_then(|mut session| {
        source
            .watch(&session.watched_paths())
            .map_err(|error| TypecheckError::new(error.to_string()))?;
        let first = session.check_all()?;
        Ok((session, first))
    });
    let (mut session, first) = match first {
        Ok(opened) => opened,
        Err(error) => {
            let _ = writeln!(out, "{error}");
            return ExitStatus::Failed;
        }
    };
    let mut status = report(&first, out);
    if let Err(error) = source.watch(&session.watched_paths()) {
        let _ = writeln!(out, "{error}");
        return ExitStatus::Failed;
    }
    let rewatcher = source.rewatcher();
    let overtaking = Mutex::new(Overtaking::of(&session));
    let (sender, changes) = channel();
    std::thread::scope(|scope| {
        let overtaking = &overtaking;
        scope.spawn(move || read(source, overtaking, &sender));
        let mut feed = Feed::new(changes);
        // Set while a batch's outcome is still owed: its run was overtaken
        // and printed nothing.
        let mut owed = false;
        while let Some(batch) = feed.next_batch(now) {
            let recheck = session.recheck_for(&batch);
            if !owed && is_empty(&recheck) {
                continue;
            }
            if !owed {
                let _ = writeln!(out, "{CHANGE_LINE}");
            }
            // The reader restarts for a change it already handed over only
            // while it holds this lock: taking it here lets every such
            // restart land before this run binds, so none refuses it.
            drop(overtaking.lock().unwrap_or_else(PoisonError::into_inner));
            let outcome = session.recheck(&recheck);
            *overtaking.lock().unwrap_or_else(PoisonError::into_inner) = Overtaking::of(&session);
            rewatcher.watch(session.watched_paths());
            owed = feed.drain();
            if !owed {
                status = report(&outcome, out);
            }
        }
        match feed.closed {
            Some(Some(failure)) => {
                let _ = writeln!(out, "{failure}");
                ExitStatus::Failed
            }
            _ => status,
        }
    })
}

/// Print an outcome's diagnostics and the line that ends a watched run.
/// Returns its exit status.
fn report(outcome: &RunOutcome, out: &mut dyn Write) -> ExitStatus {
    let _ = render_outcome(
        outcome,
        &outcome_renderer(outcome, ColumnEncoding::default()),
        out,
    );
    let errors = crate::error_count(&outcome_diagnostics(outcome));
    let _ = if errors == 1 {
        writeln!(out, "Found 1 error. Watching for file changes.")
    } else {
        writeln!(out, "Found {errors} errors. Watching for file changes.")
    };
    exit_status(outcome)
}

/// Whether a batch makes nothing stale.
fn is_empty(recheck: &Recheck) -> bool {
    recheck.projects.is_empty() && recheck.carriers.is_empty() && !recheck.reload_graph
}

/// What the reader hands the loop.
enum Change {
    Path(PathBuf, Instant),
    /// The source closed, with why when it failed. Nothing follows.
    Closed(Option<WatchError>),
}

/// Take the source's changes until it closes. Each change reaches the loop
/// before any project is restarted for it, so a check refused for a restart
/// always belongs to a run the loop learns was overtaken. Both happen under
/// the lock the loop takes before a run starts, so a change's restart never
/// lands inside the run that checks it.
fn read(
    source: &mut (dyn ChangeSource + Send),
    overtaking: &Mutex<Overtaking>,
    changes: &Sender<Change>,
) {
    while let Some((path, at)) = source.next() {
        let stale = overtaking.lock().unwrap_or_else(PoisonError::into_inner);
        if changes.send(Change::Path(path.clone(), at)).is_err() {
            return;
        }
        stale.restart(path);
    }
    let _ = changes.send(Change::Closed(source.failure().cloned()));
}

/// The projects a change makes stale, as the last run's binding says, and
/// the admission their checks pass.
struct Overtaking {
    binding: ProjectWatchProjectionBinding,
    admission: Arc<ProjectionPublicationAdmission>,
}

impl Overtaking {
    fn of(session: &WatchSession) -> Self {
        Self {
            binding: session.binding().clone(),
            admission: session.admission(),
        }
    }

    /// Restart every project `path` makes stale: a check of one that is in
    /// flight is refused when it admits its results.
    fn restart(&self, path: PathBuf) {
        for config in self.binding.recheck_for(&[path]).projects {
            self.admission
                .restart(&MapperProjectHandle::of_config(&config.to_string_lossy()));
        }
    }
}

/// The loop's side of the reader: changes debounced into batches.
struct Feed {
    changes: Receiver<Change>,
    debouncer: Debouncer,
    /// Set once the source closed, with why when it failed.
    closed: Option<Option<WatchError>>,
}

impl Feed {
    fn new(changes: Receiver<Change>) -> Self {
        Self {
            changes,
            debouncer: Debouncer::new(QUIET),
            closed: None,
        }
    }

    /// The next batch, once its latest change has been quiet for the window
    /// by `now`, or once a later change arrived after the window. After the
    /// source closed, what is pending is released at once; then `None`.
    fn next_batch(&mut self, now: &dyn Fn() -> Instant) -> Option<Vec<PathBuf>> {
        loop {
            if self.closed.is_some() {
                let deadline = self.debouncer.deadline()?;
                return self.debouncer.take_ready(deadline);
            }
            if let Some(batch) = self.debouncer.take_ready(now()) {
                return Some(batch);
            }
            let change = match self.debouncer.deadline() {
                // The wait only bounds how soon `now` is asked again; the
                // injected clock decides when the window has passed.
                Some(deadline) => match self
                    .changes
                    .recv_timeout(deadline.saturating_duration_since(now()))
                {
                    Ok(change) => change,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => Change::Closed(None),
                },
                None => self.changes.recv().unwrap_or(Change::Closed(None)),
            };
            match change {
                Change::Path(path, at) => {
                    let ready = self.debouncer.take_ready(at);
                    self.debouncer.push(path, at);
                    if ready.is_some() {
                        return ready;
                    }
                }
                Change::Closed(failure) => self.closed = Some(failure),
            }
        }
    }

    /// Take every change already handed over, without waiting. True when a
    /// path arrived.
    fn drain(&mut self) -> bool {
        let mut arrived = false;
        while self.closed.is_none() {
            match self.changes.try_recv() {
                Ok(Change::Path(path, at)) => {
                    arrived = true;
                    self.debouncer.push(path, at);
                }
                Ok(Change::Closed(failure)) => self.closed = Some(failure),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => self.closed = Some(None),
            }
        }
        arrived
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
pub(crate) mod tests;
