//! The TS7 `TypeExpr`-projection oracle SNAPSHOT GENERATOR binary
//! (`docs/arch/u0-oracle-harness-design.md` §4 generator-side table — "generators
//! are scripts, not tests").
//!
//! Run with `cargo run -p verter_session --features oracle-gen --bin oracle_gen`
//! (wrapped by the `pnpm` script). The `[[bin]]` entry declares
//! `required-features = ["oracle-gen"]`, so a default `cargo build` / `cargo clippy`
//! SKIPS it entirely — the default closure stays tsgo-free
//! (`oracle_tsgo_forbidden::tsgo_not_reachable_from_resolver`).
//!
//! It drives the pinned tsgo, applies the two-sided positive-allowlist admission,
//! and writes the checked-in snapshots — NEVER from a `#[test]`. It walks the
//! oracle-query registry (the 8 lifted rows) and writes one snapshot per spec;
//! the per-spec body is the same one the `oracle_gen_is_idempotent` gated test
//! exercises against real tsgo.

fn main() {
    match verter_session::run_oracle_gen() {
        Ok(written) => {
            eprintln!("oracle_gen: wrote {written} snapshot(s)");
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
