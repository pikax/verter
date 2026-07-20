//! Framework-neutral OXC script analysis.
//!
//! Owns the script-analysis substrate that is independent of any UI
//! framework: the raw pre-lowering statement surface ([`raw_surface`]),
//! the syntax-only routing inventory ([`route_inventory`]), and the generic
//! import/declaration binding inventory ([`bindings`]). Declaration headers
//! and dependency projection are semantic-layer responsibilities.
//!
//! Everything here is LOCAL OXC-to-owned surface capture over a single
//! parsed program — no host-backed query resolution, no cross-file
//! semantic engine. Framework-specific script semantics (Vue macros,
//! `<script setup>` classification, options API analysis) live under
//! [`super::vue::script`] and delegate to this module for the neutral
//! parts.

pub mod bindings;
pub mod raw_surface;
pub mod route_inventory;

#[cfg(test)]
#[path = "route_inventory_tests.rs"]
mod route_inventory_tests;
