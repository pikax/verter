//! The two foundational single-spec guards for the ts-compat oracle model
//! (`docs/arch/ts-compat-two-mode-model.md` §4 / §7.1 / §8 / §12).
//!
//! The model has ONE resolution spec (no `SpecVariant` / bug-for-bug / compat
//! dimension anywhere in the resolver, cache, or session surface) and asserts
//! every recorded TS-bug divergence is discharged by a live correction whose
//! `resolver(query) == correct_value`. Two guards pin that, and BOTH are
//! discriminating (a planted violation FAILS; the clean tree PASSES — without
//! leaving the planted violation in the tree, via shared pure engines exercised
//! with synthetic inputs):
//!
//! - [`resolver_is_single_spec`] — a CLOSED ABSENCE invariant (§4): (a) an
//!   EXACT-token scan over the resolver / cache / session PRODUCTION crates for
//!   ZERO hits of the closed deny-set, catching a deny-token even as an enum
//!   VARIANT name; (b) a STRUCTURAL field-inventory over a CLOSED target list of
//!   cache-key / context structs, asserting none gains a field whose NAME is in
//!   the deny-set OR whose TYPE is in a closed forbidden-selector-type set, with
//!   an explicit allowlist for the legitimate non-spec `*_profile` fields. Same
//!   family as `no_phase_archaeology_in_production_code`.
//! - [`every_correction_is_discharged`] — a finite ∃-discharge (§7.1 / §8): the
//!   on-disk correction set SET-EQUALS the registry-derived corrected-query set
//!   keyed `(row_file, row_function, query_ordinal, snapshot_id)`, and each
//!   corrected query satisfies `resolver(query) == correct_value` while the
//!   snapshot's `oracle_value` DIFFERS. In THIS block both sets are EMPTY (zero
//!   corrections — correct == tsgo for the lifted rows), so the live invariant is
//!   vacuously satisfied; the engine is proven discriminating with synthetic
//!   inputs.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

// ===========================================================================
// §4 — Closed deny-set + allowlist + forbidden-selector-type set
// ===========================================================================

/// The CLOSED deny-set of EXACT tokens that betray a second resolution spec
/// (§4). A hit ANYWHERE in production resolver / cache / session source — even as
/// an enum VARIANT name (`enum SemanticMode { Correct, TsCompat }`) or a struct
/// field (`compat_profile`) — fails the build. NO `*_profile` glob: the only
/// `*_profile` token denied is the exact `compat_profile`, so the legitimate
/// `query_profile` / `compile_profile` / `tsx_profile` fields never false-trip
/// (none CONTAINS the substring `compat_profile`).
const DENY_TOKENS: &[&str] = &[
    "SpecVariant",
    "spec_variant",
    "spec.diverge",
    "TsCompat",
    "ts_compat",
    "bug_for_bug",
    "compat_profile",
];

/// The legitimate non-spec `*_profile` fields the structural inventory (b)
/// allows on the target structs. They are query/compile/source-map presets, not
/// spec selectors. (a) never flags them — none contains a deny-token substring —
/// so this allowlist documents intent and guards the (b) field scan.
const PROFILE_ALLOWLIST: &[&str] = &["query_profile", "compile_profile", "tsx_profile"];

/// The CLOSED forbidden-selector-TYPE set for the structural inventory (b): a
/// field on a target struct whose TYPE is one of these is a spec selector even if
/// its NAME is clean. Currently the deny-token TYPE names; a future attested
/// `is_spec_selector` type would extend this set at registration (§4).
const FORBIDDEN_SELECTOR_TYPES: &[&str] = &["SpecVariant", "TsCompat"];

/// The CLOSED target list for the structural field-inventory (b): the
/// resolver-INPUT cache-key + per-key/session context surface (§4 / §8). No spec
/// input ⇒ no spec-dependent downstream value, so rooting the inventory here is
/// sufficient. Each `(type_name, source_rel)` pins where the definition lives.
const TARGET_STRUCTS: &[(&str, &str)] = &[
    (
        "SemanticQueryKey",
        "crates/verter_session/src/semantic_query.rs",
    ),
    (
        "FamilyKey",
        "crates/verter_session/src/semantic_query_memo/family.rs",
    ),
    (
        "ComponentMetaResultKey",
        "crates/verter_session/src/component_meta_result_db.rs",
    ),
    (
        "MaterializeStructureCacheKey",
        "crates/verter_session/src/component_meta_materialize.rs",
    ),
    (
        "ShapeCacheKey",
        "crates/verter_session/src/component_meta_caches.rs",
    ),
    (
        "SessionResolverContext",
        "crates/verter_session/src/resolver_core/session_resolver_context.rs",
    ),
    (
        "InstantiateContext",
        "crates/verter_session/src/semantic_query.rs",
    ),
    (
        "MacroPayloadContext",
        "crates/verter_session/src/semantic_query.rs",
    ),
    (
        "ProjectionReductionContext",
        "crates/verter_session/src/semantic_query.rs",
    ),
    (
        "SemanticQueryKeySpec",
        "crates/verter_session/src/semantic_query/query_key_spec.rs",
    ),
];

// ===========================================================================
// §4(a) — EXACT-token absence scan over production source (pure engine)
// ===========================================================================

/// Pure engine: return every `(line_number, token)` where `src` contains a
/// deny-token. Substring match — catches a deny-token used as a type, a field, OR
/// an enum variant name. Discriminating: a source naming any deny-token yields a
/// non-empty result; clean source yields empty.
fn scan_source_for_deny_tokens(src: &str) -> Vec<(usize, &'static str)> {
    let mut out = Vec::new();
    for (lineno, line) in src.lines().enumerate() {
        for &tok in DENY_TOKENS {
            if line.contains(tok) {
                out.push((lineno + 1, tok));
            }
        }
    }
    out
}

/// Whether a production `.rs` path is EXEMPT from the deny-token scan: the
/// oracle-harness / correction-metadata code + tests are the only places allowed
/// to NAME the tokens (§4). Excludes the oracle core, the pure-data registry, the
/// generator binary, and any `*_tests.rs` / `tests.rs` test module file.
fn is_token_scan_exempt(rel: &str) -> bool {
    rel.contains("/typeinfo/oracle_core/")
        || rel.contains("/typeinfo_tests/")
        || rel.contains("/bin/oracle_gen")
        || rel.contains("oracle_query_specs")
        || rel.ends_with("_tests.rs")
        || rel.ends_with("/tests.rs")
}

/// Enumerate every production `.rs` file under `crates/*/src/` (excluding
/// `benches/` / `examples/` / `target/` subdirs). Mirrors the
/// `no_phase_archaeology_in_production_code` production walk.
fn production_rs_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let crates = workspace_root().join("crates");
    let crate_dirs = match fs::read_dir(&crates) {
        Ok(it) => it,
        Err(e) => panic!("read_dir {}: {e}", crates.display()),
    };
    for crate_entry in crate_dirs.flatten() {
        let src = crate_entry.path().join("src");
        if !src.is_dir() {
            continue;
        }
        let mut stack = vec![src];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == "benches" || name == "examples" || name == "target" {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }
    }
    out.sort();
    out
}

fn rel_to_root(abs: &Path) -> String {
    abs.strip_prefix(workspace_root())
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

// ===========================================================================
// §4(b) — Structural field-inventory (pure engine)
// ===========================================================================

/// Extract the brace-delimited body of `struct <name>` / `enum <name>` from
/// `src` (the first matching definition). Brace-depth matched, so nested member
/// blocks are captured whole. `None` when the definition is not found.
fn extract_definition_body(src: &str, name: &str) -> Option<String> {
    for kw in ["struct ", "enum "] {
        let needle = format!("{kw}{name}");
        let mut from = 0;
        while let Some(rel) = src[from..].find(&needle) {
            let start = from + rel;
            // Word-boundary: the char AFTER the name must not continue an
            // identifier (so `InstantiateContext` does not match
            // `InstantiateContextExtra`).
            let after = start + needle.len();
            let next = src[after..].chars().next();
            let is_boundary = !matches!(next, Some(c) if c.is_alphanumeric() || c == '_');
            if is_boundary {
                if let Some(open) = src[after..].find('{') {
                    let body_start = after + open + 1;
                    let mut depth = 1usize;
                    let bytes = src.as_bytes();
                    let mut i = body_start;
                    while i < bytes.len() {
                        match bytes[i] {
                            b'{' => depth += 1,
                            b'}' => {
                                depth -= 1;
                                if depth == 0 {
                                    return Some(src[body_start..i].to_string());
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                }
            }
            from = start + needle.len();
        }
    }
    None
}

/// Pure engine: violations in ONE definition body. A token in the deny-set
/// (NAME or anywhere) OR a forbidden-selector TYPE present in the body is a
/// violation, UNLESS the offending token is a substring only of an allowlisted
/// `*_profile` field name. Returns human-readable failure strings.
fn struct_field_violations(
    type_name: &str,
    body: &str,
    deny_tokens: &[&str],
    forbidden_types: &[&str],
    allowlist: &[&str],
) -> Vec<String> {
    let mut out = Vec::new();
    // Strip allowlisted profile field names so a deny-token that is ONLY a
    // substring of an allowlisted field cannot false-trip. (None of the current
    // deny-tokens is such a substring, but the carve-out keeps the rule honest.)
    let mut scrubbed = body.to_string();
    for &allowed in allowlist {
        scrubbed = scrubbed.replace(allowed, "__allowlisted__");
    }
    for &tok in deny_tokens {
        if scrubbed.contains(tok) {
            out.push(format!(
                "target `{type_name}` carries a spec deny-token `{tok}` in its definition body"
            ));
        }
    }
    for &ty in forbidden_types {
        // A forbidden TYPE appears as `: <Type>` or `(<Type>` (tuple variant) or
        // `<<Type>` (generic arg). Match the bare type token; the deny-token
        // scan above already covers deny-NAMED types, so this adds the
        // type-position discrimination for a clean-named field.
        if scrubbed.contains(ty) {
            out.push(format!(
                "target `{type_name}` carries a forbidden spec-selector type `{ty}` in its definition body"
            ));
        }
    }
    out
}

// ===========================================================================
// §7.1 / §8 — correction-discharge (pure engine)
// ===========================================================================

/// A corrected-query identity (§8): `(row_file, row_function, query_ordinal,
/// snapshot_id)`.
type CorrectionKey = (String, String, u16, String);

/// One correction's live-discharge facts: whether `resolver(query) ==
/// correct_value`, and whether the snapshot's `oracle_value` DIFFERS from it
/// (the recorded TS bug). A correction is discharged ONLY when BOTH hold.
struct DischargeFact {
    key: CorrectionKey,
    resolver_equals_correct: bool,
    snapshot_differs: bool,
}

/// Pure engine: the failures of the §8 discharge invariant. (i) the on-disk
/// correction set must SET-EQUAL the registry-derived corrected-query set; (ii)
/// every correction must satisfy `resolver == correct_value` AND `snapshot
/// differs`. Empty inputs ⇒ no failures (the vacuous EMPTY-SET case here).
fn correction_discharge_failures(
    on_disk: &BTreeSet<CorrectionKey>,
    registry: &BTreeSet<CorrectionKey>,
    discharge: &[DischargeFact],
) -> Vec<String> {
    let mut out = Vec::new();
    for orphan in on_disk.difference(registry) {
        out.push(format!(
            "on-disk correction {orphan:?} has no corresponding corrected query in the registry"
        ));
    }
    for missing in registry.difference(on_disk) {
        out.push(format!(
            "corrected query {missing:?} has no on-disk correction artifact"
        ));
    }
    for fact in discharge {
        if !fact.resolver_equals_correct {
            out.push(format!(
                "correction {:?}: resolver(query) does not equal correct_value (engine did not produce the corrected answer)",
                fact.key
            ));
        }
        if !fact.snapshot_differs {
            out.push(format!(
                "correction {:?}: snapshot oracle_value does not differ from correct_value (not a recorded TS bug)",
                fact.key
            ));
        }
    }
    out
}

/// The on-disk correction root (`oracle_corrections/`), sibling to the snapshot
/// and env trees. It does not exist while the correction set is empty; the
/// enumeration treats an absent dir as the empty set.
const ORACLE_CORRECTIONS_INFIX: &str =
    "crates/verter_session/src/typeinfo/typeinfo_tests/oracle_corrections";

/// Enumerate the on-disk correction artifacts as their `CorrectionKey`s. An
/// absent / empty directory is the empty set (the current state — zero
/// corrections). Each correction file is `<...>.json`; until the
/// `DivergenceCorrection` machinery lands (deferred) there are none, so this
/// returns empty and the only real-world key source is absent.
fn enumerate_on_disk_corrections() -> BTreeSet<CorrectionKey> {
    let dir = workspace_root().join(ORACLE_CORRECTIONS_INFIX);
    let mut out = BTreeSet::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return out; // absent dir ⇒ empty correction set
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            // A real correction artifact would carry the key fields; none exist
            // yet, so reaching here means a stray file was added — surface it as
            // a degenerate key so the set-equality guard fails loudly.
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            out.insert((stem, String::new(), 0, String::new()));
        }
    }
    out
}

/// The registry-derived corrected-query set. A query is "corrected" only when a
/// `DivergenceCorrection` overlay declares it; that machinery is deferred and no
/// `QuerySpec` carries a correction marker, so the corrected-query set is EMPTY
/// (correct == tsgo for every lifted row in this block).
fn registry_corrected_queries() -> BTreeSet<CorrectionKey> {
    BTreeSet::new()
}

// ===========================================================================
// Real guards
// ===========================================================================

#[test]
fn resolver_is_single_spec() {
    // (a) EXACT-token absence scan over production resolver / cache / session
    //     source (oracle-harness / correction-metadata / test files exempt).
    let mut a_violations: Vec<String> = Vec::new();
    for path in production_rs_files() {
        let rel = rel_to_root(&path);
        if is_token_scan_exempt(&rel) {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for (lineno, tok) in scan_source_for_deny_tokens(&src) {
            a_violations.push(format!("{rel}:{lineno}: spec deny-token `{tok}`"));
        }
    }
    assert!(
        a_violations.is_empty(),
        "resolver_is_single_spec (a): production source names a spec deny-token \
         (a second resolution spec). The resolver / cache / session surface must \
         be single-spec — only oracle-harness / correction-metadata code may name \
         these tokens.\n  {}",
        a_violations.join("\n  "),
    );

    // (b) STRUCTURAL field-inventory over the closed cache-key / context target
    //     list: no field NAME in the deny-set, no field TYPE in the forbidden
    //     selector-type set (allowlisting the legit `*_profile` fields).
    let mut b_violations: Vec<String> = Vec::new();
    for (type_name, rel) in TARGET_STRUCTS {
        let src = fs::read_to_string(workspace_root().join(rel))
            .unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let body = extract_definition_body(&src, type_name).unwrap_or_else(|| {
            panic!(
                "resolver_is_single_spec (b): target `{type_name}` definition not \
                 found in {rel} — the closed target list is stale"
            )
        });
        b_violations.extend(struct_field_violations(
            type_name,
            &body,
            DENY_TOKENS,
            FORBIDDEN_SELECTOR_TYPES,
            PROFILE_ALLOWLIST,
        ));
    }
    assert!(
        b_violations.is_empty(),
        "resolver_is_single_spec (b): a cache-key / context target gained a \
         spec-selector field (name in the deny-set, or type in the forbidden \
         selector-type set).\n  {}",
        b_violations.join("\n  "),
    );
}

#[test]
fn every_correction_is_discharged() {
    let on_disk = enumerate_on_disk_corrections();
    let registry = registry_corrected_queries();
    // No DivergenceCorrection machinery in this block ⇒ no live corrected
    // queries ⇒ no discharge facts. The set-equality of the two EMPTY sets is
    // the vacuous-but-real invariant. This guard becomes NON-vacuous the moment
    // a correction overlay lands (a populated `on_disk`/`registry`); the
    // `correction_discharge_failures` engine it drives is already proven
    // discriminating against synthetic inputs below.
    let discharge: Vec<DischargeFact> = Vec::new();
    let failures = correction_discharge_failures(&on_disk, &registry, &discharge);
    assert!(
        failures.is_empty(),
        "every_correction_is_discharged: the on-disk correction set must \
         set-equal the registry-derived corrected-query set, and every \
         correction must satisfy resolver(query) == correct_value while the \
         snapshot oracle_value differs.\n  {}",
        failures.join("\n  "),
    );
    // Belt-and-suspenders: in this block both sets are genuinely empty.
    assert!(on_disk.is_empty() && registry.is_empty());
}

// ===========================================================================
// Discrimination tests — prove each engine is genuinely discriminating, with
// SYNTHETIC inputs (no planted violation is left in the tree).
// ===========================================================================

#[test]
fn single_spec_token_scan_flags_a_planted_deny_token() {
    // A reducer/cache/session source that introduces a deny-token — including as
    // an enum variant name — must be flagged.
    let planted = "enum SemanticMode { Correct, TsCompat }\n";
    let hits = scan_source_for_deny_tokens(planted);
    assert!(
        hits.iter().any(|(_, t)| *t == "TsCompat"),
        "the token scan must flag a `TsCompat` enum variant; got {hits:?}",
    );
    let planted_field = "struct InstantiateContext { compat_profile: u8 }\n";
    let hits2 = scan_source_for_deny_tokens(planted_field);
    assert!(
        hits2.iter().any(|(_, t)| *t == "compat_profile"),
        "the token scan must flag a `compat_profile` field; got {hits2:?}",
    );
    // Clean source — including the legitimate non-spec profiles — is NOT flagged.
    let clean = "struct InstantiateContext { query_profile: QueryProfile, compile_profile: u8 }\n";
    assert!(
        scan_source_for_deny_tokens(clean).is_empty(),
        "the legitimate `query_profile` / `compile_profile` fields must not trip the scan",
    );
}

#[test]
fn single_spec_field_inventory_flags_a_planted_selector_field() {
    // A deny-NAMED field on a target struct is flagged.
    let denied_name = "{ owner: u8, compat_profile: u8, mode: u8 }";
    let v1 = struct_field_violations(
        "InstantiateContext",
        denied_name,
        DENY_TOKENS,
        FORBIDDEN_SELECTOR_TYPES,
        PROFILE_ALLOWLIST,
    );
    assert!(
        !v1.is_empty(),
        "a `compat_profile` field on a target struct must be flagged by (b)",
    );

    // A deny-TYPED field whose NAME is clean is flagged by the forbidden-type
    // branch (the discrimination (b) adds over (a)).
    let denied_type = "{ owner: u8, posture: TsCompat, mode: u8 }";
    let v2 = struct_field_violations(
        "InstantiateContext",
        denied_type,
        // Use a deny-token-free name set to isolate the TYPE branch: only the
        // forbidden-TYPE check should fire here.
        &[],
        FORBIDDEN_SELECTOR_TYPES,
        PROFILE_ALLOWLIST,
    );
    assert!(
        v2.iter()
            .any(|m| m.contains("forbidden spec-selector type")),
        "a field TYPED `TsCompat` must be flagged by the forbidden-type branch; got {v2:?}",
    );

    // A clean body carrying the allowlisted `query_profile` is NOT flagged.
    let clean = "{ owner: u8, query_profile: QueryProfile, mode: u8 }";
    let v3 = struct_field_violations(
        "InstantiateContext",
        clean,
        DENY_TOKENS,
        FORBIDDEN_SELECTOR_TYPES,
        PROFILE_ALLOWLIST,
    );
    assert!(
        v3.is_empty(),
        "a clean target body with an allowlisted `query_profile` must not be flagged; got {v3:?}",
    );
}

#[test]
fn correction_discharge_engine_is_discriminating() {
    let key: CorrectionKey = (
        "index_signatures.rs".to_string(),
        "some_corrected_row".to_string(),
        0,
        "u_synthetic".to_string(),
    );

    // (1) A stray on-disk correction with no corresponding corrected query FAILS
    //     set-equality.
    let on_disk: BTreeSet<CorrectionKey> = [key.clone()].into_iter().collect();
    let registry: BTreeSet<CorrectionKey> = BTreeSet::new();
    let f1 = correction_discharge_failures(&on_disk, &registry, &[]);
    assert!(
        f1.iter()
            .any(|m| m.contains("no corresponding corrected query")),
        "a stray on-disk correction must FAIL set-equality; got {f1:?}",
    );

    // (2) A corrected query with no on-disk artifact FAILS set-equality.
    let f2 = correction_discharge_failures(&BTreeSet::new(), &on_disk, &[]);
    assert!(
        f2.iter()
            .any(|m| m.contains("no on-disk correction artifact")),
        "a corrected query with no artifact must FAIL set-equality; got {f2:?}",
    );

    // (3) A correction whose resolver != correct_value FAILS the discharge.
    let bad = DischargeFact {
        key: key.clone(),
        resolver_equals_correct: false,
        snapshot_differs: true,
    };
    let matched: BTreeSet<CorrectionKey> = [key.clone()].into_iter().collect();
    let f3 = correction_discharge_failures(&matched, &matched, std::slice::from_ref(&bad));
    assert!(
        f3.iter()
            .any(|m| m.contains("does not equal correct_value")),
        "a correction whose resolver != correct_value must FAIL; got {f3:?}",
    );

    // (4) A correction whose snapshot does NOT differ (no recorded bug) FAILS.
    let no_bug = DischargeFact {
        key: key.clone(),
        resolver_equals_correct: true,
        snapshot_differs: false,
    };
    let f4 = correction_discharge_failures(&matched, &matched, std::slice::from_ref(&no_bug));
    assert!(
        f4.iter().any(|m| m.contains("does not differ")),
        "a correction whose snapshot does not differ must FAIL; got {f4:?}",
    );

    // (5) A fully-discharged correction (matched sets + resolver == correct +
    //     snapshot differs) PASSES.
    let good = DischargeFact {
        key,
        resolver_equals_correct: true,
        snapshot_differs: true,
    };
    let f5 = correction_discharge_failures(&matched, &matched, std::slice::from_ref(&good));
    assert!(
        f5.is_empty(),
        "a fully-discharged correction must PASS; got {f5:?}",
    );
}
