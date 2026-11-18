//! Where changed paths come from.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use notify::event::{Event, EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use verter_span::path::canonicalize_path;

use super::super::watch_binding::WatchedPaths;

/// Where changed paths come from.
pub trait ChangeSource {
    /// Replace the watched set. An error names a directory the source cannot
    /// observe, so changes below it would be missed.
    fn watch(&mut self, paths: &WatchedPaths) -> Result<(), WatchError>;
    /// Block until a path changed or the source closed (`None`). A source
    /// that can no longer observe its watched set closes; [`Self::failure`]
    /// says why.
    fn next(&mut self) -> Option<(PathBuf, Instant)>;
    /// Why the source closed, when it failed.
    fn failure(&self) -> Option<&WatchError> {
        None
    }
    /// A handle that replaces the watched set from another thread while
    /// [`Self::next`] blocks. The source observes the newest set before it
    /// reports another change; a set it cannot observe closes it.
    fn rewatcher(&self) -> Rewatcher;
}

/// Hands a change source a new watched set from another thread.
#[derive(Clone)]
pub struct Rewatcher(Arc<dyn Fn(WatchedPaths) + Send + Sync>);

impl Rewatcher {
    pub fn new(deliver: impl Fn(WatchedPaths) + Send + Sync + 'static) -> Self {
        Self(Arc::new(deliver))
    }

    /// Replace the source's watched set.
    pub fn watch(&self, paths: WatchedPaths) {
        (self.0)(paths);
    }
}

/// A directory the platform watcher refused to observe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchError {
    pub directory: PathBuf,
    pub detail: String,
}

impl fmt::Display for WatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot watch '{}' for changes: {}",
            self.directory.display(),
            self.detail
        )
    }
}

/// The operating system's file events, reported as canonical paths and
/// filtered by the watched set.
pub struct FileSystemChangeSource {
    watcher: RecommendedWatcher,
    events: Receiver<Incoming>,
    /// Feeds `events` with watched sets handed over by a [`Rewatcher`].
    sender: Sender<Incoming>,
    watched: WatchedPaths,
    registered: BTreeMap<PathBuf, RecursiveMode>,
    pending: VecDeque<(PathBuf, Instant)>,
    failure: Option<WatchError>,
}

/// What wakes a blocked [`FileSystemChangeSource::next`], in arrival order.
enum Incoming {
    Event(notify::Result<Event>, Instant),
    Watch(WatchedPaths),
}

impl FileSystemChangeSource {
    /// Start the platform watcher with an empty watched set.
    pub fn new() -> notify::Result<Self> {
        let (sender, events) = channel();
        let platform = sender.clone();
        // The receive time is the event's time: the debouncer's quiet window
        // starts when the platform delivered the change.
        let watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let _ = platform.send(Incoming::Event(event, Instant::now()));
        })?;
        Ok(Self {
            watcher,
            events,
            sender,
            watched: WatchedPaths::default(),
            registered: BTreeMap::new(),
            pending: VecDeque::new(),
            failure: None,
        })
    }

    fn accept(&mut self, event: notify::Result<Event>, at: Instant) {
        let event = match event {
            Ok(event) if !event.need_rescan() => event,
            // The platform dropped events, or failed while delivering them:
            // anything watched may have changed, and its watches may be gone.
            Ok(_) | Err(_) => {
                for path in self.watched.named_paths() {
                    self.pending.push_back((path.to_path_buf(), at));
                }
                let all: Vec<PathBuf> = self.registered.keys().cloned().collect();
                self.drop_registrations(&all);
                self.reregister();
                return;
            }
        };
        let (removes, structural) = match event.kind {
            EventKind::Access(_) => return,
            EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)) => (true, true),
            EventKind::Create(_) => (false, true),
            _ => (false, false),
        };
        let mut dead = Vec::new();
        let mut moves_watches = false;
        for path in &event.paths {
            let Some(path) = path.to_str() else {
                continue;
            };
            let path = PathBuf::from(canonicalize_path(path));
            let mut reported: Vec<PathBuf> = Vec::new();
            if self.watched.reports(&path) {
                reported.push(path.clone());
            }
            if structural {
                // A directory removed, renamed or moved in takes the named
                // files below it with it; its event names only itself.
                reported.extend(self.watched.named_below(&path).map(Path::to_path_buf));
                if path.is_dir() {
                    reported.extend(self.watched.configs_reaching(&path).map(Path::to_path_buf));
                }
                // Only a change at or above an observed directory can move a
                // watch; churn elsewhere (a package install) costs nothing.
                moves_watches |= self.watched.needs_directory_within(&path)
                    || self
                        .registered
                        .range::<Path, _>((Bound::Included(path.as_path()), Bound::Unbounded))
                        .next()
                        .is_some_and(|(directory, _)| directory.starts_with(&path));
            }
            for path in reported {
                self.pending.push_back((path, at));
            }
            if removes {
                // The platform drops the watches of a removed or renamed
                // directory, even when one of the same name replaces it.
                dead.extend(
                    self.registered
                        .keys()
                        .filter(|directory| directory.starts_with(&path))
                        .cloned(),
                );
            }
        }
        if moves_watches {
            self.drop_registrations(&dead);
            self.reregister();
        }
    }

    fn drop_registrations(&mut self, directories: &[PathBuf]) {
        for directory in directories {
            if self.registered.remove(directory).is_some() {
                let _ = self.watcher.unwatch(directory);
            }
        }
    }

    /// Bring the registrations in line with the watched set after the tree
    /// changed; a failure closes the source.
    fn reregister(&mut self) {
        if let Err(error) = self.register() {
            self.failure = Some(error);
        }
    }

    fn register(&mut self) -> Result<(), WatchError> {
        let wanted = registrations(&self.watched);
        let stale: Vec<PathBuf> = self
            .registered
            .iter()
            .filter(|(directory, mode)| {
                wanted.get(*directory).map(|registration| registration.mode) != Some(**mode)
            })
            .map(|(directory, _)| directory.clone())
            .collect();
        self.drop_registrations(&stale);
        for (directory, Registration { mode, required }) in wanted {
            if self.registered.contains_key(&directory) {
                continue;
            }
            if !required {
                if self.watcher.watch(&directory, mode).is_ok() {
                    self.registered.insert(directory, mode);
                }
                continue;
            }
            let mut target = (directory, mode);
            loop {
                let (directory, mode) = target;
                match self.watcher.watch(&directory, mode) {
                    Ok(()) => {
                        self.registered.insert(directory, mode);
                        break;
                    }
                    // Removed since the registrations were computed: its
                    // nearest existing ancestor stands in for it, observed
                    // the same way (see `registrations`).
                    Err(_) if !directory.is_dir() => {
                        let Some(ancestor) = directory.parent().and_then(nearest_existing) else {
                            break;
                        };
                        if self.registered.get(&ancestor).is_some_and(|registered| {
                            *registered == RecursiveMode::Recursive
                                || mode == RecursiveMode::NonRecursive
                        }) {
                            break;
                        }
                        target = (ancestor, mode);
                    }
                    Err(error) => {
                        return Err(WatchError {
                            directory,
                            detail: error.to_string(),
                        })
                    }
                }
            }
        }
        Ok(())
    }
}

impl ChangeSource for FileSystemChangeSource {
    fn watch(&mut self, paths: &WatchedPaths) -> Result<(), WatchError> {
        self.watched = paths.clone();
        self.register()
    }

    fn next(&mut self) -> Option<(PathBuf, Instant)> {
        loop {
            if let Some(change) = self.pending.pop_front() {
                return Some(change);
            }
            if self.failure.is_some() {
                return None;
            }
            match self.events.recv().ok()? {
                Incoming::Event(event, at) => self.accept(event, at),
                Incoming::Watch(paths) => {
                    if let Err(error) = self.watch(&paths) {
                        self.failure = Some(error);
                    }
                }
            }
        }
    }

    fn failure(&self) -> Option<&WatchError> {
        self.failure.as_ref()
    }

    fn rewatcher(&self) -> Rewatcher {
        let sender = self.sender.clone();
        Rewatcher::new(move |paths| {
            let _ = sender.send(Incoming::Watch(paths));
        })
    }
}

/// One directory registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Registration {
    mode: RecursiveMode,
    /// False for an ancestor sentinel: a platform refusing it only loses the
    /// report of that ancestor's own removal, rename or replacement.
    required: bool,
}

/// The directories to register: each needed directory, or its nearest
/// existing ancestor when it does not exist yet, plus every ancestor of it up
/// to the filesystem root, without its subtree. A watch does not see its own
/// directory's ancestors removed, renamed or replaced, and some platforms
/// drop it silently then; the ancestor's parent reports it. Directories a
/// registered subtree already covers are left out.
///
/// A stand-in ancestor is observed the way the missing directory is needed.
/// One needed only for its entries is watched without its subtree, even when
/// the stand-in is a home directory or the filesystem root, as an engine
/// probe there can make it: the first missing directory appearing below the
/// stand-in is a structural event that reports the named paths below it and
/// moves the watch down.
fn registrations(paths: &WatchedPaths) -> BTreeMap<PathBuf, Registration> {
    // (whole subtree, required)
    let mut needed: BTreeMap<PathBuf, (bool, bool)> = BTreeMap::new();
    for (directory, recursive) in paths.directories() {
        let Some(existing) = nearest_existing(directory) else {
            continue;
        };
        for ancestor in existing.ancestors().skip(1) {
            needed
                .entry(ancestor.to_path_buf())
                .or_insert((false, false));
        }
        let entry = needed.entry(existing).or_insert((false, false));
        entry.0 |= recursive;
        entry.1 = true;
    }
    let mut subtrees: Vec<PathBuf> = Vec::new();
    let mut registrations = BTreeMap::new();
    // Path order puts every ancestor before its descendants.
    for (directory, (recursive, required)) in needed {
        if subtrees.iter().any(|root| directory.starts_with(root)) {
            continue;
        }
        let mode = if recursive {
            subtrees.push(directory.clone());
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        registrations.insert(directory, Registration { mode, required });
    }
    registrations
}

fn nearest_existing(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|ancestor| ancestor.is_dir())
        .map(Path::to_path_buf)
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
