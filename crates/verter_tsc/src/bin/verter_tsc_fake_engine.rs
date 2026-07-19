//! TEST-ONLY fake tsgo engine shim for the `verter_tsc` test lane — NOT a
//! shipped binary (feature-gated via `required-features`; a default
//! `cargo build -p verter_tsc` never produces it). All logic lives in
//! [`verter_tsgo_api::fake_engine`]; this shim exists so `verter_tsc`'s
//! integration tests get a `CARGO_BIN_EXE_` path to the same engine.

fn main() {
    verter_tsgo_api::fake_engine::main();
}
