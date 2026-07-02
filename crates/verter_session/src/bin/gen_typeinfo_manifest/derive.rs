//! Lookup / derivation helpers over the static ledger maps.

use std::collections::BTreeSet;
use std::process::exit;

use crate::data::{
    BLOCK_PREREQS, BLOCK_TO_MECHANISM, BLOCK_TO_REQUIRED_GUARDS, CAPABILITY_TO_MECHANISM,
    MECHANISM_TO_KEYS, PROOF_GUARD, PROOF_ORACLE, ROW_MECHANISM_OVERRIDE, SPLIT_CAPABILITIES,
};

/// Print `msg` to stderr and exit 1 (the generator's hard-error path for
/// self-consistency violations — fix the source maps, do NOT silence).
pub(crate) fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    exit(1)
}

pub(crate) fn lookup_pair(
    table: &'static [(&'static str, &'static str)],
    key: &str,
) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
}

pub(crate) fn lookup_pair_or_fail(
    table: &'static [(&'static str, &'static str)],
    key: &str,
    table_name: &str,
) -> &'static str {
    lookup_pair(table, key)
        .unwrap_or_else(|| fail(&format!("no {table_name} entry for key `{key}`")))
}

/// The row's `semantic_queries`: the FULL set of keys its MECHANISM
/// dispatches/reads (§10.4), emitted verbatim with NO per-block narrowing.
/// The key set is a fixed property of the mechanism; DAG-guard check 4
/// validates it honestly against the row's block prereqs.
pub(crate) fn keys_for_row(mech: &str) -> &'static [&'static str] {
    MECHANISM_TO_KEYS
        .iter()
        .find(|(m, _)| *m == mech)
        .map(|&(_, keys)| keys)
        .unwrap_or_else(|| {
            fail(&format!(
                "no MECHANISM_TO_KEYS entry for mechanism `{mech}`"
            ))
        })
}

pub(crate) fn block_prereqs(block: &str) -> &'static [&'static str] {
    BLOCK_PREREQS
        .iter()
        .find(|(b, _)| *b == block)
        .map(|&(_, prereqs)| prereqs)
        .unwrap_or_else(|| fail(&format!("no BLOCK_PREREQS entry for block `{block}`")))
}

pub(crate) fn block_required_guards(block: &str) -> &'static [&'static str] {
    BLOCK_TO_REQUIRED_GUARDS
        .iter()
        .find(|(b, _)| *b == block)
        .map(|&(_, guards)| guards)
        .unwrap_or_else(|| {
            fail(&format!(
                "no BLOCK_TO_REQUIRED_GUARDS entry for block `{block}`"
            ))
        })
}

/// Inverse of `BLOCK_TO_MECHANISM` — mirrors the Rust manifest test's
/// `mechanism_owning_block` (mechanism and block are in one-to-one ownership
/// correspondence).
pub(crate) fn mechanism_owning_block(mech: &str) -> &'static str {
    BLOCK_TO_MECHANISM
        .iter()
        .find(|(_, m)| *m == mech)
        .map(|&(b, _)| b)
        .unwrap_or_else(|| fail(&format!("no owning block for mechanism `{mech}`")))
}

/// A row's dominant `mechanism_id`, derived from
/// `(capability [, file::function override])` and INDEPENDENT of the
/// `block_id` column. Split capabilities MUST carry a
/// `ROW_MECHANISM_OVERRIDE` entry; single-block capabilities resolve
/// through `CAPABILITY_TO_MECHANISM`.
pub(crate) fn mechanism_for_row(cap: &str, file: &str, fn_name: &str) -> &'static str {
    if SPLIT_CAPABILITIES.contains(&cap) {
        return ROW_MECHANISM_OVERRIDE
            .iter()
            .find(|(f, n, _)| *f == file && *n == fn_name)
            .map(|&(_, _, m)| m)
            .unwrap_or_else(|| {
                fail(&format!(
                    "split-capability row {file}::{fn_name} (capability '{cap}') \
                     has no ROW_MECHANISM_OVERRIDE entry — author its row-level \
                     mechanism from §10.4.1 (do NOT fall back to a block-derived \
                     placeholder)"
                ))
            });
    }
    lookup_pair(CAPABILITY_TO_MECHANISM, cap).unwrap_or_else(|| {
        fail(&format!(
            "capability '{cap}' is neither a split capability nor in \
             CAPABILITY_TO_MECHANISM — add its row-level mechanism"
        ))
    })
}

/// capability -> its `ProofRequirement`. Oracle-pinnable capabilities use
/// `Ts7Oracle`; the mode/demand/expansion/cache/footprint/cross-file
/// capabilities use structural / negative guards (they are NOT TS-oracle
/// rows — §10.2).
pub(crate) fn proof_for_capability(cap: &str) -> String {
    if let Some(guard) = lookup_pair(PROOF_GUARD, cap) {
        return format!("ProofRequirement::StructuralGuard(GuardId::{guard})");
    }
    if let Some(oracle) = lookup_pair(PROOF_ORACLE, cap) {
        return format!("ProofRequirement::Ts7Oracle(OracleId::{oracle})");
    }
    fail(&format!(
        "no ProofRequirement mapping for capability '{cap}'"
    ))
}

/// A row/block's consumed mechanisms = the dominant mechanisms of its
/// block's DIRECT prerequisites (each a transitive prereq, so the DAG
/// guard's check 3 holds).
pub(crate) fn consumed_mechs_for_block(block: &str) -> Vec<&'static str> {
    block_prereqs(block)
        .iter()
        .map(|p| lookup_pair_or_fail(BLOCK_TO_MECHANISM, p, "BLOCK_TO_MECHANISM"))
        .collect()
}

/// Is `target` == `from_block` or a transitive prerequisite of it?
pub(crate) fn reaches(from_block: &str, target: &str) -> bool {
    if from_block == target {
        return true;
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut frontier: Vec<&str> = vec![from_block];
    while let Some(cur) = frontier.pop() {
        if !seen.insert(cur) {
            continue;
        }
        if cur == target {
            return true;
        }
        if let Some(&(_, prereqs)) = BLOCK_PREREQS.iter().find(|(b, _)| *b == cur) {
            frontier.extend_from_slice(prereqs);
        }
    }
    false
}

pub(crate) fn escape_rust_string_literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
