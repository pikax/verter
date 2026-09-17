//! The TS7 `TypeExpr`-projection oracle SNAPSHOT GENERATOR binary
//! (the TS7 oracle contract §4 generator-side table — "generators
//! are scripts, not tests").
//!
//! Run with `cargo run -p verter_session --features oracle-gen --bin oracle_gen`
//! (wrapped by the `pnpm` script). The `[[bin]]` entry declares
//! `required-features = ["oracle-gen"]`, so a default `cargo build` / `cargo clippy`
//! SKIPS it entirely — the default closure stays tsgo-free
//! (`oracle_tsgo_forbidden::tsgo_not_reachable_from_resolver`).
//!
//! It drives the pinned engine, applies the two-sided positive-allowlist
//! admission, writes one snapshot per registry spec, and removes every
//! snapshot file the run did not write (a pinned-env bump re-keys every
//! `snapshot_id`, so the superseded files would otherwise linger as orphans) —
//! NEVER from a `#[test]`. The per-spec body is the same one the
//! `oracle_gen_is_idempotent` gated test exercises against the real engine.

fn main() {
    match verter_session::run_oracle_gen() {
        Ok((written, deleted)) => {
            eprintln!("oracle_gen: wrote {written} snapshot(s), removed {deleted} stale file(s)");
        }
        Err(verter_session::GenError::TsgoUnavailable(msg)) => {
            // A tsgo-less environment is a SKIP, not a failure (no tsgo to drive).
            eprintln!("oracle_gen: SKIP — tsgo not available: {msg}");
        }
        Err(e) => {
            eprintln!("oracle_gen: FAILED — {e:?}");
            std::process::exit(1);
        }
    }
}
