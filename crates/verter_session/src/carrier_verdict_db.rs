//! Host-owned `CarrierVerdictDb` for synthetic slot-binding carriers.
//!
//! ## Contract
//!
//! Each entry binds a `CarrierIdentity` (owner scope, surface kind,
//! slot name, binding name, graph value-node identity — the full
//! disambiguator) to a `CarrierVerdict`. Currently the only verdict
//! is `DoNotDeepen`, produced eagerly by the slot-binding graph
//! publisher's no-parser-branch when it mints a symbolic
//! `TypeExpr::Ref { name: <binding_name> }` carrier.
//!
//! The cache is the single host-owned source of truth that the
//! `published_reducer` and `component_meta_registry` collection
//! short-circuits consult to decide whether a published carrier may
//! be walked or registered as a public type ref. The provenance
//! sidecar on the carrier's `ExpandedField` carries the same
//! identity inputs so consumers can build the `CarrierIdentity` key
//! without re-deriving it.
//!
//! ## Cache identity contract
//!
//! Codex's TOP RISK on the carrier-verdict cache was name-only key
//! collision — a synthetic slot parameter named `foo` getting the
//! wrong verdict because a real workspace-owned `type foo = …` alias
//! shadows it, or two slots' same-named bindings collapsing onto one
//! entry. The full identity disambiguates:
//!
//! * `scope_canonical_id` — the publishing component's canonical file.
//! * `surface_kind` — `SlotBinding` vs `Binding`.
//! * `slot_name` — distinguishes per-slot bindings within the same
//!   component.
//! * `binding_name` — the `<binding_name>` itself.
//! * `value_node` — the graph `SemanticNodeId` the publisher minted
//!   the carrier from. Two slots with the same `binding_name` always
//!   mint distinct `value_node`s, so their cache entries never
//!   collide even when slot/binding name happen to match.
//!
//! Note that `projection_mode` is NOT part of this key today. For
//! `DoNotDeepen` the verdict is mode-invariant by construction, so
//! including the mode would inflate cache entries without
//! discriminating any real semantic difference. When the future
//! `Resolved { target_type }` variant lands the resolved target
//! WILL depend on mode (Navigate vs Expanded etc.); at that point
//! extend this key with `projection_mode`.
//!
//! Env-hash dimensions (`project_identity`, `resolve_env_hash`,
//! `type_env_hash`, `lib_env_hash`) are NOT part of this key today
//! because `DoNotDeepen` is structurally invariant to env hashes and
//! `value_node` identifiers already scope each entry to a single
//! `ProjectTypeStore`'s project generation. When the future
//! `CarrierVerdict::Resolved { target_type }` variant lands its
//! resolved type WILL depend on env hashes; at that point this key
//! extends with the relevant env-hash dimensions per the R21 cache
//! scoping rule.
//!
//! ## Lifecycle
//!
//! `value_node`s are content-derived: any same-canonical content
//! edit produces fresh `value_node`s in the next semantic graph
//! generation, so stale entries become unreachable rather than
//! incorrect. `invalidate_all` is the project-wide reset called on
//! `bump_project_generation_and_evict` cascades.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

use verter_semantic::analysis::type_expand::{
    CarrierProvenance, CarrierValueNodeId, PublishedSurfaceKind,
};

/// Cache identity for a synthetic slot-binding carrier verdict.
///
/// See module docs for the per-field rationale and the codex-mandated
/// disambiguation contract.
///
/// Env-hash dimensions (`project_identity`, `resolve_env_hash`,
/// `type_env_hash`, `lib_env_hash`) are NOT part of this key. The
/// `DoNotDeepen` sentinel — the only verdict variant today — is
/// structurally invariant to env hashes by construction: it is a
/// fact about the carrier's shape (`Ref { name }`-only) and the
/// publisher's intent to NOT deepen it through the resolver. The
/// disambiguators that DO matter (`scope_canonical_id`,
/// `surface_kind`, `slot_name`, `binding_name`, `value_node`) are
/// content-derived and project-scoped via `value_node` (semantic-graph
/// identifiers are unique within one `ProjectTypeStore`'s project
/// generation). When the future
/// `CarrierVerdict::Resolved { target_type }` lands and starts
/// depending on env-hash-sensitive resolution, extend this key with
/// the relevant env-hash dimensions per the R21 cache scoping rule
/// AND with `projection_mode` (the resolved target may differ across
/// modes once Resolved exists).
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct CarrierIdentity {
    /// Owner scope: canonical file ID of the publishing component.
    pub scope_canonical_id: Arc<str>,
    /// `SlotBinding` vs `Binding` published-surface family.
    pub surface_kind: PublishedSurfaceKind,
    /// For `SlotBinding`, the defining slot's name. For `Binding`
    /// (merged-member), the slot the binding was derived from when
    /// known.
    pub slot_name: Option<Arc<str>>,
    /// The binding's identifier (matches the synthetic
    /// `TypeExpr::Ref { name }` carrier's `name`).
    pub binding_name: Arc<str>,
    /// Stable identity of the graph node the carrier was minted from.
    /// The codex-mandated disambiguator against same-named bindings
    /// across distinct slots and against real same-named type aliases
    /// (the latter has no graph value-node within the carrier's
    /// minting context).
    pub value_node: CarrierValueNodeId,
}

impl CarrierIdentity {
    /// Build a cache key from the carrier's provenance sidecar.
    #[inline]
    pub fn from_provenance(provenance: &CarrierProvenance) -> Self {
        Self {
            scope_canonical_id: provenance.scope_canonical_id.clone(),
            surface_kind: provenance.surface_kind,
            slot_name: provenance.slot_name.clone(),
            binding_name: provenance.binding_name.clone(),
            value_node: provenance.value_node,
        }
    }
}

/// Cached verdict for a `CarrierIdentity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarrierVerdict {
    /// Symbolic carrier; do NOT deepen + do NOT register as a public
    /// type ref. Downstream consumers receive the carrier as-is.
    DoNotDeepen,
}

/// Host-owned cache for synthetic slot-binding carrier verdicts.
///
/// Single instance lives on `ProjectTypeStore`. Cooperative-admission
/// is unnecessary here because the producer eagerly populates the
/// cache at carrier-mint time (the slot-binding graph publisher's
/// no-parser branch) — there is no cold-on-demand compute path.
/// Subsequent lookups by the reducer / registry short-circuits are
/// pure reads with no need to coalesce concurrent cold callers.
#[derive(Debug, Default)]
pub struct CarrierVerdictDb {
    entries: DashMap<CarrierIdentity, CarrierVerdict>,
    admissions: AtomicU64,
    lookups: AtomicU64,
    hits: AtomicU64,
}

impl CarrierVerdictDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Eagerly admit a `DoNotDeepen` verdict for the given identity.
    ///
    /// Idempotent: if an entry already exists for `key`, no-op. This
    /// is the producer-side population point — every synthetic
    /// carrier minted by the slot-binding graph publisher's
    /// no-parser branch admits its verdict here at publication time,
    /// so the first consumer read always hits cache.
    pub fn admit_do_not_deepen(&self, key: CarrierIdentity) {
        if self.entries.contains_key(&key) {
            return;
        }
        if self
            .entries
            .insert(key, CarrierVerdict::DoNotDeepen)
            .is_none()
        {
            self.admissions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Look up a verdict by identity. Returns `None` on miss.
    pub fn get(&self, key: &CarrierIdentity) -> Option<CarrierVerdict> {
        self.lookups.fetch_add(1, Ordering::Relaxed);
        let entry = self.entries.get(key)?;
        self.hits.fetch_add(1, Ordering::Relaxed);
        Some(entry.value().clone())
    }

    /// True if `key` is admitted with verdict `DoNotDeepen`. Wrapper
    /// over `get` for the common reducer/registry consult.
    pub fn is_do_not_deepen(&self, key: &CarrierIdentity) -> bool {
        matches!(self.get(key), Some(CarrierVerdict::DoNotDeepen))
    }

    /// Clear all entries. Called by the project-generation
    /// invalidation cascade on `bump_project_generation_and_evict`.
    pub fn invalidate_all(&self) {
        self.entries.clear();
    }

    /// Number of admitted entries. Diagnostic-only.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if no entries are admitted. Diagnostic-only.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total admissions counter (monotonic across project
    /// generation). Diagnostic-only.
    pub fn admissions_count(&self) -> u64 {
        self.admissions.load(Ordering::Relaxed)
    }

    /// Total lookups counter. Diagnostic-only.
    pub fn lookups_count(&self) -> u64 {
        self.lookups.load(Ordering::Relaxed)
    }

    /// Total hits counter. Diagnostic-only.
    pub fn hits_count(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_identity(
        scope: &str,
        slot: Option<&str>,
        binding: &str,
        value_node: u64,
    ) -> CarrierIdentity {
        CarrierIdentity {
            scope_canonical_id: Arc::from(scope),
            surface_kind: PublishedSurfaceKind::SlotBinding,
            slot_name: slot.map(Arc::from),
            binding_name: Arc::from(binding),
            value_node: CarrierValueNodeId(value_node),
        }
    }

    #[test]
    fn admit_then_get_returns_do_not_deepen() {
        let db = CarrierVerdictDb::new();
        let key = make_identity("/Foo.vue", Some("default"), "items", 42);
        db.admit_do_not_deepen(key.clone());
        assert!(matches!(db.get(&key), Some(CarrierVerdict::DoNotDeepen)));
        assert!(db.is_do_not_deepen(&key));
        assert_eq!(db.admissions_count(), 1);
        assert_eq!(db.lookups_count(), 2);
        assert_eq!(db.hits_count(), 2);
    }

    #[test]
    fn miss_returns_none() {
        let db = CarrierVerdictDb::new();
        let key = make_identity("/Foo.vue", Some("default"), "items", 1);
        assert!(db.get(&key).is_none());
        assert_eq!(db.lookups_count(), 1);
        assert_eq!(db.hits_count(), 0);
    }

    #[test]
    fn admission_is_idempotent_does_not_double_count() {
        let db = CarrierVerdictDb::new();
        let key = make_identity("/Foo.vue", Some("default"), "items", 7);
        db.admit_do_not_deepen(key.clone());
        db.admit_do_not_deepen(key.clone());
        db.admit_do_not_deepen(key);
        assert_eq!(db.admissions_count(), 1);
        assert_eq!(db.len(), 1);
    }

    /// Codex TOP RISK regression: two slots' same-named bindings
    /// must NOT collide on the cache key. Different `value_node`
    /// values make distinct entries.
    #[test]
    fn same_binding_name_different_value_nodes_do_not_collide() {
        let db = CarrierVerdictDb::new();
        let slot_a = make_identity("/Foo.vue", Some("slotA"), "foo", 100);
        let slot_b = make_identity("/Foo.vue", Some("slotB"), "foo", 200);
        db.admit_do_not_deepen(slot_a.clone());
        db.admit_do_not_deepen(slot_b.clone());
        assert_eq!(db.len(), 2);
        assert!(db.is_do_not_deepen(&slot_a));
        assert!(db.is_do_not_deepen(&slot_b));
    }

    /// Cross-scope isolation: same structural identity in two
    /// distinct files mints distinct cache entries because
    /// `scope_canonical_id` is part of the key. Graph node ids are
    /// scoped to one `ProjectTypeStore`'s project generation, so
    /// per-canonical disambiguation falls out naturally.
    #[test]
    fn cross_scope_value_nodes_do_not_collide() {
        let db = CarrierVerdictDb::new();
        let scope_a = make_identity("/projectA/Foo.vue", Some("default"), "items", 42);
        let scope_b = make_identity("/projectB/Foo.vue", Some("default"), "items", 42);
        db.admit_do_not_deepen(scope_a.clone());
        assert!(db.is_do_not_deepen(&scope_a));
        // Different `scope_canonical_id` produces a distinct key.
        assert!(!db.is_do_not_deepen(&scope_b));
        db.admit_do_not_deepen(scope_b.clone());
        assert_eq!(db.len(), 2);
    }

    /// Surface-kind disambiguation: a `Binding` surface entry must
    /// not collide with a `SlotBinding` surface entry that shares
    /// the rest of its identity.
    #[test]
    fn surface_kind_disambiguates_same_binding_in_different_surfaces() {
        let db = CarrierVerdictDb::new();
        let mut slot_binding = make_identity("/Foo.vue", Some("default"), "items", 42);
        slot_binding.surface_kind = PublishedSurfaceKind::SlotBinding;
        let mut binding = slot_binding.clone();
        binding.surface_kind = PublishedSurfaceKind::Binding;
        db.admit_do_not_deepen(slot_binding.clone());
        db.admit_do_not_deepen(binding.clone());
        assert_eq!(db.len(), 2);
        assert!(db.is_do_not_deepen(&slot_binding));
        assert!(db.is_do_not_deepen(&binding));
    }

    #[test]
    fn invalidate_all_clears_entries() {
        let db = CarrierVerdictDb::new();
        for i in 0..10 {
            let key = make_identity("/Foo.vue", Some("default"), "items", i);
            db.admit_do_not_deepen(key);
        }
        assert_eq!(db.len(), 10);
        db.invalidate_all();
        assert_eq!(db.len(), 0);
        assert!(db.is_empty());
    }

    #[test]
    fn from_provenance_builds_consistent_identity() {
        let provenance = CarrierProvenance {
            scope_canonical_id: Arc::from("/Foo.vue"),
            surface_kind: PublishedSurfaceKind::SlotBinding,
            slot_name: Some(Arc::from("default")),
            binding_name: Arc::from("items"),
            value_node: CarrierValueNodeId(42),
        };
        let key = CarrierIdentity::from_provenance(&provenance);
        assert_eq!(key.scope_canonical_id.as_ref(), "/Foo.vue");
        assert_eq!(key.binding_name.as_ref(), "items");
        assert_eq!(key.value_node.0, 42);
        assert!(matches!(
            key.surface_kind,
            PublishedSurfaceKind::SlotBinding
        ));
    }
}
