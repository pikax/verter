//! Formatter foundation: typed byte coordinate domains and provenance
//! identity.
//!
//! This crate owns the two disjoint private UTF-8 byte coordinate domains the
//! formatter works in — the complete authored carrier source and the rendered
//! formatter output — plus the provenance identity, carrier and table contract
//! that joins authored views to rendered segments. These are formatter-internal
//! domains, not a public capability: nothing here crosses a serialization
//! boundary. The document algebra ([`doc`]) and its bounded renderer
//! ([`render`]) are framework-neutral; the crate's only external dependency is
//! a regular-expression engine for the pinned text-width tables, so nothing
//! here can parse carrier source, analyze semantics, route or serialize a
//! request.
//!
//! The coordinate, provenance, document and renderer modules implement no
//! parser, printer, edit, map, range/cursor, route or protocol policy; the
//! private modules below are reserved homes for that later formatter work. See
//! `docs/arch/kernel/formatter.md` for the owning architecture contract.

pub mod coordinates;
pub mod doc;
pub mod provenance;
pub mod render;
mod width;

#[cfg(test)]
mod coordinates_tests;
#[cfg(test)]
mod provenance_tests;

// Reserved homes for the planned formatter modules. Each module below is
// declaration-only: its doc line names the work that fills it, and each
// owning implementation writes inside its own reserved module only.
mod cursor;
mod edit;
mod embedded;
mod explain;
mod position_map;
mod pragma;
mod printers;
mod range;
mod recovery;
mod service;
mod suppression;
mod view;
