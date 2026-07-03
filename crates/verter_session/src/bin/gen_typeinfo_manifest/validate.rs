//! Structural self-validation over the generator's static data tables.
//!
//! Every association table in `data/` is consulted through linear
//! `.iter().find(...)` lookups, so a duplicate key (or, for
//! `BLOCK_TO_MECHANISM`, a duplicate VALUE — its value side is
//! reverse-looked-up by `mechanism_owning_block`) is silently shadowed
//! rather than rejected. Likewise a duplicate §10.4.1 `(file, func)` row
//! silently overwrites its earlier occurrence, and an override entry with
//! no consuming row goes silently stale. The pure validators here RETURN
//! the offending entries; `run()` aggregates them through
//! [`validate_data_tables`] / [`validate_partition`] /
//! [`validate_overrides`] and exits with status 7 (a distinct code — NOT
//! the `fail()` status-1 self-consistency path) on any finding, in BOTH
//! generate and `--check` mode.

use std::collections::{BTreeMap, BTreeSet};

use crate::data::{
    AdditionalProofSpec, ADDITIONAL_PROOF_SPECS, BLOCK_PREREQS, BLOCK_TEXT_TO_VARIANT,
    BLOCK_TO_MECHANISM, BLOCK_TO_ORGAN, BLOCK_TO_REQUIRED_GUARDS, BLOCK_TO_UBLOCK,
    BLOCK_VERIFICATION_LABELS, CAPABILITY_TO_MECHANISM, FILE_TO_SUBSTRATE, KEY_OWNING_BLOCK,
    LIFTED_ROW_OVERRIDES, MECHANISM_TO_KEYS, PROOF_GUARD, PROOF_ORACLE, ROW_MECHANISM_OVERRIDE,
    SPLIT_CAPABILITIES,
};
use crate::model::LiftedOverride;

/// A list-valued association table (`BLOCK_PREREQS`,
/// `BLOCK_TO_REQUIRED_GUARDS`, `MECHANISM_TO_KEYS`).
type ListTable = &'static [(&'static str, &'static [&'static str])];

/// Items appearing more than once in `items`. Each offender is reported
/// once, in first-duplicate-occurrence order.
fn duplicate_strs<'a>(items: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut reported: BTreeSet<&str> = BTreeSet::new();
    let mut out = Vec::new();
    for item in items {
        if !seen.insert(item) && reported.insert(item) {
            out.push(item);
        }
    }
    out
}

/// Keys appearing more than once as the FIRST element of an association
/// table (`&[(&str, V)]` — both the `(&str, &str)` pair maps and the
/// `(&str, &[&str])` list maps). Each offender is reported once, in
/// first-duplicate-occurrence order.
pub(crate) fn duplicate_pair_keys<'a, V>(table: &'a [(&'a str, V)]) -> Vec<&'a str> {
    duplicate_strs(table.iter().map(|(k, _)| *k))
}

/// Values appearing more than once as the SECOND element of a pair table.
/// `BLOCK_TO_MECHANISM` is reverse-looked-up by value
/// (`mechanism_owning_block`), so value uniqueness is load-bearing there.
pub(crate) fn duplicate_pair_values<'a>(table: &'a [(&'a str, &'a str)]) -> Vec<&'a str> {
    duplicate_strs(table.iter().map(|(_, v)| *v))
}

/// `(file, func)` keys appearing more than once in a triple table keyed by
/// `(.0, .1)` (`ROW_MECHANISM_OVERRIDE`).
pub(crate) fn duplicate_triple_keys<'a>(
    table: &'a [(&'a str, &'a str, &'a str)],
) -> Vec<(&'a str, &'a str)> {
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut reported: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut out = Vec::new();
    for &(file, func, _) in table {
        if !seen.insert((file, func)) && reported.insert((file, func)) {
            out.push((file, func));
        }
    }
    out
}

/// Elements appearing more than once in a single-element list
/// (`SPLIT_CAPABILITIES`, `BLOCK_VERIFICATION_LABELS`) — also the per-list
/// primitive behind [`duplicate_list_values`] and
/// [`duplicate_override_list_fields`].
pub(crate) fn duplicate_elements<'a>(list: &'a [&'a str]) -> Vec<&'a str> {
    duplicate_strs(list.iter().copied())
}

/// Items present in BOTH `first` and `second`. Each shared item is
/// reported once, in `first` order (mirroring the dedup pattern of
/// `duplicate_strs`). A shared key across two tables consulted in
/// first-wins order means the second table's entry is silently-shadowed
/// dead data.
pub(crate) fn shared_strs<'a>(
    first: impl Iterator<Item = &'a str>,
    second: impl Iterator<Item = &'a str>,
) -> Vec<&'a str> {
    let second_set: BTreeSet<&str> = second.collect();
    let mut reported: BTreeSet<&str> = BTreeSet::new();
    let mut out = Vec::new();
    for item in first {
        if second_set.contains(item) && reported.insert(item) {
            out.push(item);
        }
    }
    out
}

/// `(owning key, element)` pairs for elements appearing more than once
/// WITHIN a single value list of a list-valued table. The value lists are
/// emitted verbatim into the manifest, so an internal duplicate is a
/// silent data defect.
pub(crate) fn duplicate_list_values<'a>(
    table: &'a [(&'a str, &'a [&'a str])],
) -> Vec<(&'a str, &'a str)> {
    let mut out = Vec::new();
    for &(key, list) in table {
        for elem in duplicate_elements(list) {
            out.push((key, elem));
        }
    }
    out
}

/// `(file, func, field, element)` for elements appearing more than once in
/// a `LiftedOverride`'s `semantic_queries` / `consumed_mechanisms` lists
/// (both lists are emitted per-row into the manifest, so an internal
/// duplicate is a silent data defect).
pub(crate) fn duplicate_override_list_fields(
    overrides: &[LiftedOverride],
) -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    let mut out = Vec::new();
    for o in overrides {
        for (field, list) in [
            ("semantic_queries", o.semantic_queries),
            ("consumed_mechanisms", o.consumed_mechanisms),
        ] {
            for elem in duplicate_elements(list) {
                out.push((o.file, o.func, field, elem));
            }
        }
    }
    out
}

/// `(file, func)` keys appearing more than once in an additional-proof
/// spec table (an internal duplicate spec is dead data — its row would be
/// emitted twice).
pub(crate) fn duplicate_spec_keys(
    specs: &[AdditionalProofSpec],
) -> Vec<(&'static str, &'static str)> {
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut reported: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut out = Vec::new();
    for s in specs {
        if !seen.insert((s.file, s.func)) && reported.insert((s.file, s.func)) {
            out.push((s.file, s.func));
        }
    }
    out
}

/// `(file, func)` keys appearing more than once WITHIN
/// `LIFTED_ROW_OVERRIDES` (the later entry is silently shadowed by the
/// `.find` consultation in `run()`).
pub(crate) fn duplicate_lifted_keys(overrides: &[LiftedOverride]) -> Vec<(&str, &str)> {
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut reported: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut out = Vec::new();
    for o in overrides {
        if !seen.insert((o.file, o.func)) && reported.insert((o.file, o.func)) {
            out.push((o.file, o.func));
        }
    }
    out
}

/// `(file, func)` present in BOTH `ROW_MECHANISM_OVERRIDE` and
/// `LIFTED_ROW_OVERRIDES`. A lifted row never consults
/// `ROW_MECHANISM_OVERRIDE` (`mechanism_for_row` only runs on non-lifted
/// rows), so such an override entry is by definition stale/conflicting.
pub(crate) fn overrides_conflicting_with_lifted<'a>(
    row_overrides: &'a [(&'a str, &'a str, &'a str)],
    lifted: &[LiftedOverride],
) -> Vec<(&'a str, &'a str)> {
    let lifted_keys: BTreeSet<(&str, &str)> = lifted.iter().map(|o| (o.file, o.func)).collect();
    row_overrides
        .iter()
        .filter(|&&(file, func, _)| lifted_keys.contains(&(file, func)))
        .map(|&(file, func, _)| (file, func))
        .collect()
}

/// `LIFTED_ROW_OVERRIDES` entries whose `(file, func)` is ABSENT from the
/// §10.4.1 partition. §10.4.1 is the single source of truth for every row's
/// block, lifted rows included, so a lifted override with no partition row
/// is stale/dead data.
pub(crate) fn stale_lifted_overrides<'a>(
    lifted: &'a [LiftedOverride],
    partition: &BTreeMap<(String, String), (String, String)>,
) -> Vec<(&'a str, &'a str)> {
    lifted
        .iter()
        .filter(|o| !partition.contains_key(&(o.file.to_string(), o.func.to_string())))
        .map(|o| (o.file, o.func))
        .collect()
}

/// The `(capability, file, func)` mechanism-consumer sites of the
/// AdditionalProofRow builder (`emit::build_additional_rows`): each row
/// calls `mechanism_for_row(cap, file, func)` with exactly these arguments,
/// so a `ROW_MECHANISM_OVERRIDE` entry for one of these keys IS consumed
/// whenever its capability is in `SPLIT_CAPABILITIES`, even though the key
/// is not a §10.4.1 partition row. Projected from the SAME shared
/// `ADDITIONAL_PROOF_SPECS` table the builder emits from; kept in lockstep
/// with the emitted rows by the test
/// `additional_proof_consumer_model_matches_emitted_additional_rows`.
pub(crate) fn additional_proof_mechanism_consumers(
) -> Vec<(&'static str, &'static str, &'static str)> {
    ADDITIONAL_PROOF_SPECS
        .iter()
        .map(|s| (s.capability, s.file, s.func))
        .collect()
}

/// `ROW_MECHANISM_OVERRIDE` entries with NO consuming row. An entry
/// `(file, func, mech)` is consumed only by `mechanism_for_row`, i.e. only
/// when `(file, func)` is a NON-lifted §10.4.1 partition row whose
/// capability is in `split_capabilities`, OR an AdditionalProofRow
/// mechanism-consumer site (`additional_consumers`, `(capability, file,
/// func)` triples) whose capability is in `split_capabilities`; anything
/// else is stale/dead data. Entries whose `(file, func)` is a lifted key are
/// SKIPPED here — they are owned (and reported exactly once) by
/// [`overrides_conflicting_with_lifted`].
pub(crate) fn stale_mechanism_overrides<'a>(
    row_overrides: &'a [(&'a str, &'a str, &'a str)],
    partition: &BTreeMap<(String, String), (String, String)>,
    lifted_keys: &BTreeSet<(String, String)>,
    split_capabilities: &[&str],
    additional_consumers: &[(&str, &str, &str)],
) -> Vec<(&'a str, &'a str)> {
    row_overrides
        .iter()
        .filter(|&&(file, func, _)| {
            let key = (file.to_string(), func.to_string());
            if lifted_keys.contains(&key) {
                // Owned by `overrides_conflicting_with_lifted`; flagging it
                // here too would report the same entry twice.
                return false;
            }
            let consumed_by_partition_row = partition
                .get(&key)
                .is_some_and(|(_, cap)| split_capabilities.contains(&cap.as_str()));
            let consumed_by_additional_row = additional_consumers
                .iter()
                .any(|&(cap, f, n)| f == file && n == func && split_capabilities.contains(&cap));
            !(consumed_by_partition_row || consumed_by_additional_row)
        })
        .map(|&(file, func, _)| (file, func))
        .collect()
}

/// Aggregate duplicate-key / duplicate-value / duplicate-element findings
/// over the REAL committed `const` tables. Non-empty => `run()` exits 7.
pub(crate) fn validate_data_tables() -> Vec<String> {
    let mut findings = Vec::new();
    let pair_tables: &[(&str, &[(&str, &str)])] = &[
        ("FILE_TO_SUBSTRATE", FILE_TO_SUBSTRATE),
        ("BLOCK_TEXT_TO_VARIANT", BLOCK_TEXT_TO_VARIANT),
        ("BLOCK_TO_MECHANISM", BLOCK_TO_MECHANISM),
        ("BLOCK_TO_UBLOCK", BLOCK_TO_UBLOCK),
        ("BLOCK_TO_ORGAN", BLOCK_TO_ORGAN),
        ("CAPABILITY_TO_MECHANISM", CAPABILITY_TO_MECHANISM),
        ("KEY_OWNING_BLOCK", KEY_OWNING_BLOCK),
        ("PROOF_ORACLE", PROOF_ORACLE),
        ("PROOF_GUARD", PROOF_GUARD),
    ];
    for (name, table) in pair_tables {
        for key in duplicate_pair_keys(table) {
            findings.push(format!(
                "duplicate key `{key}` in {name} — the linear `.find` lookup \
                 silently shadows every later occurrence"
            ));
        }
    }
    let list_tables: &[(&str, ListTable)] = &[
        ("BLOCK_PREREQS", BLOCK_PREREQS),
        ("BLOCK_TO_REQUIRED_GUARDS", BLOCK_TO_REQUIRED_GUARDS),
        ("MECHANISM_TO_KEYS", MECHANISM_TO_KEYS),
    ];
    for (name, table) in list_tables {
        for key in duplicate_pair_keys(table) {
            findings.push(format!(
                "duplicate key `{key}` in {name} — the linear `.find` lookup \
                 silently shadows every later occurrence"
            ));
        }
        for (key, elem) in duplicate_list_values(table) {
            findings.push(format!(
                "duplicate element `{elem}` in the {name} value list for key \
                 `{key}` — the duplicate is emitted verbatim into the manifest"
            ));
        }
    }
    for value in duplicate_pair_values(BLOCK_TO_MECHANISM) {
        findings.push(format!(
            "duplicate mechanism (value) `{value}` in BLOCK_TO_MECHANISM — \
             `mechanism_owning_block` reverse-looks-up by value, so two blocks \
             sharing a mechanism silently mis-resolve the owner"
        ));
    }
    for (file, func) in duplicate_triple_keys(ROW_MECHANISM_OVERRIDE) {
        findings.push(format!(
            "duplicate (file, func) key `{file}::{func}` in ROW_MECHANISM_OVERRIDE"
        ));
    }
    let element_lists: &[(&str, &[&str])] = &[
        ("SPLIT_CAPABILITIES", SPLIT_CAPABILITIES),
        ("BLOCK_VERIFICATION_LABELS", BLOCK_VERIFICATION_LABELS),
    ];
    for (name, list) in element_lists {
        for elem in duplicate_elements(list) {
            findings.push(format!("duplicate element `{elem}` in {name}"));
        }
    }
    for (file, func) in duplicate_spec_keys(ADDITIONAL_PROOF_SPECS) {
        findings.push(format!(
            "duplicate (file, func) key `{file}::{func}` in \
             ADDITIONAL_PROOF_SPECS — the additional-proof row would be \
             emitted twice"
        ));
    }
    for cap in shared_strs(
        SPLIT_CAPABILITIES.iter().copied(),
        CAPABILITY_TO_MECHANISM.iter().map(|(k, _)| *k),
    ) {
        findings.push(format!(
            "capability `{cap}` present in BOTH SPLIT_CAPABILITIES and \
             CAPABILITY_TO_MECHANISM — `mechanism_for_row` takes the \
             split/override branch first, so the CAPABILITY_TO_MECHANISM \
             entry is silently-shadowed dead data"
        ));
    }
    for cap in shared_strs(
        PROOF_GUARD.iter().map(|(k, _)| *k),
        PROOF_ORACLE.iter().map(|(k, _)| *k),
    ) {
        findings.push(format!(
            "capability `{cap}` present in BOTH PROOF_GUARD and PROOF_ORACLE \
             — `proof_for_capability` returns the guard branch first, so the \
             PROOF_ORACLE entry is silently shadowed"
        ));
    }
    findings
}

/// Findings for duplicate §10.4.1 `(file, func)` rows surfaced by
/// `parse_partition`. Non-empty => `run()` exits 7.
pub(crate) fn validate_partition(duplicate_keys: &[(String, String)]) -> Vec<String> {
    duplicate_keys
        .iter()
        .map(|(file, func)| {
            format!(
                "duplicate §10.4.1 partition row `{file}::{func}` — the coverage \
                 table lists this (file, func) more than once; the later row \
                 silently overwrites the earlier"
            )
        })
        .collect()
}

/// Aggregate override-staleness findings over the REAL committed override
/// tables against the parsed partition. Non-empty => `run()` exits 7.
pub(crate) fn validate_overrides(
    partition: &BTreeMap<(String, String), (String, String)>,
) -> Vec<String> {
    let mut findings = Vec::new();
    for (file, func) in duplicate_lifted_keys(LIFTED_ROW_OVERRIDES) {
        findings.push(format!(
            "duplicate (file, func) key `{file}::{func}` in LIFTED_ROW_OVERRIDES"
        ));
    }
    for (file, func, field, elem) in duplicate_override_list_fields(LIFTED_ROW_OVERRIDES) {
        findings.push(format!(
            "duplicate element `{elem}` in the `{field}` list of \
             LIFTED_ROW_OVERRIDES entry `{file}::{func}` — the list is \
             emitted per-row into the manifest"
        ));
    }
    for (file, func) in
        overrides_conflicting_with_lifted(ROW_MECHANISM_OVERRIDE, LIFTED_ROW_OVERRIDES)
    {
        findings.push(format!(
            "conflicting override `{file}::{func}` — present in BOTH \
             ROW_MECHANISM_OVERRIDE and LIFTED_ROW_OVERRIDES; a lifted row \
             never consults ROW_MECHANISM_OVERRIDE, so the entry is stale"
        ));
    }
    for (file, func) in stale_lifted_overrides(LIFTED_ROW_OVERRIDES, partition) {
        findings.push(format!(
            "stale LIFTED_ROW_OVERRIDES entry `{file}::{func}` — absent from \
             the §10.4.1 partition (the single source of truth for every \
             row's block, lifted rows included)"
        ));
    }
    let lifted_keys: BTreeSet<(String, String)> = LIFTED_ROW_OVERRIDES
        .iter()
        .map(|o| (o.file.to_string(), o.func.to_string()))
        .collect();
    let additional_consumers = additional_proof_mechanism_consumers();
    for (file, func) in stale_mechanism_overrides(
        ROW_MECHANISM_OVERRIDE,
        partition,
        &lifted_keys,
        SPLIT_CAPABILITIES,
        &additional_consumers,
    ) {
        findings.push(format!(
            "stale ROW_MECHANISM_OVERRIDE entry `{file}::{func}` — no consuming \
             row (consumed only by a NON-lifted §10.4.1 partition row or an \
             AdditionalProofRow site whose capability is in SPLIT_CAPABILITIES)"
        ));
    }
    findings
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::model::LiftedOverride;
    use crate::partition::parse_partition;

    fn lifted(file: &'static str, func: &'static str) -> LiftedOverride {
        lifted_with_lists(file, func, &[], &[])
    }

    fn lifted_with_lists(
        file: &'static str,
        func: &'static str,
        semantic_queries: &'static [&'static str],
        consumed_mechanisms: &'static [&'static str],
    ) -> LiftedOverride {
        LiftedOverride {
            file,
            func,
            mech: "SomeMechanism",
            proof: "ProofRequirement::Ts7Oracle(OracleId::UtilityComposition)",
            semantic_queries,
            consumed_mechanisms,
            unblocker: "crafted test override",
        }
    }

    fn spec(file: &'static str, func: &'static str) -> AdditionalProofSpec {
        AdditionalProofSpec {
            capability: "JsxResolution",
            file,
            func,
            block: "U2JsxFoundations",
        }
    }

    fn partition_of(
        rows: &[(&str, &str, &str, &str)],
    ) -> BTreeMap<(String, String), (String, String)> {
        rows.iter()
            .map(|&(file, func, block, cap)| {
                (
                    (file.to_string(), func.to_string()),
                    (block.to_string(), cap.to_string()),
                )
            })
            .collect()
    }

    fn lifted_set(keys: &[(&str, &str)]) -> BTreeSet<(String, String)> {
        keys.iter()
            .map(|&(f, n)| (f.to_string(), n.to_string()))
            .collect()
    }

    // --- duplicate_pair_keys ---

    #[test]
    fn duplicate_pair_keys_flags_planted_duplicate_key() {
        let table: &[(&str, &str)] = &[("alpha", "1"), ("beta", "2"), ("alpha", "3")];
        assert_eq!(duplicate_pair_keys(table), vec!["alpha"]);
    }

    #[test]
    fn duplicate_pair_keys_clean_table_reports_nothing() {
        let table: &[(&str, &str)] = &[("alpha", "1"), ("beta", "2")];
        assert_eq!(duplicate_pair_keys(table), Vec::<&str>::new());
    }

    #[test]
    fn duplicate_pair_keys_flags_planted_duplicate_in_list_valued_table() {
        let table: &[(&str, &[&str])] = &[("alpha", &["x"]), ("beta", &[]), ("alpha", &["y"])];
        assert_eq!(duplicate_pair_keys(table), vec!["alpha"]);
    }

    #[test]
    fn duplicate_pair_keys_clean_list_valued_table_reports_nothing() {
        let table: &[(&str, &[&str])] = &[("alpha", &["x"]), ("beta", &["y"])];
        assert_eq!(duplicate_pair_keys(table), Vec::<&str>::new());
    }

    // --- duplicate_pair_values ---

    #[test]
    fn duplicate_pair_values_flags_planted_shared_value() {
        let table: &[(&str, &str)] = &[("a", "mech1"), ("b", "mech2"), ("c", "mech1")];
        assert_eq!(duplicate_pair_values(table), vec!["mech1"]);
    }

    #[test]
    fn duplicate_pair_values_clean_table_reports_nothing() {
        let table: &[(&str, &str)] = &[("a", "mech1"), ("b", "mech2")];
        assert_eq!(duplicate_pair_values(table), Vec::<&str>::new());
    }

    // --- duplicate_triple_keys ---

    #[test]
    fn duplicate_triple_keys_flags_planted_duplicate_file_func() {
        let table: &[(&str, &str, &str)] = &[
            ("file.rs", "case_one", "mech1"),
            ("file.rs", "case_two", "mech2"),
            ("file.rs", "case_one", "mech3"),
        ];
        assert_eq!(duplicate_triple_keys(table), vec![("file.rs", "case_one")]);
    }

    #[test]
    fn duplicate_triple_keys_distinct_file_func_pairs_report_nothing() {
        let table: &[(&str, &str, &str)] = &[
            ("file.rs", "case_one", "mech1"),
            ("file.rs", "case_two", "mech1"),
            ("other.rs", "case_one", "mech2"),
        ];
        assert_eq!(duplicate_triple_keys(table), Vec::<(&str, &str)>::new());
    }

    // --- duplicate_elements ---

    #[test]
    fn duplicate_elements_flags_planted_duplicate() {
        assert_eq!(duplicate_elements(&["x", "y", "x"]), vec!["x"]);
    }

    #[test]
    fn duplicate_elements_clean_list_reports_nothing() {
        assert_eq!(duplicate_elements(&["x", "y", "z"]), Vec::<&str>::new());
    }

    // --- shared_strs ---

    #[test]
    fn shared_strs_flags_planted_overlap_in_first_order() {
        let first = ["beta", "alpha", "gamma"];
        let second = ["alpha", "beta", "delta"];
        assert_eq!(
            shared_strs(first.iter().copied(), second.iter().copied()),
            vec!["beta", "alpha"]
        );
    }

    #[test]
    fn shared_strs_reports_each_shared_item_once() {
        let first = ["alpha", "alpha"];
        let second = ["alpha"];
        assert_eq!(
            shared_strs(first.iter().copied(), second.iter().copied()),
            vec!["alpha"]
        );
    }

    #[test]
    fn shared_strs_disjoint_inputs_report_nothing() {
        let first = ["alpha", "beta"];
        let second = ["gamma", "delta"];
        assert_eq!(
            shared_strs(first.iter().copied(), second.iter().copied()),
            Vec::<&str>::new()
        );
    }

    // --- duplicate_list_values ---

    #[test]
    fn duplicate_list_values_flags_planted_duplicate_inside_value_list() {
        let table: &[(&str, &[&str])] = &[("alpha", &["x", "y", "x"]), ("beta", &["z"])];
        assert_eq!(duplicate_list_values(table), vec![("alpha", "x")]);
    }

    #[test]
    fn duplicate_list_values_cross_list_repetition_is_not_a_duplicate() {
        // "x" appears in BOTH keys' lists — legal; only an INTERNAL
        // repetition within one list is a defect.
        let table: &[(&str, &[&str])] = &[("alpha", &["x", "y"]), ("beta", &["x"])];
        assert_eq!(duplicate_list_values(table), Vec::<(&str, &str)>::new());
    }

    // --- duplicate_override_list_fields ---

    #[test]
    fn duplicate_semantic_queries_element_in_override_is_flagged() {
        let overrides = [lifted_with_lists(
            "file.rs",
            "case_one",
            &["TypeOf", "ResolveDecl", "TypeOf"],
            &["MechA"],
        )];
        assert_eq!(
            duplicate_override_list_fields(&overrides),
            vec![("file.rs", "case_one", "semantic_queries", "TypeOf")]
        );
    }

    #[test]
    fn duplicate_consumed_mechanisms_element_in_override_is_flagged() {
        let overrides = [lifted_with_lists(
            "file.rs",
            "case_one",
            &["TypeOf"],
            &["MechA", "MechA"],
        )];
        assert_eq!(
            duplicate_override_list_fields(&overrides),
            vec![("file.rs", "case_one", "consumed_mechanisms", "MechA")]
        );
    }

    #[test]
    fn override_with_distinct_list_fields_reports_nothing() {
        let overrides = [lifted_with_lists(
            "file.rs",
            "case_one",
            &["TypeOf", "ResolveDecl"],
            &["MechA", "MechB"],
        )];
        assert_eq!(
            duplicate_override_list_fields(&overrides),
            Vec::<(&str, &str, &str, &str)>::new()
        );
    }

    // --- duplicate_spec_keys ---

    #[test]
    fn duplicate_spec_keys_flags_planted_duplicate_file_func() {
        let specs = [
            spec("jsx.rs", "case_one"),
            spec("jsx.rs", "case_two"),
            spec("jsx.rs", "case_one"),
        ];
        assert_eq!(duplicate_spec_keys(&specs), vec![("jsx.rs", "case_one")]);
    }

    #[test]
    fn duplicate_spec_keys_distinct_specs_report_nothing() {
        // Same func under a DIFFERENT file is a distinct (file, func) key.
        let specs = [spec("jsx.rs", "case_one"), spec("mapped.rs", "case_one")];
        assert_eq!(duplicate_spec_keys(&specs), Vec::<(&str, &str)>::new());
    }

    // --- parse_partition duplicate surfacing ---

    #[test]
    fn parse_partition_surfaces_planted_duplicate_file_func_row() {
        let doc = "<!-- BEGIN U0 row→block coverage table -->\n\
                   **`U2.FOO`** (2 rows):\n\
                   - `some_file.rs::case_one` — `CallResolution`\n\
                   - `some_file.rs::case_one` — `DemandBoundary`\n\
                   <!-- END U0 row→block coverage table -->\n";
        let parsed = parse_partition(doc);
        assert_eq!(
            parsed.duplicate_keys,
            vec![("some_file.rs".to_string(), "case_one".to_string())]
        );
        assert_eq!(parsed.rows.len(), 1);
    }

    #[test]
    fn parse_partition_distinct_rows_surface_no_duplicates() {
        let doc = "<!-- BEGIN U0 row→block coverage table -->\n\
                   **`U2.FOO`** (2 rows):\n\
                   - `some_file.rs::case_one` — `CallResolution`\n\
                   - `some_file.rs::case_two` — `DemandBoundary`\n\
                   <!-- END U0 row→block coverage table -->\n";
        let parsed = parse_partition(doc);
        assert_eq!(parsed.duplicate_keys, Vec::<(String, String)>::new());
        assert_eq!(parsed.rows.len(), 2);
    }

    // --- duplicate_lifted_keys ---

    #[test]
    fn duplicate_lifted_keys_flags_planted_duplicate() {
        let overrides = [
            lifted("file.rs", "case_one"),
            lifted("file.rs", "case_two"),
            lifted("file.rs", "case_one"),
        ];
        assert_eq!(
            duplicate_lifted_keys(&overrides),
            vec![("file.rs", "case_one")]
        );
    }

    #[test]
    fn duplicate_lifted_keys_distinct_overrides_report_nothing() {
        let overrides = [lifted("file.rs", "case_one"), lifted("file.rs", "case_two")];
        assert_eq!(
            duplicate_lifted_keys(&overrides),
            Vec::<(&str, &str)>::new()
        );
    }

    // --- overrides_conflicting_with_lifted ---

    #[test]
    fn override_shadowed_by_lifted_row_is_flagged_as_conflicting() {
        let row_overrides: &[(&str, &str, &str)] = &[
            ("file.rs", "case_one", "mech1"),
            ("file.rs", "case_two", "mech2"),
        ];
        let lifted_rows = [lifted("file.rs", "case_one")];
        assert_eq!(
            overrides_conflicting_with_lifted(row_overrides, &lifted_rows),
            vec![("file.rs", "case_one")]
        );
    }

    #[test]
    fn disjoint_override_and_lifted_sets_report_no_conflict() {
        let row_overrides: &[(&str, &str, &str)] = &[("file.rs", "case_one", "mech1")];
        let lifted_rows = [lifted("file.rs", "case_two")];
        assert_eq!(
            overrides_conflicting_with_lifted(row_overrides, &lifted_rows),
            Vec::<(&str, &str)>::new()
        );
    }

    // --- stale_lifted_overrides ---

    #[test]
    fn stale_lifted_override_absent_from_partition_is_flagged() {
        let lifted_rows = [lifted("ghost.rs", "no_such_row")];
        let partition = partition_of(&[("file.rs", "case_one", "U2.FOO", "CallResolution")]);
        assert_eq!(
            stale_lifted_overrides(&lifted_rows, &partition),
            vec![("ghost.rs", "no_such_row")]
        );
    }

    #[test]
    fn lifted_override_present_in_partition_is_not_flagged() {
        let lifted_rows = [lifted("file.rs", "case_one")];
        let partition = partition_of(&[("file.rs", "case_one", "U2.FOO", "CallResolution")]);
        assert_eq!(
            stale_lifted_overrides(&lifted_rows, &partition),
            Vec::<(&str, &str)>::new()
        );
    }

    // --- stale_mechanism_overrides ---

    #[test]
    fn stale_mechanism_override_with_no_partition_row_is_flagged() {
        let row_overrides: &[(&str, &str, &str)] = &[("ghost.rs", "no_such_row", "mech1")];
        let partition = partition_of(&[("file.rs", "case_one", "U2.FOO", "CallResolution")]);
        let lifted_keys = lifted_set(&[]);
        assert_eq!(
            stale_mechanism_overrides(
                row_overrides,
                &partition,
                &lifted_keys,
                &["CallResolution"],
                &[]
            ),
            vec![("ghost.rs", "no_such_row")]
        );
    }

    #[test]
    fn stale_mechanism_override_on_non_split_capability_row_is_flagged() {
        let row_overrides: &[(&str, &str, &str)] = &[("file.rs", "case_one", "mech1")];
        let partition = partition_of(&[("file.rs", "case_one", "U2.FOO", "SingleBlockCap")]);
        let lifted_keys = lifted_set(&[]);
        assert_eq!(
            stale_mechanism_overrides(
                row_overrides,
                &partition,
                &lifted_keys,
                &["CallResolution"],
                &[]
            ),
            vec![("file.rs", "case_one")]
        );
    }

    #[test]
    fn consumed_mechanism_override_is_not_flagged_as_stale() {
        let row_overrides: &[(&str, &str, &str)] = &[("file.rs", "case_one", "mech1")];
        let partition = partition_of(&[("file.rs", "case_one", "U2.FOO", "CallResolution")]);
        let lifted_keys = lifted_set(&[]);
        assert_eq!(
            stale_mechanism_overrides(
                row_overrides,
                &partition,
                &lifted_keys,
                &["CallResolution"],
                &[]
            ),
            Vec::<(&str, &str)>::new()
        );
    }

    #[test]
    fn both_tables_override_yields_exactly_one_finding_from_the_conflict_check() {
        // An entry in BOTH ROW_MECHANISM_OVERRIDE and LIFTED_ROW_OVERRIDES is
        // owned by `overrides_conflicting_with_lifted`; the stale check must
        // NOT flag it a second time.
        let row_overrides: &[(&str, &str, &str)] = &[("file.rs", "case_one", "mech1")];
        let lifted_rows = [lifted("file.rs", "case_one")];
        let partition = partition_of(&[("file.rs", "case_one", "U2.FOO", "CallResolution")]);
        let lifted_keys = lifted_set(&[("file.rs", "case_one")]);
        assert_eq!(
            overrides_conflicting_with_lifted(row_overrides, &lifted_rows),
            vec![("file.rs", "case_one")]
        );
        assert_eq!(
            stale_mechanism_overrides(
                row_overrides,
                &partition,
                &lifted_keys,
                &["CallResolution"],
                &[]
            ),
            Vec::<(&str, &str)>::new()
        );
    }

    // --- stale_mechanism_overrides: AdditionalProofRow consumers ---

    #[test]
    fn override_consumed_by_additional_proof_row_under_split_capability_is_not_stale() {
        // (jsx.rs, jsx_case) is NOT a partition row, but the AdditionalProofRow
        // builder calls `mechanism_for_row("JsxResolution", "jsx.rs", jsx_case)`;
        // with JsxResolution split, that call READS the override — consumed.
        let row_overrides: &[(&str, &str, &str)] = &[("jsx.rs", "jsx_case", "mech1")];
        let partition = partition_of(&[]);
        let lifted_keys = lifted_set(&[]);
        let additional: &[(&str, &str, &str)] = &[("JsxResolution", "jsx.rs", "jsx_case")];
        assert_eq!(
            stale_mechanism_overrides(
                row_overrides,
                &partition,
                &lifted_keys,
                &["JsxResolution"],
                additional
            ),
            Vec::<(&str, &str)>::new()
        );
    }

    #[test]
    fn override_on_additional_proof_row_with_non_split_capability_is_still_stale() {
        // Same consumer site, but JsxResolution NOT split: `mechanism_for_row`
        // routes through CAPABILITY_TO_MECHANISM and never reads the override.
        let row_overrides: &[(&str, &str, &str)] = &[("jsx.rs", "jsx_case", "mech1")];
        let partition = partition_of(&[]);
        let lifted_keys = lifted_set(&[]);
        let additional: &[(&str, &str, &str)] = &[("JsxResolution", "jsx.rs", "jsx_case")];
        assert_eq!(
            stale_mechanism_overrides(
                row_overrides,
                &partition,
                &lifted_keys,
                &["CallResolution"],
                additional
            ),
            vec![("jsx.rs", "jsx_case")]
        );
    }

    #[test]
    fn additional_proof_consumer_model_matches_emitted_additional_rows() {
        // The consumed-set model must stay in lockstep with the ACTUAL
        // `mechanism_for_row` call sites in `emit::build_additional_rows` —
        // one `(capability, file, func)` triple per emitted AdditionalProofRow.
        let modeled: BTreeSet<(String, String, String)> = additional_proof_mechanism_consumers()
            .iter()
            .map(|&(cap, file, func)| (cap.to_string(), file.to_string(), func.to_string()))
            .collect();
        let emitted: BTreeSet<(String, String, String)> = crate::emit::build_additional_rows()
            .iter()
            .map(|r| (r.cap.clone(), r.file.clone(), r.func.clone()))
            .collect();
        assert_eq!(modeled, emitted);
        assert_eq!(modeled.len(), crate::emit::build_additional_rows().len());
    }

    // --- the committed data itself is valid ---
    // (`committed_partition_and_override_tables_are_clean`, which READS the
    // partition doc from disk, lives in `run.rs`'s test module — this module
    // stays a PURE validator with no filesystem access.)

    #[test]
    fn committed_data_tables_have_no_duplicates() {
        assert_eq!(validate_data_tables(), Vec::<String>::new());
    }
}
