//! `verter_identity` — the dependency-neutral typed-identity, profile, and
//! result-contract vocabulary beneath every owner crate in the workspace.
//!
//! # What this crate is
//!
//! This crate lands non-interchangeable identity types, canonical
//! encoding, and profile/result-contract schema types: distinct identity
//! newtypes, a tagged length-delimited canonical byte encoding, six
//! classes of profile/policy identity, and the query-identity/flight-key/
//! result-contract vocabulary a semantic query boundary composes from.
//! Every type here is a distinct
//! Rust nominal type — never a `type Alias = OtherType;` — so misuse (using
//! a [`identity::SessionHandle`] where a [`identity::StableEntityId`] is
//! required, or vice versa) is a compile error, not a review obligation. See
//! [`identity`] for the compile-fail proof.
//!
//! # What this crate is NOT
//!
//! - **Not a service.** It holds types and the canonical-encoding
//!   primitive; it runs no query, resolves no import, owns no cache, and
//!   spawns nothing. `FlightCell`/`FlightState` (the flight-runtime
//!   ownership machinery `result-contract-and-flight.md` §3 describes) is
//!   explicitly out of scope — that is `QueryRuntime`'s job
//!   (`architecture.md` §18.2), a later block's.
//! - **Not a second owner for an existing concept.** Where a concept
//!   already has a current owner (e.g. `CancellationToken` in
//!   `verter_scheduler::cancellation`), this crate does not redeclare it —
//!   see [`profile::ExecutionPolicy`]'s generic cancellation parameter.
//! - **Not a behavior migration.** It does not thread these types into
//!   `HostConfig`/`CompileProfile`/`CodegenOptions` or otherwise change what
//!   any existing code path computes. See the module docs on [`profile`].
//!
//! # Dependency position
//!
//! Layer 1 (identity/span/language/contracts) in the workspace dependency
//! matrix — see
//! `tests/cases/workspace_dependency_layers.rs`. Zero `verter_*` production
//! dependencies, including `verter_span`: a span-carrying identity is a
//! decision for whichever type owns that span-relative concept, not for
//! this neutral vocabulary crate.

#![forbid(unsafe_code)]

#[macro_use]
mod macros;

pub mod canonical;
pub mod encoding;
pub mod identity;
pub mod mapping;
pub mod profile;
