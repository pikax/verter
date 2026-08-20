//! Dependency-neutral typed-identity, profile, and result-contract
//! vocabulary.
//!
//! Distinct identity newtypes, tagged length-delimited canonical encoding,
//! profile/policy IDs, and the query-identity / flight-key / result-contract
//! types a semantic query boundary composes from. Every type is a distinct
//! Rust nominal type — never a `type Alias = OtherType;` — so passing a
//! [`identity::SessionHandle`] where a [`identity::StableEntityId`] is
//! required is a compile error. See [`identity`] for the compile-fail proof.
//!
//! This crate is not a service: it runs no query, owns no cache, and does
//! not host `FlightCell`/`FlightState` (`result-contract-and-flight.md` §3;
//! that belongs to `QueryRuntime`). It does not redeclare types owned
//! elsewhere (see [`profile::ExecutionPolicy`]'s generic cancellation
//! parameter) and does not thread these IDs into
//! `HostConfig`/`CompileProfile`/`CodegenOptions`.
//!
//! Layer 1 in the workspace dependency matrix
//! (`tests/cases/workspace_dependency_layers.rs`). Zero `verter_*`
//! production dependencies, including `verter_span`.

#![forbid(unsafe_code)]

#[macro_use]
mod macros;

pub mod canonical;
pub mod encoding;
pub mod identity;
pub mod mapping;
pub mod profile;
