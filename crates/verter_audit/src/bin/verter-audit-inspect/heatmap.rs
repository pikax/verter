//! `cache-heatmap` subcommand — sum every record's
//! [`verter_audit::store::CacheLayerBreakdown`] into a single
//! per-layer total, then print the heatmap sorted by total events
//! (hits + misses) descending.

use std::io::Write;
use std::path::Path;

use serde::Serialize;
use verter_audit::store::CacheLayerBreakdown;

use crate::io::load_records_from_dir;
use crate::OutputFormat;

/// Cache-layer hits/misses summed across the whole record set.
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct LayerTotals {
    pub layer: &'static str,
    pub hits: u64,
    pub misses: u64,
}

impl LayerTotals {
    fn total_events(&self) -> u64 {
        self.hits.saturating_add(self.misses)
    }

    fn hit_rate(&self) -> f64 {
        let total = self.total_events();
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Run the `cache-heatmap` subcommand. Returns a process exit code.
pub(crate) fn run(dir: &Path, format: OutputFormat) -> i32 {
    let outcome = load_records_from_dir(dir);
    if outcome.records.is_empty() && !outcome.errors.is_empty() {
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(
            stderr,
            "error: failed to load any records from {}",
            dir.display()
        );
        for err in &outcome.errors {
            let _ = writeln!(stderr, "  {}: {}", err.path.display(), err.message);
        }
        return 2;
    }
    if !outcome.errors.is_empty() {
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(
            stderr,
            "warning: skipped {} unreadable record(s) in {}",
            outcome.errors.len(),
            dir.display()
        );
    }

    let mut totals = LayerSums::default();
    for entry in &outcome.records {
        totals.fold(&entry.record.store.cache_layers);
    }
    // Stable layer order via the canonical name list, then sorted
    // descending by total events. Ties broken by name so the output
    // is deterministic regardless of input order.
    let mut rows = totals.into_rows();
    rows.sort_by(|a, b| {
        b.total_events()
            .cmp(&a.total_events())
            .then_with(|| a.layer.cmp(b.layer))
    });

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if format.json {
        match serde_json::to_string_pretty(&HeatmapPayload {
            dir: dir.display().to_string(),
            record_count: outcome.records.len() as u64,
            layers: rows,
        }) {
            Ok(rendered) => {
                let _ = writeln!(out, "{rendered}");
            }
            Err(e) => {
                let _ = writeln!(std::io::stderr().lock(), "error: serialise failed: {e}");
                return 2;
            }
        }
    } else {
        let _ = writeln!(
            out,
            "verter-audit-inspect cache-heatmap  dir={} records={}",
            dir.display(),
            outcome.records.len()
        );
        let _ = writeln!(
            out,
            "  {:<24} {:>12} {:>12} {:>12} {:>10}",
            "layer", "hits", "misses", "total", "hit_rate"
        );
        for row in &rows {
            let _ = writeln!(
                out,
                "  {:<24} {:>12} {:>12} {:>12} {:>10.4}",
                row.layer,
                row.hits,
                row.misses,
                row.total_events(),
                row.hit_rate()
            );
        }
    }
    0
}

/// JSON envelope for `--json` output.
#[derive(Debug, Serialize)]
struct HeatmapPayload {
    dir: String,
    record_count: u64,
    layers: Vec<LayerTotals>,
}

/// Per-layer running totals. Mirrors every field of
/// [`CacheLayerBreakdown`] so adding a new layer breaks compilation
/// here — that is intentional: the CLI must visibly show every
/// substrate-defined cache layer.
#[derive(Debug, Default)]
struct LayerSums {
    indexed: (u64, u64),
    analysis: (u64, u64),
    owner_import: (u64, u64),
    route_owned_shallow: (u64, u64),
    component_meta: (u64, u64),
    route_db: (u64, u64),
    ref_cycle: (u64, u64),
    intrinsic_registry: (u64, u64),
    semantic_graph: (u64, u64),
    materialize_structure: (u64, u64),
    materialize_memo: (u64, u64),
    prepared_surface: (u64, u64),
    prepared_member: (u64, u64),
}

impl LayerSums {
    fn fold(&mut self, breakdown: &CacheLayerBreakdown) {
        self.indexed.0 = self.indexed.0.saturating_add(breakdown.indexed.hits);
        self.indexed.1 = self.indexed.1.saturating_add(breakdown.indexed.misses);
        self.analysis.0 = self.analysis.0.saturating_add(breakdown.analysis.hits);
        self.analysis.1 = self.analysis.1.saturating_add(breakdown.analysis.misses);
        self.owner_import.0 = self
            .owner_import
            .0
            .saturating_add(breakdown.owner_import.hits);
        self.owner_import.1 = self
            .owner_import
            .1
            .saturating_add(breakdown.owner_import.misses);
        self.route_owned_shallow.0 = self
            .route_owned_shallow
            .0
            .saturating_add(breakdown.route_owned_shallow.hits);
        self.route_owned_shallow.1 = self
            .route_owned_shallow
            .1
            .saturating_add(breakdown.route_owned_shallow.misses);
        self.component_meta.0 = self
            .component_meta
            .0
            .saturating_add(breakdown.component_meta.hits);
        self.component_meta.1 = self
            .component_meta
            .1
            .saturating_add(breakdown.component_meta.misses);
        self.route_db.0 = self.route_db.0.saturating_add(breakdown.route_db.hits);
        self.route_db.1 = self.route_db.1.saturating_add(breakdown.route_db.misses);
        self.ref_cycle.0 = self.ref_cycle.0.saturating_add(breakdown.ref_cycle.hits);
        self.ref_cycle.1 = self.ref_cycle.1.saturating_add(breakdown.ref_cycle.misses);
        self.intrinsic_registry.0 = self
            .intrinsic_registry
            .0
            .saturating_add(breakdown.intrinsic_registry.hits);
        self.intrinsic_registry.1 = self
            .intrinsic_registry
            .1
            .saturating_add(breakdown.intrinsic_registry.misses);
        self.semantic_graph.0 = self
            .semantic_graph
            .0
            .saturating_add(breakdown.semantic_graph.hits);
        self.semantic_graph.1 = self
            .semantic_graph
            .1
            .saturating_add(breakdown.semantic_graph.misses);
        self.materialize_structure.0 = self
            .materialize_structure
            .0
            .saturating_add(breakdown.materialize_structure.hits);
        self.materialize_structure.1 = self
            .materialize_structure
            .1
            .saturating_add(breakdown.materialize_structure.misses);
        self.materialize_memo.0 = self
            .materialize_memo
            .0
            .saturating_add(breakdown.materialize_memo.hits);
        self.materialize_memo.1 = self
            .materialize_memo
            .1
            .saturating_add(breakdown.materialize_memo.misses);
        self.prepared_surface.0 = self
            .prepared_surface
            .0
            .saturating_add(breakdown.prepared_surface.hits);
        self.prepared_surface.1 = self
            .prepared_surface
            .1
            .saturating_add(breakdown.prepared_surface.misses);
        self.prepared_member.0 = self
            .prepared_member
            .0
            .saturating_add(breakdown.prepared_member.hits);
        self.prepared_member.1 = self
            .prepared_member
            .1
            .saturating_add(breakdown.prepared_member.misses);
    }

    fn into_rows(self) -> Vec<LayerTotals> {
        vec![
            LayerTotals {
                layer: "indexed",
                hits: self.indexed.0,
                misses: self.indexed.1,
            },
            LayerTotals {
                layer: "analysis",
                hits: self.analysis.0,
                misses: self.analysis.1,
            },
            LayerTotals {
                layer: "owner_import",
                hits: self.owner_import.0,
                misses: self.owner_import.1,
            },
            LayerTotals {
                layer: "route_owned_shallow",
                hits: self.route_owned_shallow.0,
                misses: self.route_owned_shallow.1,
            },
            LayerTotals {
                layer: "component_meta",
                hits: self.component_meta.0,
                misses: self.component_meta.1,
            },
            LayerTotals {
                layer: "route_db",
                hits: self.route_db.0,
                misses: self.route_db.1,
            },
            LayerTotals {
                layer: "ref_cycle",
                hits: self.ref_cycle.0,
                misses: self.ref_cycle.1,
            },
            LayerTotals {
                layer: "intrinsic_registry",
                hits: self.intrinsic_registry.0,
                misses: self.intrinsic_registry.1,
            },
            LayerTotals {
                layer: "semantic_graph",
                hits: self.semantic_graph.0,
                misses: self.semantic_graph.1,
            },
            LayerTotals {
                layer: "materialize_structure",
                hits: self.materialize_structure.0,
                misses: self.materialize_structure.1,
            },
            LayerTotals {
                layer: "materialize_memo",
                hits: self.materialize_memo.0,
                misses: self.materialize_memo.1,
            },
            LayerTotals {
                layer: "prepared_surface",
                hits: self.prepared_surface.0,
                misses: self.prepared_surface.1,
            },
            LayerTotals {
                layer: "prepared_member",
                hits: self.prepared_member.0,
                misses: self.prepared_member.1,
            },
        ]
    }
}
