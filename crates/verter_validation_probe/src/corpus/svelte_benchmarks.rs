//! The `pikax/svelte-benchmarks` corpus: the DATA the shared [`Corpus`]
//! adapter needs to read it.
//!
//! The corpus is an external WORKLOAD, never an oracle: the adapter reads the
//! generated `.svelte` components and nothing else. The upstream project's own
//! benchmark results, its rankings, and its harness expectations are not read,
//! not mirrored, and never become an expected output for Verter.
//!
//! Unlike the Vue corpus, this repository COMMITS no components: its `fixtures`
//! tree is produced by its own deterministic generator at the pinned revision.
//! The probe workflow runs that generator once, and the manifest inventories
//! every generated case with a content digest, so a generator whose output
//! drifts fails the lane rather than silently changing what it covers.
//!
//! Every path into the checkout is behind `feature = "external-corpus"`, so
//! the canonical hermetic run neither reads nor requires it.

use super::Corpus;
use crate::manifest::Framework;

/// The corpus this framework's cases come from.
pub const CORPUS: Corpus = Corpus {
    framework: Framework::Svelte,
    directory: "svelte-benchmarks",
    // The generator's own output root. Each corpus it writes is a directory
    // under it, so the whole generated set is one subtree walk.
    fixture_root: "fixtures",
    generated: true,
};
