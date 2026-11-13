//! The count of a root flow read's body reads, by site.
//!
//! A root flow read's body extent is open while its build runs: the checker
//! transaction's frame depth its own body runs at, how many queries it is
//! evaluating in place now, and how many of its body's requests it is
//! answering now. A dispatcher read made at that depth, inside no query
//! evaluated in place, is a read the body made itself, which the read's own
//! frame answers as a seam read.
//!
//! The count includes every such body read unless it is the answer to a
//! request that reached the root read: a read the evaluator requests and
//! the root read answers is not counted. A request answered below the root
//! (by a run that cannot suspend, at once) is counted, under the site
//! `read_in_place`.
//!
//! Each counted read is attributed to the site that made it: the
//! [`super::FlowDemandDriver`] method the evaluator called, or, outside
//! every driver method, the phase of the read's build. A read made inside a
//! contextual call the evaluator starts or resumes is attributed to that
//! call stage as well.
//!
//! Set `VERTER_FLOW_BODY_READS` to report each stage, site and family on
//! standard error the first time the thread counts it (`bt` adds the
//! evaluator frames that reached it), so a test run lists every site that
//! makes a body read.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use crate::project_semantic_dispatch::dispatch_txn::CheckerDispatchTransaction;
use crate::semantic_query::SemanticQueryKeyTag;

/// One counted read: the call stage it was made in, if any; the site that
/// made it; and the family read.
pub type BodyReadSite = (Option<&'static str>, &'static str, SemanticQueryKeyTag);

/// The open root flow read's body (see the module documentation).
#[derive(Clone, Copy)]
struct BodyExtent {
    frame_depth: usize,
    nested: u32,
    answering: u32,
}

thread_local! {
    /// The body of the root flow read whose build runs on this thread.
    static BODY: Cell<Option<BodyExtent>> = const { Cell::new(None) };
    /// The innermost site open on this thread.
    static SITE: Cell<&'static str> = const { Cell::new("<unattributed>") };
    /// The outermost call stage open on this thread.
    static STAGE: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// The reads counted on this thread, by site.
    static HITS: RefCell<BTreeMap<BodyReadSite, u64>> = const { RefCell::new(BTreeMap::new()) };
}

/// A root flow read's body extent, open until it drops; the extent around
/// it restored.
#[must_use = "a body extent is open while it is held"]
pub(in crate::project_semantic_dispatch) struct BodyScope {
    outer: Option<BodyExtent>,
}

/// Open the body extent of the root flow read whose own body runs at the
/// checker transaction's reentry frame open now.
pub(in crate::project_semantic_dispatch) fn open_body(
    txn: &CheckerDispatchTransaction,
) -> BodyScope {
    let frame_depth = txn.reentry().depth();
    BodyScope {
        outer: BODY.with(|body| {
            body.replace(Some(BodyExtent {
                frame_depth,
                nested: 0,
                answering: 0,
            }))
        }),
    }
}

impl Drop for BodyScope {
    fn drop(&mut self) {
        BODY.with(|body| body.set(self.outer));
    }
}

/// A query the open body extent evaluates in place, until it drops: its
/// own reads are not the body's.
#[must_use = "a body extent's inner extent is open while it is held"]
pub(in crate::project_semantic_dispatch) struct InnerScope {
    counted: bool,
}

/// Enter a query the open body extent evaluates in place.
pub(in crate::project_semantic_dispatch) fn enter_nested() -> InnerScope {
    let counted = BODY.with(|body| {
        body.get().is_some_and(|mut extent| {
            extent.nested += 1;
            body.set(Some(extent));
            true
        })
    });
    InnerScope { counted }
}

impl Drop for InnerScope {
    fn drop(&mut self) {
        if self.counted {
            BODY.with(|body| {
                if let Some(mut extent) = body.get() {
                    extent.nested -= 1;
                    body.set(Some(extent));
                }
            });
        }
    }
}

/// The answer to a request of the open body its root read answers, open
/// until it drops: the reads that answer it are not counted.
#[must_use = "a request's answer is open while it is held"]
pub(in crate::project_semantic_dispatch) struct AnswerScope {
    counted: bool,
}

/// Enter the answer to a request the open body made, answered by its root
/// read.
pub(in crate::project_semantic_dispatch) fn enter_request_answer() -> AnswerScope {
    let counted = BODY.with(|body| {
        body.get().is_some_and(|mut extent| {
            extent.answering += 1;
            body.set(Some(extent));
            true
        })
    });
    AnswerScope { counted }
}

impl Drop for AnswerScope {
    fn drop(&mut self) {
        if self.counted {
            BODY.with(|body| {
                if let Some(mut extent) = body.get() {
                    extent.answering -= 1;
                    body.set(Some(extent));
                }
            });
        }
    }
}

/// Count a dispatcher read of `family` made at the checker transaction's
/// frame depth `depth`, when the open body made it itself and it answers no
/// request its root read is answering.
pub(in crate::project_semantic_dispatch) fn count_read(depth: usize, family: SemanticQueryKeyTag) {
    let body_read = BODY.with(|body| {
        body.get().is_some_and(|extent| {
            extent.nested == 0 && extent.answering == 0 && extent.frame_depth == depth
        })
    });
    if body_read {
        record(family);
    }
}

/// A site open on this thread until it drops, the site around it restored.
#[must_use = "a body read site is open while it is held"]
pub struct SiteScope {
    outer: &'static str,
}

/// Open `site` on this thread: reads counted until the scope drops are
/// attributed to it, unless a site inside it opens.
pub fn enter_site(site: &'static str) -> SiteScope {
    SiteScope {
        outer: SITE.with(|current| current.replace(site)),
    }
}

impl Drop for SiteScope {
    fn drop(&mut self) {
        SITE.with(|current| current.set(self.outer));
    }
}

/// A call stage open on this thread until it drops, the stage around it
/// restored.
#[must_use = "a call stage is open while it is held"]
pub(in crate::project_semantic_dispatch) struct StageScope {
    outer: Option<&'static str>,
}

/// Open the call stage `stage`: reads counted until the scope drops are
/// attributed to it, unless a stage around it is open already.
pub(in crate::project_semantic_dispatch) fn enter_call_stage(stage: &'static str) -> StageScope {
    StageScope {
        outer: STAGE.with(|current| {
            let outer = current.get();
            current.set(outer.or(Some(stage)));
            outer
        }),
    }
}

impl Drop for StageScope {
    fn drop(&mut self) {
        STAGE.with(|current| current.set(self.outer));
    }
}

/// How the count reports what it counts, read once per process.
fn report_mode() -> Option<&'static str> {
    static MODE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    MODE.get_or_init(|| std::env::var("VERTER_FLOW_BODY_READS").ok())
        .as_deref()
}

/// Count a read of `family` at the open site.
fn record(family: SemanticQueryKeyTag) {
    let site = SITE.with(Cell::get);
    let stage = STAGE.with(Cell::get);
    let first = HITS.with(|hits| {
        let mut hits = hits.borrow_mut();
        let count = hits.entry((stage, site, family)).or_insert(0);
        *count += 1;
        *count == 1
    });
    if !first {
        return;
    }
    let stage = stage.unwrap_or("-");
    match report_mode() {
        None => {}
        Some("bt") => {
            let trace = std::backtrace::Backtrace::force_capture().to_string();
            let frames: Vec<&str> = trace
                .lines()
                .filter(|line| line.contains("project_semantic_dispatch"))
                .take(24)
                .collect();
            eprintln!(
                "flow-body-read stage={stage} site={site} family={family:?}\n{}",
                frames.join("\n")
            );
        }
        Some(_) => eprintln!("flow-body-read stage={stage} site={site} family={family:?}"),
    }
}

/// The reads counted on this thread since the last take, by site; the count
/// starts again from none.
pub fn take() -> Vec<(BodyReadSite, u64)> {
    HITS.with(|hits| std::mem::take(&mut *hits.borrow_mut()))
        .into_iter()
        .collect()
}
