//! Single consolidated integration-test binary for `verter_source_policy_gate`.
//!
//! Six of the eight guards here derive their verdict purely from reading the
//! workspace source tree (via `syn`/`walkdir`/`git ls-files`). The other two
//! (`tracked_paths_are_portable`, `output_projector_residual_guards`) build
//! against `verter_session`'s public API to check a generated/typed surface,
//! but do not RUN it — no `VerterHost`, no compiled request, no shared
//! process state. None is sensitive to `debug_assertions` or to
//! shared-process leakage between tests, which is why these guards live in
//! their own crate instead of `verter_session`'s consolidated `tests/main.rs`:
//! a plain `cargo nextest run --workspace` (Surface 1) still runs them once,
//! but Surface 2 (verter_session-only shared-process) and Surface 3
//! (shipped-cfg, package-filtered) select by PACKAGE, so neither can see
//! this crate's tests regardless of what it depends on — they are no longer
//! replayed under either surface for no behavioral reason.

mod cases;
