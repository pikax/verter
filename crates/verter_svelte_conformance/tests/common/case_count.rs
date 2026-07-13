//! The committed conformance corpus size — ONE fixture per manifest case,
//! two backend goldens each. The SINGLE test-side pin every conformance gate
//! asserts against (inventory bijection, differential compile count, golden
//! oracle-pin scan, coverage-index case count), so a manifest resize moves
//! every gate in lockstep through exactly this constant.
//!
//! The pin is a deliberate LITERAL, not `manifest().cases().len()`: the
//! inventory gates compare the LIVE manifest against this committed value —
//! a self-referential read would turn that comparison into a tautology.

/// One fixture per manifest case; two backend goldens each.
pub const CASE_COUNT: usize = 609;
