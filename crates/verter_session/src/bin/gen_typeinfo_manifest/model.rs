//! Data shapes shared across the generator's modules: the lifted-row
//! override record and the manifest row.

/// Lift metadata for a row whose `#[ignore]` has been REMOVED (an oracle
/// snapshot + `ORACLE_QUERY_SPECS` registry entry now back its
/// `oracle::run_row` body), flipping `status` Ignored -> Lifted{block_id}.
/// The row's `block_id` is NOT overridden here — it comes from §10.4.1, the
/// SINGLE source of truth for every row's block. The override carries ONLY
/// the lift metadata that is NOT expressible in §10.4.1: the mechanism /
/// proof / unblocker prose + the execution-true `semantic_queries` /
/// `consumed_mechanisms` (the ACTUAL dispatched tag set the row's resolution
/// issues, decoded from the real per-request dispatch mask by the keystone
/// guard `lifted_row_mechanism_trace_matches_manifest`).
pub(crate) struct LiftedOverride {
    pub(crate) file: &'static str,
    pub(crate) func: &'static str,
    pub(crate) mech: &'static str,
    pub(crate) proof: &'static str,
    pub(crate) semantic_queries: &'static [&'static str],
    pub(crate) consumed_mechanisms: &'static [&'static str],
    pub(crate) unblocker: &'static str,
}

/// One manifest row (both the `IgnoredTestRow` and — with the status /
/// ordinal / unblocker fields unused — the `AdditionalProofRow` shape).
pub(crate) struct Row {
    pub(crate) file: String,
    pub(crate) func: String,
    pub(crate) substrate: &'static str,
    pub(crate) cap: String,
    pub(crate) organ: &'static str,
    pub(crate) ublock: &'static str,
    pub(crate) block: &'static str,
    pub(crate) keys: Vec<&'static str>,
    pub(crate) proof: String,
    pub(crate) mech: &'static str,
    pub(crate) consumed: Vec<&'static str>,
    pub(crate) status: String,
    pub(crate) oracle_query_ordinals: u32,
    pub(crate) unblocker: String,
}
