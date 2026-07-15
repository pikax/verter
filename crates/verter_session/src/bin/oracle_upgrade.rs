//! The deterministic, TSGO-FREE v2→v3 oracle snapshot schema upgrade
//! (`docs/arch/u0-oracle-harness-design.md` §Q4).
//!
//! Run with `cargo run -p verter_session --features oracle-gen --bin oracle_upgrade`.
//! The `[[bin]]` entry declares `required-features = ["oracle-gen"]`, so a default
//! `cargo build` / `cargo clippy` SKIPS it — the default closure stays tsgo-free.
//!
//! v3 ADDS only the tsgo-free migration-fidelity mirror
//! (`migration_fingerprint_version` + `migration_fingerprint`) and bumps
//! `oracle_schema_version` 2→3; the tsgo-derived content is UNCHANGED, so this
//! upgrade is a pure, deterministic transform (NEVER drives tsgo): it injects each
//! row's retained `LIFTED_ROW_MIGRATIONS` fingerprint, recomputes the changed
//! `snapshot_id`, writes the v3 file, and deletes the stale v2 one. Re-running is
//! byte-idempotent.

fn main() {
    match verter_session::upgrade_snapshots_to_v3() {
        Ok((written, deleted)) => {
            eprintln!(
                "oracle_upgrade: wrote {written} v3 snapshot(s), removed {deleted} stale file(s)"
            );
        }
        Err(e) => {
            eprintln!("oracle_upgrade: FAILED — {e:?}");
            std::process::exit(1);
        }
    }
}
