//! Pure route-surface hashing over owned shallow input records.
//!
//! The digests here are content-free functions of an owned
//! [`ShallowInputAssembly`]: no artifact, store or project state enters them.

use std::hash::{Hash, Hasher};

use crate::analysis::types::Hash16;
use crate::inputs::shallow::ShallowInputAssembly;

/// The legacy route-surface digest of a shallow input record that exposes a
/// resolvable surface: its syntactic routing interface folded with its
/// whole-content hash.
pub fn hash_route_surface_inputs(state: &ShallowInputAssembly) -> Hash16 {
    hash_route_surface_from_syntactic(state.whole_hash, syntactic_route_interface_hash(state))
}

/// Fold a syntactic routing-interface digest with a whole-content hash into
/// the legacy route-surface digest.
pub fn hash_route_surface_from_syntactic(whole_hash: Hash16, syntactic: Hash16) -> Hash16 {
    hash16_from_sorted(|hasher| {
        b"verter:legacy-route-surface:v2".hash(hasher);
        syntactic.hash(hasher);
        whole_hash.hash(hasher);
    })
}

/// Digest the exact authored import/export routing interface of a shallow
/// input record, excluding unrelated file content and all resolved/project
/// state.
pub fn syntactic_route_interface_hash(state: &ShallowInputAssembly) -> Hash16 {
    hash16_from_sorted(|hasher| {
        b"verter:syntactic-route-interface:v2".hash(hasher);
        // The effective shallow export table includes framework-synthesised
        // defaults and its collision policy. Hash it in name order so the
        // fact covers the exact surface consumers traverse without retaining
        // insertion-order noise.
        let mut exports: Vec<(&str, &crate::inputs::shallow::ExportTarget)> = state
            .exports
            .iter()
            .map(|(name, target)| (name.as_str(), target))
            .collect();
        exports.sort_unstable_by_key(|(name, _)| *name);
        for (name, target) in &exports {
            name.hash(hasher);
            match target {
                crate::inputs::shallow::ExportTarget::Local { owner, symbol_name } => {
                    0u8.hash(hasher);
                    owner.hash(hasher);
                    symbol_name.hash(hasher);
                }
                crate::inputs::shallow::ExportTarget::Reexport {
                    source_specifier,
                    original_name,
                    is_type,
                } => {
                    1u8.hash(hasher);
                    source_specifier.hash(hasher);
                    original_name.hash(hasher);
                    is_type.hash(hasher);
                }
            }
        }

        // Owner-qualified imports are the authoritative lookup geometry.
        // The ordinary-file string table is only its compatibility
        // projection and is deliberately not a second hash input.
        let mut owner_import_targets: Vec<(
            &verter_type_expr::DeclBindingKey,
            &crate::inputs::shallow::ImportTarget,
        )> = state.owner_import_targets.iter().collect();
        owner_import_targets.sort_unstable_by_key(|(left, _)| *left);
        for (binding, target) in owner_import_targets {
            binding.hash(hasher);
            target.source_specifier.hash(hasher);
            target.imported_name.hash(hasher);
            target.is_namespace.hash(hasher);
        }

        state.export_assignment_target().hash(hasher);

        // Retain the parser-authored typed vectors in source order. Their
        // typed variants carry every owner, form, type/value capability,
        // namespace spelling, side-effect import, and export-assignment
        // coordinate. Counts, spans, statement totals, resolved canonicals,
        // and environment/project state are intentionally absent.
        let routes = state.route_inventory.as_ref();
        routes.imports.hash(hasher);
        routes.bindingless_imports.hash(hasher);
        routes.reexports.hash(hasher);
        routes.wildcard_reexports.hash(hasher);
        routes.local_exports.hash(hasher);
        routes.export_assignments.hash(hasher);
    })
}

/// Two-lane 128-bit digest: the closure runs once into each of two
/// domain-separated [`rustc_hash::FxHasher`]s whose outputs are concatenated
/// little-endian.
pub fn hash16_from_sorted(f: impl Fn(&mut rustc_hash::FxHasher)) -> Hash16 {
    let mut left = rustc_hash::FxHasher::default();
    0u8.hash(&mut left);
    f(&mut left);

    let mut right = rustc_hash::FxHasher::default();
    1u8.hash(&mut right);
    f(&mut right);

    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&left.finish().to_le_bytes());
    out[8..].copy_from_slice(&right.finish().to_le_bytes());
    out
}
