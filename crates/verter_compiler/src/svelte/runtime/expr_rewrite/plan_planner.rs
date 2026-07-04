//! Pass 2 of the fallible Svelte client expression rewriter
//! ([`crate::svelte::runtime::expr_rewrite`]): the [`RewritePlanner`]. It consumes
//! the typed [`Occurrence`](super::plan::Occurrence)s pass 1
//! ([`BindingOccurrenceCollector`](super::plan::BindingOccurrenceCollector))
//! recorded and turns them into the typed [`Edit`](super::plan::Edit)s the caller
//! applies to its `CodeTransform`.

use super::super::unsupported::UnsupportedSvelteRuntimeSurface;
use super::plan::{Edit, Occurrence};

/// Pass 2: the rewrite PLANNER. It consumes the typed occurrences pass 1 recorded
/// and emits the CodeTransform edits, OR records a refusal. A `MustRewrite`
/// occurrence that the planner cannot turn into an edit sets `unresolved` — the
/// post-pass invariant (no resolved signal/prop occurrence left un-rewritten).
pub(super) struct RewritePlanner {
    /// The emitted edits (disjoint, applied in record order).
    edits: Vec<Edit>,
    /// The first refusal, if any.
    refusal: Option<UnsupportedSvelteRuntimeSurface>,
    /// Set when an occurrence that MUST rewrite was left without an edit (the
    /// post-pass safeguard).
    unresolved: bool,
}

impl RewritePlanner {
    /// A fresh planner: no edits recorded, no refusal, nothing left unresolved.
    pub(super) fn new() -> Self {
        Self {
            edits: Vec::new(),
            refusal: None,
            unresolved: false,
        }
    }

    /// Turn each occurrence into its edits. Every occurrence variant carries a
    /// concrete rewrite decision, so the planner always emits the edits (the
    /// `unresolved` flag stays false on the supported path) — it exists as the
    /// structural seam the post-pass invariant asserts against.
    pub(super) fn plan(&mut self, occurrences: &[Occurrence]) {
        for occ in occurrences {
            match occ {
                Occurrence::ReadRewrite { span, text } => {
                    self.edits.push(Edit::Overwrite {
                        start: span.start,
                        end: span.end,
                        text: text.clone(),
                    });
                }
                Occurrence::SignalReassign {
                    head_span,
                    head_text,
                    append_at,
                    append_text,
                } => {
                    self.edits.push(Edit::Overwrite {
                        start: head_span.start,
                        end: head_span.end,
                        text: head_text.clone(),
                    });
                    self.edits.push(Edit::Append {
                        at: *append_at,
                        text: append_text.clone(),
                    });
                }
                Occurrence::SignalUpdate { span, text } => {
                    self.edits.push(Edit::Overwrite {
                        start: span.start,
                        end: span.end,
                        text: text.clone(),
                    });
                }
                Occurrence::WrapCall {
                    insert_at,
                    head_text,
                    append_at,
                    append_text,
                } => {
                    self.edits.push(Edit::Insert {
                        at: *insert_at,
                        text: head_text.clone(),
                    });
                    self.edits.push(Edit::Append {
                        at: *append_at,
                        text: append_text.clone(),
                    });
                }
                Occurrence::DropStatement { span }
                | Occurrence::RelocatedWrapperComment { span } => {
                    self.edits.push(Edit::Remove {
                        start: span.start,
                        end: span.end,
                    });
                }
            }
        }
    }

    /// Take the first refusal the planner recorded, if any (leaving `None`
    /// behind) — the caller returns the typed surface on `Some`.
    pub(super) fn take_refusal(&mut self) -> Option<UnsupportedSvelteRuntimeSurface> {
        self.refusal.take()
    }

    /// Whether an occurrence that MUST rewrite was left without an edit — the
    /// post-pass safeguard the caller asserts against.
    pub(super) fn unresolved(&self) -> bool {
        self.unresolved
    }

    /// Consume the planner and hand back the planned edits (in record order).
    pub(super) fn into_edits(self) -> Vec<Edit> {
        self.edits
    }
}
