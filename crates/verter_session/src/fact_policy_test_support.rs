//! Hosts composed with a narrow fact policy, so a gate test reaches a
//! wide-set branch — tree storage, a domain's aggregate, the strict-root
//! rewrite — with a small fixture. The policy changes only where each
//! decision falls; every algorithm, charge and validation is the one a
//! production host runs.

use std::sync::Arc;

use verter_session_query::facts::policy::{FactPolicy, FactWidths};

use crate::{HostConfig, VerterHost};

/// A policy with these widths.
pub(crate) fn policy_with(
    fresh_slice_max: u16,
    strict_root_threshold: u16,
    precise_domain_max: u16,
    extension_slice_max: u16,
) -> FactPolicy {
    FactPolicy::with_widths(FactWidths {
        fresh_slice_max,
        strict_root_threshold,
        precise_domain_max,
        extension_slice_max,
    })
    .expect("valid widths")
}

/// A policy whose only narrow decision is storage: a freshly sealed set
/// past `fresh_slice_max` facts is a tree, an extended one past 8 facts;
/// strict-root and precise-domain decisions keep production widths.
pub(crate) fn narrow_storage(fresh_slice_max: u16) -> FactPolicy {
    let production = FactWidths::PRODUCTION;
    policy_with(
        fresh_slice_max,
        production.strict_root_threshold,
        production.precise_domain_max,
        fresh_slice_max.min(8),
    )
}

/// A memory workspace sealing its resolution witnesses under `policy`.
pub(crate) fn workspace_under(policy: FactPolicy) -> Arc<verter_workspace::MemoryWorkspace> {
    Arc::new(verter_workspace::MemoryWorkspace::new_with_fact_policy(
        verter_workspace::MemoryOptions::default(),
        verter_session_query::resolution::InputResolutionBudgets::default(),
        policy,
    ))
}

/// [`VerterHost::new_standalone`] composed with `policy`.
pub(crate) fn standalone_host_under(config: HostConfig, policy: FactPolicy) -> VerterHost {
    VerterHost::new(config, workspace_under(policy))
}
