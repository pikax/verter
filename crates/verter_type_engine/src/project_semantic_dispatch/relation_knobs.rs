//! Relation evaluation knobs a host attaches to the engine.

/// Per-host relation-engine knobs, grouped off the `VerterHost` struct body.
///
/// - `force_budget_exhaustion`: trips the relation reducer's work budget on
///   its first driver pass — the deterministic trigger for the typed
///   `BudgetExceeded` public outcome and its three-layer non-admission (no
///   warm memo entry, no fact signature, no reverse-index registration).
///
/// The strict-family configuration is NOT a knob: the relation reducer reads
/// it from the effective tsconfig options of the project owning the
/// request's canonical ([`VerterHost::semantic_compiler_options_for`]), the
/// same option set that project's `type_env_hash` folds.
#[derive(Debug, Default)]
pub struct RelationHostKnobs {
    pub force_budget_exhaustion: std::sync::atomic::AtomicBool,
}
