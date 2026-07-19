//! TEST-ONLY fake tsgo engine shim — NOT a shipped binary.
//!
//! This target is feature-gated (`required-features = ["test-fake-engine"]` in
//! `Cargo.toml`, with `autobins = false`), so a default `cargo build` / release
//! does NOT produce it; the crate's test suite enables the feature through the
//! crate's dev-dependency on itself. All logic lives in
//! [`verter_tsgo_api::fake_engine`] so consumer crates' test lanes can shim the
//! same engine.

fn main() {
    verter_tsgo_api::fake_engine::main();
}
