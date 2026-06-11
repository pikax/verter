//! Framework-neutral OXC script analysis.
//!
//! Owns the script-analysis substrate that is independent of any UI
//! framework: the raw pre-lowering statement surface ([`raw_surface`]),
//! the local type-surface capture engine ([`type_surface`]), and the
//! generic import/declaration binding inventory ([`bindings`]).
//!
//! Everything here is LOCAL OXC-to-owned surface capture over a single
//! parsed program — no host-backed query resolution, no cross-file
//! semantic engine. Framework-specific script semantics (Vue macros,
//! `<script setup>` classification, options API analysis) live under
//! [`super::vue::script`] and delegate to this module for the neutral
//! parts.

pub mod bindings;
pub mod raw_surface;
pub mod type_surface;

#[cfg(test)]
#[path = "type_surface_typed_form_tests.rs"]
mod type_surface_typed_form_tests;
