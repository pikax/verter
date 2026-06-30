//! Discriminating tests for the split carrier cache (§2.7).
//!
//! The two dependency-change and map-hash discriminators are the load-bearing ones:
//! - `dependency_change_rechecks_unchanged_carrier` — a byte-identical carrier
//!   (regen skipped) whose DEPENDENCY changed STILL notifies the engine. This is
//!   the test that FAILS against a self-content-only cache (one that gated the
//!   re-check on carrier-text stability).
//! - `map_hash_change_invalidates_mapped_results` — a `map_hash` change drops
//!   cached mapped results .

use super::*;

fn h(byte: u8) -> Hash16 {
    [byte; 16]
}

fn regen_key(source: u8, profile: u64) -> RegenKey {
    RegenKey {
        source_content_hash: h(source),
        parse_env_hash: h(0x10),
        compile_profile_hash: profile,
        file_language_row_hash: h(0x20),
        helper_runtime_version: 1,
    }
}

// ───────────────────────── (a) regeneration skip ─────────────────────────

#[test]
fn identical_regen_key_skips_recodegen() {
    // Unchanged self-content env dims ⇒ carrier text is byte-stable ⇒ reuse the
    // cached carrier, no re-codegen (the beat-vize lever).
    let a = regen_key(0xAA, 7);
    let b = regen_key(0xAA, 7);
    assert!(
        RegenKey::carrier_regeneration_is_fresh(&a, &b),
        "an unchanged self-content key must skip re-codegen"
    );
}

#[test]
fn source_content_change_forces_recodegen() {
    let a = regen_key(0xAA, 7);
    let b = regen_key(0xBB, 7); // source bytes changed
    assert!(
        !RegenKey::carrier_regeneration_is_fresh(&a, &b),
        "a source-content change must force re-codegen"
    );
}

#[test]
fn compile_profile_change_forces_recodegen() {
    // A distinct compile_profile is a distinct carrier slot: the SAME source under a
    // different profile is a DIFFERENT carrier text and must not be served from the
    // peer slot.
    let profile_a = regen_key(0xAA, 1);
    let profile_b = regen_key(0xAA, 2);
    assert!(
        !RegenKey::carrier_regeneration_is_fresh(&profile_a, &profile_b),
        "a distinct compile profile must key a distinct carrier slot"
    );
}

#[test]
fn each_env_dimension_is_independently_keyed() {
    // R21: every orthogonal dimension participates; flipping any one alone busts
    // the key. (No single bundled hash that could mask a single-dim change.)
    let base = regen_key(0xAA, 7);

    let parse = RegenKey {
        parse_env_hash: h(0x99),
        ..base
    };
    let lang = RegenKey {
        file_language_row_hash: h(0x99),
        ..base
    };
    let runtime = RegenKey {
        helper_runtime_version: 999,
        ..base
    };

    assert!(!RegenKey::carrier_regeneration_is_fresh(&base, &parse));
    assert!(!RegenKey::carrier_regeneration_is_fresh(&base, &lang));
    assert!(!RegenKey::carrier_regeneration_is_fresh(&base, &runtime));
}

#[test]
fn fold_regen_key_is_deterministic_and_collision_resistant_across_dims() {
    let a = regen_key(0xAA, 7);
    assert_eq!(fold_regen_key(&a), fold_regen_key(&a), "deterministic");

    // A single-dim flip changes the folded digest (no accidental collision via
    // XOR-style folding).
    let b = RegenKey {
        parse_env_hash: h(0x11),
        ..a
    };
    assert_ne!(fold_regen_key(&a), fold_regen_key(&b));

    // Swapping which dim holds a value must NOT collide (concat-then-hash, not
    // XOR): put 0x33 in source vs in file-language row.
    let s = RegenKey {
        source_content_hash: h(0x33),
        file_language_row_hash: h(0x00),
        ..a
    };
    let l = RegenKey {
        source_content_hash: h(0x00),
        file_language_row_hash: h(0x33),
        ..a
    };
    assert_ne!(
        fold_regen_key(&s),
        fold_regen_key(&l),
        "concat-then-hash must not collide a swapped pair"
    );
}

// ─────────────────── (b) engine re-check / notification ───────────────────

/// THE dependency-change discriminator. A carrier whose TEXT is byte-identical (its `RegenKey`
/// is unchanged, so re-codegen is skipped) but whose DEPENDENCY closure
/// generation advanced (a dependency `.d.ts`-only edit) STILL notifies the
/// engine. A self-content-only cache that gated the re-check on carrier-text /
/// `RegenKey` stability would WRONGLY skip this — that is the false-green this
/// test is built to catch.
#[test]
fn dependency_change_rechecks_unchanged_carrier() {
    // The carrier text is byte-stable across the change: same RegenKey both times.
    let regen_before = regen_key(0xAA, 7);
    let regen_after = regen_key(0xAA, 7);
    assert!(
        RegenKey::carrier_regeneration_is_fresh(&regen_before, &regen_after),
        "precondition: the carrier text is byte-identical, so re-codegen IS skipped"
    );

    // But a dependency `.d.ts`-only change advanced the dependency-closure
    // generation (the dependent's resolved import surface is structurally the
    // same — same imports resolve to the same canonicals — so import_signature
    // is unchanged; ONLY the closure generation moved).
    let recheck_before = EngineRecheckState {
        import_signature_hash: h(0x55),
        closure_generation: 10,
        project_recheck_generation: 100,
    };
    let recheck_after = EngineRecheckState {
        import_signature_hash: h(0x55), // SAME resolved imports
        closure_generation: 11,         // dependency content transitioned
        project_recheck_generation: 100,
    };

    assert!(
        needs_engine_recheck(&recheck_before, &recheck_after),
        "a dependency .d.ts change (closure generation advanced) MUST re-check the \
         dependent even though its carrier text is byte-identical — this is the \
         decision a self-content-only cache gets WRONG"
    );

    // The negative control that PROVES the discriminator: a cache keyed ONLY on
    // self-content (carrier text / RegenKey) would conclude "nothing to do".
    // Express that wrong oracle and assert it disagrees with the correct one.
    let self_content_only_says_skip =
        RegenKey::carrier_regeneration_is_fresh(&regen_before, &regen_after);
    assert!(
        self_content_only_says_skip && needs_engine_recheck(&recheck_before, &recheck_after),
        "the self-content-only oracle says SKIP while the correct dependency-driven \
         oracle says RE-CHECK — the split cache is exactly what closes this gap"
    );
}

#[test]
fn import_signature_change_rechecks_unchanged_carrier() {
    // A structural import change (e.g. a `paths` remap that points the same
    // specifier at a different target) can leave the carrier text byte-identical
    // while the resolved import surface changed — re-check.
    let recheck_before = EngineRecheckState {
        import_signature_hash: h(0x55),
        closure_generation: 10,
        project_recheck_generation: 100,
    };
    let recheck_after = EngineRecheckState {
        import_signature_hash: h(0x66), // imports now resolve elsewhere
        closure_generation: 10,         // no content transition observed
        project_recheck_generation: 100,
    };
    assert!(
        needs_engine_recheck(&recheck_before, &recheck_after),
        "a resolved-import-surface change MUST re-check the dependent"
    );
}

#[test]
fn stable_dependency_closure_does_not_recheck() {
    // Nothing changed in the dependency closure ⇒ no spurious re-check (the win:
    // we forward EVERY real dependency bump but no phantom ones).
    let state = EngineRecheckState {
        import_signature_hash: h(0x55),
        closure_generation: 10,
        project_recheck_generation: 100,
    };
    assert!(
        !needs_engine_recheck(&state, &state),
        "an unchanged dependency closure must not trigger a re-check"
    );
}

#[test]
fn project_config_change_rechecks_unchanged_carrier_and_deps() {
    // §2.7(b) lists @types/lib/tsconfig/paths/package-upgrade as engine-recheck
    // triggers. These can change the diagnostic set with a byte-identical carrier,
    // unchanged imports, AND an unchanged dependency-CONTENT closure (the
    // dependency files did not transition; only the project config did). The
    // project_recheck_generation rail catches exactly this — a closure-generation-
    // only state (the gap the reviewers flagged) would WRONGLY skip it.
    let before = EngineRecheckState {
        import_signature_hash: h(0x55),
        closure_generation: 10,
        project_recheck_generation: 1, // initial tsconfig/lib env
    };
    let after = EngineRecheckState {
        import_signature_hash: h(0x55), // same resolved imports
        closure_generation: 10,         // no dependency file transitioned
        project_recheck_generation: 2,  // tsconfig/lib/paths changed
    };
    assert!(
        needs_engine_recheck(&before, &after),
        "a project-config (tsconfig/lib/paths) change MUST re-check even with a \
         byte-identical carrier, unchanged imports, and an unchanged dependency \
         content closure"
    );
}

#[test]
fn project_config_rollback_still_rechecks() {
    // A config ROLLBACK (the project generation goes DOWN, or back to a prior
    // value) can RE-INTRODUCE a diagnostic, so it must still re-check. The rail
    // compares for inequality (not strictly-greater) precisely to fail toward a
    // re-check on a rollback.
    let before = EngineRecheckState {
        import_signature_hash: h(0x55),
        closure_generation: 10,
        project_recheck_generation: 5,
    };
    let after = EngineRecheckState {
        import_signature_hash: h(0x55),
        closure_generation: 10,
        project_recheck_generation: 3, // rolled BACK
    };
    assert!(
        needs_engine_recheck(&before, &after),
        "a project-config rollback (project generation decreased) MUST still \
         re-check — a rollback can re-introduce a diagnostic"
    );
}

#[test]
fn project_recheck_generation_is_deterministic_and_sensitive() {
    // The fold is deterministic and moves when ANY of resolve/lib/identity/config
    // generation changes (concat-then-hash, R21 dims).
    let base = project_recheck_generation_from(h(0x01), h(0x02), h(0x03), 7);
    assert_eq!(
        base,
        project_recheck_generation_from(h(0x01), h(0x02), h(0x03), 7),
        "deterministic"
    );
    assert_ne!(
        base,
        project_recheck_generation_from(h(0x99), h(0x02), h(0x03), 7),
        "a resolve_env change moves the rail"
    );
    assert_ne!(
        base,
        project_recheck_generation_from(h(0x01), h(0x99), h(0x03), 7),
        "a lib_env change moves the rail"
    );
    assert_ne!(
        base,
        project_recheck_generation_from(h(0x01), h(0x02), h(0x99), 7),
        "a project_identity change moves the rail"
    );
    assert_ne!(
        base,
        project_recheck_generation_from(h(0x01), h(0x02), h(0x03), 8),
        "a tsconfig config-generation change moves the rail"
    );
}

#[test]
fn closure_generation_is_max_over_self_and_forward_deps() {
    // The closure generation is the MAX per-canonical transition generation over
    // self + the forward closure, so ANY dependency's content transition lifts it.
    let gen_of = |canonical: &str| -> u64 {
        match canonical {
            "/src/App.vue" => 3,
            "/src/types.ts" => 7, // a dependency transitioned most recently
            "/src/util.ts" => 5,
            _ => 0,
        }
    };
    let closure = ["/src/types.ts", "/src/util.ts"];
    let g = closure_generation_for("/src/App.vue", closure.iter().copied(), gen_of);
    assert_eq!(
        g, 7,
        "the dependency at gen 7 must dominate the closure generation"
    );

    // If the dependency transitions again (gen rises), the closure generation
    // rises with it — driving the dependency-change re-check.
    let gen_after = |canonical: &str| -> u64 {
        if canonical == "/src/types.ts" {
            8
        } else {
            gen_of(canonical)
        }
    };
    let g_after = closure_generation_for("/src/App.vue", closure.iter().copied(), gen_after);
    assert!(
        g_after > g,
        "a dependency transition must advance the closure generation"
    );
}

#[test]
fn import_signature_hash_is_order_sensitive_and_structural() {
    // Same resolved imports ⇒ same signature.
    let a = import_signature_hash([
        ("Foo", "/src/foo.ts", h(0x01)),
        ("Bar", "/src/bar.ts", h(0x02)),
    ]);
    let b = import_signature_hash([
        ("Foo", "/src/foo.ts", h(0x01)),
        ("Bar", "/src/bar.ts", h(0x02)),
    ]);
    assert_eq!(a, b, "identical resolved imports hash identically");

    // A target whose content identity changed ⇒ different signature (a dependency
    // whose public surface moved).
    let c = import_signature_hash([
        ("Foo", "/src/foo.ts", h(0x99)), // foo's content identity changed
        ("Bar", "/src/bar.ts", h(0x02)),
    ]);
    assert_ne!(
        a, c,
        "a target content-identity change moves the import signature"
    );

    // An added import ⇒ different signature.
    let d = import_signature_hash([
        ("Foo", "/src/foo.ts", h(0x01)),
        ("Bar", "/src/bar.ts", h(0x02)),
        ("Baz", "/src/baz.ts", h(0x03)),
    ]);
    assert_ne!(a, d, "an added import moves the import signature");
}

// ───────────────────── map-result caching  ─────────────────────

/// THE map-hash discriminator. A `map_hash` change invalidates cached MAPPED results.
#[test]
fn map_hash_change_invalidates_mapped_results() {
    let cached = h(0x42);
    let same = h(0x42);
    let changed = h(0x43);

    assert!(
        mapped_results_valid(cached, same),
        "an unchanged map_hash keeps mapped results"
    );
    assert!(
        !mapped_results_valid(cached, changed),
        "a map_hash change MUST invalidate every cached mapped result keyed by \
         the old map  — never remap a stale diagnostic through a new map"
    );
}
