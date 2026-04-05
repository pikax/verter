//! Audit sink for type solver instrumentation.
//!
//! Two concrete modes chosen at the entrypoint:
//! - `NoopAudit` — default runtime path, all methods inline to nothing.
//! - `RecordingAudit` — tests and `VERTER_SOLVER_TRACE`-enabled runs.
//!
//! The trait is generic so the compiler monomorphises away all audit overhead
//! on the default path.
//!
//! `RecordingAudit` records the event surface used by request-scoped solver
//! tracing and tests. Some events are emitted directly by `TypeQueryEngine`
//! today while the lower-level solve path still exposes test audit data
//! through `solve_type_with_audit`.

use super::recursion::RecursionKey;
use rustc_hash::FxHashMap;

// ---------------------------------------------------------------------------
// AuditSink trait
// ---------------------------------------------------------------------------

/// Trait for solver audit instrumentation. All methods have default no-op
/// implementations so `NoopAudit` compiles to zero overhead.
#[allow(unused_variables)]
pub trait AuditSink {
    fn op_cache_hit(&mut self, key_summary: &str) {}
    fn op_cache_miss(&mut self, key_summary: &str) {}
    fn prepared_ref_entry(&mut self, key: &RecursionKey) {}
    fn prepared_ref_edge(&mut self, from: &str, to: &str) {}
    fn external_decl_visit(&mut self, decl: &str) {}
    fn unresolved_root_lookup(&mut self, canonical_id: &str, name: &str) {}
    fn normalized_subject(&mut self, summary: &str) {}
    fn structural_identity_shortcut(&mut self) {}
    fn pure_surface_overlay_reuse(&mut self) {}
    fn reverse_remap_hit(&mut self) {}
    fn reverse_remap_symbolic_bail(&mut self) {}
    fn conditional_deferral(&mut self) {}
    fn conditional_relation_cache_hit(&mut self) {}
    fn indexed_access_open_skip(&mut self) {}
    fn structural_transform_fast_path(&mut self) {}
    fn incomplete_reason(&mut self, kind: &'static str) {}
}

// ---------------------------------------------------------------------------
// NoopAudit — zero-cost default
// ---------------------------------------------------------------------------

/// Default audit sink that compiles to nothing. Used on the normal runtime path.
pub struct NoopAudit;
impl AuditSink for NoopAudit {}

// ---------------------------------------------------------------------------
// RecordingAudit — test and trace path
// ---------------------------------------------------------------------------

/// Audit sink that records all solver events. Used for tests and
/// `VERTER_SOLVER_TRACE`-enabled runs.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct RecordingAudit {
    pub structural_identity_shortcuts: u32,
    pub pure_surface_overlay_reuses: u32,
    pub reverse_remap_hits: u32,
    pub reverse_remap_symbolic_bails: u32,
    pub conditional_deferrals: u32,
    pub conditional_relation_cache_hits: u32,
    pub indexed_access_open_skips: u32,
    pub structural_transform_fast_paths: u32,
    pub op_cache_hits: u32,
    pub op_cache_misses: u32,
    pub prepared_ref_entries: u32,
    pub prepared_ref_reentries: u32,
    pub detail: RecordingAuditDetail,
}

/// Detailed recording maps — only allocated/populated when `RecordingAudit`
/// is active.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct RecordingAuditDetail {
    /// Which declarations were visited (canonical_id::symbol_name -> count).
    pub external_decl_visits: FxHashMap<String, u32>,
    /// Which prepared refs were entered (RecursionKey -> count).
    pub prepared_ref_entries_detail: FxHashMap<RecursionKey, u32>,
    /// Prepared ref expansion edges (parent -> child -> count).
    pub prepared_ref_edges: FxHashMap<(String, String), u32>,
    /// Unresolved bare-name lookups.
    pub unresolved_root_lookups: FxHashMap<(String, String), u32>,
    /// Subject normalization events (summary string -> count).
    pub normalized_subjects: FxHashMap<String, u32>,
    /// Incomplete reasons.
    pub incomplete_reason_kinds: FxHashMap<&'static str, u32>,
}

impl AuditSink for RecordingAudit {
    fn op_cache_hit(&mut self, _key_summary: &str) {
        self.op_cache_hits += 1;
    }

    fn op_cache_miss(&mut self, _key_summary: &str) {
        self.op_cache_misses += 1;
    }

    fn prepared_ref_entry(&mut self, key: &RecursionKey) {
        self.prepared_ref_entries += 1;
        let count = self
            .detail
            .prepared_ref_entries_detail
            .entry(key.clone())
            .or_insert(0);
        if *count > 0 {
            self.prepared_ref_reentries += 1;
        }
        *count += 1;
    }

    fn prepared_ref_edge(&mut self, from: &str, to: &str) {
        *self
            .detail
            .prepared_ref_edges
            .entry((from.to_string(), to.to_string()))
            .or_insert(0) += 1;
    }

    fn external_decl_visit(&mut self, decl: &str) {
        *self
            .detail
            .external_decl_visits
            .entry(decl.to_string())
            .or_insert(0) += 1;
    }

    fn unresolved_root_lookup(&mut self, canonical_id: &str, name: &str) {
        *self
            .detail
            .unresolved_root_lookups
            .entry((canonical_id.to_string(), name.to_string()))
            .or_insert(0) += 1;
    }

    fn normalized_subject(&mut self, summary: &str) {
        *self
            .detail
            .normalized_subjects
            .entry(summary.to_string())
            .or_insert(0) += 1;
    }

    fn structural_identity_shortcut(&mut self) {
        self.structural_identity_shortcuts += 1;
    }

    fn pure_surface_overlay_reuse(&mut self) {
        self.pure_surface_overlay_reuses += 1;
    }

    fn reverse_remap_hit(&mut self) {
        self.reverse_remap_hits += 1;
    }

    fn reverse_remap_symbolic_bail(&mut self) {
        self.reverse_remap_symbolic_bails += 1;
    }

    fn conditional_deferral(&mut self) {
        self.conditional_deferrals += 1;
    }

    fn conditional_relation_cache_hit(&mut self) {
        self.conditional_relation_cache_hits += 1;
    }

    fn indexed_access_open_skip(&mut self) {
        self.indexed_access_open_skips += 1;
    }

    fn structural_transform_fast_path(&mut self) {
        self.structural_transform_fast_paths += 1;
    }

    fn incomplete_reason(&mut self, kind: &'static str) {
        *self.detail.incomplete_reason_kinds.entry(kind).or_insert(0) += 1;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_audit_is_zero_cost() {
        let mut audit = NoopAudit;
        audit.structural_identity_shortcut();
        audit.conditional_deferral();
        audit.indexed_access_open_skip();
        audit.op_cache_hit("test");
        audit.op_cache_miss("test");
        // NoopAudit has no state — this test verifies it compiles and
        // the default no-op methods accept calls without panic.
    }

    #[test]
    fn recording_audit_counts_events() {
        let mut audit = RecordingAudit::default();

        audit.structural_identity_shortcut();
        audit.structural_identity_shortcut();
        assert_eq!(audit.structural_identity_shortcuts, 2);
        // Negative: other counters must stay at zero
        assert_eq!(audit.conditional_deferrals, 0);
        assert_eq!(audit.indexed_access_open_skips, 0);
        assert_eq!(audit.pure_surface_overlay_reuses, 0);

        audit.conditional_deferral();
        assert_eq!(audit.conditional_deferrals, 1);

        audit.indexed_access_open_skip();
        assert_eq!(audit.indexed_access_open_skips, 1);

        audit.op_cache_hit("Instantiate(Foo)");
        audit.op_cache_miss("Instantiate(Bar)");
        assert_eq!(audit.op_cache_hits, 1);
        assert_eq!(audit.op_cache_misses, 1);
        // Negative: prepared ref counters untouched
        assert_eq!(audit.prepared_ref_entries, 0);
        assert_eq!(audit.prepared_ref_reentries, 0);
    }

    #[test]
    fn recording_audit_prepared_ref_entry_and_reentry() {
        let mut audit = RecordingAudit::default();

        let key = RecursionKey {
            canonical_id: "a.ts".into(),
            symbol_name: "Foo".into(),
            args_hash: 0,
        };

        // First entry — not a reentry
        audit.prepared_ref_entry(&key);
        assert_eq!(audit.prepared_ref_entries, 1);
        assert_eq!(audit.prepared_ref_reentries, 0);
        assert_eq!(audit.detail.prepared_ref_entries_detail[&key], 1);
        // Negative: only one key in the map
        assert_eq!(audit.detail.prepared_ref_entries_detail.len(), 1);

        // Second entry of the same key => reentry
        audit.prepared_ref_entry(&key);
        assert_eq!(audit.prepared_ref_entries, 2);
        assert_eq!(audit.prepared_ref_reentries, 1);
        assert_eq!(audit.detail.prepared_ref_entries_detail[&key], 2);

        // Different key — not a reentry for that key
        let key2 = RecursionKey {
            canonical_id: "b.ts".into(),
            symbol_name: "Bar".into(),
            args_hash: 0,
        };
        audit.prepared_ref_entry(&key2);
        assert_eq!(audit.prepared_ref_entries, 3);
        assert_eq!(
            audit.prepared_ref_reentries, 1,
            "new key should not count as reentry"
        );
        assert_eq!(audit.detail.prepared_ref_entries_detail.len(), 2);
    }

    #[test]
    fn recording_audit_external_decl_visits() {
        let mut audit = RecordingAudit::default();

        audit.external_decl_visit("types.ts::Foo");
        audit.external_decl_visit("types.ts::Foo");
        audit.external_decl_visit("other.ts::Bar");

        assert_eq!(audit.detail.external_decl_visits["types.ts::Foo"], 2);
        assert_eq!(audit.detail.external_decl_visits["other.ts::Bar"], 1);
        // Negative: exactly two distinct keys, unvisited decls absent
        assert_eq!(audit.detail.external_decl_visits.len(), 2);
        assert!(
            !audit
                .detail
                .external_decl_visits
                .contains_key("absent.ts::Baz"),
            "unvisited decl must not appear"
        );
    }
}
