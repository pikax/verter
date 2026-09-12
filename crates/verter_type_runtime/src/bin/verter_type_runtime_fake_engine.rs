//! TEST-ONLY fake tsgo engine shim for the `verter_type_runtime` test lane —
//! NOT a shipped binary (feature-gated via `required-features`; a default
//! `cargo build -p verter_type_runtime` never produces it). All logic lives in
//! [`verter_tsgo_api::fake_engine`]; this shim exists so this crate's
//! integration tests get a `CARGO_BIN_EXE_` path to the same engine, which is
//! what lets the owned-attach gate test drive a deterministic `--version`
//! answer without a real tsgo install.

fn main() {
    verter_tsgo_api::fake_engine::main();
}
