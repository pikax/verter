//! The long-churn soak runner: a hash-frozen editor workload driven through
//! the public host routes, sampled for retention with capture off, and
//! judged from the summary it writes.
//!
//! - [`manifest`]: the deterministic 100-action tranche template and the
//!   manifest (smoke: one tranche; full: 100 tranches, 10,000 actions);
//! - [`fixtures`]: the project's source texts and configurations;
//! - [`isolation`]: the component-meta batch isolation workload;
//! - [`gates`]: the LSP publication-gate lifecycle workload;
//! - [`refusal`]: the retained budget-refusal and larger-profile healing workload;
//! - [`controls`]: the negative controls, each a planted defect that must fail the run;
//! - [`run`]: execution, sampling, planted [`run::Mutation`]s, the summary;
//! - [`verify`]: the independent judgement of a written summary.
//!
//! The binary `verter-soak` runs and verifies one mode. Time and process
//! memory are reported, never judged here: every judged figure is a
//! deterministic count or retention-account byte figure.

pub mod controls;
pub mod fixtures;
pub mod gates;
pub mod isolation;
pub mod manifest;
pub mod refusal;
pub mod run;
pub mod verify;
