//! Filesystem orchestration: discovery, cross-checks, self-consistency
//! assertions, and the write / `--check` drift-gate flow.

// Build-time generator binary — NOT a semantic session path. It writes
// checked-in source artifacts via the plain `std::fs` import (the same
// convention the `gen-query-key-spec` generator bin uses); it never reads or
// writes workspace files at session runtime, so it does not route through
// `WorkspaceAccess` / the VFS boundary.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use crate::data::{
    BLOCK_TEXT_TO_VARIANT, BLOCK_TO_MECHANISM, BLOCK_TO_ORGAN, BLOCK_TO_UBLOCK, FILE_TO_SUBSTRATE,
    KEY_OWNING_BLOCK, LIFTED_ROW_OVERRIDES,
};
use crate::derive::{
    consumed_mechs_for_block, escape_rust_string_literal, fail, keys_for_row, lookup_pair,
    lookup_pair_or_fail, mechanism_for_row, mechanism_owning_block, proof_for_capability, reaches,
};
use crate::emit::{
    build_additional_rows, emit_additional_rows, emit_block_rows, emit_ignored_rows,
};
use crate::model::Row;
use crate::partition::{extract_sites, parse_partition};

pub(crate) fn run(check_only: bool) -> i32 {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf();
    let src_dir = repo_root.join("crates/verter_session/src/typeinfo/typeinfo_tests");
    if !src_dir.is_dir() {
        eprintln!("typeinfo_tests dir missing: {}", src_dir.display());
        return 2;
    }
    let out_dir = repo_root.join("crates/verter_session/tests/manifest_data");
    if !check_only {
        fs::create_dir_all(&out_dir)
            .unwrap_or_else(|e| fail(&format!("create {}: {e}", out_dir.display())));
    }

    let doc_path = repo_root.join("docs/arch/native-typeinfo-parity.md");
    let doc = fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| fail(&format!("read {}: {e}", doc_path.display())));
    let partition = parse_partition(&doc);

    // Discover live ignore sites + reasons.
    let mut discovered: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut missing_mappings: Vec<String> = Vec::new();
    let mut file_names: Vec<String> = fs::read_dir(&src_dir)
        .unwrap_or_else(|e| fail(&format!("read dir {}: {e}", src_dir.display())))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    file_names.sort();
    for name in file_names {
        if !name.ends_with(".rs") {
            continue;
        }
        let path = src_dir.join(&name);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|e| fail(&format!("read {}: {e}", path.display())));
        let sites = extract_sites(&source);
        if sites.is_empty() {
            continue;
        }
        if lookup_pair(FILE_TO_SUBSTRATE, &name).is_none() {
            missing_mappings.push(name);
            continue;
        }
        for (reason, fn_name) in sites {
            discovered.insert((name.clone(), fn_name), reason);
        }
    }

    if !missing_mappings.is_empty() {
        eprintln!("error: typeinfo-test files without a FILE_TO_SUBSTRATE mapping:");
        for name in &missing_mappings {
            eprintln!("  - {name}");
        }
        return 3;
    }

    // Cross-check discovery vs §10.4.1 partition. A LIFTED row is no longer a
    // live `#[ignore]` site (its body is `oracle::run_row`), so it is expected
    // to be in the partition but NOT in the discovered set; every OTHER row
    // must agree row-for-row.
    let disc_keys: BTreeSet<(String, String)> = discovered.keys().cloned().collect();
    let part_keys: BTreeSet<(String, String)> = partition.keys().cloned().collect();
    let lifted_keys: BTreeSet<(String, String)> = LIFTED_ROW_OVERRIDES
        .iter()
        .map(|o| (o.file.to_string(), o.func.to_string()))
        .collect();
    let lifted_not_in_partition: Vec<&(String, String)> =
        lifted_keys.difference(&part_keys).collect();
    let lifted_still_ignored: Vec<&(String, String)> =
        lifted_keys.intersection(&disc_keys).collect();
    if !lifted_not_in_partition.is_empty() || !lifted_still_ignored.is_empty() {
        eprintln!("error: lifted-row override set is inconsistent:");
        for k in &lifted_not_in_partition {
            eprintln!(
                "  lifted row absent from §10.4.1 partition: {} :: {}",
                k.0, k.1
            );
        }
        for k in &lifted_still_ignored {
            eprintln!(
                "  lifted row still carries a live `#[ignore]`: {} :: {}",
                k.0, k.1
            );
        }
        return 4;
    }
    let only_disc: Vec<&(String, String)> = disc_keys.difference(&part_keys).collect();
    let only_part: Vec<&(String, String)> = part_keys
        .iter()
        .filter(|k| !disc_keys.contains(*k) && !lifted_keys.contains(*k))
        .collect();
    if !only_disc.is_empty() || !only_part.is_empty() {
        eprintln!("error: §10.4.1 partition does not match the live ignore set:");
        for k in &only_disc {
            eprintln!("  live-only (no partition row): {} :: {}", k.0, k.1);
        }
        for k in &only_part {
            eprintln!(
                "  partition-only (no live ignore, not lifted): {} :: {}",
                k.0, k.1
            );
        }
        return 4;
    }

    // Build the IgnoredTestRows in (file, function) sorted order. The row set
    // is the live discovered ignores UNION the lifted rows (which are no
    // longer discovered but stay in the table with `status: Lifted`).
    let all_keys: BTreeSet<(String, String)> = disc_keys.union(&lifted_keys).cloned().collect();
    let mut rows: Vec<Row> = Vec::new();
    for (file, func) in &all_keys {
        let (block_text, cap) = partition
            .get(&(file.clone(), func.clone()))
            .expect("every emitted row key is in the partition (checked above)");
        let block_var = lookup_pair(BLOCK_TEXT_TO_VARIANT, block_text).unwrap_or_else(|| {
            fail(&format!(
                "unknown block id text in the §10.4.1 partition: {block_text}"
            ))
        });
        let overr = LIFTED_ROW_OVERRIDES
            .iter()
            .find(|o| o.file == file.as_str() && o.func == func.as_str());
        let (mech, proof, status, unblocker, row_keys, row_consumed) = match overr {
            // Lifted: `block_var` comes from §10.4.1 (parsed above) — the
            // SINGLE source of truth for every row's block_id, including
            // lifted rows. The override supplies ONLY the lift metadata that
            // is NOT in §10.4.1: mechanism / proof / unblocker + the
            // execution-true semantic_queries / consumed set.
            Some(o) => (
                o.mech,
                o.proof.to_string(),
                format!("IgnoreStatus::Lifted {{ block_id: TypeInfoParityBlockId::{block_var} }}"),
                escape_rust_string_literal(o.unblocker),
                o.semantic_queries.to_vec(),
                o.consumed_mechanisms.to_vec(),
            ),
            // mechanism_id is ROW-LEVEL, derived from capability/override —
            // INDEPENDENT of block_var (the partition's block column).
            None => {
                let mech = mechanism_for_row(cap, file, func);
                (
                    mech,
                    proof_for_capability(cap),
                    "IgnoreStatus::Ignored".to_string(),
                    escape_rust_string_literal(&discovered[&(file.clone(), func.clone())]),
                    keys_for_row(mech).to_vec(),
                    consumed_mechs_for_block(block_var),
                )
            }
        };
        // §Q4: the number of oracle queries the row declares. A LIFTED row
        // issues exactly its registry-entry count (one `QuerySpec` per lifted
        // row today); a non-lifted row issues none. The Rust guard
        // `registry_entry_count_matches_declared` cross-checks this against
        // the ACTUAL `ORACLE_QUERY_SPECS` count.
        let oracle_query_ordinals: u32 = if overr.is_some() { 1 } else { 0 };
        rows.push(Row {
            file: file.clone(),
            func: func.clone(),
            substrate: lookup_pair_or_fail(FILE_TO_SUBSTRATE, file, "FILE_TO_SUBSTRATE"),
            cap: cap.clone(),
            organ: lookup_pair_or_fail(BLOCK_TO_ORGAN, block_var, "BLOCK_TO_ORGAN"),
            ublock: lookup_pair_or_fail(BLOCK_TO_UBLOCK, block_var, "BLOCK_TO_UBLOCK"),
            block: block_var,
            keys: row_keys,
            proof,
            mech,
            consumed: row_consumed,
            status,
            oracle_query_ordinals,
            unblocker,
        });
    }

    if rows.len() != 362 {
        eprintln!("error: expected 362 IgnoredTestRows, built {}", rows.len());
        return 5;
    }

    // Generation-time self-consistency assertions (NON-circular): the
    // row-level mechanism (from capability/override) and the partition's
    // block_id are INDEPENDENT sources; the correct table requires them to
    // agree, and that every mechanism's full key set is reachable from the
    // row's block. A failure here means the override/capability mechanism map
    // or the partition disagrees — fix the source, do NOT silence.
    for r in &rows {
        let owner = mechanism_owning_block(r.mech);
        if owner != r.block {
            fail(&format!(
                "mechanism/block disagreement: {}::{} has row-level mechanism {} \
                 owned by {}, but the §10.4.1 partition places it in {}. \
                 Reconcile ROW_MECHANISM_OVERRIDE / CAPABILITY_TO_MECHANISM with \
                 the partition (do NOT derive mechanism from block).",
                r.file, r.func, r.mech, owner, r.block
            ));
        }
        for k in &r.keys {
            let Some(key_owner) = lookup_pair(KEY_OWNING_BLOCK, k) else {
                fail(&format!(
                    "unknown semantic-query key: {}::{} (mechanism {}) consumes {}, \
                     which has no entry in KEY_OWNING_BLOCK. Add {} to \
                     KEY_OWNING_BLOCK to match the live `key_owning_block` arms in \
                     typeinfo_ignored_test_manifest.rs.",
                    r.file, r.func, r.mech, k, k
                ));
            };
            if !reaches(r.block, key_owner) {
                fail(&format!(
                    "unreachable key: {}::{} (mechanism {}) consumes {} owned by {}, \
                     not reachable from block {}. Fix MECHANISM_TO_KEYS or the block \
                     prereqs.",
                    r.file, r.func, r.mech, k, key_owner, r.block
                ));
            }
        }
    }

    let additional = build_additional_rows();

    // The generated artifacts, computed in memory. The Rust guard tests only
    // diff/fail; this bin is the SOLE writer. `--check` mode regenerates
    // these in memory and FAILS (non-zero) if any committed file diverges,
    // WITHOUT writing — so CI / a pnpm script can detect generator drift
    // without a tracked-source side effect.
    let generated: Vec<(&str, String)> = vec![
        (
            "typeinfo_ignored_test_manifest_rows.rs",
            emit_ignored_rows(&rows),
        ),
        (
            "typeinfo_additional_proof_rows.rs",
            emit_additional_rows(&additional),
        ),
        ("typeinfo_parity_blocks.rs", emit_block_rows()),
    ];

    if check_only {
        let mut drifted: Vec<&str> = Vec::new();
        for (name, content) in &generated {
            let path = out_dir.join(name);
            // Normalize CRLF -> LF on the committed side so a CRLF checkout
            // (core.autocrlf) does not report false drift; the generated
            // content is always LF. The LF-normalization is INTENTIONAL per
            // the Cross-Platform Portability rule: freshness is an
            // LF-normalized content compare, never a raw byte compare.
            let committed = if path.exists() {
                Some(
                    fs::read_to_string(&path)
                        .unwrap_or_else(|e| fail(&format!("read {}: {e}", path.display())))
                        .replace("\r\n", "\n"),
                )
            } else {
                None
            };
            if committed.as_deref() != Some(content.as_str()) {
                drifted.push(name);
            }
        }
        if !drifted.is_empty() {
            eprintln!(
                "error: committed typeinfo manifest is STALE vs the generator \
                 for the following file(s):"
            );
            for name in &drifted {
                eprintln!("  - crates/verter_session/tests/manifest_data/{name}");
            }
            eprintln!("Regenerate with `pnpm gen:typeinfo-manifest` and commit the result.");
            return 6;
        }
        eprintln!(
            "check: {} generated manifest file(s) match the regenerated output \
             ({} IgnoredTestRows, {} AdditionalProofRows, {} BlockContractRows)",
            generated.len(),
            rows.len(),
            additional.len(),
            BLOCK_TO_MECHANISM.len()
        );
        return 0;
    }

    for (name, content) in &generated {
        let path = out_dir.join(name);
        fs::write(&path, content)
            .unwrap_or_else(|e| fail(&format!("write {}: {e}", path.display())));
    }
    eprintln!(
        "wrote {} IgnoredTestRows, {} AdditionalProofRows, {} BlockContractRows",
        rows.len(),
        additional.len(),
        BLOCK_TO_MECHANISM.len()
    );
    0
}
