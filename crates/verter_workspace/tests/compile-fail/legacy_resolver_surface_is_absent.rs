//! The prohibited workspace resolver-authority paths must not resolve.
//!
//! Separate imports force a diagnostic at every prohibited path. The public
//! `verter_workspace` crate and its public `resolver` module are intentional
//! prefixes: failures at either crate segment would make this fixture blind.

use verter_workspace::resolver::ProjectResolver as ResolverProjectResolver;
use verter_workspace::ProjectResolver as RootProjectResolver;
use verter_workspace::resolver::NativeProjectResolver;
use verter_workspace::resolver::test_support::legacy_preferred_specifier_candidates;
use verter_workspace::resolver::test_support::legacy_project_exact_result;
use verter_workspace::resolver::test_support::legacy_resolve_for_project_with_reader;
use verter_workspace::resolver::test_support::legacy_resolve_with_reader;

fn main() {}
