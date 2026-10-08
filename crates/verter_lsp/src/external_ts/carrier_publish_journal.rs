//! The incremental on-disk publication format of the carrier store: a small
//! commit pointer (`head.json`), one compacted base per generation
//! (`snapshot-<generation>.json`) and one append-only record journal per
//! generation (`journal-<generation>.log`).
//!
//! ## Why a journal instead of one whole manifest
//!
//! A publication touches one source. Rewriting (and, on the reader side,
//! re-parsing) a manifest that names every carrier of every project makes the
//! N-th publication cost O(N), so a session of N publications costs O(N²) on both
//! sides of the process boundary. Here a publication appends ONE framed record
//! naming only the rows it changes; the writer keeps the folded state in memory,
//! and a reader tails the journal from the byte offset it already consumed. Both
//! sides therefore process work proportional to the records written since they
//! last looked. When the journal holds at least as many records as the store has
//! live rows (with a fixed floor) the writer folds it into the next generation's
//! base, so a cold reader's load stays proportional to the live rows plus a
//! bounded tail, and compaction costs O(1) amortized per record.
//!
//! ## Commit boundary
//!
//! A record is committed exactly when its full line — `<fnv1a32 hex> <json>\n` —
//! is on disk. A reader applies only complete lines whose checksum and JSON both
//! verify, in order, and never advances past the first line that does not; so a
//! crash mid-append (a torn tail) is invisible to every reader, and the next
//! writer truncates it before appending. A complete invalid line FOLLOWED by more
//! bytes is corruption, not a torn tail, and fails closed. A complete invalid line
//! that is the LAST bytes of the journal is treated as a torn tail (a crash on a
//! filesystem without ordered appends can leave a newline-terminated partial
//! record), so the next writer truncates it and that record's row operations —
//! including any retraction — are lost, not deferred; failing closed there would
//! wedge the store permanently on one bad record. Compaction writes the
//! next base and an empty next journal BEFORE the atomic `head.json` swap, so a
//! crash at any compaction step leaves the previous generation authoritative and
//! complete. The previous generation's files are retained across one compaction so
//! a reader that read the old head can finish loading it.
//!
//! ## Records name resolved rows, never publish intent
//!
//! The writer resolves each publication's reconciliation (owned-set union, prune,
//! retraction) against its folded state and journals only the resulting row
//! operations ([`JournalOp`]). A reader is therefore a plain applier: the
//! publication semantics live in exactly one place (the writer), and the Node
//! reader mirrors only this primitive vocabulary.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Manifest, OwnedSource, ProjectEntry, ReadyFile};

/// The on-disk format version `head.json` carries. A reader that finds any other
/// value fails closed (it never guesses at a layout it does not know).
pub(crate) const STORE_FORMAT: u32 = 2;

/// The commit pointer naming the authoritative generation.
pub(crate) const HEAD_FILE: &str = "head.json";

/// The cross-process writer lock file (an advisory exclusive lock is held across
/// every commit, so two LSP processes over one workspace serialize their appends).
pub(crate) const WRITER_LOCK_FILE: &str = "writer.lock";

/// A journal is folded into the next generation's base once it holds at least
/// `max(COMPACTION_FLOOR, live rows)` records.
pub(crate) const COMPACTION_FLOOR: u64 = 64;

/// `snapshot-<generation>.json`.
pub(crate) fn snapshot_file(generation: u64) -> String {
    format!("snapshot-{generation}.json")
}

/// `journal-<generation>.log`.
pub(crate) fn journal_file(generation: u64) -> String {
    format!("journal-{generation}.log")
}

/// The generation a `snapshot-<g>.json` / `journal-<g>.log` file name belongs to.
pub(crate) fn parse_generation_file(name: &str) -> Option<u64> {
    let stem = name
        .strip_prefix("snapshot-")
        .and_then(|rest| rest.strip_suffix(".json"))
        .or_else(|| {
            name.strip_prefix("journal-")
                .and_then(|rest| rest.strip_suffix(".log"))
        })?;
    stem.parse().ok()
}

/// The commit pointer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoreHead {
    pub format: u32,
    pub generation: u64,
    /// Minted when the store is created: a store that vanished and was re-created
    /// restarts at generation 1, and this tells a follower its offsets are void.
    pub instance: String,
    pub host_version: String,
}

/// One primitive row operation. A record's ops apply in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum JournalOp {
    /// Ensure the project entry exists (an empty project is still a project).
    ProjectPut { project: String },
    /// Drop every owned row of the project (the authoritative owned-set rewrite).
    OwnedClear { project: String },
    /// Replace the owned rows of one source; the source moves to the end of the
    /// project's owned order.
    OwnedPut {
        project: String,
        source_uri: String,
        rows: Vec<OwnedSource>,
    },
    /// Drop the owned rows of one source.
    OwnedDel { project: String, source_uri: String },
    /// Insert or replace one ready entry.
    ReadyPut {
        project: String,
        provider_uri: String,
        file: ReadyFile,
    },
    /// Drop one ready entry.
    ReadyDel {
        project: String,
        provider_uri: String,
    },
}

/// One committed publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JournalRecord {
    pub epoch: u64,
    pub ops: Vec<JournalOp>,
}

/// FNV-1a 32 over the record JSON bytes — the per-line torn-write check the Node
/// reader recomputes byte-for-byte.
///
/// The constants are the standard FNV-1a 32-bit parameters — offset basis
/// `0x811c9dc5`, prime `0x01000193` — and the order is the FNV-1a order (XOR the
/// byte, THEN multiply), per the FNV reference
/// (<http://www.isthe.com/chongo/tech/comp/fnv/>, draft-eastlake-fnv). The
/// published test vectors are pinned in `carrier_publish_journal_tests.rs`.
pub(crate) fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &b in bytes {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Frame one record as a journal line.
pub(crate) fn frame_record(record: &JournalRecord) -> std::io::Result<Vec<u8>> {
    let json = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    let mut line = Vec::with_capacity(json.len() + 10);
    line.extend_from_slice(format!("{:08x} ", fnv1a32(&json)).as_bytes());
    line.extend_from_slice(&json);
    line.push(b'\n');
    Ok(line)
}

/// Decode one complete line (without its `\n`), or `None` when it does not verify.
pub(crate) fn decode_line(line: &[u8]) -> Option<JournalRecord> {
    if line.len() < 9 || line[8] != b' ' {
        return None;
    }
    let declared = u32::from_str_radix(std::str::from_utf8(&line[..8]).ok()?, 16).ok()?;
    let json = &line[9..];
    if fnv1a32(json) != declared {
        return None;
    }
    serde_json::from_slice(json).ok()
}

fn invalid_data(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

// ── folded state ─────────────────────────────────────────────────────────────

/// One project's folded rows, indexed so every primitive op is O(log n).
#[derive(Debug, Clone, Default)]
pub(crate) struct ProjectState {
    /// Owned rows grouped per source, in owned order (order key → source + rows).
    owned: BTreeMap<u64, (String, Vec<OwnedSource>)>,
    /// Source → its owned-order key.
    owned_order: HashMap<String, u64>,
    /// Provider URI → number of owned rows naming it.
    provider_refs: HashMap<String, usize>,
    owned_rows: usize,
    pub(crate) ready: BTreeMap<String, ReadyFile>,
}

impl ProjectState {
    /// The owned rows of `source_uri`, if the project owns it.
    pub(crate) fn owned_rows_of(&self, source_uri: &str) -> Option<&[OwnedSource]> {
        let order = self.owned_order.get(source_uri)?;
        self.owned.get(order).map(|(_, rows)| rows.as_slice())
    }

    /// How many owned rows name `provider_uri`.
    pub(crate) fn owned_ref_count(&self, provider_uri: &str) -> usize {
        self.provider_refs.get(provider_uri).copied().unwrap_or(0)
    }

    /// Every provider URI an owned row names.
    pub(crate) fn owned_providers(&self) -> impl Iterator<Item = &str> {
        self.provider_refs.keys().map(String::as_str)
    }

    fn remove_source(&mut self, source_uri: &str) {
        let Some(order) = self.owned_order.remove(source_uri) else {
            return;
        };
        if let Some((_, rows)) = self.owned.remove(&order) {
            self.owned_rows -= rows.len();
            for row in &rows {
                if let Some(count) = self.provider_refs.get_mut(&row.provider_uri) {
                    *count -= 1;
                    if *count == 0 {
                        self.provider_refs.remove(&row.provider_uri);
                    }
                }
            }
        }
    }

    fn put_source(&mut self, order: u64, source_uri: String, rows: Vec<OwnedSource>) {
        self.remove_source(&source_uri);
        for row in &rows {
            *self
                .provider_refs
                .entry(row.provider_uri.clone())
                .or_default() += 1;
        }
        self.owned_rows += rows.len();
        self.owned_order.insert(source_uri.clone(), order);
        self.owned.insert(order, (source_uri, rows));
    }

    fn clear_owned(&mut self) {
        self.owned.clear();
        self.owned_order.clear();
        self.provider_refs.clear();
        self.owned_rows = 0;
    }

    fn live_rows(&self) -> usize {
        self.owned_rows + self.ready.len()
    }

    fn to_entry(&self) -> ProjectEntry {
        ProjectEntry {
            owned_sources: self
                .owned
                .values()
                .flat_map(|(_, rows)| rows.iter().cloned())
                .collect(),
            ready_files: self.ready.clone(),
        }
    }
}

/// The folded publication state: a base snapshot plus every applied record.
#[derive(Debug, Clone)]
pub(crate) struct StoreState {
    pub(crate) epoch: u64,
    pub(crate) host_version: String,
    pub(crate) projects: BTreeMap<String, ProjectState>,
    next_order: u64,
}

impl StoreState {
    pub(crate) fn empty(epoch: u64, host_version: String) -> Self {
        Self {
            epoch,
            host_version,
            projects: BTreeMap::new(),
            next_order: 0,
        }
    }

    /// Fold a base snapshot. Owned rows group per source in first-appearance order.
    pub(crate) fn from_manifest(manifest: Manifest) -> Self {
        let mut state = Self::empty(manifest.epoch, manifest.host_version);
        for (project_uri, entry) in manifest.projects {
            let mut grouped: Vec<(String, Vec<OwnedSource>)> = Vec::new();
            let mut index: HashMap<String, usize> = HashMap::new();
            for row in entry.owned_sources {
                match index.get(&row.source_uri) {
                    Some(&at) => grouped[at].1.push(row),
                    None => {
                        index.insert(row.source_uri.clone(), grouped.len());
                        grouped.push((row.source_uri.clone(), vec![row]));
                    }
                }
            }
            let project = state.projects.entry(project_uri).or_default();
            for (source_uri, rows) in grouped {
                let order = state.next_order;
                state.next_order += 1;
                project.put_source(order, source_uri, rows);
            }
            project.ready = entry.ready_files;
        }
        state
    }

    /// Materialize the folded state as a [`Manifest`] (a base snapshot, or a
    /// diagnostics view). O(live rows).
    pub(crate) fn to_manifest(&self) -> Manifest {
        Manifest {
            epoch: self.epoch,
            host_version: self.host_version.clone(),
            projects: self
                .projects
                .iter()
                .map(|(uri, project)| (uri.clone(), project.to_entry()))
                .collect(),
        }
    }

    /// Owned rows plus ready entries across every project — the compaction
    /// threshold's measure of what a base snapshot costs to write and load.
    pub(crate) fn live_rows(&self) -> u64 {
        self.projects
            .values()
            .map(ProjectState::live_rows)
            .sum::<usize>() as u64
    }

    /// Apply one primitive op.
    pub(crate) fn apply(&mut self, op: JournalOp) {
        match op {
            JournalOp::ProjectPut { project } => {
                self.projects.entry(project).or_default();
            }
            JournalOp::OwnedClear { project } => {
                self.projects.entry(project).or_default().clear_owned();
            }
            JournalOp::OwnedPut {
                project,
                source_uri,
                rows,
            } => {
                let order = self.next_order;
                self.next_order += 1;
                self.projects
                    .entry(project)
                    .or_default()
                    .put_source(order, source_uri, rows);
            }
            JournalOp::OwnedDel {
                project,
                source_uri,
            } => {
                if let Some(state) = self.projects.get_mut(&project) {
                    state.remove_source(&source_uri);
                }
            }
            JournalOp::ReadyPut {
                project,
                provider_uri,
                file,
            } => {
                self.projects
                    .entry(project)
                    .or_default()
                    .ready
                    .insert(provider_uri, file);
            }
            JournalOp::ReadyDel {
                project,
                provider_uri,
            } => {
                if let Some(state) = self.projects.get_mut(&project) {
                    state.ready.remove(&provider_uri);
                }
            }
        }
    }

    /// Apply one committed record (its ops in order, then its epoch).
    pub(crate) fn apply_record(&mut self, record: JournalRecord) {
        for op in record.ops {
            self.apply(op);
        }
        self.epoch = record.epoch;
    }
}

// ── deterministic work accounting (optional observation) ─────────────────────

/// Work a cursor performed: the deterministic counters the publication-cost
/// acceptance reads. OPTIONAL measurement state — compiled only into test and
/// `semantic-observe` builds; no production decision reads it.
#[cfg(any(test, feature = "semantic-observe"))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreWork {
    /// Base snapshots loaded.
    pub snapshot_loads: u64,
    /// Rows (owned + ready) folded from loaded base snapshots.
    pub snapshot_rows_loaded: u64,
    /// Journal records decoded and applied.
    pub records_applied: u64,
    /// Journal bytes read.
    pub journal_bytes_read: u64,
    /// Records appended (writer only).
    pub records_appended: u64,
    /// Compactions completed (writer only).
    pub compactions: u64,
    /// Rows written into compacted base snapshots (writer only).
    pub compaction_rows_written: u64,
}

/// Record work on a [`StoreWork`] — a no-op that compiles away outside test and
/// `semantic-observe` builds, so no production path checks an enable flag.
macro_rules! observe_work {
    ($work:expr, |$w:ident| $body:expr) => {{
        #[cfg(any(test, feature = "semantic-observe"))]
        {
            let $w = &mut $work;
            $body;
        }
    }};
}
pub(crate) use observe_work;

// ── cursor: load + tail ──────────────────────────────────────────────────────

/// A folded view of one generation at a journal byte offset. The writer and the
/// Rust reader share it; the Node reader mirrors it.
#[derive(Debug, Clone)]
pub(crate) struct StoreCursor {
    pub(crate) generation: u64,
    pub(crate) instance: String,
    /// Bytes of `journal-<generation>.log` already applied (always a line end).
    pub(crate) journal_offset: u64,
    /// Records applied from the current journal (the compaction measure).
    pub(crate) journal_records: u64,
    pub(crate) state: StoreState,
}

/// What a tail found beyond the last complete valid record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TailEnd {
    /// The journal ends exactly at a record boundary.
    Clean,
    /// Trailing bytes that are not (yet) a complete valid record: an append in
    /// flight, or one a crash interrupted. Never applied.
    Torn,
}

/// Read and parse `head.json`. `Ok(None)` only when it does not exist.
pub(crate) fn read_head(dir: &Path) -> std::io::Result<Option<StoreHead>> {
    let bytes = match std::fs::read(dir.join(HEAD_FILE)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let head: StoreHead = serde_json::from_slice(&bytes).map_err(|e| {
        invalid_data(format!(
            "carrier store head is present but unparseable: {e}"
        ))
    })?;
    if head.format != STORE_FORMAT {
        return Err(invalid_data(format!(
            "carrier store head names format {} (this build reads format {STORE_FORMAT})",
            head.format
        )));
    }
    Ok(Some(head))
}

impl StoreCursor {
    /// Load generation `head.generation` from scratch: its base, then its journal.
    pub(crate) fn load(
        dir: &Path,
        head: &StoreHead,
        #[allow(unused_variables)] work: &mut ObservedWork,
    ) -> std::io::Result<(Self, TailEnd)> {
        let snapshot_path = dir.join(snapshot_file(head.generation));
        let bytes = std::fs::read(&snapshot_path)?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| {
            invalid_data(format!(
                "carrier store base {} is present but unparseable: {e}",
                snapshot_path.display()
            ))
        })?;
        let state = StoreState::from_manifest(manifest);
        observe_work!(*work, |w| {
            w.snapshot_loads += 1;
            w.snapshot_rows_loaded += state.live_rows();
        });
        let mut cursor = Self {
            generation: head.generation,
            instance: head.instance.clone(),
            journal_offset: 0,
            journal_records: 0,
            state,
        };
        let end = cursor.tail(dir, work)?;
        Ok((cursor, end))
    }

    /// Apply every complete valid record appended past `journal_offset`.
    ///
    /// Reads only the bytes past the offset. Fails closed (`InvalidData`) on a
    /// complete invalid line that more bytes follow, on a non-advancing epoch, and
    /// on a journal shorter than the consumed offset (the caller reloads).
    pub(crate) fn tail(
        &mut self,
        dir: &Path,
        #[allow(unused_variables)] work: &mut ObservedWork,
    ) -> std::io::Result<TailEnd> {
        let path = self.journal_path(dir);
        let mut file = std::fs::File::open(&path)?;
        let len = file.metadata()?.len();
        if len < self.journal_offset {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "carrier store journal {} shrank below its consumed offset",
                    path.display()
                ),
            ));
        }
        if len == self.journal_offset {
            return Ok(TailEnd::Clean);
        }
        file.seek(SeekFrom::Start(self.journal_offset))?;
        let mut bytes = Vec::with_capacity((len - self.journal_offset) as usize);
        file.read_to_end(&mut bytes)?;
        observe_work!(*work, |w| w.journal_bytes_read += bytes.len() as u64);

        let mut at = 0usize;
        while at < bytes.len() {
            let Some(newline) = bytes[at..].iter().position(|&b| b == b'\n') else {
                return Ok(TailEnd::Torn);
            };
            let line = &bytes[at..at + newline];
            let next = at + newline + 1;
            match decode_line(line) {
                Some(record) if record.epoch > self.state.epoch => {
                    self.state.apply_record(record);
                    self.journal_offset += next as u64 - at as u64;
                    self.journal_records += 1;
                    observe_work!(*work, |w| w.records_applied += 1);
                }
                Some(record) => {
                    return Err(invalid_data(format!(
                        "carrier store journal {} record epoch {} does not advance past {}",
                        path.display(),
                        record.epoch,
                        self.state.epoch
                    )));
                }
                None if next == bytes.len() => return Ok(TailEnd::Torn),
                None => {
                    return Err(invalid_data(format!(
                        "carrier store journal {} holds a corrupt record at byte {}",
                        path.display(),
                        self.journal_offset
                    )));
                }
            }
            at = next;
        }
        Ok(TailEnd::Clean)
    }

    /// Whether this cursor folds the generation `head` names.
    pub(crate) fn follows(&self, head: &StoreHead) -> bool {
        self.generation == head.generation && self.instance == head.instance
    }

    pub(crate) fn journal_path(&self, dir: &Path) -> PathBuf {
        dir.join(journal_file(self.generation))
    }
}

/// The work sink a cursor records into: a real [`StoreWork`] in test and
/// `semantic-observe` builds, a zero-sized `NoWork` otherwise.
#[cfg(any(test, feature = "semantic-observe"))]
pub(crate) type ObservedWork = StoreWork;
#[cfg(not(any(test, feature = "semantic-observe")))]
pub(crate) type ObservedWork = NoWork;

/// The zero-sized work sink of builds without `semantic-observe`.
#[cfg(not(any(test, feature = "semantic-observe")))]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoWork;

/// Load the authoritative generation from scratch, retrying when a concurrent
/// compaction retires the generation between the head read and the base/journal
/// open. `Ok(None)` only when no head exists. Never writes; a torn tail is left
/// unapplied.
pub(crate) fn load_published(
    dir: &Path,
    work: &mut ObservedWork,
) -> std::io::Result<Option<StoreCursor>> {
    let mut attempts = 0;
    loop {
        let Some(head) = read_head(dir)? else {
            return Ok(None);
        };
        match StoreCursor::load(dir, &head, work) {
            Ok((cursor, _)) => return Ok(Some(cursor)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && attempts < 3 => {
                attempts += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(invalid_data(format!(
                    "carrier store head names generation {} whose files are missing: {e}",
                    head.generation
                )));
            }
            Err(e) => return Err(e),
        }
    }
}
