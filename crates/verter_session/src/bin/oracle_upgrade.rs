//! The deterministic, TSGO-FREE v3→v4 oracle snapshot re-keying
//! (`docs/arch/refactor/rev11/charters/expansion-native-checker/NCK4.md` §Q4 +
//! `docs/arch/refactor/rev11/charters/expansion-native-checker/NCK4.md`).
//!
//! Run with `cargo run -p verter_session --features oracle-gen --bin oracle_upgrade`.
//! The `[[bin]]` entry declares `required-features = ["oracle-gen"]`, so a default
//! `cargo build` / `cargo clippy` SKIPS it — the default closure stays tsgo-free.
//!
//! v4 ADDS the closed `relation_verdict` value kind; for the existing
//! `structured_type_expr` snapshots the ONLY change is `oracle_schema_version`
//! 3→4 flowing into `snapshot_id` through `PinnedEnv`. The tsgo-derived content
//! is UNCHANGED, so this upgrade is a pure, deterministic transform (NEVER
//! drives tsgo): it bumps the stored version, recomputes the changed
//! `snapshot_id`, writes the v4 file, and deletes the stale v3 one. Re-running
//! is byte-idempotent; `relation_verdict` files (generated fresh at v4) are
//! skipped.

fn main() {
    match verter_session::upgrade_snapshots_to_v4() {
        Ok((written, deleted)) => {
            eprintln!(
                "oracle_upgrade: wrote {written} v4 snapshot(s), removed {deleted} stale file(s)"
            );
        }
        Err(e) => {
            eprintln!("oracle_upgrade: FAILED — {e:?}");
            std::process::exit(1);
        }
    }
}
