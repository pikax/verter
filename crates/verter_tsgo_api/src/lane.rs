//! The two scheduling lanes over the one client/process.
//!
//! The tsgo `--api` wire is single-flight (one request out, read until the
//! matching response returns, callbacks serviced inline — syncChannel.js). The
//! lanes are therefore scheduling CLASSES that feed the single-writer actor's
//! request queue, not two concurrent in-flight channels:
//!
//! - [`Lane::Interactive`]: higher priority + cancellable. Interactive requests
//!   jump ahead of queued batch requests; an interactive request can be
//!   cancelled while still queued (and its handle future resolves to
//!   `Cancelled` immediately even if the request is already in flight — the
//!   single-flight wire cannot abort a request mid-read, so true preemption is
//!   a process restart; see [`crate::actor`]).
//! - [`Lane::Batch`]: lower priority + preemptible. Batch requests run on the
//!   same warm session and yield to interactive work between requests.
//!
//! This is two scheduling classes over ONE client, ONE process, ONE codec — not
//! two transport implementations.

/// A scheduling lane for a request.
///
/// [`Lane::Interactive`] is the default: an unspecified request is treated as
/// interactive (the safe, responsive choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Lane {
    /// Higher-priority, cancellable interactive work (hover, diagnostics on the
    /// active file). Drained before [`Lane::Batch`].
    #[default]
    Interactive,
    /// Lower-priority, preemptible batch work (project-wide passes). Yields to
    /// interactive work between requests.
    Batch,
}

impl Lane {
    /// A numeric priority where a LOWER value is drained first. Interactive
    /// outranks batch.
    pub fn priority(self) -> u8 {
        match self {
            Lane::Interactive => 0,
            Lane::Batch => 1,
        }
    }

    /// Whether this lane is preemptible — i.e. yields its turn to a
    /// higher-priority lane when both have queued work. Batch is preemptible;
    /// interactive is not (it is already the top lane).
    pub fn is_preemptible(self) -> bool {
        matches!(self, Lane::Batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_outranks_batch() {
        assert!(Lane::Interactive.priority() < Lane::Batch.priority());
    }

    #[test]
    fn ordering_puts_interactive_first() {
        let mut lanes = [Lane::Batch, Lane::Interactive, Lane::Batch];
        lanes.sort();
        assert_eq!(lanes[0], Lane::Interactive, "interactive sorts first");
    }

    #[test]
    fn only_batch_is_preemptible() {
        assert!(Lane::Batch.is_preemptible());
        assert!(!Lane::Interactive.is_preemptible());
    }
}
