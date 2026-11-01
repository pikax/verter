//! Formatter foundation: typed byte coordinate domains and provenance
//! identity.
//!
//! This crate owns the two disjoint private UTF-8 byte coordinate domains the
//! formatter works in — the complete authored carrier source and the rendered
//! formatter output — plus the provenance identity, carrier and table contract
//! that joins authored views to rendered segments. These are formatter-internal
//! domains, not a public capability: nothing here crosses a serialization
//! boundary. Its only external dependency is a regular-expression engine for
//! pinned text-width tables; it cannot parse carrier source, analyze semantics,
//! route or serialize a request.
//!
//! The coordinate and provenance wrappers implement no parser, printer, map,
//! range/cursor, route or protocol policy; the modules below are reserved
//! homes for that later formatter work. See
//! `docs/arch/kernel/formatter.md` for the owning architecture contract.

pub mod coordinates;
pub mod provenance;

#[cfg(test)]
mod coordinates_tests;
#[cfg(test)]
mod provenance_tests;

// Document construction and rendering are framework-neutral. Other modules
// remain reserved for their respective owners.
mod cursor;
pub mod doc;
pub mod edit;
mod embedded;
mod explain;
mod position_map;
mod pragma;
mod printers;
mod range;
mod recovery;
pub mod render;
mod service;
mod suppression;
mod view;

#[cfg(test)]
#[path = "../tests/unit/provenance_render.rs"]
mod provenance_render_tests;

#[cfg(test)]
#[path = "../tests/unit/edit_geometry.rs"]
mod edit_geometry_tests;

mod width;
