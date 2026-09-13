//! The `pikax/vue-benchmarks` corpus: the DATA the shared [`Corpus`] adapter
//! needs to read it.
//!
//! The corpus is an external WORKLOAD, never an oracle: the adapter reads its
//! `.vue` fixtures and nothing else. The upstream project's own expectations,
//! its `known-failures.json`, and its benchmark results are not read, not
//! mirrored, and never become an expected output for Verter.
//!
//! The checkout is pinned by commit in `manifest/vue.toml` and provisioned
//! only by the dedicated probe workflow. Every path into it is behind
//! `feature = "external-corpus"`, so the canonical hermetic run neither reads
//! nor requires it.

use super::Corpus;
use crate::manifest::Framework;

/// The corpus this framework's cases come from.
pub const CORPUS: Corpus = Corpus {
    framework: Framework::Vue,
    // The components are COMMITTED upstream, so the checkout alone is the
    // whole provisioning step.
    directory: "vue-benchmarks",
    fixture_root: "tests/confirm/fixtures",
};
