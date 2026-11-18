//! Differences between the two routes that are known and accepted.
//!
//! One file per target, so the tasks that fill the targets never edit the
//! same file. A row names a corpus case and why its output differs, or why
//! the target route cannot render it yet. A row whose case stops differing,
//! or starts rendering, fails the run, so the tables cannot rot.

mod client_tree;
mod fine_grained;
mod server;

use super::{Mode, Target};

/// Why a case's output differs between the routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExpectedKind {
    /// The in-place route panics on this case.
    LegacyPanicked,
    /// The in-place route refuses this case.
    LegacyRefused,
    /// The in-place output is wrong; the target output is the corrected one.
    /// The reason names the Vue golden the target output follows.
    Corrected { reason: &'static str },
    /// The target route does not render this case yet; the reason names
    /// what it lacks.
    Pending { reason: &'static str },
}

/// One accepted difference.
#[derive(Debug, Clone, Copy)]
pub(super) struct ExpectedDifference {
    /// The corpus case name.
    pub case: &'static str,
    pub kind: ExpectedKind,
}

/// The accepted differences of one target.
pub(super) fn rows(target: Target, mode: Mode) -> &'static [ExpectedDifference] {
    match target {
        Target::ClientTree if mode == Mode::Inline => client_tree::INLINE_ROWS,
        Target::ClientTree => client_tree::ROWS,
        Target::Server => server::ROWS,
        Target::FineGrained => fine_grained::ROWS,
    }
}
