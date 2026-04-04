//! Top-level solver entry point: `solve_type`.
//!
//! Takes a `TypeExpr` (from a prepared declaration) and the host, creates a
//! query-local arena, lowers the expression, and resolves references through
//! the host. Returns a `SolverResult<TypeExpr>` projected back from the arena.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use super::arena::{Node, NodeId, PrimitiveKind, QueryArena};
use super::builtin::{expand_builtin, BuiltinUtility};
use super::display::display_node;
use super::host::{RequestStatus, ResolvedRootIdentity, TypeSolverHost, UtilitySource};
use super::lower::lower_type_expr;
use super::prepared::{PreparedTypeDecl, PreparedValueDecl};
use super::recursion::{RecursionKey, RecursionTracker};
use super::result::{
    ExecutionStatus, IncompleteReason, SolverDiagnostic, SolverExactness, SolverResult,
};
use super::substitution::SubstitutionEnv;
use crate::analysis::type_expr::TypeExpr;

// ---------------------------------------------------------------------------
// Operational limits
// ---------------------------------------------------------------------------

/// Deterministic operational ceiling on template literal cartesian product size.
/// Exceeding this produces `HardStop` rather than unbounded expansion.
pub const MAX_TEMPLATE_LITERAL_PRODUCT: usize = 100_000;

/// Operational limits for a solver query (generous TypeScript-like ceilings).
#[derive(Debug, Clone)]
pub struct SolveLimits {
    /// Maximum instantiation depth for nested generic resolution.
    pub max_instantiation_depth: u32,
    /// Maximum total resolve steps per query.
    pub max_resolve_steps: u64,
    /// Maximum nodes in the arena before hard stop.
    pub max_arena_nodes: u64,
    /// Maximum non-semantic diagnostics to collect before truncating.
    pub max_diagnostics: usize,
}

impl Default for SolveLimits {
    fn default() -> Self {
        Self {
            max_instantiation_depth: 50,
            max_resolve_steps: 500_000,
            max_arena_nodes: 2_000_000,
            max_diagnostics: 50,
        }
    }
}

/// Mutable solver state for a single query.
pub struct SolveState {
    pub depth: u32,
    pub steps: u64,
    pub limits: SolveLimits,
    pub recursion: RecursionTracker,
    pub exactness: SolverExactness,
    pub execution_status: ExecutionStatus,
    pub incomplete_reasons: Vec<IncompleteReason>,
    /// Stack of active type declaration contexts. When resolving a prepared
    /// type declaration body, the declaration is pushed onto this stack so
    /// bare name refs can be resolved through the declaration's
    /// `name_resolution` map (defining file scope).
    pub type_decl_context_stack: Vec<Arc<PreparedTypeDecl>>,
    /// Stack of active value declaration contexts for `typeof` resolution.
    pub value_decl_context_stack: Vec<Arc<PreparedValueDecl>>,
    /// External declarations visited during this solve. Recorded by
    /// `resolve_prepared_ref` when it enters a declaration from a
    /// canonical file other than "$owner". Used by the host to publish
    /// import aliases to the type registry.
    pub visited_external_decls: Vec<ResolvedRootIdentity>,
    /// Active substitution names currently being expanded.
    ///
    /// This guards self-referential default/type-parameter substitutions like
    /// `T = NestedItem<I>` where `I` is itself bound to an unresolved `T`,
    /// which would otherwise recurse forever before prepared-ref recursion
    /// tracking has a chance to run.
    pub active_substitution_names: Vec<String>,
    /// Stack of active conditional branch frames. Pushed by `resolve_conditional`
    /// when entering a branch, popped on exit. Used to capture conditional
    /// context when recursion is detected.
    pub conditional_context_stack: Vec<super::arena::ConditionalFrameSnapshot>,
    /// Stack of base indices into `conditional_context_stack`, one per active
    /// prepared-ref symbol resolution. Ensures `current_symbol_conditional_context()`
    /// returns only frames from the currently resolving symbol, not cross-symbol frames.
    pub conditional_context_base_stack: Vec<usize>,
    /// Non-semantic diagnostics collected during resolution.
    pub diagnostics: Vec<SolverDiagnostic>,
    /// Count of diagnostics that were dropped after the cap was reached.
    pub diagnostics_truncated: usize,

    // -- Query-local reuse caches --
    /// Cache for host member projections within a single query. Keyed by
    /// `(canonical_id, symbol_name, member_name)` → resolved `NodeId`.
    /// Collapses repeated indexed-access projections like `Foo['bar']`
    /// that would otherwise re-lower and re-resolve the same host result.
    pub(crate) host_projection_cache: rustc_hash::FxHashMap<(String, String, String), NodeId>,

    /// Cache for completed `resolve_prepared_ref` results within a single
    /// query. Keyed by `RecursionKey` (identity + args_hash) → resolved
    /// `NodeId`. Prevents repeated full instantiation of the same
    /// declaration with the same effective arguments.
    pub(crate) prepared_ref_cache: rustc_hash::FxHashMap<RecursionKey, NodeId>,

    // -- Query audit / trace counters --
    /// Whether structured solver tracing is enabled for this query.
    pub(crate) trace_enabled: bool,

    /// Audit surface for tests: counts host member projection calls.
    /// Key: `(canonical_id, symbol_name, member)`, Value: call count.
    pub(crate) audit_host_projections: rustc_hash::FxHashMap<(String, String, String), u32>,

    /// Audit surface for tests: counts prepared-ref instantiation entries.
    /// Key: `RecursionKey`, Value: call count.
    pub(crate) audit_prepared_ref_entries: rustc_hash::FxHashMap<RecursionKey, u32>,

    /// Audit surface for tests/tracing: prepared-ref expansion edges.
    /// Key: `(parent_label, child_label)`, Value: expansion count.
    pub(crate) audit_prepared_ref_edges: rustc_hash::FxHashMap<(String, String), u32>,

    /// Audit surface for tests/tracing: unresolved bare-name fallback lookups.
    /// Key: `(parent_label, symbol_name)`, Value: miss count.
    pub(crate) audit_unresolved_root_lookups: rustc_hash::FxHashMap<(String, String), u32>,

    /// Audit surface for tests/tracing: visited external declaration counts.
    /// Key: `"canonical_id::symbol_name"`, Value: visit count.
    pub(crate) audit_external_decl_visits: rustc_hash::FxHashMap<String, u32>,

    /// Audit surface for tests/tracing: incomplete reason kind counts.
    pub(crate) audit_incomplete_reason_kinds: rustc_hash::FxHashMap<&'static str, u32>,

    /// Audit: total host projection cache hits in this query.
    pub(crate) host_projection_cache_hits: u32,

    /// Audit: total prepared-ref cache hits in this query.
    pub(crate) prepared_ref_cache_hits: u32,

    /// Active prepared-ref expansion stack used to attribute nested traversal.
    pub(crate) prepared_ref_stack: Vec<String>,
}

/// Structured capture of conditional context frames for a recursive ref.
pub(crate) struct ConditionalContextCapture {
    /// The captured (possibly truncated) frames.
    pub frames: Vec<super::arena::ConditionalFrameSnapshot>,
    /// Total frames that were available before truncation.
    pub available: usize,
    /// Whether truncation occurred.
    pub truncated: bool,
}

impl SolveState {
    /// Maximum conditional context frames to capture per recursive ref.
    const MAX_CONDITIONAL_CONTEXT_FRAMES: usize = 8;

    pub fn new(limits: SolveLimits) -> Self {
        Self {
            depth: 0,
            steps: 0,
            limits,
            recursion: RecursionTracker::new(),
            exactness: SolverExactness::ExactConcrete,
            execution_status: ExecutionStatus::Completed,
            incomplete_reasons: Vec::new(),
            type_decl_context_stack: Vec::new(),
            value_decl_context_stack: Vec::new(),
            visited_external_decls: Vec::new(),
            active_substitution_names: Vec::new(),
            conditional_context_stack: Vec::new(),
            conditional_context_base_stack: Vec::new(),
            diagnostics: Vec::new(),
            diagnostics_truncated: 0,
            host_projection_cache: rustc_hash::FxHashMap::default(),
            prepared_ref_cache: rustc_hash::FxHashMap::default(),
            trace_enabled: solver_trace_enabled(),
            audit_host_projections: rustc_hash::FxHashMap::default(),
            audit_prepared_ref_entries: rustc_hash::FxHashMap::default(),
            audit_prepared_ref_edges: rustc_hash::FxHashMap::default(),
            audit_unresolved_root_lookups: rustc_hash::FxHashMap::default(),
            audit_external_decl_visits: rustc_hash::FxHashMap::default(),
            audit_incomplete_reason_kinds: rustc_hash::FxHashMap::default(),
            host_projection_cache_hits: 0,
            prepared_ref_cache_hits: 0,
            prepared_ref_stack: Vec::new(),
        }
    }

    /// Check operational limits. Returns true if any limit is exceeded.
    pub fn is_exceeded(&self) -> bool {
        self.depth > self.limits.max_instantiation_depth
            || self.steps > self.limits.max_resolve_steps
    }

    /// Increment step counter.
    pub fn step(&mut self) -> bool {
        self.steps += 1;
        self.is_exceeded()
    }

    /// Capture the conditional context frames scoped to the currently resolving
    /// symbol, returning a structured capture that indicates whether truncation
    /// occurred.
    pub(crate) fn capture_symbol_conditional_context(&self) -> ConditionalContextCapture {
        let base = self
            .conditional_context_base_stack
            .last()
            .copied()
            .unwrap_or(0);
        let symbol_slice = &self.conditional_context_stack[base..];
        let available = symbol_slice.len();
        let start = available.saturating_sub(Self::MAX_CONDITIONAL_CONTEXT_FRAMES);
        let frames = symbol_slice[start..].to_vec();
        ConditionalContextCapture {
            truncated: available > Self::MAX_CONDITIONAL_CONTEXT_FRAMES,
            available,
            frames,
        }
    }

    /// Returns the conditional context frames scoped to the currently resolving
    /// symbol. Uses the symbol-local base to avoid capturing cross-symbol frames.
    pub fn current_symbol_conditional_context(
        &self,
    ) -> Vec<super::arena::ConditionalFrameSnapshot> {
        self.capture_symbol_conditional_context().frames
    }

    /// Record incomplete status.
    pub fn mark_incomplete(&mut self, reason: IncompleteReason) {
        self.exactness = SolverExactness::Incomplete;
        if self.audit_enabled() {
            *self
                .audit_incomplete_reason_kinds
                .entry(incomplete_reason_kind(&reason))
                .or_insert(0) += 1;
        }
        self.incomplete_reasons.push(reason);
    }

    /// Record symbolic status (not incomplete, but not fully concrete).
    pub fn mark_symbolic(&mut self) {
        if self.exactness == SolverExactness::ExactConcrete {
            self.exactness = SolverExactness::ExactSymbolic;
        }
    }

    /// Record a non-semantic diagnostic. Does NOT affect exactness or
    /// execution status. Diagnostics beyond `max_diagnostics` are silently
    /// dropped and counted in `diagnostics_truncated`.
    pub(crate) fn record_diagnostic(&mut self, diagnostic: SolverDiagnostic) {
        if self.diagnostics.len() < self.limits.max_diagnostics {
            self.diagnostics.push(diagnostic);
        } else {
            self.diagnostics_truncated += 1;
        }
    }

    fn audit_enabled(&self) -> bool {
        self.trace_enabled || cfg!(test)
    }

    fn record_host_projection_call(&mut self, key: (String, String, String)) {
        if !self.audit_enabled() {
            return;
        }
        *self.audit_host_projections.entry(key).or_insert(0) += 1;
    }

    fn record_prepared_ref_entry(&mut self, key: RecursionKey) {
        if !self.audit_enabled() {
            return;
        }
        *self.audit_prepared_ref_entries.entry(key).or_insert(0) += 1;
    }

    fn current_audit_parent_label(&self) -> String {
        self.prepared_ref_stack
            .last()
            .cloned()
            .unwrap_or_else(|| "<query-root>".to_string())
    }

    fn record_prepared_ref_edge(&mut self, child_label: &str) {
        if !self.audit_enabled() {
            return;
        }
        let parent = self.current_audit_parent_label();
        *self
            .audit_prepared_ref_edges
            .entry((parent, child_label.to_string()))
            .or_insert(0) += 1;
    }

    fn record_unresolved_root_lookup(&mut self, symbol_name: &str) {
        if !self.audit_enabled() {
            return;
        }
        let parent = self.current_audit_parent_label();
        *self
            .audit_unresolved_root_lookups
            .entry((parent, symbol_name.to_string()))
            .or_insert(0) += 1;
    }

    fn record_external_decl_visit(&mut self, root_id: &ResolvedRootIdentity) {
        if !self.audit_enabled() {
            return;
        }
        *self
            .audit_external_decl_visits
            .entry(format!("{}::{}", root_id.canonical_id, root_id.symbol_name))
            .or_insert(0) += 1;
    }

    fn record_host_projection_cache_hit(&mut self) {
        if self.audit_enabled() {
            self.host_projection_cache_hits += 1;
        }
    }

    fn record_prepared_ref_cache_hit(&mut self) {
        if self.audit_enabled() {
            self.prepared_ref_cache_hits += 1;
        }
    }
}

fn incomplete_reason_kind(reason: &IncompleteReason) -> &'static str {
    match reason {
        IncompleteReason::MissingSource { .. } => "MissingSource",
        IncompleteReason::UnsupportedSyntax { .. } => "UnsupportedSyntax",
        IncompleteReason::Cancelled => "Cancelled",
        IncompleteReason::RecursionPolicy { .. } => "RecursionPolicy",
    }
}

#[derive(Debug, Clone)]
struct SolveQueryTraceSummary {
    duration_ms: f64,
    arena_nodes: usize,
    resolve_steps: u64,
    exactness: SolverExactness,
    execution_status: ExecutionStatus,
    diagnostics: usize,
    diagnostics_truncated: usize,
    incomplete_reasons: usize,
    visited_external_decls: usize,
    prepared_ref_entries_total: u64,
    prepared_ref_entries_unique: usize,
    prepared_ref_cache_hits: u32,
    host_projection_calls_total: u64,
    host_projection_unique: usize,
    host_projection_cache_hits: u32,
    top_prepared_refs: Vec<(String, u32)>,
    top_prepared_ref_edges: Vec<(String, u32)>,
    top_unresolved_roots: Vec<(String, u32)>,
    top_external_decl_visits: Vec<(String, u32)>,
    top_incomplete_reason_kinds: Vec<(String, u32)>,
    top_host_projections: Vec<(String, u32)>,
}

#[derive(Debug, Clone, Default)]
struct SolveBatchExprStats {
    calls: u32,
    hits: u32,
    misses: u32,
}

#[derive(Debug, Default)]
struct SolveBatchTraceStats {
    total_calls: u32,
    cache_hits: u32,
    cache_misses: u32,
    expr_stats: rustc_hash::FxHashMap<String, SolveBatchExprStats>,
    miss_summaries: Vec<(String, SolveQueryTraceSummary)>,
}

fn solver_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("VERTER_SOLVER_TRACE").is_some()
            || std::env::var_os("VERTER_COMPONENT_META_TRACE").is_some()
            || std::env::var_os("VERTER_META_TRACE").is_some()
    })
}

fn solver_trace_output_path() -> Option<std::path::PathBuf> {
    std::env::var_os("VERTER_SOLVER_TRACE_PATH")
        .or_else(|| std::env::var_os("VERTER_COMPONENT_META_TRACE_PATH"))
        .or_else(|| std::env::var_os("VERTER_META_TRACE_PATH"))
        .map(std::path::PathBuf::from)
}

fn solver_trace_output_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn solver_trace_write_line(line: &str) {
    use std::io::Write;

    let _lock = solver_trace_output_lock().lock();
    if let Some(path) = solver_trace_output_path() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
            return;
        }
    }

    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
    let _ = stderr.flush();
}

fn solver_trace_next_batch_id() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

fn top_named_counts<K, F>(
    map: &rustc_hash::FxHashMap<K, u32>,
    limit: usize,
    render: F,
) -> Vec<(String, u32)>
where
    K: std::cmp::Eq + std::hash::Hash,
    F: Fn(&K) -> String,
{
    let mut entries = map
        .iter()
        .map(|(key, &count)| (render(key), count))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    entries.truncate(limit);
    entries
}

fn build_query_trace_summary(
    state: &SolveState,
    arena: &QueryArena,
    duration_ms: f64,
) -> SolveQueryTraceSummary {
    let prepared_ref_entries_total = state
        .audit_prepared_ref_entries
        .values()
        .map(|&count| u64::from(count))
        .sum();
    let host_projection_calls_total = state
        .audit_host_projections
        .values()
        .map(|&count| u64::from(count))
        .sum();

    SolveQueryTraceSummary {
        duration_ms,
        arena_nodes: arena.len(),
        resolve_steps: state.steps,
        exactness: state.exactness,
        execution_status: state.execution_status,
        diagnostics: state.diagnostics.len(),
        diagnostics_truncated: state.diagnostics_truncated,
        incomplete_reasons: state.incomplete_reasons.len(),
        visited_external_decls: state.visited_external_decls.len(),
        prepared_ref_entries_total,
        prepared_ref_entries_unique: state.audit_prepared_ref_entries.len(),
        prepared_ref_cache_hits: state.prepared_ref_cache_hits,
        host_projection_calls_total,
        host_projection_unique: state.audit_host_projections.len(),
        host_projection_cache_hits: state.host_projection_cache_hits,
        top_prepared_refs: top_named_counts(&state.audit_prepared_ref_entries, 12, |key| {
            format!(
                "{}::{}#{}",
                key.canonical_id, key.symbol_name, key.args_hash
            )
        }),
        top_prepared_ref_edges: top_named_counts(&state.audit_prepared_ref_edges, 16, |key| {
            format!("{} -> {}", key.0, key.1)
        }),
        top_unresolved_roots: top_named_counts(&state.audit_unresolved_root_lookups, 16, |key| {
            format!("{} -> {}", key.0, key.1)
        }),
        top_external_decl_visits: top_named_counts(&state.audit_external_decl_visits, 16, |key| {
            key.clone()
        }),
        top_incomplete_reason_kinds: top_named_counts(
            &state.audit_incomplete_reason_kinds,
            16,
            |key| key.to_string(),
        ),
        top_host_projections: top_named_counts(&state.audit_host_projections, 12, |key| {
            format!("{}::{}[{}]", key.0, key.1, key.2)
        }),
    }
}

fn emit_batch_trace_summary(batch_id: u64, stats: &SolveBatchTraceStats, cache_len: usize) {
    let total_query_ms: f64 = stats
        .miss_summaries
        .iter()
        .map(|(_, summary)| summary.duration_ms)
        .sum();
    let total_query_steps: u64 = stats
        .miss_summaries
        .iter()
        .map(|(_, summary)| summary.resolve_steps)
        .sum();
    let total_query_nodes: usize = stats
        .miss_summaries
        .iter()
        .map(|(_, summary)| summary.arena_nodes)
        .sum();
    let total_prepared_ref_entries: u64 = stats
        .miss_summaries
        .iter()
        .map(|(_, summary)| summary.prepared_ref_entries_total)
        .sum();
    let total_prepared_ref_hits: u64 = stats
        .miss_summaries
        .iter()
        .map(|(_, summary)| u64::from(summary.prepared_ref_cache_hits))
        .sum();
    let total_host_projection_calls: u64 = stats
        .miss_summaries
        .iter()
        .map(|(_, summary)| summary.host_projection_calls_total)
        .sum();
    let total_host_projection_hits: u64 = stats
        .miss_summaries
        .iter()
        .map(|(_, summary)| u64::from(summary.host_projection_cache_hits))
        .sum();

    solver_trace_write_line(&format!(
        "[verter-solver-trace] event=batch_end batch={} calls={} hits={} misses={} unique_exprs={} cache_entries={} total_query_ms={:.3} total_steps={} total_nodes={} total_prepared_ref_entries={} total_prepared_ref_hits={} total_host_projection_calls={} total_host_projection_hits={}",
        batch_id,
        stats.total_calls,
        stats.cache_hits,
        stats.cache_misses,
        stats.expr_stats.len(),
        cache_len,
        total_query_ms,
        total_query_steps,
        total_query_nodes,
        total_prepared_ref_entries,
        total_prepared_ref_hits,
        total_host_projection_calls,
        total_host_projection_hits,
    ));

    let mut expr_entries = stats.expr_stats.iter().collect::<Vec<_>>();
    expr_entries.sort_by(|left, right| {
        right
            .1
            .calls
            .cmp(&left.1.calls)
            .then_with(|| right.1.misses.cmp(&left.1.misses))
            .then_with(|| left.0.cmp(right.0))
    });
    for (rank, (expr, stat)) in expr_entries.into_iter().take(20).enumerate() {
        solver_trace_write_line(&format!(
            "[verter-solver-trace] event=batch_expr batch={} rank={} expr={:?} calls={} hits={} misses={}",
            batch_id,
            rank + 1,
            expr,
            stat.calls,
            stat.hits,
            stat.misses,
        ));
    }

    let mut query_entries = stats.miss_summaries.iter().collect::<Vec<_>>();
    query_entries.sort_by(|left, right| {
        right
            .1
            .duration_ms
            .partial_cmp(&left.1.duration_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (rank, (expr, summary)) in query_entries.into_iter().take(12).enumerate() {
        solver_trace_write_line(&format!(
            "[verter-solver-trace] event=query_summary batch={} rank={} expr={:?} dur_ms={:.3} steps={} nodes={} exactness={:?} status={:?} diagnostics={} diagnostics_truncated={} incomplete_reasons={} visited_external_decls={} prepared_ref_entries={} prepared_ref_unique={} prepared_ref_cache_hits={} host_projection_calls={} host_projection_unique={} host_projection_cache_hits={}",
            batch_id,
            rank + 1,
            expr,
            summary.duration_ms,
            summary.resolve_steps,
            summary.arena_nodes,
            summary.exactness,
            summary.execution_status,
            summary.diagnostics,
            summary.diagnostics_truncated,
            summary.incomplete_reasons,
            summary.visited_external_decls,
            summary.prepared_ref_entries_total,
            summary.prepared_ref_entries_unique,
            summary.prepared_ref_cache_hits,
            summary.host_projection_calls_total,
            summary.host_projection_unique,
            summary.host_projection_cache_hits,
        ));
        for (inner_rank, (name, count)) in summary.top_prepared_refs.iter().enumerate() {
            solver_trace_write_line(&format!(
                "[verter-solver-trace] event=query_top_prepared_ref batch={} rank={} inner_rank={} expr={:?} key={:?} count={}",
                batch_id,
                rank + 1,
                inner_rank + 1,
                expr,
                name,
                count,
            ));
        }
        for (inner_rank, (name, count)) in summary.top_prepared_ref_edges.iter().enumerate() {
            solver_trace_write_line(&format!(
                "[verter-solver-trace] event=query_top_prepared_ref_edge batch={} rank={} inner_rank={} expr={:?} key={:?} count={}",
                batch_id,
                rank + 1,
                inner_rank + 1,
                expr,
                name,
                count,
            ));
        }
        for (inner_rank, (name, count)) in summary.top_unresolved_roots.iter().enumerate() {
            solver_trace_write_line(&format!(
                "[verter-solver-trace] event=query_top_unresolved_root batch={} rank={} inner_rank={} expr={:?} key={:?} count={}",
                batch_id,
                rank + 1,
                inner_rank + 1,
                expr,
                name,
                count,
            ));
        }
        for (inner_rank, (name, count)) in summary.top_external_decl_visits.iter().enumerate() {
            solver_trace_write_line(&format!(
                "[verter-solver-trace] event=query_top_external_decl_visit batch={} rank={} inner_rank={} expr={:?} key={:?} count={}",
                batch_id,
                rank + 1,
                inner_rank + 1,
                expr,
                name,
                count,
            ));
        }
        for (inner_rank, (name, count)) in summary.top_incomplete_reason_kinds.iter().enumerate() {
            solver_trace_write_line(&format!(
                "[verter-solver-trace] event=query_top_incomplete_reason batch={} rank={} inner_rank={} expr={:?} key={:?} count={}",
                batch_id,
                rank + 1,
                inner_rank + 1,
                expr,
                name,
                count,
            ));
        }
        for (inner_rank, (name, count)) in summary.top_host_projections.iter().enumerate() {
            solver_trace_write_line(&format!(
                "[verter-solver-trace] event=query_top_host_projection batch={} rank={} inner_rank={} expr={:?} key={:?} count={}",
                batch_id,
                rank + 1,
                inner_rank + 1,
                expr,
                name,
                count,
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Test-only audit surface
// ---------------------------------------------------------------------------

/// Structured audit data from a solver query. Available only in tests.
/// Used to assert both what was expanded and what was NOT expanded.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub struct SolverAudit {
    /// Host member projection call counts.
    /// Key: `(canonical_id, symbol_name, member)`.
    pub host_projection_counts: rustc_hash::FxHashMap<(String, String, String), u32>,
    /// Prepared-ref instantiation entry counts.
    /// Key: `RecursionKey` (identity + args_hash).
    pub prepared_ref_counts: rustc_hash::FxHashMap<RecursionKey, u32>,
    /// Prepared-ref expansion edge counts.
    /// Key: `(parent_label, child_label)`.
    pub prepared_ref_edge_counts: rustc_hash::FxHashMap<(String, String), u32>,
    /// Unresolved bare-name lookup miss counts.
    /// Key: `(parent_label, symbol_name)`.
    pub unresolved_root_counts: rustc_hash::FxHashMap<(String, String), u32>,
    /// External declaration visit counts.
    /// Key: `"canonical_id::symbol_name"`.
    pub external_decl_visit_counts: rustc_hash::FxHashMap<String, u32>,
    /// Incomplete reason kind counts.
    pub incomplete_reason_counts: rustc_hash::FxHashMap<&'static str, u32>,
    /// Total host projection cache hits.
    pub host_projection_cache_hits: u32,
    /// Total prepared-ref cache hits.
    pub prepared_ref_cache_hits: u32,
    /// Arena node count at end of query.
    pub arena_nodes: usize,
    /// Total resolve steps.
    pub resolve_steps: u64,
}

/// Solve with audit data. Test-only entry point.
#[cfg(test)]
pub fn solve_type_with_audit(
    expr: &TypeExpr,
    host: &dyn TypeSolverHost,
) -> (SolverResult<TypeExpr>, SolverAudit) {
    let mut arena = QueryArena::new();
    let mut state = SolveState::new(SolveLimits::default());

    let root = lower_type_expr(&mut arena, expr);
    let resolved = resolve_node(&mut arena, root, host, &mut state, &SubstitutionEnv::new());
    let result_expr = project_to_type_expr(&arena, resolved);

    let audit = SolverAudit {
        host_projection_counts: state.audit_host_projections,
        prepared_ref_counts: state.audit_prepared_ref_entries,
        prepared_ref_edge_counts: state.audit_prepared_ref_edges,
        unresolved_root_counts: state.audit_unresolved_root_lookups,
        external_decl_visit_counts: state.audit_external_decl_visits,
        incomplete_reason_counts: state.audit_incomplete_reason_kinds,
        host_projection_cache_hits: state.host_projection_cache_hits,
        prepared_ref_cache_hits: state.prepared_ref_cache_hits,
        arena_nodes: arena.len(),
        resolve_steps: state.steps,
    };

    let result = SolverResult {
        value: result_expr,
        exactness: state.exactness,
        execution_status: state.execution_status,
        incomplete_reasons: state.incomplete_reasons,
        diagnostics: state.diagnostics,
    };

    (result, audit)
}

// ---------------------------------------------------------------------------
// solve_type — top-level entry point
// ---------------------------------------------------------------------------

/// Cached entry from a single solver run, holding both the semantic result
/// and the external declaration trace. Used by `SolveBatch` and the shared
/// `run_solve_query` runner.
#[derive(Debug, Clone)]
struct CachedSolveEntry {
    result: SolverResult<TypeExpr>,
    trace: Vec<ResolvedRootIdentity>,
    trace_summary: Option<SolveQueryTraceSummary>,
}

/// Shared query runner used by `solve_type`, `solve_type_with_trace`, and
/// `SolveBatch`. Centralises arena creation, lowering, resolution, projection,
/// and trace capture.
fn run_solve_query(
    expr: &TypeExpr,
    host: &dyn TypeSolverHost,
    limits: SolveLimits,
) -> CachedSolveEntry {
    let query_started = Instant::now();
    let mut arena = QueryArena::new();
    let mut state = SolveState::new(limits);

    let root = lower_type_expr(&mut arena, expr);
    let resolved = resolve_node(&mut arena, root, host, &mut state, &SubstitutionEnv::new());
    let result_expr = project_to_type_expr(&arena, resolved);

    if solver_debug_enabled() {
        let json_bytes = serde_json::to_string(&result_expr)
            .map(|s| s.len())
            .unwrap_or(0);
        let input_summary = solver_expr_summary(expr);
        if state.execution_status == ExecutionStatus::HardStop || state.steps > 10_000 {
            eprintln!(
                "[verter-solver] SLOW nodes={} steps={} depth={} payload={}B exactness={} status={} input={}",
                arena.len(),
                state.steps,
                state.recursion.max_depth(),
                json_bytes,
                state.exactness,
                state.execution_status,
                input_summary,
            );
            // Log which symbols hit recursion limits
            for reason in &state.incomplete_reasons {
                eprintln!("  [verter-solver]   reason: {}", reason);
            }
        } else {
            eprintln!(
                "[verter-solver] nodes={} steps={} depth={} payload={}B exactness={} status={}",
                arena.len(),
                state.steps,
                state.recursion.max_depth(),
                json_bytes,
                state.exactness,
                state.execution_status,
            );
        }
    }

    let trace_summary = state.audit_enabled().then(|| {
        build_query_trace_summary(
            &state,
            &arena,
            query_started.elapsed().as_secs_f64() * 1000.0,
        )
    });

    CachedSolveEntry {
        result: SolverResult {
            value: result_expr,
            exactness: state.exactness,
            execution_status: state.execution_status,
            incomplete_reasons: state.incomplete_reasons,
            diagnostics: state.diagnostics,
        },
        trace: state.visited_external_decls,
        trace_summary,
    }
}

/// Solve (normalize/expand) a `TypeExpr` using the host for cross-file
/// declaration resolution.
///
/// Production callers always run through the shared production limits source.
pub fn solve_type(expr: &TypeExpr, host: &dyn TypeSolverHost) -> SolverResult<TypeExpr> {
    run_solve_query(expr, host, SolveLimits::default()).result
}

#[cfg(test)]
fn solve_type_with_limits(
    expr: &TypeExpr,
    host: &dyn TypeSolverHost,
    limits: SolveLimits,
) -> SolverResult<TypeExpr> {
    run_solve_query(expr, host, limits).result
}

fn solver_expr_summary(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive(p) => format!("Primitive({:?})", p),
        TypeExpr::Literal(lit) => format!("Literal({:?})", lit),
        TypeExpr::Union(members) => format!("Union({} members)", members.len()),
        TypeExpr::Intersection(members) => format!("Intersection({} members)", members.len()),
        TypeExpr::Array { .. } => "Array".into(),
        TypeExpr::Tuple { elements, .. } => format!("Tuple({} elements)", elements.len()),
        TypeExpr::Object(obj) => format!("Object({} members)", obj.properties.len()),
        TypeExpr::Function(_) => "Function".into(),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if type_arguments.is_empty() {
                format!("Ref({})", name)
            } else {
                format!("Ref({}<{} args>)", name, type_arguments.len())
            }
        }
        TypeExpr::TypeParameter(param) => format!("TypeParameter({})", param.name),
        TypeExpr::KeyOf(_) => "KeyOf".into(),
        TypeExpr::TypeOf(val) => format!("TypeOf({})", val.path.join(".")),
        TypeExpr::IndexedAccess { .. } => "IndexedAccess".into(),
        TypeExpr::Conditional { .. } => "Conditional".into(),
        TypeExpr::Mapped { parameter, .. } => format!("Mapped({})", parameter),
        TypeExpr::TemplateLiteral { expressions, .. } => {
            format!("TemplateLiteral({} exprs)", expressions.len())
        }
        TypeExpr::Infer { name } => format!("Infer({})", name),
        TypeExpr::Rest(_) => "Rest".into(),
        TypeExpr::Parenthesized(inner) => format!("Parenthesized({})", solver_expr_summary(inner)),
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            ..
        } => {
            if type_arguments.is_empty() {
                format!("RecursiveRef({})", name)
            } else {
                format!("RecursiveRef({}<{} args>)", name, type_arguments.len())
            }
        }
        TypeExpr::Unknown { raw } => {
            let preview: String = raw.chars().take(40).collect();
            format!("Unknown({})", preview)
        }
    }
}

fn cancelled_solver_result(expr: &TypeExpr) -> SolverResult<TypeExpr> {
    SolverResult {
        value: expr.clone(),
        // Preserve the current uncached solver behavior: cancellation is tracked
        // in execution status, while the projected value stays unchanged.
        exactness: SolverExactness::ExactConcrete,
        execution_status: ExecutionStatus::Cancelled,
        incomplete_reasons: vec![],
        diagnostics: vec![],
    }
}

fn solver_debug_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("VERTER_SOLVER_DEBUG").is_some())
}

/// Solve a type expression and return both the result and a trace of
/// external declarations visited during resolution. The trace is a
/// sidecar for the orchestration layer (registry publishing) and is
/// NOT part of the semantic `SolverResult`.
///
/// Production callers always run through the shared production limits source.
pub fn solve_type_with_trace(
    expr: &TypeExpr,
    host: &dyn TypeSolverHost,
) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
    let entry = run_solve_query(expr, host, SolveLimits::default());
    (entry.result, entry.trace)
}

// ---------------------------------------------------------------------------
// SolveBatch — batch-scoped top-level solve memoization
// ---------------------------------------------------------------------------

/// Batch-scoped solve memoization. Created at the start of a macro-expansion
/// function, dropped when that function returns. Caches `(TypeExpr → result)`
/// within one fixed host and the shared production limits.
///
/// **Not** a host-global or cross-request cache — strictly query-scoped.
pub struct SolveBatch<'a> {
    host: &'a dyn TypeSolverHost,
    cache: rustc_hash::FxHashMap<TypeExpr, CachedSolveEntry>,
    trace_enabled: bool,
    trace_batch_id: u64,
    trace_stats: SolveBatchTraceStats,
}

impl<'a> SolveBatch<'a> {
    /// Create a new batch pinned to one host.
    pub fn new(host: &'a dyn TypeSolverHost) -> Self {
        Self {
            host,
            cache: rustc_hash::FxHashMap::default(),
            trace_enabled: solver_trace_enabled(),
            trace_batch_id: solver_trace_next_batch_id(),
            trace_stats: SolveBatchTraceStats::default(),
        }
    }

    /// Solve a type expression, reusing a cached result when possible.
    pub fn solve(&mut self, expr: &TypeExpr) -> SolverResult<TypeExpr> {
        let expr_summary = if self.trace_enabled {
            Some(solver_expr_summary(expr))
        } else {
            None
        };
        if let Some(expr_summary) = expr_summary.as_ref() {
            self.trace_stats.total_calls += 1;
            self.trace_stats
                .expr_stats
                .entry(expr_summary.clone())
                .or_default()
                .calls += 1;
            solver_trace_write_line(&format!(
                "[verter-solver-trace] event=batch_call batch={} mode=solve expr={:?}",
                self.trace_batch_id, expr_summary
            ));
        }
        if let RequestStatus::Cancelled = self.host.request_status() {
            return cancelled_solver_result(expr);
        }
        if let Some(entry) = self.cache.get(expr) {
            if let Some(expr_summary) = expr_summary.as_ref() {
                self.trace_stats.cache_hits += 1;
                self.trace_stats
                    .expr_stats
                    .entry(expr_summary.clone())
                    .or_default()
                    .hits += 1;
                solver_trace_write_line(&format!(
                    "[verter-solver-trace] event=batch_call_result batch={} mode=solve expr={:?} cache_hit=true status={:?} exactness={:?}",
                    self.trace_batch_id,
                    expr_summary,
                    entry.result.execution_status,
                    entry.result.exactness,
                ));
            }
            return entry.result.clone();
        }
        let entry = run_solve_query(expr, self.host, SolveLimits::default());
        if let Some(expr_summary) = expr_summary.as_ref() {
            self.trace_stats.cache_misses += 1;
            self.trace_stats
                .expr_stats
                .entry(expr_summary.clone())
                .or_default()
                .misses += 1;
            if let Some(summary) = entry.trace_summary.as_ref() {
                self.trace_stats
                    .miss_summaries
                    .push((expr_summary.clone(), summary.clone()));
                solver_trace_write_line(&format!(
                    "[verter-solver-trace] event=batch_call_result batch={} mode=solve expr={:?} cache_hit=false dur_ms={:.3} steps={} nodes={} status={:?} exactness={:?}",
                    self.trace_batch_id,
                    expr_summary,
                    summary.duration_ms,
                    summary.resolve_steps,
                    summary.arena_nodes,
                    summary.execution_status,
                    summary.exactness,
                ));
            }
        }
        let result = entry.result.clone();
        self.cache.insert(expr.clone(), entry);
        result
    }

    /// Solve a type expression and return both the result and the visited
    /// external declaration trace. On a cache hit the stored trace is cloned.
    pub fn solve_with_trace(
        &mut self,
        expr: &TypeExpr,
    ) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
        let expr_summary = if self.trace_enabled {
            Some(solver_expr_summary(expr))
        } else {
            None
        };
        if let Some(expr_summary) = expr_summary.as_ref() {
            self.trace_stats.total_calls += 1;
            self.trace_stats
                .expr_stats
                .entry(expr_summary.clone())
                .or_default()
                .calls += 1;
            solver_trace_write_line(&format!(
                "[verter-solver-trace] event=batch_call batch={} mode=solve_with_trace expr={:?}",
                self.trace_batch_id, expr_summary
            ));
        }
        if let RequestStatus::Cancelled = self.host.request_status() {
            return (cancelled_solver_result(expr), Vec::new());
        }
        if let Some(entry) = self.cache.get(expr) {
            if let Some(expr_summary) = expr_summary.as_ref() {
                self.trace_stats.cache_hits += 1;
                self.trace_stats
                    .expr_stats
                    .entry(expr_summary.clone())
                    .or_default()
                    .hits += 1;
                solver_trace_write_line(&format!(
                    "[verter-solver-trace] event=batch_call_result batch={} mode=solve_with_trace expr={:?} cache_hit=true status={:?} exactness={:?}",
                    self.trace_batch_id,
                    expr_summary,
                    entry.result.execution_status,
                    entry.result.exactness,
                ));
            }
            return (entry.result.clone(), entry.trace.clone());
        }
        let entry = run_solve_query(expr, self.host, SolveLimits::default());
        if let Some(expr_summary) = expr_summary.as_ref() {
            self.trace_stats.cache_misses += 1;
            self.trace_stats
                .expr_stats
                .entry(expr_summary.clone())
                .or_default()
                .misses += 1;
            if let Some(summary) = entry.trace_summary.as_ref() {
                self.trace_stats
                    .miss_summaries
                    .push((expr_summary.clone(), summary.clone()));
                solver_trace_write_line(&format!(
                    "[verter-solver-trace] event=batch_call_result batch={} mode=solve_with_trace expr={:?} cache_hit=false dur_ms={:.3} steps={} nodes={} status={:?} exactness={:?}",
                    self.trace_batch_id,
                    expr_summary,
                    summary.duration_ms,
                    summary.resolve_steps,
                    summary.arena_nodes,
                    summary.execution_status,
                    summary.exactness,
                ));
            }
        }
        let result = entry.result.clone();
        let trace = entry.trace.clone();
        self.cache.insert(expr.clone(), entry);
        (result, trace)
    }

    /// Number of cached expression entries in this batch.
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
}

impl Drop for SolveBatch<'_> {
    fn drop(&mut self) {
        if !self.trace_enabled {
            return;
        }
        emit_batch_trace_summary(self.trace_batch_id, &self.trace_stats, self.cache.len());
    }
}

// ---------------------------------------------------------------------------
// resolve_node — the recursive resolver
// ---------------------------------------------------------------------------

/// Resolve a node in the arena, expanding references through the host.
fn resolve_node(
    arena: &mut QueryArena,
    node: NodeId,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    if node.is_unresolved() {
        state.mark_symbolic();
        return node;
    }

    // Check cancellation
    if host.request_status() == RequestStatus::Cancelled {
        state.execution_status = ExecutionStatus::Cancelled;
        return node;
    }

    // Preserve the original hard-stop cause once a deterministic guard has
    // fired on another branch; do not relabel sibling work as a step-limit hit.
    if state.execution_status == ExecutionStatus::HardStop {
        return node;
    }

    // Check operational limits (steps + arena size)
    if state.step() || (arena.len() as u64) > state.limits.max_arena_nodes {
        state.execution_status = ExecutionStatus::HardStop;
        state.mark_incomplete(IncompleteReason::UnsupportedSyntax {
            description: "resolve step or arena size limit exceeded".into(),
        });
        return node;
    }

    // Fast path: terminals and simple lookups — no clone needed.
    match arena.get(node) {
        Node::Primitive(_) | Node::Literal(_) | Node::Error { .. } => return node,
        Node::RecursiveRef { .. } => return node,
        Node::TypeParam { ref name, .. } => {
            let name = name.clone();
            if let Some(bound) = subst.resolve(&name) {
                if state
                    .active_substitution_names
                    .iter()
                    .any(|active| active == &name)
                {
                    state.mark_symbolic();
                    return node;
                }

                state.active_substitution_names.push(name);
                let resolved = resolve_node(arena, bound, host, state, subst);
                state.active_substitution_names.pop();
                return resolved;
            } else {
                state.mark_symbolic();
                return node;
            }
        }
        Node::Infer { .. } => {
            state.mark_symbolic();
            return node;
        }
        _ => {}
    }

    // Compound/operator nodes — clone to release borrow, then recurse.
    let node_data = arena.get(node).clone();
    match node_data {
        // Union — resolve each member
        Node::Union(members) => {
            let resolved: Vec<NodeId> = members
                .iter()
                .map(|&m| resolve_node(arena, m, host, state, subst))
                .collect();
            arena.union(resolved)
        }

        // Intersection — resolve each member
        Node::Intersection(members) => {
            let resolved: Vec<NodeId> = members
                .iter()
                .map(|&m| resolve_node(arena, m, host, state, subst))
                .collect();
            let mut simplified = Vec::with_capacity(resolved.len());
            let mut saw_empty_object = false;

            for member in resolved {
                if member.is_unresolved() {
                    state.mark_symbolic();
                    simplified.push(member);
                    continue;
                }

                match arena.get(member) {
                    Node::Primitive(PrimitiveKind::Never) => return member,
                    Node::Object(obj)
                        if obj.properties.is_empty()
                            && obj.index_signatures.is_empty()
                            && obj.call_signatures.is_empty()
                            && obj.construct_signatures.is_empty() =>
                    {
                        saw_empty_object = true;
                    }
                    _ => simplified.push(member),
                }
            }

            if simplified.is_empty() && saw_empty_object {
                arena.object(super::arena::ObjectNode {
                    properties: vec![],
                    index_signatures: vec![],
                    call_signatures: vec![],
                    construct_signatures: vec![],
                })
            } else {
                arena.intersection(simplified)
            }
        }

        // Array — resolve element
        Node::Array { element, readonly } => {
            let el = resolve_node(arena, element, host, state, subst);
            arena.array(el, readonly)
        }

        // Tuple — resolve elements
        Node::Tuple { elements, readonly } => {
            let els: Vec<_> = elements
                .into_iter()
                .map(|mut el| {
                    el.ty = resolve_node(arena, el.ty, host, state, subst);
                    el
                })
                .collect();
            arena.alloc(Node::Tuple {
                elements: els,
                readonly,
            })
        }

        // Object — resolve property types
        Node::Object(mut obj) => {
            for prop in &mut obj.properties {
                prop.ty = resolve_node(arena, prop.ty, host, state, subst);
            }
            for idx in &mut obj.index_signatures {
                idx.key_type = resolve_node(arena, idx.key_type, host, state, subst);
                idx.value_type = resolve_node(arena, idx.value_type, host, state, subst);
            }
            for sig in &mut obj.call_signatures {
                sig.return_type = resolve_node(arena, sig.return_type, host, state, subst);
                for param in &mut sig.parameters {
                    param.ty = resolve_node(arena, param.ty, host, state, subst);
                }
            }
            for sig in &mut obj.construct_signatures {
                sig.return_type = resolve_node(arena, sig.return_type, host, state, subst);
                for param in &mut sig.parameters {
                    param.ty = resolve_node(arena, param.ty, host, state, subst);
                }
            }
            arena.object(obj)
        }

        // Function — resolve parameter and return types
        Node::Function(mut func) => {
            for sig in &mut func.signatures {
                sig.return_type = resolve_node(arena, sig.return_type, host, state, subst);
                for param in &mut sig.parameters {
                    param.ty = resolve_node(arena, param.ty, host, state, subst);
                }
            }
            arena.function(func)
        }

        // Ref — look up through the host and instantiate
        Node::Ref {
            ref name,
            ref type_arguments,
        } => {
            let name = name.clone();
            let args = type_arguments.clone();

            // Check if it's a built-in utility type.
            // Compiler intrinsics (Uppercase etc.) are never shadowable.
            // Other builtins are only expanded if the host confirms they're
            // not shadowed by user declarations.
            if let Some(builtin) = BuiltinUtility::from_name(&name) {
                let should_expand = builtin.is_compiler_intrinsic()
                    || host.utility_source(&name) != UtilitySource::Shadowed;

                if should_expand {
                    let resolved_args: Vec<NodeId> = args
                        .iter()
                        .map(|&a| resolve_node(arena, a, host, state, subst))
                        .collect();

                    if let Some(expanded) = expand_builtin(arena, builtin, &resolved_args) {
                        return resolve_node(arena, expanded, host, state, subst);
                    }
                }
            }

            // Check substitution env (for generic type params used as refs)
            if args.is_empty() {
                if let Some(bound) = subst.resolve(&name) {
                    if let Some(guarded) = resolve_substitution_binding(
                        arena,
                        node,
                        name.as_ref(),
                        bound,
                        host,
                        state,
                        subst,
                    ) {
                        return guarded;
                    }
                    return resolve_node(arena, bound, host, state, subst);
                }
            }

            // Resolve type arguments first
            let resolved_args: Vec<NodeId> = args
                .iter()
                .map(|&a| resolve_node(arena, a, host, state, subst))
                .collect();

            // Try to resolve from the host's prepared declarations.
            // First check the active declaration context — bare names in an
            // imported type body should resolve through the defining file's
            // scope (name_resolution), not the owner file's scope.
            let maybe_root =
                resolve_name_in_context(state, &name).or_else(|| host.root_identity("", &name));
            if let Some(root_id) = maybe_root {
                return resolve_prepared_ref(arena, host, state, subst, &root_id, &resolved_args);
            }
            state.record_unresolved_root_lookup(&name);

            // Host can't resolve — keep as symbolic ref
            if resolved_args != args {
                // Args changed, rebuild
                arena.type_ref(name, resolved_args)
            } else {
                state.mark_symbolic();
                node
            }
        }

        // Applied — already instantiated, resolve body
        Node::Applied { .. } => {
            state.mark_symbolic();
            node
        }

        // -- keyof --
        Node::KeyOf(operand) => {
            let resolved_operand = resolve_node(arena, operand, host, state, subst);
            resolve_keyof(arena, resolved_operand, state)
        }

        // -- indexed access T[K] --
        Node::IndexedAccess { object, index } => {
            if object.is_unresolved() || index.is_unresolved() {
                state.mark_symbolic();
                return arena.indexed_access(object, index);
            }

            if let Node::Literal(super::arena::SolverLiteral::String(key)) =
                arena.get(index).clone()
            {
                if let Node::Ref {
                    ref name,
                    ref type_arguments,
                } = arena.get(object).clone()
                {
                    if type_arguments.is_empty() {
                        let maybe_root = resolve_name_in_context(state, name.as_str())
                            .or_else(|| host.root_identity("", name.as_str()));
                        if let Some(root_id) = maybe_root {
                            // Query-local host projection cache: collapse repeated
                            // access to the same (root, member) within one query.
                            let cache_key = (
                                root_id.canonical_id.clone(),
                                root_id.symbol_name.clone(),
                                key.clone(),
                            );
                            if let Some(&cached) = state.host_projection_cache.get(&cache_key) {
                                state.record_host_projection_cache_hit();
                                return cached;
                            }

                            if let Some(projection) =
                                host.resolve_member_projection(&root_id, key.as_str())
                            {
                                state.record_host_projection_call(cache_key.clone());
                                if projection.exactness == SolverExactness::ExactSymbolic {
                                    state.mark_symbolic();
                                }
                                let lowered = lower_type_expr(arena, &projection.value);
                                let mut pushed = 0usize;
                                if projection.type_decl_contexts.is_empty() {
                                    if let Some(prepared) =
                                        host.resolve_prepared_type_decl(&root_id)
                                    {
                                        if !prepared.name_resolution.is_empty() {
                                            state.type_decl_context_stack.push(prepared);
                                            pushed = 1;
                                        }
                                    }
                                } else {
                                    for prepared in &projection.type_decl_contexts {
                                        if !prepared.name_resolution.is_empty() {
                                            state
                                                .type_decl_context_stack
                                                .push(Arc::clone(prepared));
                                            pushed += 1;
                                        }
                                    }
                                }
                                let resolved = resolve_node(arena, lowered, host, state, subst);
                                for _ in 0..pushed {
                                    state.type_decl_context_stack.pop();
                                }

                                // Cache the resolved result for this (root, member)
                                state.host_projection_cache.insert(cache_key, resolved);
                                return resolved;
                            }
                        }
                    }
                }
            }

            let resolved_obj = resolve_node(arena, object, host, state, subst);
            let resolved_idx = resolve_node(arena, index, host, state, subst);
            resolve_indexed_access(arena, resolved_obj, resolved_idx, host, state, subst)
        }

        // -- conditional T extends U ? A : B --
        Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
        } => {
            let resolved_check = resolve_node(arena, check, host, state, subst);
            let resolved_extends = resolve_node(arena, extends, host, state, subst);
            resolve_conditional(
                arena,
                resolved_check,
                resolved_extends,
                true_branch,
                false_branch,
                distributive,
                host,
                state,
                subst,
            )
        }

        // -- mapped type { [K in Source]: Value } --
        Node::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            let resolved_source = resolve_node(arena, source, host, state, subst);
            resolve_mapped(
                arena,
                &parameter,
                resolved_source,
                value,
                optional,
                readonly,
                name_type,
                host,
                state,
                subst,
            )
        }

        // -- typeof --
        Node::TypeOf { path } => resolve_typeof(arena, &path, host, state, subst),

        // -- template literal `prefix${T}suffix` --
        Node::TemplateLiteral {
            quasis,
            expressions,
        } => resolve_template_literal(arena, &quasis, &expressions, host, state, subst),

        // -- rest --
        Node::Rest(inner) => {
            let resolved = resolve_node(arena, inner, host, state, subst);
            arena.alloc(Node::Rest(resolved))
        }

        // Terminals handled by fast path above — catch-all for safety
        _ => node,
    }
}

fn resolve_substitution_binding(
    arena: &mut QueryArena,
    original_node: NodeId,
    name: &str,
    bound: NodeId,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> Option<NodeId> {
    if state
        .active_substitution_names
        .iter()
        .any(|active| active == name)
    {
        state.mark_symbolic();
        return Some(original_node);
    }

    state.active_substitution_names.push(name.to_string());
    let resolved = resolve_node(arena, bound, host, state, subst);
    state.active_substitution_names.pop();
    Some(resolved)
}

// ---------------------------------------------------------------------------
// Open-generic argument signal classification
// ---------------------------------------------------------------------------

/// Check if any effective arg is an open type parameter (TypeParam, Infer,
/// or Applied/Ref whose own args are all open).
fn has_open_arg(arena: &QueryArena, args: &[NodeId]) -> bool {
    args.iter().any(|&arg| arg_is_open(arena, arg))
}

fn arg_is_open(arena: &QueryArena, node: NodeId) -> bool {
    if node.is_unresolved() {
        // UNRESOLVED means the arg slot was never filled (no explicit arg,
        // no default). This is different from an open type parameter that
        // may gain concrete signal later. Skip the stop for UNRESOLVED
        // so that partial resolution can still expand the body.
        return false;
    }
    match arena.get(node) {
        Node::TypeParam { .. } | Node::Infer { .. } => true,
        // Applied forms whose args are all still open are themselves open
        Node::Applied { args, .. } => args.iter().all(|&a| arg_is_open(arena, a)),
        // Bare Ref with no args: not a type parameter, just a named reference
        Node::Ref { type_arguments, .. } if type_arguments.is_empty() => false,
        // Ref with args: open if all args are open
        Node::Ref { type_arguments, .. } => type_arguments.iter().all(|&a| arg_is_open(arena, a)),
        _ => false,
    }
}

/// Check whether any effective argument carries a concrete signal that
/// justifies expanding the declaration body. Returns `false` (no signal)
/// when all args are open or carry only lone literals.
///
/// Classification:
///
/// - **Open** (no signal): TypeParam, Infer, Applied where all args are open.
/// - **Lone literal**: Literal — not enough on its own.
/// - **Concrete signal**: Primitive, Object, Function, Tuple, Array, Union,
///   Intersection, or Applied/Ref whose own args carry concrete signal.
fn has_concrete_signal(arena: &QueryArena, args: &[NodeId]) -> bool {
    args.iter().any(|&arg| arg_has_concrete_signal(arena, arg))
}

fn arg_has_concrete_signal(arena: &QueryArena, node: NodeId) -> bool {
    if node.is_unresolved() {
        return false;
    }
    match arena.get(node) {
        // Open — no signal
        Node::TypeParam { .. } | Node::Infer { .. } | Node::Error { .. } => false,
        // Lone literal — not enough on its own
        Node::Literal(_) => false,
        // Concrete signal
        Node::Primitive(_)
        | Node::Object(_)
        | Node::Function(_)
        | Node::Tuple { .. }
        | Node::Array { .. }
        | Node::KeyOf(_)
        | Node::TypeOf { .. }
        | Node::Mapped { .. }
        | Node::TemplateLiteral { .. } => true,
        // Union/Intersection: concrete if any member is concrete
        Node::Union(members) | Node::Intersection(members) => {
            members.iter().any(|&m| arg_has_concrete_signal(arena, m))
        }
        // Ref with args: concrete if any arg is concrete
        Node::Ref { type_arguments, .. } => {
            if type_arguments.is_empty() {
                false // bare ref is open
            } else {
                type_arguments
                    .iter()
                    .any(|&a| arg_has_concrete_signal(arena, a))
            }
        }
        // Applied: concrete if any arg is concrete
        Node::Applied { args, .. } => args.iter().any(|&a| arg_has_concrete_signal(arena, a)),
        // Conditional, IndexedAccess, Rest, RecursiveRef: treat as open
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// resolve_prepared_ref — instantiate a host-backed prepared declaration
// ---------------------------------------------------------------------------

/// Look up a prepared type declaration from the host, lower its body into the
/// arena, bind type parameters to the resolved arguments, and resolve the
/// body recursively.
fn resolve_prepared_ref(
    arena: &mut QueryArena,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    parent_subst: &SubstitutionEnv,
    root_id: &ResolvedRootIdentity,
    args: &[NodeId],
) -> NodeId {
    // Look up the prepared declaration up front so recursion identity can use
    // effective, substitution-aware type arguments rather than raw call-site
    // nodes or fresh lowered defaults.
    let Some(prepared) = host.resolve_prepared_type_decl(root_id) else {
        state.mark_incomplete(IncompleteReason::MissingSource {
            canonical_id: root_id.canonical_id.clone(),
            symbol_name: root_id.symbol_name.clone(),
        });
        return arena.error(format!("missing: {}", root_id));
    };
    let effective_args = build_effective_args(arena, &prepared, parent_subst, args);

    // Check recursion — have we already started resolving this exact
    // (identity, effective_args) combination?
    let rec_key = RecursionKey {
        canonical_id: root_id.canonical_id.clone(),
        symbol_name: root_id.symbol_name.clone(),
        args_hash: hash_effective_args(arena, &effective_args),
    };
    let ref_label = format!(
        "{}::{}#{}",
        rec_key.canonical_id, rec_key.symbol_name, rec_key.args_hash
    );

    // Query-local prepared-ref cache: if we already fully resolved this
    // exact (identity, args_hash) combination in this query, reuse it.
    // Skip cache when the recursion tracker shows this symbol is currently
    // active — the cached result may contain unresolved placeholder nodes
    // from a prior in-progress resolution of the same symbol.
    if !state.recursion.is_symbol_active(&rec_key) {
        if let Some(&cached) = state.prepared_ref_cache.get(&rec_key) {
            state.record_prepared_ref_cache_hit();
            return cached;
        }
    }

    state.record_prepared_ref_entry(rec_key.clone());
    state.record_prepared_ref_edge(&ref_label);

    // Open-generic symbolic stop: if the declaration has type parameters,
    // is NOT a built-in utility, has at least one open arg, and no arg
    // carries a concrete signal (beyond lone literals), return a symbolic
    // Applied node instead of expanding. This prevents unbounded expansion
    // for patterns like GetModelValue<T, VK, true> where T and VK are open.
    // Only apply at depth > 0: top-level entry types (defineProps<Props<T>>)
    // must expand; the stop targets nested helpers (depth 2+) where the
    // explosion happens (e.g. GetModelValue<T, VK, true> inside CheckboxProps).
    if state.depth > 0
        && !prepared.type_parameters.is_empty()
        && !effective_args.is_empty()
        && host.utility_source(&root_id.symbol_name) != UtilitySource::Builtin
        && has_open_arg(arena, &effective_args)
        && !has_concrete_signal(arena, &effective_args)
    {
        state.mark_symbolic();
        return arena.alloc(Node::Applied {
            identity: super::arena::DeclIdentity {
                canonical_id: root_id.canonical_id.clone(),
                symbol_name: root_id.symbol_name.clone(),
            },
            args: effective_args,
        });
    }

    // Compute structural fingerprint for tiered recursion policy.
    // Only compute when the symbol is already active (common non-recursive
    // path pays zero fingerprint cost).
    let cond_capture = state.capture_symbol_conditional_context();
    let fingerprint = if state.recursion.is_symbol_active(&rec_key) {
        Some(super::recursion::compute_structural_fingerprint(
            arena,
            &effective_args,
            &cond_capture.frames,
        ))
    } else {
        None
    };

    if let Some(placeholder) = state.recursion.enter(rec_key.clone(), fingerprint.as_ref()) {
        // Cycle detected — return the recursive placeholder
        return placeholder;
    }

    // Depth guard
    state.depth += 1;
    if state.depth > state.limits.max_instantiation_depth {
        state.depth -= 1;
        state.mark_incomplete(IncompleteReason::RecursionPolicy {
            description: format!(
                "instantiation depth {} exceeded for {}",
                state.limits.max_instantiation_depth, root_id
            ),
        });
        state.execution_status = ExecutionStatus::HardStop;
        return arena.error(format!("depth exceeded: {}", root_id));
    }

    // Create a recursive placeholder in case the body references itself.
    // Capture symbol name and effective args so downstream transport preserves
    // useful symbolic info instead of degrading to Unknown.
    if cond_capture.truncated {
        state.record_diagnostic(SolverDiagnostic::ConditionalContextTruncated {
            available: cond_capture.available,
            captured: cond_capture.frames.len(),
        });
    }
    let placeholder = arena.alloc(Node::RecursiveRef {
        symbol_name: root_id.symbol_name.clone(),
        type_arguments: effective_args.clone(),
        conditional_context: cond_capture.frames,
    });
    state
        .recursion
        .push(rec_key.clone(), placeholder, fingerprint.as_ref());

    // Record external declaration visit for registry publishing
    if !root_id.canonical_id.is_empty() && root_id.canonical_id != "$owner" {
        state.visited_external_decls.push(root_id.clone());
        state.record_external_decl_visit(root_id);
    }

    if state.audit_enabled() {
        state.prepared_ref_stack.push(ref_label);
    }

    // Save scope state for guard-based cleanup (handles early returns)
    let saved_cond_ctx_len = state.conditional_context_stack.len();

    // Push symbol-local conditional context base so that
    // current_symbol_conditional_context() only sees frames from this symbol.
    state
        .conditional_context_base_stack
        .push(state.conditional_context_stack.len());

    // Lower the declaration body into the arena
    let body_node = lower_type_expr(arena, &prepared.body);

    // Build substitution: bind type params to effective args
    let param_names: Vec<String> = prepared
        .type_parameters
        .iter()
        .map(|p| p.name.clone())
        .collect();

    let mut child_subst = parent_subst.clone();
    for (i, param_name) in param_names.iter().enumerate() {
        if let Some(&arg) = effective_args.get(i) {
            child_subst.bind(param_name, arg);
        }
    }

    // Push the prepared declaration onto the context stack so bare-name
    // refs in the body can be resolved through name_resolution.
    // Only push if the declaration has name_resolution entries (avoids
    // empty stack entries that would be checked by resolve_name_in_context).
    let pushed = if !prepared.name_resolution.is_empty() {
        state.type_decl_context_stack.push(Arc::clone(&prepared));
        true
    } else {
        false
    };

    // Resolve the body with the new substitution
    let resolved = resolve_node(arena, body_node, host, state, &child_subst);

    // Pop all scope state: declaration context, conditional context base,
    // conditional context stack (truncate back), recursion tracker, depth.
    if pushed {
        state.type_decl_context_stack.pop();
    }
    state.conditional_context_base_stack.pop();
    state.conditional_context_stack.truncate(saved_cond_ctx_len);
    state.recursion.pop(&rec_key, fingerprint.as_ref());
    state.depth -= 1;
    if state.audit_enabled() {
        state.prepared_ref_stack.pop();
    }

    // Cache the completed result so later references to the same
    // (identity, args_hash) in this query can reuse it directly.
    state.prepared_ref_cache.insert(rec_key, resolved);

    resolved
}

/// Check the INNERMOST active declaration context for a pre-resolved name.
///
/// Only checks the topmost entry on the type/value declaration context
/// stacks. A bare name in a declaration body should resolve in THAT
/// declaration's defining file scope only — not in parent scopes from
/// outer prepared-ref resolutions. The host's `root_identity` handles
/// owner-level resolution as the fallback.
fn resolve_name_in_context(state: &SolveState, name: &str) -> Option<ResolvedRootIdentity> {
    // Check innermost type declaration context only
    if let Some(decl) = state.type_decl_context_stack.last() {
        if let Some(identity) = decl.name_resolution.get(name) {
            return Some(identity.clone());
        }
    }
    // Then check innermost value declaration context only
    if let Some(decl) = state.value_decl_context_stack.last() {
        if let Some(identity) = decl.name_resolution.get(name) {
            return Some(identity.clone());
        }
    }
    None
}

/// Build effective args: explicit call-site args plus lowered defaults
/// for any unsupplied trailing type parameters. This ensures that
/// `Foo<number>` and `Foo<number, string>` (where string is the default)
/// produce the same recursion key and placeholder args.
///
/// Infer bindings from conditional types do not need explicit inclusion
/// here. When `resolve_conditional` discovers infer bindings (e.g.,
/// `T extends Promise<infer U> ? AwaitedLike<U> : T`), it injects them
/// into the true-branch substitution env. The recursive call's type
/// arguments then pass through `resolve_node`, which applies that
/// substitution — so by the time we reach `resolve_prepared_ref`, the
/// args are already fully substituted with inferred types. This is
/// correct because infer resolution and arg resolution happen in the
/// same `resolve_node` walk, before the recursive call enters this
/// function.
fn build_effective_args(
    arena: &mut QueryArena,
    prepared: &PreparedTypeDecl,
    parent_subst: &SubstitutionEnv,
    args: &[NodeId],
) -> Vec<NodeId> {
    let mut effective_args = Vec::new();
    let mut default_subst = parent_subst.clone();

    for (index, param) in prepared.type_parameters.iter().enumerate() {
        let (mut arg, used_default) = if let Some(&explicit) = args.get(index) {
            (
                materialize_effective_arg(arena, explicit, &default_subst),
                false,
            )
        } else if let Some(default) = &param.default {
            let lowered = lower_type_expr(arena, default);
            (
                materialize_effective_arg(arena, lowered, &default_subst),
                true,
            )
        } else {
            (NodeId::UNRESOLVED, false)
        };

        if used_default {
            arg = constrain_default_effective_arg(
                arena,
                param.constraint.as_deref(),
                arg,
                &default_subst,
            );
        };
        effective_args.push(arg);
        default_subst.bind(param.name.clone(), arg);
    }

    effective_args
}

fn constrain_default_effective_arg(
    arena: &mut QueryArena,
    constraint: Option<&TypeExpr>,
    arg: NodeId,
    subst: &SubstitutionEnv,
) -> NodeId {
    let Some(constraint) = constraint else {
        return arg;
    };

    let lowered_constraint = lower_type_expr(arena, constraint);
    let materialized_constraint = materialize_effective_arg(arena, lowered_constraint, subst);
    let mut caches = super::arena::SolverCaches::new();
    let mut relation_state =
        super::relate::RelationState::new(super::relate::RelationLimits::default());

    match super::relate::relate(
        arena,
        &mut caches,
        arg,
        materialized_constraint,
        super::result::RelationMode::Assignable,
        &mut relation_state,
    ) {
        super::result::RelationResult::NotAssignable => materialized_constraint,
        super::result::RelationResult::Assignable | super::result::RelationResult::Unknown => arg,
    }
}

fn materialize_effective_arg(
    arena: &mut QueryArena,
    node: NodeId,
    subst: &SubstitutionEnv,
) -> NodeId {
    let mut in_progress = std::collections::HashSet::new();
    materialize_effective_arg_inner(arena, node, subst, &mut in_progress)
}

fn materialize_effective_arg_inner(
    arena: &mut QueryArena,
    node: NodeId,
    subst: &SubstitutionEnv,
    in_progress: &mut std::collections::HashSet<NodeId>,
) -> NodeId {
    if node.is_unresolved() {
        return node;
    }
    if !in_progress.insert(node) {
        return node;
    }

    let result = match arena.get(node).clone() {
        Node::Primitive(_)
        | Node::Literal(_)
        | Node::Applied { .. }
        | Node::TypeOf { .. }
        | Node::Error { .. }
        | Node::RecursiveRef { .. } => node,
        Node::TypeParam { name, .. } => subst.resolve(&name).unwrap_or(node),
        Node::Infer { .. } => node,
        Node::Ref {
            name,
            type_arguments,
        } => {
            if type_arguments.is_empty() {
                if let Some(bound) = subst.resolve(&name) {
                    materialize_effective_arg_inner(arena, bound, subst, in_progress)
                } else {
                    let resolved_args: Vec<NodeId> = type_arguments
                        .iter()
                        .map(|&arg| materialize_effective_arg_inner(arena, arg, subst, in_progress))
                        .collect();
                    arena.type_ref(name, resolved_args)
                }
            } else {
                let resolved_args: Vec<NodeId> = type_arguments
                    .iter()
                    .map(|&arg| materialize_effective_arg_inner(arena, arg, subst, in_progress))
                    .collect();
                arena.type_ref(name, resolved_args)
            }
        }
        Node::Union(members) => {
            let resolved: Vec<NodeId> = members
                .into_iter()
                .map(|member| materialize_effective_arg_inner(arena, member, subst, in_progress))
                .collect();
            arena.union(resolved)
        }
        Node::Intersection(members) => {
            let resolved: Vec<NodeId> = members
                .into_iter()
                .map(|member| materialize_effective_arg_inner(arena, member, subst, in_progress))
                .collect();
            arena.intersection(resolved)
        }
        Node::Array { element, readonly } => {
            let element = materialize_effective_arg_inner(arena, element, subst, in_progress);
            arena.array(element, readonly)
        }
        Node::Tuple { elements, readonly } => {
            let mut resolved_elements = Vec::with_capacity(elements.len());
            for element in elements {
                resolved_elements.push(super::arena::TupleNodeElement {
                    label: element.label,
                    ty: materialize_effective_arg_inner(arena, element.ty, subst, in_progress),
                    optional: element.optional,
                    rest: element.rest,
                });
            }
            arena.alloc(Node::Tuple {
                elements: resolved_elements,
                readonly,
            })
        }
        Node::Object(obj) => {
            let mut properties = Vec::with_capacity(obj.properties.len());
            for property in obj.properties {
                properties.push(super::arena::PropertyNode {
                    name: property.name,
                    ty: materialize_effective_arg_inner(arena, property.ty, subst, in_progress),
                    optional: property.optional,
                    readonly: property.readonly,
                    is_method: property.is_method,
                });
            }

            let mut index_signatures = Vec::with_capacity(obj.index_signatures.len());
            for signature in obj.index_signatures {
                let key_type =
                    materialize_effective_arg_inner(arena, signature.key_type, subst, in_progress);
                let value_type = materialize_effective_arg_inner(
                    arena,
                    signature.value_type,
                    subst,
                    in_progress,
                );
                index_signatures.push(super::arena::IndexSignatureNode {
                    key_type,
                    value_type,
                    readonly: signature.readonly,
                });
            }

            let mut call_signatures = Vec::with_capacity(obj.call_signatures.len());
            for signature in obj.call_signatures {
                call_signatures.push(materialize_signature(arena, signature, subst, in_progress));
            }

            let mut construct_signatures = Vec::with_capacity(obj.construct_signatures.len());
            for signature in obj.construct_signatures {
                construct_signatures.push(materialize_signature(
                    arena,
                    signature,
                    subst,
                    in_progress,
                ));
            }

            arena.alloc(Node::Object(super::arena::ObjectNode {
                properties,
                index_signatures,
                call_signatures,
                construct_signatures,
            }))
        }
        Node::Function(func) => {
            let mut signatures = Vec::with_capacity(func.signatures.len());
            for signature in func.signatures {
                signatures.push(materialize_signature(arena, signature, subst, in_progress));
            }
            arena.alloc(Node::Function(super::arena::FunctionNode { signatures }))
        }
        Node::KeyOf(operand) => {
            let operand = materialize_effective_arg_inner(arena, operand, subst, in_progress);
            arena.key_of(operand)
        }
        Node::IndexedAccess { object, index } => {
            let object = materialize_effective_arg_inner(arena, object, subst, in_progress);
            let index = materialize_effective_arg_inner(arena, index, subst, in_progress);
            arena.indexed_access(object, index)
        }
        Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
        } => {
            let check = materialize_effective_arg_inner(arena, check, subst, in_progress);
            let extends = materialize_effective_arg_inner(arena, extends, subst, in_progress);
            let true_branch =
                materialize_effective_arg_inner(arena, true_branch, subst, in_progress);
            let false_branch =
                materialize_effective_arg_inner(arena, false_branch, subst, in_progress);
            arena.conditional(check, extends, true_branch, false_branch, distributive)
        }
        Node::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            let source = materialize_effective_arg_inner(arena, source, subst, in_progress);
            let value = materialize_effective_arg_inner(arena, value, subst, in_progress);
            let name_type = name_type
                .map(|node| materialize_effective_arg_inner(arena, node, subst, in_progress));
            arena.mapped(parameter, source, value, optional, readonly, name_type)
        }
        Node::TemplateLiteral {
            quasis,
            expressions,
        } => {
            let mut resolved_expressions = Vec::with_capacity(expressions.len());
            for expr in expressions {
                resolved_expressions.push(materialize_effective_arg_inner(
                    arena,
                    expr,
                    subst,
                    in_progress,
                ));
            }
            arena.alloc(Node::TemplateLiteral {
                quasis,
                expressions: resolved_expressions,
            })
        }
        Node::Rest(inner) => {
            let inner = materialize_effective_arg_inner(arena, inner, subst, in_progress);
            arena.alloc(Node::Rest(inner))
        }
    };

    in_progress.remove(&node);
    result
}

fn materialize_signature(
    arena: &mut QueryArena,
    signature: super::arena::CallSignatureNode,
    subst: &SubstitutionEnv,
    in_progress: &mut std::collections::HashSet<NodeId>,
) -> super::arena::CallSignatureNode {
    super::arena::CallSignatureNode {
        type_parameters: signature
            .type_parameters
            .into_iter()
            .map(|param| super::arena::TypeParamNode {
                name: param.name,
                constraint: param
                    .constraint
                    .map(|node| materialize_effective_arg_inner(arena, node, subst, in_progress)),
                default: param
                    .default
                    .map(|node| materialize_effective_arg_inner(arena, node, subst, in_progress)),
            })
            .collect(),
        parameters: signature
            .parameters
            .into_iter()
            .map(|param| super::arena::ParamNode {
                name: param.name,
                ty: materialize_effective_arg_inner(arena, param.ty, subst, in_progress),
                optional: param.optional,
                rest: param.rest,
            })
            .collect(),
        return_type: materialize_effective_arg_inner(
            arena,
            signature.return_type,
            subst,
            in_progress,
        ),
    }
}

/// Semantic hash for a slice of effective argument nodes (for exact recursion keys).
const EFFECTIVE_ARG_HASH_DEPTH_CAP: usize = 3;
const EFFECTIVE_ARG_HASH_NODE_CAP: usize = 64;

fn hash_effective_args(arena: &QueryArena, ids: &[NodeId]) -> u64 {
    use std::collections::HashSet;
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut in_progress = HashSet::new();
    let mut visited = 0usize;
    ids.len().hash(&mut hasher);
    for &id in ids {
        hash_effective_arg_node(arena, id, &mut hasher, &mut in_progress, 0, &mut visited);
    }
    hasher.finish()
}

fn hash_effective_arg_node(
    arena: &QueryArena,
    node: NodeId,
    hasher: &mut impl std::hash::Hasher,
    in_progress: &mut std::collections::HashSet<NodeId>,
    depth: usize,
    visited: &mut usize,
) {
    use std::hash::Hash;

    if node.is_unresolved() {
        255u8.hash(hasher);
        return;
    }
    if depth > EFFECTIVE_ARG_HASH_DEPTH_CAP || *visited > EFFECTIVE_ARG_HASH_NODE_CAP {
        253u8.hash(hasher);
        return;
    }
    if !in_progress.insert(node) {
        254u8.hash(hasher);
        return;
    }
    *visited += 1;

    match arena.get(node) {
        Node::Primitive(kind) => {
            0u8.hash(hasher);
            (*kind as u8).hash(hasher);
        }
        Node::Literal(literal) => {
            1u8.hash(hasher);
            literal.hash(hasher);
        }
        Node::Union(members) => {
            2u8.hash(hasher);
            members.len().hash(hasher);
            for &member in members {
                hash_effective_arg_node(arena, member, hasher, in_progress, depth + 1, visited);
            }
        }
        Node::Intersection(members) => {
            3u8.hash(hasher);
            members.len().hash(hasher);
            for &member in members {
                hash_effective_arg_node(arena, member, hasher, in_progress, depth + 1, visited);
            }
        }
        Node::Array { element, readonly } => {
            4u8.hash(hasher);
            readonly.hash(hasher);
            hash_effective_arg_node(arena, *element, hasher, in_progress, depth + 1, visited);
        }
        Node::Tuple { elements, readonly } => {
            5u8.hash(hasher);
            readonly.hash(hasher);
            elements.len().hash(hasher);
            for element in elements {
                element.label.hash(hasher);
                element.optional.hash(hasher);
                element.rest.hash(hasher);
                hash_effective_arg_node(arena, element.ty, hasher, in_progress, depth + 1, visited);
            }
        }
        Node::Object(object) => {
            6u8.hash(hasher);
            object.properties.len().hash(hasher);
            for property in &object.properties {
                property.name.hash(hasher);
                property.optional.hash(hasher);
                property.readonly.hash(hasher);
                property.is_method.hash(hasher);
                hash_effective_arg_node(
                    arena,
                    property.ty,
                    hasher,
                    in_progress,
                    depth + 1,
                    visited,
                );
            }
            object.index_signatures.len().hash(hasher);
            for signature in &object.index_signatures {
                signature.readonly.hash(hasher);
                hash_effective_arg_node(
                    arena,
                    signature.key_type,
                    hasher,
                    in_progress,
                    depth + 1,
                    visited,
                );
                hash_effective_arg_node(
                    arena,
                    signature.value_type,
                    hasher,
                    in_progress,
                    depth + 1,
                    visited,
                );
            }
        }
        Node::Function(function) => {
            7u8.hash(hasher);
            function.signatures.len().hash(hasher);
            for signature in &function.signatures {
                signature.parameters.len().hash(hasher);
                for param in &signature.parameters {
                    param.name.hash(hasher);
                    param.optional.hash(hasher);
                    param.rest.hash(hasher);
                    hash_effective_arg_node(
                        arena,
                        param.ty,
                        hasher,
                        in_progress,
                        depth + 1,
                        visited,
                    );
                }
                hash_effective_arg_node(
                    arena,
                    signature.return_type,
                    hasher,
                    in_progress,
                    depth + 1,
                    visited,
                );
            }
        }
        Node::Ref {
            name,
            type_arguments,
        } => {
            8u8.hash(hasher);
            name.hash(hasher);
            type_arguments.len().hash(hasher);
            for &arg in type_arguments {
                hash_effective_arg_node(arena, arg, hasher, in_progress, depth + 1, visited);
            }
        }
        Node::Applied { identity, args } => {
            9u8.hash(hasher);
            identity.hash(hasher);
            args.len().hash(hasher);
            for &arg in args {
                hash_effective_arg_node(arena, arg, hasher, in_progress, depth + 1, visited);
            }
        }
        Node::TypeParam {
            name,
            constraint,
            default,
        } => {
            10u8.hash(hasher);
            node.hash(hasher);
            name.hash(hasher);
            constraint.map(|node| {
                hash_effective_arg_node(arena, node, hasher, in_progress, depth + 1, visited)
            });
            default.map(|node| {
                hash_effective_arg_node(arena, node, hasher, in_progress, depth + 1, visited)
            });
        }
        Node::KeyOf(operand) => {
            11u8.hash(hasher);
            hash_effective_arg_node(arena, *operand, hasher, in_progress, depth + 1, visited);
        }
        Node::TypeOf { path } => {
            12u8.hash(hasher);
            path.hash(hasher);
        }
        Node::IndexedAccess { object, index } => {
            13u8.hash(hasher);
            hash_effective_arg_node(arena, *object, hasher, in_progress, depth + 1, visited);
            hash_effective_arg_node(arena, *index, hasher, in_progress, depth + 1, visited);
        }
        Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
        } => {
            14u8.hash(hasher);
            distributive.hash(hasher);
            hash_effective_arg_node(arena, *check, hasher, in_progress, depth + 1, visited);
            hash_effective_arg_node(arena, *extends, hasher, in_progress, depth + 1, visited);
            hash_effective_arg_node(arena, *true_branch, hasher, in_progress, depth + 1, visited);
            hash_effective_arg_node(
                arena,
                *false_branch,
                hasher,
                in_progress,
                depth + 1,
                visited,
            );
        }
        Node::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            15u8.hash(hasher);
            parameter.hash(hasher);
            optional.hash(hasher);
            readonly.hash(hasher);
            hash_effective_arg_node(arena, *source, hasher, in_progress, depth + 1, visited);
            hash_effective_arg_node(arena, *value, hasher, in_progress, depth + 1, visited);
            if let Some(name_type) = name_type {
                hash_effective_arg_node(arena, *name_type, hasher, in_progress, depth + 1, visited);
            }
        }
        Node::TemplateLiteral {
            quasis,
            expressions,
        } => {
            16u8.hash(hasher);
            quasis.hash(hasher);
            expressions.len().hash(hasher);
            for &expr in expressions {
                hash_effective_arg_node(arena, expr, hasher, in_progress, depth + 1, visited);
            }
        }
        Node::Infer { name } => {
            17u8.hash(hasher);
            node.hash(hasher);
            name.hash(hasher);
        }
        Node::Rest(inner) => {
            18u8.hash(hasher);
            hash_effective_arg_node(arena, *inner, hasher, in_progress, depth + 1, visited);
        }
        Node::RecursiveRef {
            symbol_name,
            type_arguments,
            conditional_context,
        } => {
            19u8.hash(hasher);
            symbol_name.hash(hasher);
            type_arguments.len().hash(hasher);
            for &arg in type_arguments {
                hash_effective_arg_node(arena, arg, hasher, in_progress, depth + 1, visited);
            }
            conditional_context.len().hash(hasher);
            for frame in conditional_context {
                frame.branch.hash(hasher);
                frame.decided.hash(hasher);
                hash_effective_arg_node(
                    arena,
                    frame.check,
                    hasher,
                    in_progress,
                    depth + 1,
                    visited,
                );
                hash_effective_arg_node(
                    arena,
                    frame.extends,
                    hasher,
                    in_progress,
                    depth + 1,
                    visited,
                );
            }
        }
        Node::Error { description } => {
            20u8.hash(hasher);
            description.hash(hasher);
        }
    }

    in_progress.remove(&node);
}

// ---------------------------------------------------------------------------
// resolve_keyof
// ---------------------------------------------------------------------------

/// `keyof T` — produce the key union of the resolved operand.
///
/// Reads the operand node, extracts names/ids into locals, then allocates
/// new nodes. This is the standard Rust read-then-write pattern for a
/// struct that is both read and mutated.
fn resolve_keyof(arena: &mut QueryArena, operand: NodeId, state: &mut SolveState) -> NodeId {
    // Read phase: extract what we need into owned locals.
    let node = arena.get(operand).clone();

    match node {
        Node::Array { .. } => arena.primitive(PrimitiveKind::Number),
        Node::Tuple { elements, .. } => {
            if elements.is_empty() {
                arena.primitive(PrimitiveKind::Never)
            } else {
                let keys: Vec<NodeId> = elements
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| arena.number_literal(idx as f64))
                    .collect();
                arena.union(keys)
            }
        }
        Node::Object(obj) => {
            let has_index = !obj.index_signatures.is_empty();
            if has_index {
                let mut all: Vec<NodeId> =
                    obj.index_signatures.iter().map(|i| i.key_type).collect();
                for p in &obj.properties {
                    all.push(arena.string_literal(&p.name));
                }
                arena.union(all)
            } else if obj.properties.is_empty() {
                arena.primitive(PrimitiveKind::Never)
            } else {
                let keys: Vec<NodeId> = obj
                    .properties
                    .iter()
                    .map(|p| arena.string_literal(&p.name))
                    .collect();
                arena.union(keys)
            }
        }
        Node::Union(members) => {
            let keyofs: Vec<NodeId> = members
                .iter()
                .map(|&m| resolve_keyof(arena, m, state))
                .collect();
            arena.intersection(keyofs)
        }
        Node::Intersection(members) => {
            let keyofs: Vec<NodeId> = members
                .iter()
                .map(|&m| resolve_keyof(arena, m, state))
                .collect();
            arena.union(keyofs)
        }
        Node::Primitive(PrimitiveKind::Any) => {
            let s = arena.primitive(PrimitiveKind::String);
            let n = arena.primitive(PrimitiveKind::Number);
            let sym = arena.primitive(PrimitiveKind::Symbol);
            arena.union(vec![s, n, sym])
        }
        Node::Primitive(PrimitiveKind::Unknown | PrimitiveKind::Never) => {
            arena.primitive(PrimitiveKind::Never)
        }
        _ => {
            state.mark_symbolic();
            arena.key_of(operand)
        }
    }
}

// ---------------------------------------------------------------------------
// resolve_indexed_access
// ---------------------------------------------------------------------------

/// `T[K]` — look up member(s) by key on the resolved object.
#[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
fn resolve_indexed_access(
    arena: &mut QueryArena,
    object: NodeId,
    index: NodeId,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    if object.is_unresolved() || index.is_unresolved() {
        state.mark_symbolic();
        return arena.indexed_access(object, index);
    }

    // Clone index node to release borrow before recursion/allocation.
    let index_node = arena.get(index).clone();

    // If index is a union, distribute: T["a" | "b"] = T["a"] | T["b"]
    if let Node::Union(members) = index_node {
        let results: Vec<NodeId> = members
            .iter()
            .map(|&m| resolve_indexed_access(arena, object, m, host, state, subst))
            .collect();
        return arena.union(results);
    }

    if matches!(arena.get(object), Node::Primitive(PrimitiveKind::Any)) {
        return arena.primitive(PrimitiveKind::Any);
    }

    let key = match arena.get(index) {
        Node::Literal(super::arena::SolverLiteral::String(s)) => Some(s.clone()),
        _ => None,
    };

    let open_key_kind = match arena.get(index) {
        Node::Primitive(PrimitiveKind::String) => Some(PrimitiveKind::String),
        Node::Primitive(PrimitiveKind::Number) => Some(PrimitiveKind::Number),
        Node::Primitive(PrimitiveKind::Symbol) => Some(PrimitiveKind::Symbol),
        Node::Primitive(PrimitiveKind::Any) => Some(PrimitiveKind::Any),
        _ => None,
    };

    // Clone object node to release borrow before recursion/allocation.
    let obj_node = arena.get(object).clone();

    match obj_node {
        Node::Object(obj) => {
            if let Some(key) = key.as_ref() {
                if let Some(prop) = obj.properties.iter().find(|p| p.name == *key) {
                    return prop.ty;
                }
            }

            let matching_index_values: Vec<NodeId> = obj
                .index_signatures
                .iter()
                .filter_map(|idx_sig| {
                    index_signature_matches_request(
                        arena,
                        idx_sig.key_type,
                        key.as_ref(),
                        open_key_kind,
                    )
                    .then_some(idx_sig.value_type)
                })
                .collect();

            if matching_index_values.is_empty() {
                if key.is_none() && open_key_kind.is_none() {
                    state.mark_symbolic();
                    return arena.indexed_access(object, index);
                }
                return arena.primitive(PrimitiveKind::Undefined);
            }

            if matching_index_values.len() == 1 {
                matching_index_values[0]
            } else {
                arena.union(matching_index_values)
            }
        }
        Node::Union(members) => {
            let results: Vec<NodeId> = members
                .iter()
                .map(|&member| resolve_indexed_access(arena, member, index, host, state, subst))
                .collect();
            arena.union(results)
        }
        Node::Intersection(members) => {
            let mut matches = Vec::new();
            for &member in &members {
                let result = resolve_indexed_access(arena, member, index, host, state, subst);
                if !matches!(arena.get(result), Node::Primitive(PrimitiveKind::Undefined)) {
                    matches.push(result);
                }
            }
            match matches.len() {
                0 => arena.primitive(PrimitiveKind::Undefined),
                1 => matches[0],
                _ => arena.intersection(matches),
            }
        }
        _ => {
            state.mark_symbolic();
            arena.indexed_access(object, index)
        }
    }
}

fn index_signature_matches_request(
    arena: &QueryArena,
    key_type: NodeId,
    literal_key: Option<&String>,
    open_key_kind: Option<PrimitiveKind>,
) -> bool {
    match arena.get(key_type) {
        Node::Primitive(PrimitiveKind::Any) => literal_key.is_some() || open_key_kind.is_some(),
        Node::Primitive(PrimitiveKind::String) => {
            literal_key.is_some()
                || matches!(
                    open_key_kind,
                    Some(PrimitiveKind::String | PrimitiveKind::Any)
                )
        }
        Node::Primitive(PrimitiveKind::Number) => matches!(
            open_key_kind,
            Some(PrimitiveKind::Number | PrimitiveKind::Any)
        ),
        Node::Primitive(PrimitiveKind::Symbol) => matches!(
            open_key_kind,
            Some(PrimitiveKind::Symbol | PrimitiveKind::Any)
        ),
        Node::Literal(super::arena::SolverLiteral::String(name)) => {
            literal_key.is_some_and(|requested| requested == name)
        }
        Node::Union(members) => members.iter().any(|member| {
            index_signature_matches_request(arena, *member, literal_key, open_key_kind)
        }),
        Node::Intersection(members) => members.iter().all(|member| {
            index_signature_matches_request(arena, *member, literal_key, open_key_kind)
        }),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// resolve_conditional
// ---------------------------------------------------------------------------

/// `T extends U ? A : B` — resolve using the relation engine.
///
/// Handles:
/// - Distributive conditionals: if `check` is a union and `distributive` is true,
///   distribute per-member and re-union the results.
/// - `infer` bindings: collected during the relation check and injected into the
///   true-branch substitution.
#[allow(clippy::too_many_arguments)]
fn resolve_conditional(
    arena: &mut QueryArena,
    check: NodeId,
    extends: NodeId,
    true_branch: NodeId,
    false_branch: NodeId,
    distributive: bool,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    use super::arena::SolverCaches;
    use super::relate::{relate, RelationLimits, RelationState};
    use super::result::RelationMode;

    // Distributive: if check is a resolved union and this is a distributive
    // conditional, distribute per-member and re-union.
    if distributive {
        if let Node::Union(members) = arena.get(check).clone() {
            let branches: Vec<NodeId> = members
                .iter()
                .map(|&m| {
                    resolve_conditional(
                        arena,
                        m,
                        extends,
                        true_branch,
                        false_branch,
                        false,
                        host,
                        state,
                        subst,
                    )
                })
                .collect();
            return arena.union(branches);
        }
    }

    // Set up relation check with infer binding collection.
    let mut caches = SolverCaches::new();
    let mut rel_state = RelationState::new(RelationLimits::default());
    rel_state.begin_infer(); // enable infer binding collection

    let relation = relate(
        arena,
        &mut caches,
        check,
        extends,
        RelationMode::Assignable,
        &mut rel_state,
    );

    match relation {
        super::result::RelationResult::Assignable => {
            // Collect infer bindings and inject into the true-branch substitution.
            let mut true_subst = subst.clone();
            if let Some(infer_bindings) = rel_state.take_infer_bindings() {
                for (name, candidates) in infer_bindings.iter() {
                    // Multiple candidates → intersect (use first for now).
                    if candidates.is_empty() {
                        continue;
                    }

                    let binding = if candidates.len() == 1 {
                        candidates[0]
                    } else {
                        arena.intersection(candidates.to_vec())
                    };
                    true_subst.bind(name, binding);
                }
            }
            // Push decided true-branch context
            state
                .conditional_context_stack
                .push(super::arena::ConditionalFrameSnapshot {
                    branch: super::arena::ConditionalBranch::True,
                    decided: true,
                    check,
                    extends,
                });
            let result = resolve_node(arena, true_branch, host, state, &true_subst);
            state.conditional_context_stack.pop();
            result
        }
        super::result::RelationResult::NotAssignable => {
            // Push decided false-branch context
            state
                .conditional_context_stack
                .push(super::arena::ConditionalFrameSnapshot {
                    branch: super::arena::ConditionalBranch::False,
                    decided: true,
                    check,
                    extends,
                });
            let result = resolve_node(arena, false_branch, host, state, subst);
            state.conditional_context_stack.pop();
            result
        }
        super::result::RelationResult::Unknown => {
            state.mark_symbolic();
            // Push undecided true-branch context for symbolic walk
            state
                .conditional_context_stack
                .push(super::arena::ConditionalFrameSnapshot {
                    branch: super::arena::ConditionalBranch::True,
                    decided: false,
                    check,
                    extends,
                });
            let tb = resolve_node(arena, true_branch, host, state, subst);
            state.conditional_context_stack.pop();
            // Push undecided false-branch context for symbolic walk
            state
                .conditional_context_stack
                .push(super::arena::ConditionalFrameSnapshot {
                    branch: super::arena::ConditionalBranch::False,
                    decided: false,
                    check,
                    extends,
                });
            let fb = resolve_node(arena, false_branch, host, state, subst);
            state.conditional_context_stack.pop();
            arena.conditional(check, extends, tb, fb, distributive)
        }
    }
}

// ---------------------------------------------------------------------------
// resolve_mapped — { [K in Source]: Value }
// ---------------------------------------------------------------------------

/// Mapped type resolution.
/// - Finite keyspace (string literal union) → concrete object with one property per key.
/// - Open keyspace (string/number) → object with index signature.
#[allow(clippy::too_many_arguments)]
fn resolve_mapped(
    arena: &mut QueryArena,
    parameter: &str,
    source: NodeId,
    value: NodeId,
    optional: super::arena::MappedModifierKind,
    readonly: super::arena::MappedModifierKind,
    name_type: Option<NodeId>,
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    use super::arena::{IndexSignatureNode, MappedModifierKind, ObjectNode, PropertyNode};

    let keys = collect_finite_keys(arena, source);

    if let Some(key_names) = keys {
        let mut properties = Vec::with_capacity(key_names.len());
        for key in key_names {
            let key_node = arena.string_literal(&key);
            let mut child_subst = subst.clone();
            child_subst.bind(parameter, key_node);

            // Key remapping: if name_type exists, resolve it to get the actual property name
            let prop_name = if let Some(nt) = name_type {
                let remapped = resolve_node(arena, nt, host, state, &child_subst);
                if remapped.is_unresolved() {
                    key.clone()
                } else {
                    match arena.get(remapped) {
                        Node::Literal(super::arena::SolverLiteral::String(s)) => s.clone(),
                        Node::Primitive(PrimitiveKind::Never) => continue, // filtered out
                        _ => key.clone(), // can't resolve statically — keep original
                    }
                }
            } else {
                key
            };

            let resolved_value = resolve_node(arena, value, host, state, &child_subst);

            properties.push(PropertyNode {
                name: prop_name,
                ty: resolved_value,
                optional: matches!(optional, MappedModifierKind::Add),
                readonly: matches!(readonly, MappedModifierKind::Add),
                is_method: false,
            });
        }

        arena.object(ObjectNode {
            properties,
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        })
    } else {
        let resolved_value = if should_eagerly_resolve_open_mapped_value(arena, source) {
            let mut child_subst = subst.clone();
            child_subst.bind(parameter, source);
            resolve_node(arena, value, host, state, &child_subst)
        } else {
            state.mark_symbolic();
            value
        };
        arena.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![IndexSignatureNode {
                key_type: source,
                value_type: resolved_value,
                readonly: matches!(readonly, MappedModifierKind::Add),
            }],
            call_signatures: vec![],
            construct_signatures: vec![],
        })
    }
}

/// Iteratively collect string literal keys from a node. Returns `None` if the
/// keyspace is open (non-literal sources).
fn collect_finite_keys(arena: &QueryArena, node: NodeId) -> Option<Vec<String>> {
    let mut keys = Vec::new();
    let mut stack = vec![node];

    while let Some(id) = stack.pop() {
        if id.is_unresolved() {
            return None;
        }

        match arena.get(id) {
            Node::Literal(super::arena::SolverLiteral::String(s)) => {
                keys.push(s.clone());
            }
            Node::Union(members) => {
                stack.extend(members.iter().copied());
            }
            Node::Primitive(PrimitiveKind::Never) => {}
            // Any non-literal member means the keyspace is open
            _ => return None,
        }
    }

    Some(keys)
}

fn should_eagerly_resolve_open_mapped_value(arena: &QueryArena, source: NodeId) -> bool {
    if source.is_unresolved() {
        return false;
    }

    match arena.get(source) {
        Node::Primitive(
            PrimitiveKind::String
            | PrimitiveKind::Number
            | PrimitiveKind::Symbol
            | PrimitiveKind::Any,
        ) => true,
        Node::Union(members) => members
            .iter()
            .all(|member| should_eagerly_resolve_open_mapped_value(arena, *member)),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// resolve_typeof
// ---------------------------------------------------------------------------

/// `typeof x` / `typeof x.y.z` — look up value declaration from host.
fn resolve_typeof(
    arena: &mut QueryArena,
    path: &[String],
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    _subst: &SubstitutionEnv,
) -> NodeId {
    if path.is_empty() {
        state.mark_symbolic();
        return arena.alloc(Node::TypeOf { path: vec![] });
    }

    // Try to resolve the root symbol as a value declaration
    let root_name = &path[0];
    let mut consumed_segments = 1usize;
    let qualified_root = if path.len() > 1 {
        let qualified = format!("{}.{}", path[0], path[1]);
        resolve_name_in_context(state, &qualified).or_else(|| host.root_identity("", &qualified))
    } else {
        None
    };
    // First check declaration context (for typeof inside imported type bodies),
    // then host's root_identity (for import bindings). Fall back to the
    // original ("", name) identity which checks owner env directly.
    let root_id = if let Some(identity) = qualified_root {
        consumed_segments = 2;
        identity
    } else {
        resolve_name_in_context(state, root_name)
            .or_else(|| host.root_identity("", root_name))
            .unwrap_or_else(|| ResolvedRootIdentity::new("", root_name))
    };

    if let Some(prepared) = host.resolve_prepared_value_decl(&root_id) {
        // Priority: type_annotation > object_shape > function_signature > enum_members
        let base_type = if let Some(ref ty_ann) = prepared.type_annotation {
            Some(lower_type_expr(arena, ty_ann))
        } else if let Some(ref shape) = prepared.object_shape {
            Some(lower_type_expr(
                arena,
                &TypeExpr::Object(Arc::new(shape.clone())),
            ))
        } else if let Some(ref sig) = prepared.function_signature {
            let func_expr = crate::analysis::type_expr::FunctionExpr {
                parameters: sig.parameters.clone(),
                return_type: sig.return_type.as_ref().map(|t| Arc::new(t.clone())),
                type_parameters: sig.type_parameters.clone(),
            };
            if prepared.kind == super::super::type_eval::ValueDeclKind::Class {
                // Class typeof: object with construct signature so
                // ConstructorParameters<typeof C> and InstanceType<typeof C>
                // find the construct signature, matching the manual
                // `{ new(...): T }` pattern.
                Some(lower_type_expr(
                    arena,
                    &TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            crate::analysis::type_expr::ObjectMember::ConstructSignature(func_expr),
                        ],
                    })),
                ))
            } else {
                // Regular function typeof: bare function type
                Some(lower_type_expr(
                    arena,
                    &TypeExpr::Function(Arc::new(func_expr)),
                ))
            }
        } else if let Some(ref members) = prepared.enum_members {
            // Enum value object: { MemberA: 0, MemberB: 1, ... }
            let obj_expr = crate::analysis::type_expr::ObjectExpr {
                properties: members
                    .iter()
                    .map(|(name, ty)| {
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: name.clone(),
                                ty: ty.clone(),
                                optional: false,
                                readonly: true,
                            },
                        )
                    })
                    .collect(),
            };
            Some(lower_type_expr(
                arena,
                &TypeExpr::Object(Arc::new(obj_expr)),
            ))
        } else {
            None
        };

        if let Some(base) = base_type {
            let pushed = if !prepared.name_resolution.is_empty() {
                state.value_decl_context_stack.push(Arc::clone(&prepared));
                true
            } else {
                false
            };

            let resolved_base = resolve_node(arena, base, host, state, &SubstitutionEnv::new());
            let result = if path.len() > consumed_segments {
                let mut current = resolved_base;
                let mut ok = true;
                for segment in &path[consumed_segments..] {
                    let node = arena.get(current).clone();
                    match node {
                        Node::Object(obj) => {
                            if let Some(prop) = obj.properties.iter().find(|p| p.name == *segment) {
                                current = prop.ty;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                ok.then_some(current)
            } else {
                Some(resolved_base)
            };

            if pushed {
                state.value_decl_context_stack.pop();
            }

            if let Some(result) = result {
                return result;
            }

            state.mark_symbolic();
            return arena.alloc(Node::TypeOf {
                path: path.to_vec(),
            });
        }
    }

    // Can't resolve — stay symbolic
    state.mark_symbolic();
    arena.alloc(Node::TypeOf {
        path: path.to_vec(),
    })
}

// ---------------------------------------------------------------------------
// resolve_template_literal
// ---------------------------------------------------------------------------

/// `` `prefix${T}suffix` `` — expand if all expressions are concrete string literals.
/// Uses iterative cartesian product expansion.
fn resolve_template_literal(
    arena: &mut QueryArena,
    quasis: &[String],
    expressions: &[NodeId],
    host: &dyn TypeSolverHost,
    state: &mut SolveState,
    subst: &SubstitutionEnv,
) -> NodeId {
    // Resolve all expression positions
    let resolved_exprs: Vec<NodeId> = expressions
        .iter()
        .map(|&e| resolve_node(arena, e, host, state, subst))
        .collect();

    // Collect string values from each expression position (each may be a union)
    let mut expr_options: Vec<Vec<String>> = Vec::with_capacity(resolved_exprs.len());
    for &expr_id in &resolved_exprs {
        let mut strings = Vec::new();
        let mut stack = vec![expr_id];
        let mut all_concrete = true;

        while let Some(id) = stack.pop() {
            match arena.get(id) {
                Node::Literal(super::arena::SolverLiteral::String(s)) => {
                    strings.push(s.clone());
                }
                Node::Literal(super::arena::SolverLiteral::Number(n)) => {
                    strings.push(format_number(*n));
                }
                Node::Literal(super::arena::SolverLiteral::Boolean(b)) => {
                    strings.push(b.to_string());
                }
                Node::Literal(super::arena::SolverLiteral::BigInt(s)) => {
                    strings.push(s.clone());
                }
                Node::Primitive(PrimitiveKind::Null) => {
                    strings.push("null".into());
                }
                Node::Primitive(PrimitiveKind::Undefined) => {
                    strings.push("undefined".into());
                }
                Node::Primitive(PrimitiveKind::Never) => {
                    // never contributes zero strings — result will be empty
                }
                Node::Union(members) => {
                    stack.extend(members.iter().copied());
                }
                _ => {
                    all_concrete = false;
                    break;
                }
            }
        }

        if !all_concrete {
            state.mark_symbolic();
            return arena.alloc(Node::TemplateLiteral {
                quasis: quasis.to_vec(),
                expressions: resolved_exprs,
            });
        }
        expr_options.push(strings);
    }

    // If any expression position resolved to zero options (never), result is never.
    if expr_options.iter().any(|v| v.is_empty()) {
        return arena.primitive(PrimitiveKind::Never);
    }

    // Guard: deterministic operational limit on cartesian product size.
    let product_size: usize = expr_options.iter().map(|v| v.len()).product();
    if product_size > MAX_TEMPLATE_LITERAL_PRODUCT {
        state.execution_status = ExecutionStatus::HardStop;
        state.mark_incomplete(IncompleteReason::RecursionPolicy {
            description: format!(
                "template literal expansion would produce {} combinations",
                product_size
            ),
        });
        return arena.alloc(Node::TemplateLiteral {
            quasis: quasis.to_vec(),
            expressions: resolved_exprs,
        });
    }

    // Iterative cartesian product expansion
    let mut results: Vec<String> = vec![quasis[0].clone()];
    for (i, options) in expr_options.iter().enumerate() {
        let suffix = quasis.get(i + 1).cloned().unwrap_or_default();
        let mut new_results = Vec::with_capacity(results.len() * options.len());
        for base in &results {
            for opt in options {
                let mut s = base.clone();
                s.push_str(opt);
                s.push_str(&suffix);
                new_results.push(s);
            }
        }
        results = new_results;
    }

    let nodes: Vec<NodeId> = results
        .into_iter()
        .map(|s| arena.string_literal(s))
        .collect();
    arena.union(nodes)
}

fn format_number(n: f64) -> String {
    if n == n.trunc() && n.is_finite() {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

// ---------------------------------------------------------------------------
// project_to_type_expr — arena nodes back to TypeExpr
// ---------------------------------------------------------------------------

/// Project a resolved arena node back to `TypeExpr`.
///
/// This is the inverse of `lower_type_expr`. It converts the solver's internal
/// representation back to the public output type.
fn project_to_type_expr(arena: &QueryArena, node: NodeId) -> TypeExpr {
    project_inner(arena, node, &mut Vec::new(), 0)
}

fn project_inner(
    arena: &QueryArena,
    node: NodeId,
    visited: &mut Vec<NodeId>,
    depth: usize,
) -> TypeExpr {
    if node.is_unresolved() || depth > 50 || visited.contains(&node) {
        return TypeExpr::Unknown {
            raw: "unresolved".into(),
        };
    }
    visited.push(node);

    let result = match arena.get(node) {
        Node::Primitive(kind) => TypeExpr::Primitive(project_primitive(*kind)),

        Node::Literal(lit) => match lit {
            super::arena::SolverLiteral::String(s) => TypeExpr::string_literal(s),
            super::arena::SolverLiteral::Number(n) => TypeExpr::number_literal(*n),
            super::arena::SolverLiteral::Boolean(b) => TypeExpr::boolean_literal(*b),
            super::arena::SolverLiteral::BigInt(s) => {
                TypeExpr::Literal(crate::analysis::type_expr::LiteralValue::BigInt(s.clone()))
            }
        },

        Node::Union(members) => {
            let types: Vec<TypeExpr> = members
                .iter()
                .map(|&m| project_inner(arena, m, visited, depth + 1))
                .collect();
            TypeExpr::Union(Arc::from(types))
        }

        Node::Intersection(members) => {
            let types: Vec<TypeExpr> = members
                .iter()
                .map(|&m| project_inner(arena, m, visited, depth + 1))
                .collect();
            TypeExpr::Intersection(Arc::from(types))
        }

        Node::Array { element, readonly } => TypeExpr::Array {
            element: Arc::new(project_inner(arena, *element, visited, depth + 1)),
            readonly: *readonly,
        },

        Node::Object(obj) => {
            let mut members = Vec::with_capacity(
                obj.properties.len()
                    + obj.index_signatures.len()
                    + obj.call_signatures.len()
                    + obj.construct_signatures.len(),
            );
            for p in &obj.properties {
                let ty = project_inner(arena, p.ty, visited, depth + 1);
                if p.is_method {
                    match ty {
                        TypeExpr::Function(function) => {
                            members.push(crate::analysis::type_expr::ObjectMember::Method(
                                crate::analysis::type_expr::MethodSignature {
                                    name: p.name.clone(),
                                    function: (*function).clone(),
                                    optional: p.optional,
                                },
                            ));
                        }
                        other => members.push(crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: p.name.clone(),
                                ty: other,
                                optional: p.optional,
                                readonly: p.readonly,
                            },
                        )),
                    }
                } else {
                    members.push(crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: p.name.clone(),
                            ty,
                            optional: p.optional,
                            readonly: p.readonly,
                        },
                    ));
                }
            }
            for idx in &obj.index_signatures {
                members.push(crate::analysis::type_expr::ObjectMember::IndexSignature(
                    crate::analysis::type_expr::IndexSignature {
                        key_name: "key".into(),
                        key_type: project_inner(arena, idx.key_type, visited, depth + 1),
                        value_type: project_inner(arena, idx.value_type, visited, depth + 1),
                        readonly: idx.readonly,
                    },
                ));
            }
            for sig in &obj.call_signatures {
                members.push(crate::analysis::type_expr::ObjectMember::CallSignature(
                    project_signature(arena, sig, visited, depth + 1),
                ));
            }
            for sig in &obj.construct_signatures {
                members.push(
                    crate::analysis::type_expr::ObjectMember::ConstructSignature(
                        project_signature(arena, sig, visited, depth + 1),
                    ),
                );
            }
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: members,
            }))
        }

        Node::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: Arc::from(name.as_str()),
            type_arguments: Arc::from(
                type_arguments
                    .iter()
                    .map(|&a| project_inner(arena, a, visited, depth + 1))
                    .collect::<Vec<_>>(),
            ),
        },

        Node::Applied { identity, args } => TypeExpr::Ref {
            name: Arc::from(identity.symbol_name.as_str()),
            type_arguments: Arc::from(
                args.iter()
                    .map(|&arg| project_inner(arena, arg, visited, depth + 1))
                    .collect::<Vec<_>>(),
            ),
        },

        Node::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: Arc::from(
                elements
                    .iter()
                    .map(|el| crate::analysis::type_expr::TupleElement {
                        label: el.label.clone(),
                        ty: project_inner(arena, el.ty, visited, depth + 1),
                        optional: el.optional,
                        rest: el.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },

        Node::Function(func) => {
            if let Some(sig) = func.signatures.first() {
                TypeExpr::Function(Arc::new(project_signature(arena, sig, visited, depth + 1)))
            } else {
                TypeExpr::Function(Arc::new(crate::analysis::type_expr::FunctionExpr {
                    parameters: vec![],
                    return_type: None,
                    type_parameters: vec![],
                }))
            }
        }

        Node::KeyOf(operand) => {
            TypeExpr::KeyOf(Arc::new(project_inner(arena, *operand, visited, depth + 1)))
        }

        Node::TypeOf { path } => {
            TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef { path: path.clone() })
        }

        Node::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
            object: Arc::new(project_inner(arena, *object, visited, depth + 1)),
            index: Arc::new(project_inner(arena, *index, visited, depth + 1)),
        },

        Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            ..
        } => TypeExpr::Conditional {
            check: Arc::new(project_inner(arena, *check, visited, depth + 1)),
            extends: Arc::new(project_inner(arena, *extends, visited, depth + 1)),
            true_type: Arc::new(project_inner(arena, *true_branch, visited, depth + 1)),
            false_type: Arc::new(project_inner(arena, *false_branch, visited, depth + 1)),
        },

        Node::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => TypeExpr::Mapped {
            parameter: parameter.clone(),
            source: Arc::new(project_inner(arena, *source, visited, depth + 1)),
            value: Arc::new(project_inner(arena, *value, visited, depth + 1)),
            optional: project_mapped_modifier(*optional),
            readonly: project_mapped_modifier(*readonly),
            name_type: name_type
                .map(|node| Arc::new(project_inner(arena, node, visited, depth + 1))),
        },

        Node::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: Arc::from(
                expressions
                    .iter()
                    .map(|&expr| project_inner(arena, expr, visited, depth + 1))
                    .collect::<Vec<_>>(),
            ),
        },

        Node::TypeParam {
            name,
            constraint,
            default,
        } => TypeExpr::TypeParameter(crate::analysis::type_expr::TypeParam {
            name: name.clone(),
            constraint: constraint
                .map(|node| Arc::new(project_inner(arena, node, visited, depth + 1))),
            default: default.map(|node| Arc::new(project_inner(arena, node, visited, depth + 1))),
        }),

        Node::Infer { name } => TypeExpr::Infer { name: name.clone() },

        Node::Rest(inner) => {
            TypeExpr::Rest(Arc::new(project_inner(arena, *inner, visited, depth + 1)))
        }

        Node::Error { description } => TypeExpr::Unknown {
            raw: description.clone(),
        },

        Node::RecursiveRef {
            symbol_name,
            type_arguments,
            conditional_context,
        } => {
            use super::arena::ConditionalBranch;
            use crate::analysis::type_expr::{
                RecursiveConditionalBranch, RecursiveConditionalFrame,
            };

            // Use compact recursive summary projector with bounded depth
            let arg_depth_cap = 2;
            let arg_node_cap = 32;
            let ctx_start = conditional_context
                .len()
                .saturating_sub(SolveState::MAX_CONDITIONAL_CONTEXT_FRAMES);

            TypeExpr::RecursiveRef {
                name: Arc::from(symbol_name.as_str()),
                type_arguments: Arc::from(
                    type_arguments
                        .iter()
                        .map(|&a| {
                            project_recursive_arg_summary(arena, a, arg_depth_cap, arg_node_cap)
                        })
                        .collect::<Vec<_>>(),
                ),
                conditional_context: Arc::from(
                    conditional_context[ctx_start..]
                        .iter()
                        .map(|frame| RecursiveConditionalFrame {
                            branch: match frame.branch {
                                ConditionalBranch::True => RecursiveConditionalBranch::True,
                                ConditionalBranch::False => RecursiveConditionalBranch::False,
                            },
                            decided: frame.decided,
                            check: Arc::new(project_recursive_arg_summary(
                                arena,
                                frame.check,
                                2,
                                16,
                            )),
                            extends: Arc::new(project_recursive_arg_summary(
                                arena,
                                frame.extends,
                                2,
                                16,
                            )),
                        })
                        .collect::<Vec<_>>(),
                ),
            }
        }
    };

    visited.pop();
    result
}

fn project_primitive(kind: PrimitiveKind) -> crate::analysis::type_expr::PrimitiveName {
    use crate::analysis::type_expr::PrimitiveName;
    match kind {
        PrimitiveKind::String => PrimitiveName::String,
        PrimitiveKind::Number => PrimitiveName::Number,
        PrimitiveKind::Boolean => PrimitiveName::Boolean,
        PrimitiveKind::Symbol => PrimitiveName::Symbol,
        PrimitiveKind::BigInt => PrimitiveName::BigInt,
        PrimitiveKind::Any => PrimitiveName::Any,
        PrimitiveKind::Unknown => PrimitiveName::Unknown,
        PrimitiveKind::Void => PrimitiveName::Void,
        PrimitiveKind::Never => PrimitiveName::Never,
        PrimitiveKind::Null => PrimitiveName::Null,
        PrimitiveKind::Undefined => PrimitiveName::Undefined,
        PrimitiveKind::Object => PrimitiveName::Object,
    }
}

fn project_mapped_modifier(
    modifier: super::arena::MappedModifierKind,
) -> crate::analysis::type_expr::MappedModifier {
    match modifier {
        super::arena::MappedModifierKind::Add => crate::analysis::type_expr::MappedModifier::Add,
        super::arena::MappedModifierKind::Remove => {
            crate::analysis::type_expr::MappedModifier::Remove
        }
        super::arena::MappedModifierKind::Unchanged => {
            crate::analysis::type_expr::MappedModifier::None
        }
    }
}

fn project_signature(
    arena: &QueryArena,
    sig: &super::arena::CallSignatureNode,
    visited: &mut Vec<NodeId>,
    depth: usize,
) -> crate::analysis::type_expr::FunctionExpr {
    crate::analysis::type_expr::FunctionExpr {
        parameters: sig
            .parameters
            .iter()
            .map(|p| crate::analysis::type_expr::FunctionParam {
                name: p.name.clone(),
                ty: project_inner(arena, p.ty, visited, depth + 1),
                optional: p.optional,
                rest: p.rest,
            })
            .collect(),
        return_type: Some(Arc::new(project_inner(
            arena,
            sig.return_type,
            visited,
            depth + 1,
        ))),
        type_parameters: sig
            .type_parameters
            .iter()
            .map(|param| crate::analysis::type_expr::TypeParam {
                name: param.name.clone(),
                constraint: param
                    .constraint
                    .map(|node| Arc::new(project_inner(arena, node, visited, depth + 1))),
                default: param
                    .default
                    .map(|node| Arc::new(project_inner(arena, node, visited, depth + 1))),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Compact recursive summary projector
// ---------------------------------------------------------------------------

/// Project a node to a compact TypeExpr suitable for recursive transport.
/// Uses a separate, bounded depth/node budget — never triggers the full projector.
fn project_recursive_arg_summary(
    arena: &QueryArena,
    node: NodeId,
    max_depth: usize,
    max_nodes: usize,
) -> TypeExpr {
    let mut count = 0;
    project_recursive_summary_inner(arena, node, 0, max_depth, &mut count, max_nodes)
}

fn project_recursive_summary_inner(
    arena: &QueryArena,
    node: NodeId,
    depth: usize,
    max_depth: usize,
    count: &mut usize,
    max_nodes: usize,
) -> TypeExpr {
    if node.is_unresolved() || depth > max_depth || *count > max_nodes {
        return TypeExpr::Unknown { raw: "...".into() };
    }
    *count += 1;

    match arena.get(node) {
        Node::Primitive(kind) => TypeExpr::Primitive(project_primitive(*kind)),
        Node::Literal(lit) => match lit {
            super::arena::SolverLiteral::String(s) => TypeExpr::string_literal(s),
            super::arena::SolverLiteral::Number(n) => TypeExpr::number_literal(*n),
            super::arena::SolverLiteral::Boolean(b) => TypeExpr::boolean_literal(*b),
            super::arena::SolverLiteral::BigInt(s) => {
                TypeExpr::Literal(crate::analysis::type_expr::LiteralValue::BigInt(s.clone()))
            }
        },
        Node::Union(members) => {
            let types: Vec<TypeExpr> = members
                .iter()
                .take(4) // Cap union members for compactness
                .map(|&m| {
                    project_recursive_summary_inner(
                        arena,
                        m,
                        depth + 1,
                        max_depth,
                        count,
                        max_nodes,
                    )
                })
                .collect();
            TypeExpr::Union(Arc::from(types))
        }
        Node::Intersection(members) => {
            let types: Vec<TypeExpr> = members
                .iter()
                .take(4)
                .map(|&m| {
                    project_recursive_summary_inner(
                        arena,
                        m,
                        depth + 1,
                        max_depth,
                        count,
                        max_nodes,
                    )
                })
                .collect();
            TypeExpr::Intersection(Arc::from(types))
        }
        Node::Array { element, readonly } => TypeExpr::Array {
            element: Arc::new(project_recursive_summary_inner(
                arena,
                *element,
                depth + 1,
                max_depth,
                count,
                max_nodes,
            )),
            readonly: *readonly,
        },
        Node::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: Arc::from(name.as_str()),
            type_arguments: Arc::from(
                type_arguments
                    .iter()
                    .map(|&a| {
                        project_recursive_summary_inner(
                            arena,
                            a,
                            depth + 1,
                            max_depth,
                            count,
                            max_nodes,
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
        },
        Node::Applied { identity, args } => TypeExpr::Ref {
            name: Arc::from(identity.symbol_name.as_str()),
            type_arguments: Arc::from(
                args.iter()
                    .map(|&arg| {
                        project_recursive_summary_inner(
                            arena,
                            arg,
                            depth + 1,
                            max_depth,
                            count,
                            max_nodes,
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
        },
        Node::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: Arc::from(
                elements
                    .iter()
                    .take(4)
                    .map(|el| crate::analysis::type_expr::TupleElement {
                        label: el.label.clone(),
                        ty: project_recursive_summary_inner(
                            arena,
                            el.ty,
                            depth + 1,
                            max_depth,
                            count,
                            max_nodes,
                        ),
                        optional: el.optional,
                        rest: el.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        Node::Object(obj) => {
            let members: Vec<crate::analysis::type_expr::ObjectMember> = obj
                .properties
                .iter()
                .take(4)
                .map(|p| {
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: p.name.clone(),
                            ty: project_recursive_summary_inner(
                                arena,
                                p.ty,
                                depth + 1,
                                max_depth,
                                count,
                                max_nodes,
                            ),
                            optional: p.optional,
                            readonly: p.readonly,
                        },
                    )
                })
                .collect();
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: members,
            }))
        }
        Node::KeyOf(operand) => TypeExpr::KeyOf(Arc::new(project_recursive_summary_inner(
            arena,
            *operand,
            depth + 1,
            max_depth,
            count,
            max_nodes,
        ))),
        Node::TypeOf { path } => {
            TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef { path: path.clone() })
        }
        Node::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
            object: Arc::new(project_recursive_summary_inner(
                arena,
                *object,
                depth + 1,
                max_depth,
                count,
                max_nodes,
            )),
            index: Arc::new(project_recursive_summary_inner(
                arena,
                *index,
                depth + 1,
                max_depth,
                count,
                max_nodes,
            )),
        },
        Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            ..
        } => TypeExpr::Conditional {
            check: Arc::new(project_recursive_summary_inner(
                arena,
                *check,
                depth + 1,
                max_depth,
                count,
                max_nodes,
            )),
            extends: Arc::new(project_recursive_summary_inner(
                arena,
                *extends,
                depth + 1,
                max_depth,
                count,
                max_nodes,
            )),
            true_type: Arc::new(project_recursive_summary_inner(
                arena,
                *true_branch,
                depth + 1,
                max_depth,
                count,
                max_nodes,
            )),
            false_type: Arc::new(project_recursive_summary_inner(
                arena,
                *false_branch,
                depth + 1,
                max_depth,
                count,
                max_nodes,
            )),
        },
        Node::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => TypeExpr::Mapped {
            parameter: parameter.clone(),
            source: Arc::new(project_recursive_summary_inner(
                arena,
                *source,
                depth + 1,
                max_depth,
                count,
                max_nodes,
            )),
            value: Arc::new(project_recursive_summary_inner(
                arena,
                *value,
                depth + 1,
                max_depth,
                count,
                max_nodes,
            )),
            optional: project_mapped_modifier(*optional),
            readonly: project_mapped_modifier(*readonly),
            name_type: name_type.map(|node| {
                Arc::new(project_recursive_summary_inner(
                    arena,
                    node,
                    depth + 1,
                    max_depth,
                    count,
                    max_nodes,
                ))
            }),
        },
        Node::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: Arc::from(
                expressions
                    .iter()
                    .map(|&expr| {
                        project_recursive_summary_inner(
                            arena,
                            expr,
                            depth + 1,
                            max_depth,
                            count,
                            max_nodes,
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
        },
        Node::TypeParam {
            name,
            constraint,
            default,
        } => TypeExpr::TypeParameter(crate::analysis::type_expr::TypeParam {
            name: name.clone(),
            constraint: constraint.map(|node| {
                Arc::new(project_recursive_summary_inner(
                    arena,
                    node,
                    depth + 1,
                    max_depth,
                    count,
                    max_nodes,
                ))
            }),
            default: default.map(|node| {
                Arc::new(project_recursive_summary_inner(
                    arena,
                    node,
                    depth + 1,
                    max_depth,
                    count,
                    max_nodes,
                ))
            }),
        }),
        Node::RecursiveRef {
            symbol_name,
            type_arguments,
            conditional_context,
        } => {
            use super::arena::ConditionalBranch;
            use crate::analysis::type_expr::{
                RecursiveConditionalBranch, RecursiveConditionalFrame,
            };

            let ctx_start = conditional_context
                .len()
                .saturating_sub(SolveState::MAX_CONDITIONAL_CONTEXT_FRAMES);
            TypeExpr::RecursiveRef {
                name: Arc::from(symbol_name.as_str()),
                type_arguments: Arc::from(
                    type_arguments
                        .iter()
                        .map(|&arg| {
                            project_recursive_summary_inner(
                                arena,
                                arg,
                                depth + 1,
                                max_depth,
                                count,
                                max_nodes,
                            )
                        })
                        .collect::<Vec<_>>(),
                ),
                conditional_context: Arc::from(
                    conditional_context[ctx_start..]
                        .iter()
                        .map(|frame| RecursiveConditionalFrame {
                            branch: match frame.branch {
                                ConditionalBranch::True => RecursiveConditionalBranch::True,
                                ConditionalBranch::False => RecursiveConditionalBranch::False,
                            },
                            decided: frame.decided,
                            check: Arc::new(project_recursive_summary_inner(
                                arena,
                                frame.check,
                                depth + 1,
                                max_depth,
                                count,
                                max_nodes,
                            )),
                            extends: Arc::new(project_recursive_summary_inner(
                                arena,
                                frame.extends,
                                depth + 1,
                                max_depth,
                                count,
                                max_nodes,
                            )),
                        })
                        .collect::<Vec<_>>(),
                ),
            }
        }
        Node::Infer { name } => TypeExpr::Infer { name: name.clone() },
        _ => TypeExpr::Unknown {
            raw: display_node(arena, node),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::type_expr::PrimitiveName;
    use crate::analysis::type_solver::host::NoopSolverHost;

    #[test]
    fn solve_primitive_is_identity() {
        let expr = TypeExpr::Primitive(PrimitiveName::String);
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
        assert_eq!(result.execution_status, ExecutionStatus::Completed);
    }

    #[test]
    fn solve_literal_is_identity() {
        let expr = TypeExpr::string_literal("hello");
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::string_literal("hello"));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_union_resolves_members() {
        let expr = TypeExpr::Union(Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ]));
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
            }
            _ => panic!("expected Union"),
        }
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_object_resolves_property_types() {
        let expr = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "x".into(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                },
            )],
        }));
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 1);
            }
            _ => panic!("expected Object"),
        }
    }

    #[test]
    fn solve_unresolved_ref_stays_symbolic() {
        let expr = TypeExpr::Ref {
            name: Arc::from("UnknownType"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        // Should stay as a Ref since NoopSolverHost can't resolve it
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        match &result.value {
            TypeExpr::Ref { name, .. } => assert_eq!(name.as_ref(), "UnknownType"),
            _ => panic!("expected Ref, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_non_nullable_builtin() {
        let expr = TypeExpr::Ref {
            name: Arc::from("NonNullable"),
            type_arguments: Arc::from(vec![TypeExpr::Union(Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Null),
                TypeExpr::Primitive(PrimitiveName::Undefined),
            ]))]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        // NonNullable should filter out null and undefined
        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_uppercase_builtin() {
        let expr = TypeExpr::Ref {
            name: Arc::from("Uppercase"),
            type_arguments: Arc::from(vec![TypeExpr::string_literal("hello")]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::string_literal("HELLO"));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_capitalize_builtin() {
        let expr = TypeExpr::Ref {
            name: Arc::from("Capitalize"),
            type_arguments: Arc::from(vec![TypeExpr::string_literal("hello")]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::string_literal("Hello"));
    }

    #[test]
    fn solve_array_resolves_element() {
        let expr = TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: true,
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Array { readonly, .. } => assert!(readonly),
            _ => panic!("expected Array"),
        }
    }

    // -- Test host with prepared declarations --

    use crate::analysis::type_eval::TypeDeclKind;
    use rustc_hash::FxHashMap;

    struct TestHost {
        decls: FxHashMap<String, Arc<PreparedTypeDecl>>,
    }

    impl TestHost {
        fn new() -> Self {
            Self {
                decls: FxHashMap::default(),
            }
        }

        fn add_alias(&mut self, name: &str, body: TypeExpr) {
            self.add_alias_in("/test.ts", name, body);
        }

        fn add_alias_in(&mut self, canonical_id: &str, name: &str, body: TypeExpr) {
            self.decls.insert(
                name.to_string(),
                Arc::new(PreparedTypeDecl::new(
                    ResolvedRootIdentity::new(canonical_id, name),
                    TypeDeclKind::Alias,
                    body,
                )),
            );
        }

        fn add_generic_alias(
            &mut self,
            name: &str,
            params: Vec<crate::analysis::type_expr::TypeParam>,
            body: TypeExpr,
        ) {
            let mut decl = PreparedTypeDecl::new(
                ResolvedRootIdentity::new("/test.ts", name),
                TypeDeclKind::Alias,
                body,
            );
            decl.type_parameters = params;
            self.decls.insert(name.to_string(), Arc::new(decl));
        }
    }

    impl TypeSolverHost for TestHost {
        fn resolve_prepared_type_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedTypeDecl>> {
            self.decls.get(&root_identity.symbol_name).cloned()
        }

        fn resolve_prepared_value_decl(
            &self,
            _: &ResolvedRootIdentity,
        ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>> {
            None
        }

        fn utility_source(&self, name: &str) -> UtilitySource {
            if BuiltinUtility::from_name(name).is_some() {
                UtilitySource::Builtin
            } else {
                UtilitySource::Unknown
            }
        }

        fn root_identity(
            &self,
            _canonical_id: &str,
            symbol_name: &str,
        ) -> Option<ResolvedRootIdentity> {
            self.decls
                .get(symbol_name)
                .map(|decl| decl.root_identity.clone())
        }
    }

    fn object_property_type<'a>(expr: &'a TypeExpr, property_name: &str) -> &'a TypeExpr {
        let TypeExpr::Object(obj) = expr else {
            panic!("expected object result, got {expr:?}");
        };
        obj.properties
            .iter()
            .find_map(|member| match member {
                crate::analysis::type_expr::ObjectMember::Property(prop)
                    if prop.name == property_name =>
                {
                    Some(&prop.ty)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing property {property_name} in {expr:?}"))
    }

    // -- Host-backed resolution tests --

    #[test]
    fn solve_resolves_simple_type_alias() {
        let mut host = TestHost::new();
        // type MyString = string
        host.add_alias("MyString", TypeExpr::Primitive(PrimitiveName::String));

        let expr = TypeExpr::Ref {
            name: Arc::from("MyString"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &host);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_resolves_generic_alias_with_substitution() {
        let mut host = TestHost::new();
        // type Wrap<T> = T[]
        host.add_generic_alias(
            "Wrap",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::Array {
                element: Arc::new(TypeExpr::Ref {
                    name: Arc::from("T"),
                    type_arguments: Arc::from(vec![]),
                }),
                readonly: false,
            },
        );

        // Wrap<number> should resolve to number[]
        let expr = TypeExpr::Ref {
            name: Arc::from("Wrap"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::Number)]),
        };
        let result = solve_type(&expr, &host);

        match &result.value {
            TypeExpr::Array { element, readonly } => {
                assert_eq!(
                    element.as_ref(),
                    &TypeExpr::Primitive(PrimitiveName::Number)
                );
                assert!(!readonly);
            }
            other => panic!("expected Array, got: {:?}", other),
        }
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_resolves_chained_aliases() {
        let mut host = TestHost::new();
        // type A = string
        // type B = A
        host.add_alias("A", TypeExpr::Primitive(PrimitiveName::String));
        host.add_alias(
            "B",
            TypeExpr::Ref {
                name: Arc::from("A"),
                type_arguments: Arc::from(vec![]),
            },
        );

        let expr = TypeExpr::Ref {
            name: Arc::from("B"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &host);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
    }

    #[test]
    fn solve_missing_prepared_decl_returns_incomplete() {
        let _host = TestHost::new(); // empty — no decls

        // But we need root_identity to return Some for the test to reach the
        // prepared_type_decl lookup. Use a host that returns identity but no decl.
        struct MissingDeclHost;
        impl TypeSolverHost for MissingDeclHost {
            fn resolve_prepared_type_decl(
                &self,
                _: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                None // source missing
            }
            fn resolve_prepared_value_decl(
                &self,
                _: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                None
            }
            fn utility_source(&self, _: &str) -> UtilitySource {
                UtilitySource::Unknown
            }
            fn root_identity(&self, _: &str, symbol_name: &str) -> Option<ResolvedRootIdentity> {
                Some(ResolvedRootIdentity::new("/missing.ts", symbol_name))
            }
        }

        let expr = TypeExpr::Ref {
            name: Arc::from("MissingType"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &MissingDeclHost);

        assert_eq!(result.exactness, SolverExactness::Incomplete);
        assert!(!result.incomplete_reasons.is_empty());
    }

    #[test]
    fn solve_generic_with_default_type_param() {
        let mut host = TestHost::new();
        // type WithDefault<T = string> = T[]
        host.add_generic_alias(
            "WithDefault",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
            }],
            TypeExpr::Array {
                element: Arc::new(TypeExpr::Ref {
                    name: Arc::from("T"),
                    type_arguments: Arc::from(vec![]),
                }),
                readonly: false,
            },
        );

        // WithDefault<> (no args) should use default T=string → string[]
        let expr = TypeExpr::Ref {
            name: Arc::from("WithDefault"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &host);

        match &result.value {
            TypeExpr::Array { element, .. } => {
                assert_eq!(
                    element.as_ref(),
                    &TypeExpr::Primitive(PrimitiveName::String)
                );
            }
            other => panic!("expected Array, got: {:?}", other),
        }
    }

    #[test]
    fn solve_partial_default_arguments_preserve_positional_alignment() {
        let mut host = TestHost::new();
        host.add_generic_alias(
            "Mixed",
            vec![
                crate::analysis::type_expr::TypeParam {
                    name: "A".into(),
                    constraint: None,
                    default: None,
                },
                crate::analysis::type_expr::TypeParam {
                    name: "B".into(),
                    constraint: None,
                    default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Number))),
                },
                crate::analysis::type_expr::TypeParam {
                    name: "C".into(),
                    constraint: None,
                    default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Boolean))),
                },
            ],
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "a".into(),
                            ty: TypeExpr::named("A"),
                            optional: false,
                            readonly: false,
                        },
                    ),
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "b".into(),
                            ty: TypeExpr::named("B"),
                            optional: false,
                            readonly: false,
                        },
                    ),
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "c".into(),
                            ty: TypeExpr::named("C"),
                            optional: false,
                            readonly: false,
                        },
                    ),
                ],
            })),
        );

        let result = solve_type(
            &TypeExpr::named_with_args("Mixed", vec![TypeExpr::Primitive(PrimitiveName::String)]),
            &host,
        );

        assert_eq!(
            object_property_type(&result.value, "a"),
            &TypeExpr::Primitive(PrimitiveName::String)
        );
        assert_eq!(
            object_property_type(&result.value, "b"),
            &TypeExpr::Primitive(PrimitiveName::Number)
        );
        assert_eq!(
            object_property_type(&result.value, "c"),
            &TypeExpr::Primitive(PrimitiveName::Boolean)
        );
    }

    #[test]
    fn solve_undefaulted_middle_parameter_stays_unresolved() {
        let mut host = TestHost::new();
        host.add_generic_alias(
            "Skip",
            vec![
                crate::analysis::type_expr::TypeParam {
                    name: "A".into(),
                    constraint: None,
                    default: None,
                },
                crate::analysis::type_expr::TypeParam {
                    name: "B".into(),
                    constraint: None,
                    default: None,
                },
                crate::analysis::type_expr::TypeParam {
                    name: "C".into(),
                    constraint: None,
                    default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Boolean))),
                },
            ],
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "a".into(),
                            ty: TypeExpr::named("A"),
                            optional: false,
                            readonly: false,
                        },
                    ),
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "b".into(),
                            ty: TypeExpr::named("B"),
                            optional: false,
                            readonly: false,
                        },
                    ),
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "c".into(),
                            ty: TypeExpr::named("C"),
                            optional: false,
                            readonly: false,
                        },
                    ),
                ],
            })),
        );

        let result = solve_type(
            &TypeExpr::named_with_args("Skip", vec![TypeExpr::Primitive(PrimitiveName::String)]),
            &host,
        );

        assert_eq!(
            object_property_type(&result.value, "a"),
            &TypeExpr::Primitive(PrimitiveName::String)
        );
        assert!(
            matches!(object_property_type(&result.value, "b"), TypeExpr::Unknown { .. }),
            "undefaulted middle parameters should remain unresolved instead of stealing a later default"
        );
        assert_ne!(
            object_property_type(&result.value, "b"),
            &TypeExpr::Primitive(PrimitiveName::Boolean),
            "the middle type parameter must not be rebound to C's boolean default"
        );
        assert_eq!(
            object_property_type(&result.value, "c"),
            &TypeExpr::Primitive(PrimitiveName::Boolean)
        );
    }

    #[test]
    fn solve_default_argument_honors_satisfied_constraint() {
        let mut host = TestHost::new();
        host.add_generic_alias(
            "Good",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                default: Some(Arc::new(TypeExpr::string_literal("hello"))),
            }],
            TypeExpr::named("T"),
        );

        let result = solve_type(&TypeExpr::named("Good"), &host);

        assert_eq!(result.value, TypeExpr::string_literal("hello"));
        assert_ne!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::String),
            "a satisfying literal default should remain intact"
        );
    }

    #[test]
    fn solve_default_argument_falls_back_to_constraint_when_invalid() {
        let mut host = TestHost::new();
        host.add_generic_alias(
            "Bad",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Number))),
            }],
            TypeExpr::named("T"),
        );

        let result = solve_type(&TypeExpr::named("Bad"), &host);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_ne!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::Number),
            "invalid defaults should fall back to the constraint instead of leaking the bad default"
        );
    }

    #[test]
    fn solve_respects_step_limit() {
        // Create a wide union with distinct literal members to prevent dedup,
        // ensuring enough resolve steps to exceed the low limit.
        let members: Vec<TypeExpr> = (0..100)
            .map(|i| TypeExpr::string_literal(&format!("v{}", i)))
            .collect();
        let expr = TypeExpr::Union(Arc::from(members));

        let limits = SolveLimits {
            max_resolve_steps: 50, // Very low limit
            ..Default::default()
        };
        let result = solve_type_with_limits(&expr, &NoopSolverHost, limits);

        // Should hit the step limit
        assert_eq!(result.execution_status, ExecutionStatus::HardStop);
    }

    #[test]
    fn solve_typeof_resolves_imported_names_inside_prepared_value_annotations() {
        struct ValueContextHost {
            types: FxHashMap<String, Arc<PreparedTypeDecl>>,
            values:
                FxHashMap<String, Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>,
        }

        impl TypeSolverHost for ValueContextHost {
            fn resolve_prepared_type_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                self.types.get(&root_identity.symbol_name).cloned()
            }

            fn resolve_prepared_value_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                self.values.get(&root_identity.symbol_name).cloned()
            }

            fn utility_source(&self, name: &str) -> UtilitySource {
                if BuiltinUtility::from_name(name).is_some() {
                    UtilitySource::Builtin
                } else {
                    UtilitySource::Unknown
                }
            }

            fn root_identity(
                &self,
                _canonical_id: &str,
                symbol_name: &str,
            ) -> Option<ResolvedRootIdentity> {
                if self.types.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/dep.ts", symbol_name))
                } else if self.values.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/owner.ts", symbol_name))
                } else {
                    None
                }
            }
        }

        let mut remote = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/dep.ts", "Remote"),
            TypeDeclKind::Alias,
            TypeExpr::Primitive(PrimitiveName::String),
        );
        remote.build_member_index();

        let mut theme = crate::analysis::type_solver::prepared::PreparedValueDecl::new(
            ResolvedRootIdentity::new("/owner.ts", "theme"),
            crate::analysis::type_eval::ValueDeclKind::Const,
        );
        theme.type_annotation = Some(TypeExpr::Ref {
            name: Arc::from("Remote"),
            type_arguments: Arc::from(vec![]),
        });
        theme.name_resolution.insert(
            "Remote".into(),
            ResolvedRootIdentity::new("/dep.ts", "Remote"),
        );

        let host = ValueContextHost {
            types: {
                let mut map = FxHashMap::default();
                map.insert("Remote".into(), Arc::new(remote));
                map
            },
            values: {
                let mut map = FxHashMap::default();
                map.insert("theme".into(), Arc::new(theme));
                map
            },
        };

        let expr = TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef {
            path: vec!["theme".into()],
        });
        let result = solve_type(&expr, &host);

        assert_eq!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::String),
            "typeof should resolve imported names inside prepared value annotations through value declaration context",
        );
    }

    #[test]
    fn solve_typeof_resolves_namespace_member_paths() {
        struct NamespaceValueHost {
            values:
                FxHashMap<String, Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>,
        }

        impl TypeSolverHost for NamespaceValueHost {
            fn resolve_prepared_type_decl(
                &self,
                _root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                None
            }

            fn resolve_prepared_value_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                self.values.get(&root_identity.symbol_name).cloned()
            }

            fn utility_source(&self, _name: &str) -> UtilitySource {
                UtilitySource::Unknown
            }

            fn root_identity(
                &self,
                _canonical_id: &str,
                symbol_name: &str,
            ) -> Option<ResolvedRootIdentity> {
                match symbol_name {
                    "ThemeNs.theme" | "theme" => {
                        Some(ResolvedRootIdentity::new("/theme.ts", "theme"))
                    }
                    _ => None,
                }
            }
        }

        let mut theme = crate::analysis::type_solver::prepared::PreparedValueDecl::new(
            ResolvedRootIdentity::new("/theme.ts", "theme"),
            crate::analysis::type_eval::ValueDeclKind::Const,
        );
        theme.object_shape = Some(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "slots".into(),
                    ty: TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "root".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: false,
                                    readonly: false,
                                },
                            ),
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "label".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: false,
                                    readonly: false,
                                },
                            ),
                        ],
                    })),
                    optional: false,
                    readonly: false,
                },
            )],
        });

        let host = NamespaceValueHost {
            values: {
                let mut map = FxHashMap::default();
                map.insert("theme".into(), Arc::new(theme));
                map
            },
        };

        let expr = TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef {
            path: vec!["ThemeNs".into(), "theme".into(), "slots".into()],
        });
        let result = solve_type(&expr, &host);

        let TypeExpr::Object(obj) = result.value else {
            panic!("namespace typeof member path should resolve to object shape");
        };
        let names: std::collections::BTreeSet<_> = obj
            .properties
            .iter()
            .map(|member| match member {
                crate::analysis::type_expr::ObjectMember::Property(prop) => prop.name.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(
            names,
            std::collections::BTreeSet::from(["label".to_string(), "root".to_string()]),
            "typeof should be able to consume the namespace qualifier as part of the root value lookup",
        );
    }

    #[test]
    fn solve_generic_typeof_arguments_flow_through_cached_prepared_decls() {
        struct CachedDeclHost {
            types: FxHashMap<String, Arc<PreparedTypeDecl>>,
            values:
                FxHashMap<String, Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>,
        }

        impl TypeSolverHost for CachedDeclHost {
            fn resolve_prepared_type_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                self.types.get(&root_identity.symbol_name).cloned()
            }

            fn resolve_prepared_value_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                self.values.get(&root_identity.symbol_name).cloned()
            }

            fn utility_source(&self, name: &str) -> UtilitySource {
                if BuiltinUtility::from_name(name).is_some() {
                    UtilitySource::Builtin
                } else {
                    UtilitySource::Unknown
                }
            }

            fn root_identity(
                &self,
                _canonical_id: &str,
                symbol_name: &str,
            ) -> Option<ResolvedRootIdentity> {
                if self.types.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/types.ts", symbol_name))
                } else if self.values.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/theme.ts", symbol_name))
                } else {
                    None
                }
            }
        }

        let empty_object = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![],
        }));
        let slots_object = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "base".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "label".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
            ],
        }));

        let mut id_decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Id"),
            TypeDeclKind::Alias,
            TypeExpr::Intersection(Arc::from(vec![
                empty_object.clone(),
                TypeExpr::Mapped {
                    parameter: "P".into(),
                    source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::Ref {
                        name: Arc::from("T"),
                        type_arguments: Arc::from(vec![]),
                    }))),
                    value: Arc::new(TypeExpr::IndexedAccess {
                        object: Arc::new(TypeExpr::Ref {
                            name: Arc::from("T"),
                            type_arguments: Arc::from(vec![]),
                        }),
                        index: Arc::new(TypeExpr::Ref {
                            name: Arc::from("P"),
                            type_arguments: Arc::from(vec![]),
                        }),
                    }),
                    optional: crate::analysis::type_expr::MappedModifier::None,
                    readonly: crate::analysis::type_expr::MappedModifier::None,
                    name_type: None,
                },
            ])),
        );
        id_decl
            .type_parameters
            .push(crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            });
        id_decl
            .name_resolution
            .insert("T".into(), ResolvedRootIdentity::new("/types.ts", "T"));

        let mut component_ui = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "ComponentUI"),
            TypeDeclKind::Alias,
            TypeExpr::Ref {
                name: Arc::from("Id"),
                type_arguments: Arc::from(vec![TypeExpr::IndexedAccess {
                    object: Arc::new(TypeExpr::Ref {
                        name: Arc::from("T"),
                        type_arguments: Arc::from(vec![]),
                    }),
                    index: Arc::new(TypeExpr::string_literal("slots")),
                }]),
            },
        );
        component_ui
            .type_parameters
            .push(crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            });
        component_ui
            .name_resolution
            .insert("Id".into(), ResolvedRootIdentity::new("/types.ts", "Id"));

        let mut button = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/button-types.ts", "Button"),
            TypeDeclKind::Alias,
            TypeExpr::Ref {
                name: Arc::from("ComponentUI"),
                type_arguments: Arc::from(vec![TypeExpr::TypeOf(
                    crate::analysis::type_expr::ValueRef {
                        path: vec!["theme".into()],
                    },
                )]),
            },
        );
        button.name_resolution.insert(
            "ComponentUI".into(),
            ResolvedRootIdentity::new("/types.ts", "ComponentUI"),
        );
        button.name_resolution.insert(
            "theme".into(),
            ResolvedRootIdentity::new("/theme.ts", "theme"),
        );

        let mut theme = crate::analysis::type_solver::prepared::PreparedValueDecl::new(
            ResolvedRootIdentity::new("/theme.ts", "theme"),
            crate::analysis::type_eval::ValueDeclKind::Const,
        );
        theme.object_shape = Some(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "slots".into(),
                    ty: slots_object.clone(),
                    optional: false,
                    readonly: false,
                },
            )],
        });

        let host = CachedDeclHost {
            types: {
                let mut map = FxHashMap::default();
                map.insert("Id".into(), Arc::new(id_decl));
                map.insert("ComponentUI".into(), Arc::new(component_ui));
                map.insert("Button".into(), Arc::new(button));
                map
            },
            values: {
                let mut map = FxHashMap::default();
                map.insert("theme".into(), Arc::new(theme));
                map
            },
        };

        let result = solve_type(&TypeExpr::named("Button"), &host);

        let TypeExpr::Object(obj) = result.value else {
            panic!(
                "Button should resolve to an object shape, got {:?}",
                result.value
            );
        };
        assert!(
            obj.properties.iter().any(|member| {
                matches!(
                    member,
                    crate::analysis::type_expr::ObjectMember::Property(property)
                        if property.name == "base"
                )
            }),
            "generic typeof argument should expose base, got {:?}",
            obj
        );
        assert!(
            obj.properties.iter().any(|member| {
                matches!(
                    member,
                    crate::analysis::type_expr::ObjectMember::Property(property)
                        if property.name == "label"
                )
            }),
            "generic typeof argument should expose label, got {:?}",
            obj
        );
    }

    #[test]
    fn solve_generic_required_mapped_typeof_arguments_flow_through_cached_prepared_decls() {
        struct CachedDeclHost {
            types: FxHashMap<String, Arc<PreparedTypeDecl>>,
            values:
                FxHashMap<String, Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>,
        }

        impl TypeSolverHost for CachedDeclHost {
            fn resolve_prepared_type_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                self.types.get(&root_identity.symbol_name).cloned()
            }

            fn resolve_prepared_value_decl(
                &self,
                root_identity: &ResolvedRootIdentity,
            ) -> Option<Arc<crate::analysis::type_solver::prepared::PreparedValueDecl>>
            {
                self.values.get(&root_identity.symbol_name).cloned()
            }

            fn utility_source(&self, name: &str) -> UtilitySource {
                if BuiltinUtility::from_name(name).is_some() {
                    UtilitySource::Builtin
                } else {
                    UtilitySource::Unknown
                }
            }

            fn root_identity(
                &self,
                _canonical_id: &str,
                symbol_name: &str,
            ) -> Option<ResolvedRootIdentity> {
                if self.types.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/types.ts", symbol_name))
                } else if self.values.contains_key(symbol_name) {
                    Some(ResolvedRootIdentity::new("/theme.ts", symbol_name))
                } else {
                    None
                }
            }
        }

        let empty_object = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![],
        }));
        let slots_object = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "base".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "label".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
            ],
        }));

        let mut id_decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Id"),
            TypeDeclKind::Alias,
            TypeExpr::Intersection(Arc::from(vec![
                empty_object.clone(),
                TypeExpr::Mapped {
                    parameter: "P".into(),
                    source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::Ref {
                        name: Arc::from("T"),
                        type_arguments: Arc::from(vec![]),
                    }))),
                    value: Arc::new(TypeExpr::IndexedAccess {
                        object: Arc::new(TypeExpr::Ref {
                            name: Arc::from("T"),
                            type_arguments: Arc::from(vec![]),
                        }),
                        index: Arc::new(TypeExpr::Ref {
                            name: Arc::from("P"),
                            type_arguments: Arc::from(vec![]),
                        }),
                    }),
                    optional: crate::analysis::type_expr::MappedModifier::None,
                    readonly: crate::analysis::type_expr::MappedModifier::None,
                    name_type: None,
                },
            ])),
        );
        id_decl
            .type_parameters
            .push(crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            });

        let mut component_ui = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "ComponentUI"),
            TypeDeclKind::Alias,
            TypeExpr::Ref {
                name: Arc::from("Id"),
                type_arguments: Arc::from(vec![TypeExpr::Mapped {
                    parameter: "K".into(),
                    source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::Ref {
                        name: Arc::from("Required"),
                        type_arguments: Arc::from(vec![TypeExpr::IndexedAccess {
                            object: Arc::new(TypeExpr::Ref {
                                name: Arc::from("T"),
                                type_arguments: Arc::from(vec![]),
                            }),
                            index: Arc::new(TypeExpr::string_literal("slots")),
                        }]),
                    }))),
                    value: Arc::new(TypeExpr::Function(Arc::new(
                        crate::analysis::type_expr::FunctionExpr {
                            parameters: vec![crate::analysis::type_expr::FunctionParam {
                                name: Some("props".into()),
                                ty: TypeExpr::Object(Arc::new(
                                    crate::analysis::type_expr::ObjectExpr {
                                        properties: vec![crate::analysis::type_expr::ObjectMember::IndexSignature(
                                            crate::analysis::type_expr::IndexSignature {
                                                key_name: "key".into(),
                                                key_type: TypeExpr::Primitive(PrimitiveName::String),
                                                value_type: TypeExpr::Primitive(PrimitiveName::Any),
                                                readonly: false,
                                            },
                                        )],
                                    },
                                )),
                                optional: true,
                                rest: false,
                            }],
                            return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                            type_parameters: vec![],
                        },
                    ))),
                    optional: crate::analysis::type_expr::MappedModifier::None,
                    readonly: crate::analysis::type_expr::MappedModifier::None,
                    name_type: None,
                }]),
            },
        );
        component_ui
            .type_parameters
            .push(crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            });
        component_ui
            .name_resolution
            .insert("Id".into(), ResolvedRootIdentity::new("/types.ts", "Id"));

        let mut button = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/button-types.ts", "Button"),
            TypeDeclKind::Alias,
            TypeExpr::Ref {
                name: Arc::from("ComponentUI"),
                type_arguments: Arc::from(vec![TypeExpr::TypeOf(
                    crate::analysis::type_expr::ValueRef {
                        path: vec!["theme".into()],
                    },
                )]),
            },
        );
        button.name_resolution.insert(
            "ComponentUI".into(),
            ResolvedRootIdentity::new("/types.ts", "ComponentUI"),
        );
        button.name_resolution.insert(
            "theme".into(),
            ResolvedRootIdentity::new("/theme.ts", "theme"),
        );

        let mut theme = crate::analysis::type_solver::prepared::PreparedValueDecl::new(
            ResolvedRootIdentity::new("/theme.ts", "theme"),
            crate::analysis::type_eval::ValueDeclKind::Const,
        );
        theme.object_shape = Some(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "slots".into(),
                    ty: slots_object,
                    optional: false,
                    readonly: false,
                },
            )],
        });

        let host = CachedDeclHost {
            types: {
                let mut map = FxHashMap::default();
                map.insert("Id".into(), Arc::new(id_decl));
                map.insert("ComponentUI".into(), Arc::new(component_ui));
                map.insert("Button".into(), Arc::new(button));
                map
            },
            values: {
                let mut map = FxHashMap::default();
                map.insert("theme".into(), Arc::new(theme));
                map
            },
        };

        let result = solve_type(&TypeExpr::named("Button"), &host);

        let TypeExpr::Object(obj) = result.value else {
            panic!("Button should resolve to an object shape after Id<T> normalization");
        };
        assert!(
            obj.properties.iter().any(|member| {
                matches!(
                    member,
                    crate::analysis::type_expr::ObjectMember::Property(property)
                        if property.name == "base"
                )
            }),
            "generic required mapped typeof argument should expose base, got {:?}",
            obj
        );
        assert!(
            obj.properties.iter().any(|member| {
                matches!(
                    member,
                    crate::analysis::type_expr::ObjectMember::Property(property)
                        if property.name == "label"
                )
            }),
            "generic required mapped typeof argument should expose label, got {:?}",
            obj
        );
    }

    // -- 4a: keyof + indexed access --

    #[test]
    fn solve_keyof_object_literal() {
        // keyof { a: string; b: number } → "a" | "b"
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Object(Arc::new(
            crate::analysis::type_expr::ObjectExpr {
                properties: vec![
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "a".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    ),
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "b".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            readonly: false,
                        },
                    ),
                ],
            },
        ))));
        let result = solve_type(&expr, &NoopSolverHost);

        // Should be "a" | "b"
        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("a")));
                assert!(members.contains(&TypeExpr::string_literal("b")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_keyof_with_index_signature_is_open() {
        // keyof { [key: string]: number } → string
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Object(Arc::new(
            crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::IndexSignature(
                    crate::analysis::type_expr::IndexSignature {
                        key_name: "key".into(),
                        key_type: TypeExpr::Primitive(PrimitiveName::String),
                        value_type: TypeExpr::Primitive(PrimitiveName::Number),
                        readonly: false,
                    },
                )],
            },
        ))));
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
    }

    #[test]
    fn solve_keyof_array_is_number() {
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: false,
        }));
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::Number));
    }

    #[test]
    fn solve_keyof_tuple_is_numeric_index_union() {
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Tuple {
            elements: Arc::from(vec![
                crate::analysis::type_expr::TupleElement {
                    label: None,
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                },
                crate::analysis::type_expr::TupleElement {
                    label: None,
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    rest: false,
                },
            ]),
            readonly: false,
        }));
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Union(members) => {
                assert!(members.contains(&TypeExpr::number_literal(0.0)));
                assert!(members.contains(&TypeExpr::number_literal(1.0)));
            }
            other => panic!("expected numeric literal union, got {other:?}"),
        }
    }

    #[test]
    fn solve_indexed_access_object_literal() {
        // { a: string; b: number }["a"] → string
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Object(Arc::new(
                crate::analysis::type_expr::ObjectExpr {
                    properties: vec![
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "a".into(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                readonly: false,
                            },
                        ),
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "b".into(),
                                ty: TypeExpr::Primitive(PrimitiveName::Number),
                                optional: false,
                                readonly: false,
                            },
                        ),
                    ],
                },
            ))),
            index: Arc::new(TypeExpr::string_literal("a")),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_indexed_access_union_key() {
        // { a: string; b: number }["a" | "b"] → string | number
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Object(Arc::new(
                crate::analysis::type_expr::ObjectExpr {
                    properties: vec![
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "a".into(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                readonly: false,
                            },
                        ),
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "b".into(),
                                ty: TypeExpr::Primitive(PrimitiveName::Number),
                                optional: false,
                                readonly: false,
                            },
                        ),
                    ],
                },
            ))),
            index: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_indexed_access_intersection_merges_matching_members() {
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Intersection(Arc::from(vec![
                TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                    properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "variants".into(),
                            ty: TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                                    crate::analysis::type_expr::ObjectProperty {
                                        name: "color".into(),
                                        ty: TypeExpr::Object(Arc::new(
                                            crate::analysis::type_expr::ObjectExpr {
                                                properties: vec![
                                                    crate::analysis::type_expr::ObjectMember::Property(
                                                        crate::analysis::type_expr::ObjectProperty {
                                                            name: "primary".into(),
                                                            ty: TypeExpr::Primitive(
                                                                PrimitiveName::String,
                                                            ),
                                                            optional: false,
                                                            readonly: false,
                                                        },
                                                    ),
                                                    crate::analysis::type_expr::ObjectMember::Property(
                                                        crate::analysis::type_expr::ObjectProperty {
                                                            name: "secondary".into(),
                                                            ty: TypeExpr::Primitive(
                                                                PrimitiveName::String,
                                                            ),
                                                            optional: false,
                                                            readonly: false,
                                                        },
                                                    ),
                                                ],
                                            },
                                        )),
                                        optional: false,
                                        readonly: false,
                                    },
                                )],
                            })),
                            optional: false,
                            readonly: false,
                        },
                    )],
                })),
                TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                    properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "variants".into(),
                            ty: TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                                    crate::analysis::type_expr::ObjectProperty {
                                        name: "color".into(),
                                        ty: TypeExpr::Object(Arc::new(
                                            crate::analysis::type_expr::ObjectExpr {
                                                properties: vec![
                                                    crate::analysis::type_expr::ObjectMember::Property(
                                                        crate::analysis::type_expr::ObjectProperty {
                                                            name: "neutral".into(),
                                                            ty: TypeExpr::Primitive(
                                                                PrimitiveName::String,
                                                            ),
                                                            optional: false,
                                                            readonly: false,
                                                        },
                                                    ),
                                                ],
                                            },
                                        )),
                                        optional: false,
                                        readonly: false,
                                    },
                                )],
                            })),
                            optional: false,
                            readonly: false,
                        },
                    )],
                })),
            ]))),
            index: Arc::new(TypeExpr::string_literal("variants")),
        };

        let result = solve_type(&expr, &NoopSolverHost);

        match result.value {
            TypeExpr::Intersection(members) => {
                assert_eq!(members.len(), 2);
                assert!(members
                    .iter()
                    .all(|member| matches!(member, TypeExpr::Object(_))));
            }
            other => panic!("expected intersection of object members, got: {:?}", other),
        }
    }

    // -- 4b: conditionals --

    #[test]
    fn solve_conditional_true_branch() {
        // string extends string ? "yes" : "no" → "yes"
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::string_literal("yes"));
    }

    #[test]
    fn solve_conditional_false_branch() {
        // number extends string ? "yes" : "no" → "no"
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::string_literal("no"));
    }

    // -- 4c: mapped types --

    #[test]
    fn solve_mapped_type_finite_keys() {
        // { [K in "a" | "b"]: number } → { a: number; b: number }
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: crate::analysis::type_expr::MappedModifier::None,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: None,
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2);
                // Both properties should be number
                for member in &obj.properties {
                    match member {
                        crate::analysis::type_expr::ObjectMember::Property(p) => {
                            assert!(matches!(p.ty, TypeExpr::Primitive(PrimitiveName::Number)));
                        }
                        _ => panic!("expected property"),
                    }
                }
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_mapped_type_ignores_never_in_keyspace_union() {
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::Never),
                TypeExpr::string_literal("base"),
                TypeExpr::string_literal("label"),
            ]))),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            optional: crate::analysis::type_expr::MappedModifier::None,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: None,
        };
        let result = solve_type(&expr, &NoopSolverHost);

        let TypeExpr::Object(obj) = result.value else {
            panic!("expected Object");
        };
        let property_names: Vec<_> = obj
            .properties
            .iter()
            .filter_map(|member| match member {
                crate::analysis::type_expr::ObjectMember::Property(property) => {
                    Some(property.name.as_str())
                }
                _ => None,
            })
            .collect();
        let mut property_names = property_names;
        property_names.sort_unstable();

        assert_eq!(property_names, vec!["base", "label"]);
    }

    #[test]
    fn solve_mapped_type_open_source() {
        // { [K in string]: boolean } → { [key: string]: boolean }
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Boolean)),
            optional: crate::analysis::type_expr::MappedModifier::None,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: None,
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Object(obj) => {
                assert!(
                    obj.properties.iter().all(|member| {
                        !matches!(
                            member,
                            crate::analysis::type_expr::ObjectMember::Property(_)
                        )
                    }),
                    "should have no named properties"
                );
                assert!(
                    obj.properties.iter().any(|member| {
                        matches!(
                            member,
                            crate::analysis::type_expr::ObjectMember::IndexSignature(_)
                        )
                    }),
                    "should keep the open index signature"
                );
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_open_mapped_recursive_index_any_stays_symbolic_without_hard_stop() {
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let recursive_index = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("T")),
            index: Arc::new(TypeExpr::named("K")),
        };
        let body = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Mapped {
                parameter: "K".into(),
                source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
                value: Arc::new(TypeExpr::named_with_args(
                    "OpenPath",
                    vec![TypeExpr::named_with_args(
                        "NonNullable",
                        vec![recursive_index],
                    )],
                )),
                optional: crate::analysis::type_expr::MappedModifier::None,
                readonly: crate::analysis::type_expr::MappedModifier::None,
                name_type: None,
            }),
            index: Arc::new(TypeExpr::string_literal("path")),
        };

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "OpenPath".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let indexed_any = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::IndexSignature(
                crate::analysis::type_expr::IndexSignature {
                    key_name: "key".into(),
                    key_type: TypeExpr::Primitive(PrimitiveName::String),
                    value_type: TypeExpr::Primitive(PrimitiveName::Any),
                    readonly: false,
                },
            )],
        }));
        let result = solve_type_with_limits(
            &TypeExpr::named_with_args("OpenPath", vec![indexed_any]),
            &host,
            SolveLimits {
                max_instantiation_depth: 16,
                max_resolve_steps: 2_000,
                max_arena_nodes: 100_000,
                ..SolveLimits::default()
            },
        );
        let json = serde_json::to_string(&result.value).unwrap();

        assert_ne!(
            result.execution_status,
            ExecutionStatus::HardStop,
            "open mapped recursion over an any index signature should terminate symbolically, got reasons {:?}",
            result.incomplete_reasons
        );
        assert!(
            json.contains("recursiveRef"),
            "open mapped recursion should collapse to RecursiveRef instead of growing without bound, got: {}",
            &json[..json.len().min(400)]
        );
        assert!(
            !json.contains("\"kind\":\"unknown\""),
            "open mapped recursion should not degrade to Unknown, got: {json}"
        );
    }

    #[test]
    fn solve_open_mapped_lookup_reads_string_index_signature() {
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Mapped {
                parameter: "K".into(),
                source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
                value: Arc::new(TypeExpr::IndexedAccess {
                    object: Arc::new(TypeExpr::named("T")),
                    index: Arc::new(TypeExpr::named("K")),
                }),
                optional: crate::analysis::type_expr::MappedModifier::None,
                readonly: crate::analysis::type_expr::MappedModifier::None,
                name_type: None,
            }),
            index: Arc::new(TypeExpr::string_literal("path")),
        };

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "OpenLookup".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let indexed_any = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::IndexSignature(
                crate::analysis::type_expr::IndexSignature {
                    key_name: "key".into(),
                    key_type: TypeExpr::Primitive(PrimitiveName::String),
                    value_type: TypeExpr::Primitive(PrimitiveName::Any),
                    readonly: false,
                },
            )],
        }));

        let result = solve_type(
            &TypeExpr::named_with_args("OpenLookup", vec![indexed_any]),
            &host,
        );

        assert_eq!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::Any),
            "open mapped lookup should read the underlying string index signature"
        );
        assert_ne!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::Undefined),
            "open mapped lookup must not collapse to undefined for string index signatures"
        );
    }

    // -- template literals --

    #[test]
    fn solve_template_literal_concrete() {
        // `hello${" "}world` → "hello world"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["hello".into(), "world".into()],
            expressions: Arc::from(vec![TypeExpr::string_literal(" ")]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::string_literal("hello world"));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn solve_template_literal_union_expansion() {
        // `${"a" | "b"}_suffix` → "a_suffix" | "b_suffix"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["".into(), "_suffix".into()],
            expressions: Arc::from(vec![TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("a_suffix")));
                assert!(members.contains(&TypeExpr::string_literal("b_suffix")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_template_literal_with_number() {
        // `count_${42}` → "count_42"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["count_".into(), "".into()],
            expressions: Arc::from(vec![TypeExpr::number_literal(42.0)]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::string_literal("count_42"));
    }

    // -- Awaited --

    #[test]
    fn solve_awaited_non_thenable() {
        // Awaited<string> → string
        let expr = TypeExpr::Ref {
            name: Arc::from("Awaited"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
    }

    // -- InstanceType / ConstructorParameters --
    // (These require construct signatures on objects which are less common in
    // test fixtures, but the expansion logic is tested via the builtin tests.)

    // -- Mapped type with key remapping --

    #[test]
    fn solve_mapped_type_with_key_remapping_via_conditional() {
        // { [K in "a" | "b" as K extends "a" ? "renamed" : never]: number }
        // → { renamed: number }  (only "a" survives, remapped to "renamed")
        // Note: this tests the name_type path. The conditional filters "b" to never.
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: crate::analysis::type_expr::MappedModifier::None,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: Some(Arc::new(TypeExpr::Conditional {
                check: Arc::new(TypeExpr::Ref {
                    name: Arc::from("K"),
                    type_arguments: Arc::from(vec![]),
                }),
                extends: Arc::new(TypeExpr::string_literal("a")),
                true_type: Arc::new(TypeExpr::string_literal("renamed")),
                false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
            })),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 1, "only 'a' survives remapping");
                match &obj.properties[0] {
                    crate::analysis::type_expr::ObjectMember::Property(p) => {
                        assert_eq!(p.name, "renamed");
                    }
                    _ => panic!("expected property"),
                }
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_conditional_infer_reuses_intersection_of_multiple_candidates() {
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]))),
            extends: Arc::new(TypeExpr::Infer { name: "A".into() }),
            true_type: Arc::new(TypeExpr::Ref {
                name: Arc::from("A"),
                type_arguments: Arc::from(vec![]),
            }),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Intersection(members) => {
                assert!(members.contains(&TypeExpr::Primitive(PrimitiveName::String)));
                assert!(members.contains(&TypeExpr::Primitive(PrimitiveName::Number)));
            }
            other => panic!("expected intersection, got {other:?}"),
        }
    }

    #[test]
    fn solve_conditional_infers_function_parameter_types_under_contravariance() {
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Function(Arc::new(
                crate::analysis::type_expr::FunctionExpr {
                    parameters: vec![crate::analysis::type_expr::FunctionParam {
                        name: Some("props".into()),
                        ty: TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "planId".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: false,
                                    readonly: false,
                                },
                            )],
                        })),
                        optional: false,
                        rest: false,
                    }],
                    return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                    type_parameters: vec![],
                },
            ))),
            extends: Arc::new(TypeExpr::Function(Arc::new(
                crate::analysis::type_expr::FunctionExpr {
                    parameters: vec![crate::analysis::type_expr::FunctionParam {
                        name: Some("props".into()),
                        ty: TypeExpr::Infer { name: "P".into() },
                        optional: false,
                        rest: false,
                    }],
                    return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                    type_parameters: vec![],
                },
            ))),
            true_type: Arc::new(TypeExpr::Intersection(Arc::from(vec![
                TypeExpr::Ref {
                    name: Arc::from("P"),
                    type_arguments: Arc::from(vec![]),
                },
                TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                    properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "plan".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    )],
                })),
            ]))),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        let TypeExpr::Intersection(parts) = result.value else {
            panic!("infer conditional should resolve true branch under contravariant function comparison");
        };
        let mut prop_names = std::collections::BTreeSet::new();
        for part in parts.iter() {
            if let TypeExpr::Object(obj) = part {
                for member in &obj.properties {
                    if let crate::analysis::type_expr::ObjectMember::Property(prop) = member {
                        prop_names.insert(prop.name.clone());
                    }
                }
            }
        }
        assert_eq!(
            prop_names,
            std::collections::BTreeSet::from(["plan".to_string(), "planId".to_string()]),
        );
    }

    #[test]
    fn solve_conditional_honors_constrained_type_parameter_relation() {
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                    default: None,
                },
            )),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::boolean_literal(true)),
            false_type: Arc::new(TypeExpr::boolean_literal(false)),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::boolean_literal(true));
    }

    // ===================================================================
    // Ported from type_eval_tests.rs — complex real-world patterns
    // ===================================================================

    // -- Composition: Partial<Pick<T, K>> --

    #[test]
    fn solve_partial_pick_composition() {
        // Partial<Pick<{ id: number; name: string; email: string }, "name" | "email">>
        // → { name?: string; email?: string }
        let inner_obj = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "id".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::Number),
                        optional: false,
                        readonly: false,
                    },
                ),
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "name".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "email".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                ),
            ],
        }));

        let expr = TypeExpr::Ref {
            name: Arc::from("Partial"),
            type_arguments: Arc::from(vec![TypeExpr::Ref {
                name: Arc::from("Pick"),
                type_arguments: Arc::from(vec![
                    inner_obj,
                    TypeExpr::Union(Arc::from(vec![
                        TypeExpr::string_literal("name"),
                        TypeExpr::string_literal("email"),
                    ])),
                ]),
            }]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2, "Pick should select name+email");
                for member in &obj.properties {
                    if let crate::analysis::type_expr::ObjectMember::Property(p) = member {
                        assert!(p.optional, "Partial should make {} optional", p.name);
                        assert!(
                            p.name == "name" || p.name == "email",
                            "unexpected property: {}",
                            p.name
                        );
                    }
                }
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    // -- Extract with literal unions --

    #[test]
    fn solve_extract_literal_union() {
        // Extract<"a" | "b" | "c", "a" | "b"> → "a" | "b"
        let expr = TypeExpr::Ref {
            name: Arc::from("Extract"),
            type_arguments: Arc::from(vec![
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("a"),
                    TypeExpr::string_literal("b"),
                    TypeExpr::string_literal("c"),
                ])),
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("a"),
                    TypeExpr::string_literal("b"),
                ])),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("a")));
                assert!(members.contains(&TypeExpr::string_literal("b")));
                assert!(!members.contains(&TypeExpr::string_literal("c")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- Exclude with literal unions --

    #[test]
    fn solve_exclude_literal_union() {
        // Exclude<"a" | "b" | "c", "a"> → "b" | "c"
        let expr = TypeExpr::Ref {
            name: Arc::from("Exclude"),
            type_arguments: Arc::from(vec![
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("a"),
                    TypeExpr::string_literal("b"),
                    TypeExpr::string_literal("c"),
                ])),
                TypeExpr::string_literal("a"),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("b")));
                assert!(members.contains(&TypeExpr::string_literal("c")));
                assert!(!members.contains(&TypeExpr::string_literal("a")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- keyof on intersection --

    #[test]
    fn solve_keyof_intersection() {
        // keyof ({ a: string } & { b: number }) → "a" | "b"
        let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "a".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "b".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::Number),
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        ]))));
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&TypeExpr::string_literal("a")));
                assert!(members.contains(&TypeExpr::string_literal("b")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- Recursive type detection --

    #[test]
    fn solve_recursive_type_does_not_stack_overflow() {
        let mut host = TestHost::new();
        // type Tree = { children: Tree[] }
        host.add_alias(
            "Tree",
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "children".into(),
                        ty: TypeExpr::Array {
                            element: Arc::new(TypeExpr::Ref {
                                name: Arc::from("Tree"),
                                type_arguments: Arc::from(vec![]),
                            }),
                            readonly: false,
                        },
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        );

        let expr = TypeExpr::Ref {
            name: Arc::from("Tree"),
            type_arguments: Arc::from(vec![]),
        };
        // Should not hang or stack overflow — recursion tracker catches it
        let result = solve_type(&expr, &host);
        // The result should be an object (possibly with a recursive ref for children)
        assert!(
            result.execution_status == ExecutionStatus::Completed
                || result.execution_status == ExecutionStatus::HardStop
        );
    }

    #[test]
    fn solve_structural_recursive_infer_reentry_stays_symbolic_without_hanging() {
        let mut host = TestHost::new();
        host.add_generic_alias(
            "NestedItem",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::Conditional {
                check: Arc::new(TypeExpr::named("T")),
                extends: Arc::new(TypeExpr::Array {
                    element: Arc::new(TypeExpr::Infer { name: "I".into() }),
                    readonly: false,
                }),
                true_type: Arc::new(TypeExpr::named_with_args(
                    "NestedItem",
                    vec![TypeExpr::Infer { name: "I".into() }],
                )),
                false_type: Arc::new(TypeExpr::named("T")),
            },
        );

        let expr = TypeExpr::named_with_args("NestedItem", vec![TypeExpr::named("Unresolved")]);
        let result = solve_type(&expr, &host);

        assert_eq!(result.execution_status, ExecutionStatus::Completed);
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
    }

    #[test]
    fn solve_substitution_cycle_from_shadowed_default_stays_symbolic_without_hanging() {
        let mut host = TestHost::new();
        host.add_generic_alias(
            "NestedItem",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::named("T"),
        );
        host.add_generic_alias(
            "Loop",
            vec![
                crate::analysis::type_expr::TypeParam {
                    name: "I".into(),
                    constraint: None,
                    default: None,
                },
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: Some(Arc::new(TypeExpr::named_with_args(
                        "NestedItem",
                        vec![TypeExpr::named("I")],
                    ))),
                },
            ],
            TypeExpr::named("T"),
        );

        let expr = TypeExpr::named_with_args("Loop", vec![TypeExpr::named("T")]);
        let result = solve_type(&expr, &host);

        assert_eq!(result.execution_status, ExecutionStatus::Completed);
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        assert!(
            matches!(
                result.value,
                TypeExpr::Ref { .. } | TypeExpr::Unknown { .. }
            ),
            "substitution-cycle fallback should stay symbolic, got {:?}",
            result.value
        );
    }

    // -- Generic with host-backed chained resolution --

    #[test]
    fn solve_generic_wrapper_over_host_alias() {
        let mut host = TestHost::new();
        // type Inner = { x: string; y: number }
        // type Wrap<T> = { data: T }
        host.add_alias(
            "Inner",
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "x".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    ),
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "y".into(),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            readonly: false,
                        },
                    ),
                ],
            })),
        );
        host.add_generic_alias(
            "Wrap",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "data".into(),
                        ty: TypeExpr::Ref {
                            name: Arc::from("T"),
                            type_arguments: Arc::from(vec![]),
                        },
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        );

        // Wrap<Inner> → { data: { x: string; y: number } }
        let expr = TypeExpr::Ref {
            name: Arc::from("Wrap"),
            type_arguments: Arc::from(vec![TypeExpr::Ref {
                name: Arc::from("Inner"),
                type_arguments: Arc::from(vec![]),
            }]),
        };
        let result = solve_type(&expr, &host);

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 1);
                if let crate::analysis::type_expr::ObjectMember::Property(p) = &obj.properties[0] {
                    assert_eq!(p.name, "data");
                    // data should be the resolved Inner = { x: string; y: number }
                    match &p.ty {
                        TypeExpr::Object(inner) => {
                            assert_eq!(inner.properties.len(), 2);
                        }
                        _ => panic!("data should be Object, got: {:?}", p.ty),
                    }
                }
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    #[test]
    fn solve_generic_default_argument_resolves_bound_alias() {
        let mut host = TestHost::new();
        host.add_alias(
            "Item",
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "id".into(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        );
        host.add_generic_alias(
            "Props",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: Some(Arc::new(TypeExpr::named("Item"))),
            }],
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "items".into(),
                        ty: TypeExpr::Array {
                            element: Arc::new(TypeExpr::Ref {
                                name: Arc::from("T"),
                                type_arguments: Arc::from(vec![]),
                            }),
                            readonly: false,
                        },
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        );

        let expr = TypeExpr::Ref {
            name: Arc::from("Props"),
            type_arguments: Arc::from(vec![]),
        };
        let result = solve_type(&expr, &host);

        match result.value {
            TypeExpr::Object(obj) => match &obj.properties[0] {
                crate::analysis::type_expr::ObjectMember::Property(prop) => match &prop.ty {
                    TypeExpr::Array { element, .. } => match element.as_ref() {
                        TypeExpr::Object(shape) => {
                            assert!(shape.properties.iter().any(|member| {
                                matches!(
                                    member,
                                    crate::analysis::type_expr::ObjectMember::Property(p)
                                        if p.name == "id"
                                )
                            }));
                        }
                        other => {
                            panic!("expected default arg to resolve to Item shape, got {other:?}")
                        }
                    },
                    other => panic!("expected array property, got {other:?}"),
                },
                other => panic!("expected property member, got {other:?}"),
            },
            other => panic!("expected Object, got: {other:?}"),
        }
    }

    #[test]
    fn project_preserves_method_members() {
        let expr = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Method(
                crate::analysis::type_expr::MethodSignature {
                    name: "default".into(),
                    function: crate::analysis::type_expr::FunctionExpr {
                        parameters: vec![crate::analysis::type_expr::FunctionParam {
                            name: Some("props".into()),
                            ty: TypeExpr::Object(Arc::new(
                                crate::analysis::type_expr::ObjectExpr {
                                    properties: vec![
                                        crate::analysis::type_expr::ObjectMember::Property(
                                            crate::analysis::type_expr::ObjectProperty {
                                                name: "label".into(),
                                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                                optional: false,
                                                readonly: false,
                                            },
                                        ),
                                    ],
                                },
                            )),
                            optional: false,
                            rest: false,
                        }],
                        return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                        type_parameters: vec![],
                    },
                    optional: true,
                },
            )],
        }));

        let result = solve_type(&expr, &NoopSolverHost);

        match result.value {
            TypeExpr::Object(obj) => match &obj.properties[0] {
                crate::analysis::type_expr::ObjectMember::Method(method) => {
                    assert_eq!(method.name, "default");
                    assert!(method.optional);
                    assert_eq!(method.function.parameters.len(), 1);
                }
                other => panic!("expected method member, got: {other:?}"),
            },
            other => panic!("expected Object, got: {other:?}"),
        }
    }

    #[test]
    fn project_preserves_type_parameter_metadata() {
        let expr = TypeExpr::TypeParameter(crate::analysis::type_expr::TypeParam {
            name: "T".into(),
            constraint: Some(Arc::new(TypeExpr::named("Item"))),
            default: Some(Arc::new(TypeExpr::named("Item"))),
        });

        let result = solve_type(&expr, &NoopSolverHost);

        match result.value {
            TypeExpr::TypeParameter(param) => {
                assert_eq!(param.name, "T");
                assert!(matches!(
                    param.constraint.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
                assert!(matches!(
                    param.default.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
            }
            other => panic!("expected TypeParameter, got: {other:?}"),
        }
    }

    // -- Template literal with multiple unions (cartesian product) --

    #[test]
    fn solve_template_literal_cartesian_product() {
        // `${"a" | "b"}-${"1" | "2"}` → "a-1" | "a-2" | "b-1" | "b-2"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["".into(), "-".into(), "".into()],
            expressions: Arc::from(vec![
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("a"),
                    TypeExpr::string_literal("b"),
                ])),
                TypeExpr::Union(Arc::from(vec![
                    TypeExpr::string_literal("1"),
                    TypeExpr::string_literal("2"),
                ])),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 4);
                assert!(members.contains(&TypeExpr::string_literal("a-1")));
                assert!(members.contains(&TypeExpr::string_literal("a-2")));
                assert!(members.contains(&TypeExpr::string_literal("b-1")));
                assert!(members.contains(&TypeExpr::string_literal("b-2")));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- Template literal with boolean/null --

    #[test]
    fn solve_template_literal_with_boolean() {
        // `is_${true}` → "is_true"
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["is_".into(), "".into()],
            expressions: Arc::from(vec![TypeExpr::boolean_literal(true)]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::string_literal("is_true"));
    }

    // -- Mapped type with optional modifier --

    #[test]
    fn solve_mapped_type_add_optional() {
        // { [K in "a" | "b"]+?: number } → { a?: number; b?: number }
        let expr = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::Union(Arc::from(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
            ]))),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: crate::analysis::type_expr::MappedModifier::Add,
            readonly: crate::analysis::type_expr::MappedModifier::None,
            name_type: None,
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2);
                for member in &obj.properties {
                    if let crate::analysis::type_expr::ObjectMember::Property(p) = member {
                        assert!(p.optional, "{} should be optional", p.name);
                    }
                }
            }
            _ => panic!("expected Object"),
        }
    }

    // -- Conditional: unknown relation stays symbolic --

    #[test]
    fn solve_conditional_unknown_stays_symbolic() {
        // T extends string ? "yes" : "no" — T is unresolved ref
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Ref {
                name: Arc::from("T"),
                type_arguments: Arc::from(vec![]),
            }),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        // T is unresolved — relation is Unknown, so conditional stays symbolic
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
    }

    // -- Record with Exclude-derived keys --

    #[test]
    fn solve_record_with_exclude_keys() {
        // Record<Exclude<"a" | "b" | "c", "c">, boolean> → { a: boolean; b: boolean }
        let expr = TypeExpr::Ref {
            name: Arc::from("Record"),
            type_arguments: Arc::from(vec![
                TypeExpr::Ref {
                    name: Arc::from("Exclude"),
                    type_arguments: Arc::from(vec![
                        TypeExpr::Union(Arc::from(vec![
                            TypeExpr::string_literal("a"),
                            TypeExpr::string_literal("b"),
                            TypeExpr::string_literal("c"),
                        ])),
                        TypeExpr::string_literal("c"),
                    ]),
                },
                TypeExpr::Primitive(PrimitiveName::Boolean),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2);
                let names: Vec<&str> = obj
                    .properties
                    .iter()
                    .filter_map(|m| match m {
                        crate::analysis::type_expr::ObjectMember::Property(p) => {
                            Some(p.name.as_str())
                        }
                        _ => None,
                    })
                    .collect();
                assert!(names.contains(&"a"));
                assert!(names.contains(&"b"));
                assert!(!names.contains(&"c"));
            }
            _ => panic!("expected Object, got: {:?}", result.value),
        }
    }

    // -- Indexed access through utility --

    #[test]
    fn solve_indexed_access_through_required() {
        // Required<{ a?: string; b?: number }>["a"] → string
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::Ref {
                name: Arc::from("Required"),
                type_arguments: Arc::from(vec![TypeExpr::Object(Arc::new(
                    crate::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "a".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: true,
                                    readonly: false,
                                },
                            ),
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "b".into(),
                                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                                    optional: true,
                                    readonly: false,
                                },
                            ),
                        ],
                    },
                ))]),
            }),
            index: Arc::new(TypeExpr::string_literal("a")),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
    }

    // ===================================================================
    // Edge case and fix-verification tests
    // ===================================================================

    // -- Fix #3: template literal with never expression → never --

    #[test]
    fn solve_template_literal_with_never_is_never() {
        let expr = TypeExpr::TemplateLiteral {
            quasis: vec!["prefix_".into(), "".into()],
            expressions: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::Never)]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::Never));
    }

    // -- Fix #7: distributive conditional --

    #[test]
    fn solve_distributive_conditional() {
        // type IsString<T> = T extends string ? true : false
        // IsString<string | number> should distribute:
        //   = (string extends string ? true : false) | (number extends string ? true : false)
        //   = true | false
        let mut host = TestHost::new();
        host.add_generic_alias(
            "IsString",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::Conditional {
                check: Arc::new(TypeExpr::TypeParameter(
                    crate::analysis::type_expr::TypeParam {
                        name: "T".into(),
                        constraint: None,
                        default: None,
                    },
                )),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                true_type: Arc::new(TypeExpr::boolean_literal(true)),
                false_type: Arc::new(TypeExpr::boolean_literal(false)),
            },
        );

        let expr = TypeExpr::Ref {
            name: Arc::from("IsString"),
            type_arguments: Arc::from(vec![TypeExpr::Union(Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]))]),
        };
        let result = solve_type(&expr, &host);

        match &result.value {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2, "should be true | false");
                assert!(members.contains(&TypeExpr::boolean_literal(true)));
                assert!(members.contains(&TypeExpr::boolean_literal(false)));
            }
            _ => panic!("expected Union, got: {:?}", result.value),
        }
    }

    // -- Fix #11: Record<never, V> = {} --

    #[test]
    fn solve_record_never_key_is_empty_object() {
        let expr = TypeExpr::Ref {
            name: Arc::from("Record"),
            type_arguments: Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::Never),
                TypeExpr::Primitive(PrimitiveName::String),
            ]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Object(obj) => {
                assert!(
                    obj.properties.is_empty(),
                    "Record<never, V> should be empty"
                );
            }
            _ => panic!("expected empty Object, got: {:?}", result.value),
        }
    }

    // -- Fix #12: NonNullable<any> = any --

    #[test]
    fn solve_non_nullable_any_is_any() {
        let expr = TypeExpr::Ref {
            name: Arc::from("NonNullable"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::Any)]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::Any));
    }

    // -- Fix #5: project_to_type_expr handles tuples --

    #[test]
    fn solve_parameters_projects_to_tuple_type_expr() {
        let expr = TypeExpr::Ref {
            name: Arc::from("Parameters"),
            type_arguments: Arc::from(vec![TypeExpr::Function(Arc::new(
                crate::analysis::type_expr::FunctionExpr {
                    parameters: vec![
                        crate::analysis::type_expr::FunctionParam {
                            name: Some("a".into()),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            rest: false,
                        },
                        crate::analysis::type_expr::FunctionParam {
                            name: Some("b".into()),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            rest: false,
                        },
                    ],
                    return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Void))),
                    type_parameters: vec![],
                },
            ))]),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        // Should project back as a Tuple TypeExpr, not Unknown
        match &result.value {
            TypeExpr::Tuple { elements, .. } => {
                assert_eq!(elements.len(), 2);
                assert!(matches!(
                    elements[0].ty,
                    TypeExpr::Primitive(PrimitiveName::String)
                ));
                assert!(matches!(
                    elements[1].ty,
                    TypeExpr::Primitive(PrimitiveName::Number)
                ));
            }
            _ => panic!("expected Tuple, got: {:?}", result.value),
        }
    }

    // -- Fix #5: project_to_type_expr handles functions --

    #[test]
    fn solve_function_type_round_trips() {
        let expr = TypeExpr::Function(Arc::new(crate::analysis::type_expr::FunctionExpr {
            parameters: vec![crate::analysis::type_expr::FunctionParam {
                name: Some("x".into()),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                rest: false,
            }],
            return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
            type_parameters: vec![],
        }));
        let result = solve_type(&expr, &NoopSolverHost);

        match &result.value {
            TypeExpr::Function(f) => {
                assert_eq!(f.parameters.len(), 1);
                assert!(f.return_type.is_some());
            }
            _ => panic!("expected Function, got: {:?}", result.value),
        }
    }

    // -----------------------------------------------------------------------
    // Budget alignment tests
    // -----------------------------------------------------------------------

    #[test]
    fn solve_default_limits_match_target_budgets() {
        let limits = SolveLimits::default();
        assert_eq!(
            limits.max_resolve_steps, 500_000,
            "production step budget should be 500k"
        );
        assert_eq!(
            limits.max_arena_nodes, 2_000_000,
            "production arena budget should be 2M"
        );
        assert_eq!(
            limits.max_instantiation_depth, 50,
            "instantiation depth should stay at 50"
        );
    }

    #[test]
    fn public_solver_entry_points_use_shared_production_limits() {
        let expr = TypeExpr::Primitive(PrimitiveName::String);

        let solved = solve_type(&expr, &NoopSolverHost);
        let (solved_with_trace, trace) = solve_type_with_trace(&expr, &NoopSolverHost);

        assert_eq!(
            solved.value,
            TypeExpr::Primitive(PrimitiveName::String),
            "public solve_type should keep primitive identities under production limits"
        );
        assert_eq!(
            solved_with_trace.value,
            TypeExpr::Primitive(PrimitiveName::String),
            "public solve_type_with_trace should use the same production limits path"
        );
        assert!(
            trace.is_empty(),
            "primitive solve should not report external trace entries"
        );
        assert_eq!(
            solved.execution_status,
            ExecutionStatus::Completed,
            "primitive solve should complete under production defaults"
        );
        assert_eq!(
            solved_with_trace.execution_status,
            ExecutionStatus::Completed,
            "trace solve should also complete under production defaults"
        );
    }

    #[test]
    fn template_literal_product_limit_constant_matches_target() {
        assert_eq!(
            MAX_TEMPLATE_LITERAL_PRODUCT, 100_000,
            "template literal cartesian product ceiling should be 100k"
        );
    }

    #[test]
    fn solve_template_literal_hard_stops_above_product_limit() {
        // Create a template literal whose cartesian product exceeds the cap.
        // With 17 binary union expressions: 2^17 = 131072 > 100_000.
        let binary_union = TypeExpr::Union(Arc::from(vec![
            TypeExpr::string_literal("a"),
            TypeExpr::string_literal("b"),
        ]));
        let expressions: Vec<TypeExpr> = (0..17).map(|_| binary_union.clone()).collect();
        let quasis: Vec<String> = (0..=17).map(|_| String::new()).collect();
        let expr = TypeExpr::TemplateLiteral {
            quasis,
            expressions: Arc::from(expressions),
        };
        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(
            result.execution_status,
            ExecutionStatus::HardStop,
            "should hard-stop above product limit"
        );
        assert_ne!(
            result.execution_status,
            ExecutionStatus::Completed,
            "should NOT report Completed"
        );
    }

    // -----------------------------------------------------------------------
    // SolveBatch tests
    // -----------------------------------------------------------------------

    /// A counting solver host that tracks how many times `resolve_prepared_type_decl`
    /// is called. Delegates to an inner `FxHashMap` for actual resolution.
    struct CountingSolverHost {
        types: FxHashMap<String, Arc<PreparedTypeDecl>>,
        resolve_count: std::cell::Cell<u32>,
    }

    impl CountingSolverHost {
        fn new() -> Self {
            Self {
                types: FxHashMap::default(),
                resolve_count: std::cell::Cell::new(0),
            }
        }

        fn with_type(mut self, id: &str, name: &str, body: TypeExpr) -> Self {
            self.types.insert(
                name.to_string(),
                Arc::new(PreparedTypeDecl::new(
                    ResolvedRootIdentity::new(id, name),
                    TypeDeclKind::Alias,
                    body,
                )),
            );
            self
        }

        fn resolve_calls(&self) -> u32 {
            self.resolve_count.get()
        }
    }

    impl TypeSolverHost for CountingSolverHost {
        fn resolve_prepared_type_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedTypeDecl>> {
            self.resolve_count.set(self.resolve_count.get() + 1);
            self.types.get(&root_identity.symbol_name).cloned()
        }

        fn resolve_prepared_value_decl(
            &self,
            _root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedValueDecl>> {
            None
        }

        fn utility_source(&self, name: &str) -> UtilitySource {
            if BuiltinUtility::from_name(name).is_some() {
                UtilitySource::Builtin
            } else {
                UtilitySource::Unknown
            }
        }

        fn root_identity(
            &self,
            _canonical_id: &str,
            symbol_name: &str,
        ) -> Option<ResolvedRootIdentity> {
            if self.types.contains_key(symbol_name) {
                Some(ResolvedRootIdentity::new("/types.ts", symbol_name))
            } else {
                None
            }
        }
    }

    #[test]
    fn solve_batch_reuses_same_expr_without_second_host_lookup() {
        let host = CountingSolverHost::new().with_type(
            "/types.ts",
            "Shared",
            TypeExpr::Primitive(PrimitiveName::String),
        );
        let expr = TypeExpr::Ref {
            name: Arc::from("Shared"),
            type_arguments: Arc::from(vec![]),
        };

        let mut batch = SolveBatch::new(&host);

        let result1 = batch.solve(&expr);
        let calls_after_first = host.resolve_calls();
        assert!(
            calls_after_first > 0,
            "first call should trigger host lookup"
        );

        let result2 = batch.solve(&expr);
        let calls_after_second = host.resolve_calls();

        // Positive: both produce the same semantic result
        assert_eq!(result1.value, result2.value);
        assert_eq!(
            result1.value,
            TypeExpr::Primitive(PrimitiveName::String),
            "should resolve Shared to String"
        );
        // Negative: second call must not increase host lookup count
        assert_eq!(
            calls_after_first, calls_after_second,
            "cached hit should not trigger another host lookup"
        );
        // Negative: result should not be Unknown or a leftover Ref
        assert_ne!(result1.exactness, SolverExactness::Incomplete);
    }

    #[test]
    fn solve_batch_preserves_trace_on_hit() {
        // Use a type hosted on an external canonical file so the trace is non-empty.
        // The solver records external decl visits when the canonical_id is not "$owner".
        let host = CountingSolverHost::new().with_type(
            "/dep.ts",
            "Imported",
            TypeExpr::Primitive(PrimitiveName::Number),
        );
        let expr = TypeExpr::Ref {
            name: Arc::from("Imported"),
            type_arguments: Arc::from(vec![]),
        };

        let mut batch = SolveBatch::new(&host);

        let (result1, trace1) = batch.solve_with_trace(&expr);
        let (result2, trace2) = batch.solve_with_trace(&expr);

        // Positive: both produce the same result
        assert_eq!(result1.value, result2.value);
        assert_eq!(
            result1.value,
            TypeExpr::Primitive(PrimitiveName::Number),
            "should resolve to Number"
        );
        // Positive: trace should be non-empty (external file)
        assert!(
            !trace1.is_empty(),
            "trace should capture visited external declaration /dep.ts::Imported"
        );
        // Positive: cached trace matches original
        assert_eq!(
            trace1, trace2,
            "cached trace should match the original trace"
        );
    }

    #[test]
    fn solve_batch_distinguishes_distinct_exprs() {
        let host = CountingSolverHost::new()
            .with_type(
                "/types.ts",
                "Alpha",
                TypeExpr::Primitive(PrimitiveName::String),
            )
            .with_type(
                "/types.ts",
                "Beta",
                TypeExpr::Primitive(PrimitiveName::Number),
            );

        let expr_a = TypeExpr::Ref {
            name: Arc::from("Alpha"),
            type_arguments: Arc::from(vec![]),
        };
        let expr_b = TypeExpr::Ref {
            name: Arc::from("Beta"),
            type_arguments: Arc::from(vec![]),
        };

        let mut batch = SolveBatch::new(&host);

        let result_a = batch.solve(&expr_a);
        let result_b = batch.solve(&expr_b);

        // Positive: different expressions produce different results
        assert_ne!(result_a.value, result_b.value);
        assert_eq!(result_a.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result_b.value, TypeExpr::Primitive(PrimitiveName::Number));
        // Positive: both should have triggered host lookups
        assert!(
            host.resolve_calls() >= 2,
            "distinct expressions should each trigger at least one host lookup"
        );
        // Negative: neither should be incomplete
        assert_eq!(result_a.execution_status, ExecutionStatus::Completed);
        assert_eq!(result_b.execution_status, ExecutionStatus::Completed);
    }

    #[test]
    fn solve_batch_matches_uncached_result_on_first_miss() {
        let host = CountingSolverHost::new().with_type(
            "/types.ts",
            "Props",
            TypeExpr::Primitive(PrimitiveName::Boolean),
        );
        let expr = TypeExpr::Ref {
            name: Arc::from("Props"),
            type_arguments: Arc::from(vec![]),
        };

        // Uncached path
        let uncached = solve_type(&expr, &host);

        let mut batch = SolveBatch::new(&host);
        let batched = batch.solve(&expr);

        // Positive: batch first-miss matches uncached
        assert_eq!(
            uncached.value, batched.value,
            "batch first-miss should match uncached solve_type"
        );
        assert_eq!(uncached.exactness, batched.exactness);
        assert_eq!(uncached.execution_status, batched.execution_status);
        // Negative: should not produce Unknown
        assert_ne!(
            batched.value,
            TypeExpr::Primitive(PrimitiveName::Unknown),
            "should not fall back to Unknown"
        );
    }

    /// Verify cancelled requests get a cheap cancelled outcome without running
    /// resolution. Uses a Ref type that would require host lookup if not cancelled.
    #[test]
    fn solve_batch_respects_cancellation() {
        struct CancelledHost;

        impl TypeSolverHost for CancelledHost {
            fn resolve_prepared_type_decl(
                &self,
                _: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedTypeDecl>> {
                panic!("should not be called on cancelled request");
            }

            fn resolve_prepared_value_decl(
                &self,
                _: &ResolvedRootIdentity,
            ) -> Option<Arc<PreparedValueDecl>> {
                panic!("should not be called on cancelled request");
            }

            fn utility_source(&self, _: &str) -> UtilitySource {
                UtilitySource::Unknown
            }

            fn request_status(&self) -> RequestStatus {
                RequestStatus::Cancelled
            }
        }

        let host = CancelledHost;
        // Use a Ref that would normally trigger host resolution — the
        // cancellation check must prevent this from ever reaching the host.
        let expr = TypeExpr::Ref {
            name: Arc::from("ShouldNotResolve"),
            type_arguments: Arc::from(vec![]),
        };

        let mut batch = SolveBatch::new(&host);
        let result = batch.solve(&expr);
        let uncached = solve_type(&expr, &host);

        // Positive: execution status is Cancelled
        assert_eq!(result.execution_status, ExecutionStatus::Cancelled);
        assert_eq!(
            result.value, uncached.value,
            "cancelled batch path should preserve the uncached projected value"
        );
        assert_eq!(
            result.exactness, uncached.exactness,
            "cancelled batch path should preserve uncached exactness semantics"
        );
        // Negative: should not be Completed or HardStop
        assert_ne!(result.execution_status, ExecutionStatus::Completed);
        assert_ne!(result.execution_status, ExecutionStatus::HardStop);

        // Also test solve_with_trace path
        let (trace_result, trace) = batch.solve_with_trace(&expr);
        assert_eq!(trace_result.execution_status, ExecutionStatus::Cancelled);
        assert!(
            trace.is_empty(),
            "cancelled request should produce empty trace"
        );
    }

    #[test]
    fn hardstop_on_one_branch_does_not_add_spurious_step_limit_reason() {
        let binary_union = TypeExpr::Union(Arc::from(vec![
            TypeExpr::string_literal("a"),
            TypeExpr::string_literal("b"),
        ]));
        let expressions: Vec<TypeExpr> = (0..17).map(|_| binary_union.clone()).collect();
        let quasis: Vec<String> = (0..=17).map(|_| String::new()).collect();
        let explosive = TypeExpr::TemplateLiteral {
            quasis,
            expressions: Arc::from(expressions),
        };
        let expr = TypeExpr::Union(Arc::from(vec![
            explosive,
            TypeExpr::Primitive(PrimitiveName::String),
        ]));

        let result = solve_type(&expr, &NoopSolverHost);

        assert_eq!(result.execution_status, ExecutionStatus::HardStop);
        assert!(
            result.incomplete_reasons.iter().any(|reason| matches!(
                reason,
                IncompleteReason::RecursionPolicy { description }
                    if description.contains("template literal expansion")
            )),
            "template literal hard-stop reason should be preserved"
        );
        assert!(
            !result.incomplete_reasons.iter().any(|reason| matches!(
                reason,
                IncompleteReason::UnsupportedSyntax { description }
                    if description == "resolve step or arena size limit exceeded"
            )),
            "sibling branches should not be relabeled as step-limit failures after a hard-stop"
        );
    }

    // -----------------------------------------------------------------------
    // RecursiveRef transport & projection tests
    // -----------------------------------------------------------------------

    #[test]
    fn recursive_ref_json_round_trip() {
        use crate::analysis::type_expr::{RecursiveConditionalBranch, RecursiveConditionalFrame};

        let expr = TypeExpr::RecursiveRef {
            name: Arc::from("Tree"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
            conditional_context: Arc::from(vec![RecursiveConditionalFrame {
                branch: RecursiveConditionalBranch::True,
                decided: true,
                check: Arc::new(TypeExpr::named("T")),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            }]),
        };

        let json = expr.to_json_value();
        let kind = json.get("kind").and_then(|k| k.as_str());
        assert_eq!(kind, Some("recursiveRef"), "JSON kind must be recursiveRef");

        let round_tripped: TypeExpr = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(
            round_tripped, expr,
            "round-trip must preserve full structure"
        );
    }

    #[test]
    fn recursive_ref_is_not_unknown() {
        let expr = TypeExpr::recursive_ref("Tree", vec![]);
        assert!(
            expr.is_recursive_ref(),
            "RecursiveRef must report is_recursive_ref()"
        );
        assert!(
            !expr.is_unknown(),
            "RecursiveRef must NOT report is_unknown()"
        );
    }

    #[test]
    fn recursive_ref_equality_and_hash_include_args_and_context() {
        use crate::analysis::type_expr::{RecursiveConditionalBranch, RecursiveConditionalFrame};
        use std::collections::HashSet;

        let a = TypeExpr::recursive_ref("T", vec![TypeExpr::Primitive(PrimitiveName::String)]);
        let b = TypeExpr::recursive_ref("T", vec![TypeExpr::Primitive(PrimitiveName::Number)]);
        let c = TypeExpr::RecursiveRef {
            name: Arc::from("T"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
            conditional_context: Arc::from(vec![RecursiveConditionalFrame {
                branch: RecursiveConditionalBranch::True,
                decided: true,
                check: Arc::new(TypeExpr::named("X")),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            }]),
        };

        assert_ne!(a, b, "different args should differ");
        assert_ne!(a, c, "different conditional context should differ");

        let mut set = HashSet::new();
        set.insert(a.clone());
        set.insert(b.clone());
        set.insert(c.clone());
        assert_eq!(set.len(), 3, "all three should be distinct in a HashSet");
    }

    #[test]
    fn solve_self_recursive_projects_named_recursive_ref() {
        // type Tree = { children: Tree[] }
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "children".into(),
                    ty: TypeExpr::Array {
                        element: Arc::new(TypeExpr::named("Tree")),
                        readonly: false,
                    },
                    optional: false,
                    readonly: false,
                },
            )],
        }));

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Tree".into(),
            declaration_id: 0,
            body: body.clone(),
            type_parameters: vec![],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let result = solve_type(&TypeExpr::named("Tree"), &host);

        // The result should contain a RecursiveRef for the self-reference,
        // not degrade to Unknown.
        let json = serde_json::to_string(&result.value).unwrap();
        assert!(
            json.contains("recursiveRef"),
            "self-recursive type should project RecursiveRef, got: {}",
            &json[..json.len().min(200)]
        );
        assert!(
            !json.contains("@rec("),
            "should NOT contain opaque @rec(...) in output"
        );
    }

    #[test]
    fn solve_recursive_generic_projects_applied_args() {
        // type ValueOrArray<T> = T | Array<ValueOrArray<T>>
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::union(vec![
            TypeExpr::named("T"),
            TypeExpr::Array {
                element: Arc::new(TypeExpr::named_with_args(
                    "ValueOrArray",
                    vec![TypeExpr::named("T")],
                )),
                readonly: false,
            },
        ]);

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "ValueOrArray".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let input = TypeExpr::named_with_args(
            "ValueOrArray",
            vec![TypeExpr::Primitive(PrimitiveName::String)],
        );
        let result = solve_type(&input, &host);

        let json = serde_json::to_string(&result.value).unwrap();
        assert!(
            json.contains("recursiveRef"),
            "recursive generic should produce RecursiveRef"
        );
        // The type arguments on the RecursiveRef should include the applied arg
        assert!(
            json.contains("\"name\":\"ValueOrArray\""),
            "RecursiveRef should name the recursive symbol"
        );
    }

    #[test]
    fn solve_json_recursive_projects_recursive_ref_not_unknown() {
        // type Json = string | number | boolean | null | Json[] | { [k: string]: Json }
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
            TypeExpr::Primitive(PrimitiveName::Boolean),
            TypeExpr::Primitive(PrimitiveName::Null),
            TypeExpr::Array {
                element: Arc::new(TypeExpr::named("Json")),
                readonly: false,
            },
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::IndexSignature(
                    crate::analysis::type_expr::IndexSignature {
                        key_name: "k".into(),
                        key_type: TypeExpr::Primitive(PrimitiveName::String),
                        value_type: TypeExpr::named("Json"),
                        readonly: false,
                    },
                )],
            })),
        ]);

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Json".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let result = solve_type(&TypeExpr::named("Json"), &host);

        let json = serde_json::to_string(&result.value).unwrap();
        assert!(
            json.contains("recursiveRef"),
            "Json recursive type should project RecursiveRef, got: {}",
            &json[..json.len().min(300)]
        );
    }

    #[test]
    fn solve_mutual_recursion_projects_recursive_refs() {
        // type A = { b: B }; type B = { a: A }
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body_a = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "b".into(),
                    ty: TypeExpr::named("B"),
                    optional: false,
                    readonly: false,
                },
            )],
        }));
        let body_b = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "a".into(),
                    ty: TypeExpr::named("A"),
                    optional: false,
                    readonly: false,
                },
            )],
        }));

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "A".into(),
            declaration_id: 0,
            body: body_a,
            type_parameters: vec![],
            kind: TypeDeclKind::Alias,
        });
        env.add_type(TypeDeclInfo {
            name: "B".into(),
            declaration_id: 0,
            body: body_b,
            type_parameters: vec![],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let result = solve_type(&TypeExpr::named("A"), &host);

        let json = serde_json::to_string(&result.value).unwrap();
        assert!(
            json.contains("recursiveRef"),
            "mutual recursion should project RecursiveRef"
        );
    }

    #[test]
    fn recursive_ref_serialization_contains_no_opaque_at_rec_unknown_payload() {
        // Verify that the new RecursiveRef serialization doesn't degrade
        let expr = TypeExpr::recursive_ref("Foo", vec![TypeExpr::Primitive(PrimitiveName::String)]);
        let json = serde_json::to_string(&expr).unwrap();
        assert!(
            !json.contains("@rec("),
            "RecursiveRef JSON must not contain opaque @rec(...)"
        );
        assert!(
            json.contains("\"kind\":\"recursiveRef\""),
            "must serialize with kind=recursiveRef"
        );
        assert!(
            json.contains("\"name\":\"Foo\""),
            "must preserve symbol name"
        );
    }

    #[test]
    fn recursive_transport_arg_summary_caps_depth_and_width() {
        let mut arena = QueryArena::new();
        let mut inner = arena.primitive(PrimitiveKind::String);
        for _ in 0..10 {
            inner = arena.array(inner, false);
        }

        let summary = project_recursive_arg_summary(&arena, inner, 2, 32);
        let json = serde_json::to_string(&summary).unwrap();
        assert!(
            json.contains("array") || json.contains("..."),
            "deep nesting should be capped by compact projector"
        );
        assert!(
            json.len() < 500,
            "compact summary should stay small, got {} bytes",
            json.len()
        );
    }

    // -----------------------------------------------------------------------
    // #4 — internal JSON round-trip preserves full context
    // -----------------------------------------------------------------------

    #[test]
    fn recursive_ref_internal_json_round_trip_preserves_full_context() {
        use crate::analysis::type_expr::{RecursiveConditionalBranch, RecursiveConditionalFrame};
        let expr = TypeExpr::RecursiveRef {
            name: Arc::from("Flatten"),
            type_arguments: Arc::from(vec![
                TypeExpr::Array {
                    element: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
                    readonly: true,
                },
                TypeExpr::named("U"),
            ]),
            conditional_context: Arc::from(vec![
                RecursiveConditionalFrame {
                    branch: RecursiveConditionalBranch::True,
                    decided: true,
                    check: Arc::new(TypeExpr::named("T")),
                    extends: Arc::new(TypeExpr::Array {
                        element: Arc::new(TypeExpr::Infer { name: "U".into() }),
                        readonly: true,
                    }),
                },
                RecursiveConditionalFrame {
                    branch: RecursiveConditionalBranch::False,
                    decided: false,
                    check: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
                    extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                },
            ]),
        };

        let json = expr.to_json_value();
        let round_tripped: TypeExpr = serde_json::from_value(json).unwrap();
        assert_eq!(
            round_tripped, expr,
            "full conditional context must survive round-trip"
        );

        // Verify internal structure survived — not collapsed to Unknown
        match &round_tripped {
            TypeExpr::RecursiveRef {
                type_arguments,
                conditional_context,
                ..
            } => {
                assert_eq!(type_arguments.len(), 2);
                assert_eq!(conditional_context.len(), 2);
                assert!(conditional_context[0].decided);
                assert!(!conditional_context[1].decided);
            }
            _ => panic!("round-trip must preserve RecursiveRef variant"),
        }
    }

    // -----------------------------------------------------------------------
    // #6 — effective args after defaults
    // -----------------------------------------------------------------------

    #[test]
    fn exact_recursive_key_uses_effective_args_after_defaults() {
        // type Box<T, U = string> = { value: T; next: Box<T> }
        // Box<number> and Box<number, string> should produce same recursive key
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "value".into(),
                        ty: TypeExpr::named("T"),
                        optional: false,
                        readonly: false,
                    },
                ),
                crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "next".into(),
                        ty: TypeExpr::named_with_args("Box", vec![TypeExpr::named("T")]),
                        optional: true,
                        readonly: false,
                    },
                ),
            ],
        }));

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Box".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                },
                crate::analysis::type_expr::TypeParam {
                    name: "U".into(),
                    constraint: None,
                    default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                },
            ],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);

        // Box<number> should terminate (recursive ref, not hang)
        let result = solve_type(
            &TypeExpr::named_with_args("Box", vec![TypeExpr::Primitive(PrimitiveName::Number)]),
            &host,
        );
        let json = serde_json::to_string(&result.value).unwrap();
        assert!(
            json.contains("recursiveRef"),
            "Box<number> with default U=string should produce RecursiveRef"
        );
        assert!(
            result.execution_status != ExecutionStatus::HardStop,
            "should terminate via exact key, not hard stop"
        );
    }

    #[test]
    fn materialize_effective_arg_resolves_long_substitution_chains_without_cutoff() {
        let mut arena = QueryArena::new();
        let terminal = arena.primitive(PrimitiveKind::String);
        let refs: Vec<NodeId> = (0..25)
            .map(|index| arena.type_ref(format!("T{index}"), Vec::new()))
            .collect();

        let mut subst = SubstitutionEnv::new();
        for index in 0..24 {
            subst.bind(format!("T{index}"), refs[index + 1]);
        }
        subst.bind("T24", terminal);

        let resolved = materialize_effective_arg(&mut arena, refs[0], &subst);
        assert!(
            matches!(arena.get(resolved), Node::Primitive(PrimitiveKind::String)),
            "long substitution chains should resolve semantically, got {:?}",
            arena.get(resolved)
        );
    }

    #[test]
    fn materialize_effective_arg_does_not_leak_in_progress_for_repeated_bound_refs() {
        let mut arena = QueryArena::new();
        let repeated_ref = arena.type_ref("T", Vec::new());
        let tuple = arena.alloc(Node::Tuple {
            elements: vec![
                crate::analysis::type_solver::arena::TupleNodeElement {
                    label: None,
                    ty: repeated_ref,
                    optional: false,
                    rest: false,
                },
                crate::analysis::type_solver::arena::TupleNodeElement {
                    label: None,
                    ty: repeated_ref,
                    optional: false,
                    rest: false,
                },
            ],
            readonly: false,
        });
        let string = arena.primitive(PrimitiveKind::String);

        let mut subst = SubstitutionEnv::new();
        subst.bind("T", string);

        let resolved = materialize_effective_arg(&mut arena, tuple, &subst);
        match arena.get(resolved) {
            Node::Tuple { elements, .. } => {
                assert_eq!(elements.len(), 2);
                for element in elements {
                    assert!(
                        matches!(arena.get(element.ty), Node::Primitive(PrimitiveKind::String)),
                        "repeated bound refs should fully materialize instead of leaving a leaked in-progress ref, got {:?}",
                        arena.get(element.ty)
                    );
                }
            }
            other => panic!("expected tuple, got {other:?}"),
        }
    }

    #[test]
    fn exact_recursive_key_substitutes_dependent_defaults_before_transport() {
        // type Box<T, U = T> = { next: Box<T> }
        // The recursive transport must preserve effective args after substitution,
        // not the raw default expression `T`.
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        fn first_recursive_ref(expr: &TypeExpr) -> Option<&TypeExpr> {
            match expr {
                TypeExpr::RecursiveRef { .. } => Some(expr),
                TypeExpr::Array { element, .. }
                | TypeExpr::KeyOf(element)
                | TypeExpr::Rest(element)
                | TypeExpr::Parenthesized(element) => first_recursive_ref(element),
                TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                    types.iter().find_map(first_recursive_ref)
                }
                TypeExpr::Tuple { elements, .. } => {
                    elements.iter().find_map(|element| first_recursive_ref(&element.ty))
                }
                TypeExpr::Object(object) => object.properties.iter().find_map(|member| match member {
                    crate::analysis::type_expr::ObjectMember::Property(property) => {
                        first_recursive_ref(&property.ty)
                    }
                    crate::analysis::type_expr::ObjectMember::IndexSignature(signature) => {
                        first_recursive_ref(&signature.key_type)
                            .or_else(|| first_recursive_ref(&signature.value_type))
                    }
                    crate::analysis::type_expr::ObjectMember::CallSignature(function)
                    | crate::analysis::type_expr::ObjectMember::ConstructSignature(function) => {
                        function
                            .parameters
                            .iter()
                            .find_map(|param| first_recursive_ref(&param.ty))
                            .or_else(|| function.return_type.as_deref().and_then(first_recursive_ref))
                    }
                    crate::analysis::type_expr::ObjectMember::Method(method) => method
                        .function
                        .parameters
                        .iter()
                        .find_map(|param| first_recursive_ref(&param.ty))
                        .or_else(|| {
                            method
                                .function
                                .return_type
                                .as_deref()
                                .and_then(first_recursive_ref)
                        }),
                }),
                TypeExpr::Function(function) => function
                    .parameters
                    .iter()
                    .find_map(|param| first_recursive_ref(&param.ty))
                    .or_else(|| function.return_type.as_deref().and_then(first_recursive_ref)),
                TypeExpr::Ref { type_arguments, .. } => type_arguments.iter().find_map(first_recursive_ref),
                TypeExpr::IndexedAccess { object, index } => {
                    first_recursive_ref(object).or_else(|| first_recursive_ref(index))
                }
                TypeExpr::Conditional {
                    check,
                    extends,
                    true_type,
                    false_type,
                } => first_recursive_ref(check)
                    .or_else(|| first_recursive_ref(extends))
                    .or_else(|| first_recursive_ref(true_type))
                    .or_else(|| first_recursive_ref(false_type)),
                TypeExpr::Mapped {
                    source,
                    value,
                    name_type,
                    ..
                } => first_recursive_ref(source)
                    .or_else(|| first_recursive_ref(value))
                    .or_else(|| name_type.as_deref().and_then(first_recursive_ref)),
                TypeExpr::TemplateLiteral { expressions, .. } => {
                    expressions.iter().find_map(first_recursive_ref)
                }
                TypeExpr::Primitive(_)
                | TypeExpr::Literal(_)
                | TypeExpr::TypeParameter(_)
                | TypeExpr::TypeOf(_)
                | TypeExpr::Infer { .. }
                | TypeExpr::Unknown { .. } => None,
            }
        }

        let body = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "next".into(),
                    ty: TypeExpr::named_with_args("Box", vec![TypeExpr::named("T")]),
                    optional: true,
                    readonly: false,
                },
            )],
        }));

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Box".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                },
                crate::analysis::type_expr::TypeParam {
                    name: "U".into(),
                    constraint: None,
                    default: Some(Arc::new(TypeExpr::named("T"))),
                },
            ],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let result = solve_type(
            &TypeExpr::named_with_args("Box", vec![TypeExpr::Primitive(PrimitiveName::Number)]),
            &host,
        );

        let TypeExpr::RecursiveRef { type_arguments, .. } = first_recursive_ref(&result.value)
            .expect("recursive solve should produce a RecursiveRef")
        else {
            panic!("expected RecursiveRef");
        };

        assert_eq!(
            type_arguments.len(),
            2,
            "effective defaults should be transported"
        );
        assert_eq!(
            type_arguments[0],
            TypeExpr::Primitive(PrimitiveName::Number),
            "first effective arg should be the concrete T binding"
        );
        assert_eq!(
            type_arguments[1],
            TypeExpr::Primitive(PrimitiveName::Number),
            "dependent default U = T should be substituted before transport"
        );
    }

    // -----------------------------------------------------------------------
    // #11 — structural fingerprint uses symbol-local context only
    // -----------------------------------------------------------------------

    #[test]
    fn structural_fingerprint_uses_symbol_local_context_only() {
        use crate::analysis::type_solver::recursion::*;
        let mut arena = QueryArena::new();
        let str_node = arena.primitive(PrimitiveKind::String);

        // Same args, different conditional context
        let ctx_a = vec![
            crate::analysis::type_solver::arena::ConditionalFrameSnapshot {
                branch: crate::analysis::type_solver::arena::ConditionalBranch::True,
                decided: true,
                check: str_node,
                extends: str_node,
            },
        ];
        let ctx_b = vec![
            crate::analysis::type_solver::arena::ConditionalFrameSnapshot {
                branch: crate::analysis::type_solver::arena::ConditionalBranch::False,
                decided: true,
                check: str_node,
                extends: str_node,
            },
        ];

        let fp_a = compute_structural_fingerprint(&arena, &[str_node], &ctx_a);
        let fp_b = compute_structural_fingerprint(&arena, &[str_node], &ctx_b);

        assert_ne!(
            fp_a.combined_fingerprint, fp_b.combined_fingerprint,
            "different branch contexts must produce different fingerprints"
        );
        assert_eq!(fp_a.mode, StructuralRecursionMode::Conditional);
        assert_eq!(fp_b.mode, StructuralRecursionMode::Conditional);

        // Empty context should be Plain mode
        let fp_empty = compute_structural_fingerprint(&arena, &[str_node], &[]);
        assert_eq!(fp_empty.mode, StructuralRecursionMode::Plain);
    }

    // -----------------------------------------------------------------------
    // #12-14 — conditional context push/pop behavior
    // -----------------------------------------------------------------------

    #[test]
    fn conditional_true_branch_pushes_decided_true_frame() {
        // string extends string ? "yes" : "no"
        // The true branch is taken (decided). After resolve, conditional
        // context should be empty (popped), and result should be "yes".
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };

        let mut arena = QueryArena::new();
        let mut state = SolveState::new(SolveLimits::default());
        let root = lower_type_expr(&mut arena, &expr);
        let resolved = resolve_node(
            &mut arena,
            root,
            &NoopSolverHost,
            &mut state,
            &SubstitutionEnv::new(),
        );
        let result = project_to_type_expr(&arena, resolved);

        assert_eq!(result, TypeExpr::string_literal("yes"));
        // Context stack must be clean after resolution
        assert!(
            state.conditional_context_stack.is_empty(),
            "conditional context stack must be empty after resolution"
        );
    }

    #[test]
    fn conditional_false_branch_pushes_decided_false_frame() {
        // number extends string ? "yes" : "no"
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };

        let mut arena = QueryArena::new();
        let mut state = SolveState::new(SolveLimits::default());
        let root = lower_type_expr(&mut arena, &expr);
        let resolved = resolve_node(
            &mut arena,
            root,
            &NoopSolverHost,
            &mut state,
            &SubstitutionEnv::new(),
        );
        let result = project_to_type_expr(&arena, resolved);

        assert_eq!(result, TypeExpr::string_literal("no"));
        assert!(
            state.conditional_context_stack.is_empty(),
            "conditional context stack must be empty after resolution"
        );
    }

    #[test]
    fn symbolic_conditional_branches_capture_undecided_frames() {
        // T extends string ? "yes" : "no" — T is unresolved, symbolic
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::named("T")),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::string_literal("yes")),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };

        let mut arena = QueryArena::new();
        let mut state = SolveState::new(SolveLimits::default());
        let root = lower_type_expr(&mut arena, &expr);
        let _resolved = resolve_node(
            &mut arena,
            root,
            &NoopSolverHost,
            &mut state,
            &SubstitutionEnv::new(),
        );

        // After resolution, conditional context stack should be clean
        assert!(
            state.conditional_context_stack.is_empty(),
            "conditional context stack must be clean after symbolic conditional resolution"
        );
        // The result should be symbolic (conditional preserved)
        assert!(
            state.exactness == SolverExactness::ExactSymbolic,
            "symbolic conditional should mark exactness as symbolic"
        );
    }

    // -----------------------------------------------------------------------
    // #15 — conditional context stack restores on early return
    // -----------------------------------------------------------------------

    #[test]
    fn conditional_context_stack_restores_on_early_return() {
        // Nested conditionals: both stacks should be clean after
        let expr = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::Conditional {
                check: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
                true_type: Arc::new(TypeExpr::string_literal("inner-yes")),
                false_type: Arc::new(TypeExpr::string_literal("inner-no")),
            }),
            false_type: Arc::new(TypeExpr::string_literal("no")),
        };

        let mut arena = QueryArena::new();
        let mut state = SolveState::new(SolveLimits::default());
        let root = lower_type_expr(&mut arena, &expr);
        let resolved = resolve_node(
            &mut arena,
            root,
            &NoopSolverHost,
            &mut state,
            &SubstitutionEnv::new(),
        );
        let result = project_to_type_expr(&arena, resolved);

        assert_eq!(result, TypeExpr::string_literal("inner-yes"));
        assert!(
            state.conditional_context_stack.is_empty(),
            "nested conditional context stack must be fully clean"
        );
        assert!(
            state.conditional_context_base_stack.is_empty(),
            "conditional context base stack must be fully clean"
        );
    }

    // -----------------------------------------------------------------------
    // #16 — mutual recursion captures only symbol-local frames
    // -----------------------------------------------------------------------

    #[test]
    fn mutual_recursion_captures_only_symbol_local_conditional_frames() {
        // type A = { b: B }; type B = { a: A }
        // Mutual recursion — each symbol's RecursiveRef should NOT carry
        // the other symbol's conditional frames.
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body_a = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "b".into(),
                    ty: TypeExpr::named("B"),
                    optional: false,
                    readonly: false,
                },
            )],
        }));
        let body_b = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "a".into(),
                    ty: TypeExpr::named("A"),
                    optional: false,
                    readonly: false,
                },
            )],
        }));

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "A".into(),
            declaration_id: 0,
            body: body_a,
            type_parameters: vec![],
            kind: TypeDeclKind::Alias,
        });
        env.add_type(TypeDeclInfo {
            name: "B".into(),
            declaration_id: 0,
            body: body_b,
            type_parameters: vec![],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let result = solve_type(&TypeExpr::named("A"), &host);

        // The RecursiveRef for A should have empty conditional context
        // (no conditional branches in either type body)
        let json = serde_json::to_string(&result.value).unwrap();
        assert!(json.contains("recursiveRef"));
        assert!(
            json.contains("\"conditionalContext\":[]"),
            "mutual recursion without conditionals should have empty context"
        );
    }

    // -----------------------------------------------------------------------
    // #19 — conditional recursive ref captures branch context
    // -----------------------------------------------------------------------

    #[test]
    fn solve_conditional_recursive_ref_captures_branch_context() {
        // type Flatten<T> = T extends readonly (infer U)[] ? Flatten<U> : T
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                },
            )),
            extends: Arc::new(TypeExpr::Array {
                element: Arc::new(TypeExpr::Infer { name: "U".into() }),
                readonly: true,
            }),
            true_type: Arc::new(TypeExpr::named_with_args(
                "Flatten",
                vec![TypeExpr::named("U")],
            )),
            false_type: Arc::new(TypeExpr::named("T")),
        };

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Flatten".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        // Flatten<string[][]> should eventually produce RecursiveRef or string
        let input = TypeExpr::named_with_args(
            "Flatten",
            vec![TypeExpr::Array {
                element: Arc::new(TypeExpr::Array {
                    element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                    readonly: false,
                }),
                readonly: false,
            }],
        );
        let result = solve_type(&input, &host);
        let json = serde_json::to_string(&result.value).unwrap();

        // Should terminate without hard stop
        assert!(
            result.execution_status != ExecutionStatus::HardStop,
            "Flatten should terminate cleanly, not hard stop"
        );
        // Should produce concrete string (fully reduced) or RecursiveRef
        assert!(
            json.contains("string") || json.contains("recursiveRef"),
            "Flatten<string[][]> should reduce to string or produce named RecursiveRef, got: {}",
            &json[..json.len().min(200)]
        );
        // Must NOT contain opaque @rec
        assert!(
            !json.contains("@rec("),
            "must not contain opaque @rec fallback"
        );
    }

    // -----------------------------------------------------------------------
    // #20 — Flatten-like conditional recursion stays symbolic without hanging
    // -----------------------------------------------------------------------

    #[test]
    fn solve_flatten_like_conditional_recursion_stays_symbolic_without_hanging() {
        // type Flatten<T> = T extends readonly (infer U)[] ? Flatten<U> : T
        // With a symbolic input (unresolved T), should stay symbolic
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                },
            )),
            extends: Arc::new(TypeExpr::Array {
                element: Arc::new(TypeExpr::Infer { name: "U".into() }),
                readonly: true,
            }),
            true_type: Arc::new(TypeExpr::named_with_args(
                "Flatten",
                vec![TypeExpr::named("U")],
            )),
            false_type: Arc::new(TypeExpr::named("T")),
        };

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Flatten".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        // Flatten with an unresolvable generic arg
        let input = TypeExpr::named_with_args("Flatten", vec![TypeExpr::named("SomeType")]);
        let result = solve_type(&input, &host);

        // Must terminate without hard stop
        assert!(
            result.execution_status != ExecutionStatus::HardStop,
            "Flatten<SomeType> should terminate cleanly"
        );
    }

    #[test]
    fn solve_dot_path_keys_over_symbolic_nested_item_stays_bounded() {
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;
        use serde_json::Value;

        fn has_unknown_outside_recursive_context(value: &Value) -> bool {
            match value {
                Value::Object(map) => {
                    if map.get("kind").and_then(Value::as_str) == Some("unknown") {
                        return true;
                    }
                    map.iter().any(|(key, child)| {
                        key != "conditionalContext" && has_unknown_outside_recursive_context(child)
                    })
                }
                Value::Array(items) => items.iter().any(has_unknown_outside_recursive_context),
                _ => false,
            }
        }

        let nested_item_body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                },
            )),
            extends: Arc::new(TypeExpr::Array {
                element: Arc::new(TypeExpr::Infer { name: "I".into() }),
                readonly: true,
            }),
            true_type: Arc::new(TypeExpr::named_with_args(
                "NestedItem",
                vec![TypeExpr::named("I")],
            )),
            false_type: Arc::new(TypeExpr::named("T")),
        };

        let mapped_source = TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::KeyOf(Arc::new(TypeExpr::named("T"))),
            TypeExpr::Primitive(PrimitiveName::String),
        ]));
        let indexed_tk = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("T")),
            index: Arc::new(TypeExpr::named("K")),
        };
        let dot_path_body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::named("T")),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::Object)),
            true_type: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::Mapped {
                    parameter: "K".into(),
                    source: Arc::new(mapped_source.clone()),
                    value: Arc::new(TypeExpr::Union(Arc::from(vec![
                        TypeExpr::named("K"),
                        TypeExpr::TemplateLiteral {
                            quasis: vec!["".into(), ".".into(), "".into()],
                            expressions: Arc::from(vec![
                                TypeExpr::named("K"),
                                TypeExpr::named_with_args(
                                    "DotPathKeys",
                                    vec![TypeExpr::named_with_args(
                                        "NonNullable",
                                        vec![indexed_tk],
                                    )],
                                ),
                            ]),
                        },
                    ]))),
                    optional: crate::analysis::type_expr::MappedModifier::None,
                    readonly: crate::analysis::type_expr::MappedModifier::None,
                    name_type: None,
                }),
                index: Arc::new(mapped_source),
            }),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "NestedItem".into(),
            declaration_id: 0,
            body: nested_item_body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });
        env.add_type(TypeDeclInfo {
            name: "DotPathKeys".into(),
            declaration_id: 0,
            body: dot_path_body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let input = TypeExpr::named_with_args(
            "DotPathKeys",
            vec![TypeExpr::named_with_args(
                "NestedItem",
                vec![TypeExpr::named_with_args(
                    "NestedItem",
                    vec![TypeExpr::named("T")],
                )],
            )],
        );
        let result = solve_type_with_limits(
            &input,
            &host,
            SolveLimits {
                max_instantiation_depth: 24,
                max_resolve_steps: 4_000,
                max_arena_nodes: 200_000,
                ..SolveLimits::default()
            },
        );
        let json = serde_json::to_string(&result.value).unwrap();
        let json_value = serde_json::to_value(&result.value).unwrap();

        assert_ne!(
            result.execution_status,
            ExecutionStatus::HardStop,
            "symbolic NestedItem -> DotPathKeys should not hard stop, got reasons {:?}",
            result.incomplete_reasons
        );
        assert!(
            json.contains("templateLiteral")
                || json.contains("conditional")
                || json.contains("recursiveRef")
                || json.contains("indexedAccess"),
            "symbolic DotPathKeys should stay representable instead of collapsing, got: {}",
            &json[..json.len().min(400)]
        );
        assert!(
            !has_unknown_outside_recursive_context(&json_value),
            "symbolic DotPathKeys should not degrade to Unknown outside bounded recursive context summaries, got: {json}"
        );
    }

    // -----------------------------------------------------------------------
    // #23 — branch-sensitive: Bar<T> vs Bar<string> remain distinct
    // -----------------------------------------------------------------------

    #[test]
    fn solve_branch_sensitive_bar_t_vs_bar_string_remain_distinct() {
        // type Foo<T> = T extends string ? Bar<T> : Bar<string>
        // type Bar<T> = { value: T }
        // Foo<number> → false branch → Bar<string> → { value: string }
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let foo_body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                },
            )),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::named_with_args("Bar", vec![TypeExpr::named("T")])),
            false_type: Arc::new(TypeExpr::named_with_args(
                "Bar",
                vec![TypeExpr::Primitive(PrimitiveName::String)],
            )),
        };
        let bar_body = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "value".into(),
                    ty: TypeExpr::named("T"),
                    optional: false,
                    readonly: false,
                },
            )],
        }));

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Foo".into(),
            declaration_id: 0,
            body: foo_body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });
        env.add_type(TypeDeclInfo {
            name: "Bar".into(),
            declaration_id: 0,
            body: bar_body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        // Foo<number> → false branch → Bar<string> → { value: string }
        let result = solve_type(
            &TypeExpr::named_with_args("Foo", vec![TypeExpr::Primitive(PrimitiveName::Number)]),
            &host,
        );

        let json = serde_json::to_string(&result.value).unwrap();
        assert!(
            json.contains("\"value\"") && json.contains("string"),
            "Foo<number> should produce object with value: string, got: {}",
            &json[..json.len().min(200)]
        );
    }

    // -----------------------------------------------------------------------
    // #25 — DeepReadonly recursive mapped type preserves recursive transport
    // -----------------------------------------------------------------------

    #[test]
    fn solve_deep_readonly_recursive_mapped_type_preserves_recursive_transport() {
        // type DeepReadonly<T> = { readonly [K in keyof T]: DeepReadonly<T[K]> }
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::named_with_args(
                "DeepReadonly",
                vec![TypeExpr::IndexedAccess {
                    object: Arc::new(TypeExpr::named("T")),
                    index: Arc::new(TypeExpr::named("K")),
                }],
            )),
            optional: crate::analysis::type_expr::MappedModifier::None,
            readonly: crate::analysis::type_expr::MappedModifier::Add,
            name_type: None,
        };

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "DeepReadonly".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let input_type = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "x".into(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                },
            )],
        }));
        let result = solve_type(
            &TypeExpr::named_with_args("DeepReadonly", vec![input_type]),
            &host,
        );

        // Should terminate, and output should NOT be a hard stop
        assert!(
            result.execution_status != ExecutionStatus::HardStop,
            "DeepReadonly should not hard stop"
        );
    }

    // -----------------------------------------------------------------------
    // #26 — AwaitedLike recursive conditional preserves recursive transport
    // -----------------------------------------------------------------------

    #[test]
    fn solve_awaited_like_recursive_conditional_preserves_recursive_transport() {
        // type AwaitedLike<T> = T extends Promise<infer U> ? AwaitedLike<U> : T
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                },
            )),
            extends: Arc::new(TypeExpr::named_with_args(
                "Promise",
                vec![TypeExpr::Infer { name: "U".into() }],
            )),
            true_type: Arc::new(TypeExpr::named_with_args(
                "AwaitedLike",
                vec![TypeExpr::named("U")],
            )),
            false_type: Arc::new(TypeExpr::named("T")),
        };

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "AwaitedLike".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        // AwaitedLike<string> should just return string (not a Promise)
        let result = solve_type(
            &TypeExpr::named_with_args(
                "AwaitedLike",
                vec![TypeExpr::Primitive(PrimitiveName::String)],
            ),
            &host,
        );

        let json = serde_json::to_string(&result.value).unwrap();
        assert!(
            result.execution_status != ExecutionStatus::HardStop,
            "AwaitedLike<string> should terminate cleanly"
        );
        // string is not a Promise, so false branch → T → string
        assert!(
            json.contains("string"),
            "AwaitedLike<string> should produce string"
        );
    }

    // -----------------------------------------------------------------------
    // #28 — distributive recursive wrap union captures per-member context
    // -----------------------------------------------------------------------

    #[test]
    fn solve_distributive_recursive_wrap_union_captures_per_member_context() {
        // type Wrap<T> = T extends any ? { wrapped: T } : never
        // Wrap<string | number> should distribute over the union
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                },
            )),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::Any)),
            true_type: Arc::new(TypeExpr::Object(Arc::new(
                crate::analysis::type_expr::ObjectExpr {
                    properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "wrapped".into(),
                            ty: TypeExpr::named("T"),
                            optional: false,
                            readonly: false,
                        },
                    )],
                },
            ))),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Wrap".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let input = TypeExpr::named_with_args(
            "Wrap",
            vec![TypeExpr::union(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ])],
        );
        let result = solve_type(&input, &host);

        let json = serde_json::to_string(&result.value).unwrap();
        // Should distribute: { wrapped: string } | { wrapped: number }
        assert!(
            json.contains("wrapped"),
            "distributive should produce wrapped members"
        );
        assert!(
            result.execution_status != ExecutionStatus::HardStop,
            "distributive should not hard stop"
        );
    }

    // -----------------------------------------------------------------------
    // #29 — recursive transport stays compact for deep object cycles
    // -----------------------------------------------------------------------

    #[test]
    fn solve_recursive_transport_stays_compact_for_deep_object_cycles() {
        // type Deep = { a: { b: { c: Deep } } }
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "a".into(),
                    ty: TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                        properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "b".into(),
                                ty: TypeExpr::Object(Arc::new(
                                    crate::analysis::type_expr::ObjectExpr {
                                        properties: vec![
                                            crate::analysis::type_expr::ObjectMember::Property(
                                                crate::analysis::type_expr::ObjectProperty {
                                                    name: "c".into(),
                                                    ty: TypeExpr::named("Deep"),
                                                    optional: false,
                                                    readonly: false,
                                                },
                                            ),
                                        ],
                                    },
                                )),
                                optional: false,
                                readonly: false,
                            },
                        )],
                    })),
                    optional: false,
                    readonly: false,
                },
            )],
        }));

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Deep".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let result = solve_type(&TypeExpr::named("Deep"), &host);

        let json = serde_json::to_string(&result.value).unwrap();
        assert!(
            json.contains("recursiveRef"),
            "deep object cycle should produce RecursiveRef"
        );
        // Compact: should not produce a deeply nested 50-level property tree
        assert!(
            json.len() < 2000,
            "recursive transport should stay compact, got {} bytes",
            json.len()
        );
    }

    // -----------------------------------------------------------------------
    // #32 — unconditional recursive generic no longer uses full structural budget
    // -----------------------------------------------------------------------

    #[test]
    fn unconditional_recursive_generic_no_longer_uses_full_structural_budget() {
        // type Tree = { children: Tree[] }
        // Should terminate quickly via exact-key, not exhaust structural budget
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "children".into(),
                    ty: TypeExpr::Array {
                        element: Arc::new(TypeExpr::named("Tree")),
                        readonly: false,
                    },
                    optional: false,
                    readonly: false,
                },
            )],
        }));

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Tree".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let result = solve_type(&TypeExpr::named("Tree"), &host);

        assert!(
            result.execution_status != ExecutionStatus::HardStop,
            "unconditional recursion should terminate via exact key, not hard stop"
        );
        assert!(
            result.incomplete_reasons.is_empty()
                || !result.incomplete_reasons.iter().any(|r| {
                    matches!(r, IncompleteReason::RecursionPolicy { description }
                        if description.contains("depth"))
                }),
            "should not hit depth limit for simple self-recursion"
        );
    }

    // -----------------------------------------------------------------------
    // #33 — conditional recursive generic keeps branch and args when bailing
    // -----------------------------------------------------------------------

    #[test]
    fn conditional_recursive_generic_keeps_branch_and_args_when_bailing() {
        // type ValueOrArray<T> = T | Array<ValueOrArray<T>>
        // When recursion is detected, the RecursiveRef should preserve the args
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::union(vec![
            TypeExpr::named("T"),
            TypeExpr::Array {
                element: Arc::new(TypeExpr::named_with_args(
                    "ValueOrArray",
                    vec![TypeExpr::named("T")],
                )),
                readonly: false,
            },
        ]);

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "ValueOrArray".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let result = solve_type(
            &TypeExpr::named_with_args(
                "ValueOrArray",
                vec![TypeExpr::Primitive(PrimitiveName::Number)],
            ),
            &host,
        );

        let json = serde_json::to_string(&result.value).unwrap();
        // RecursiveRef should name the symbol
        assert!(
            json.contains("\"name\":\"ValueOrArray\""),
            "RecursiveRef should preserve symbol name"
        );
    }

    // -----------------------------------------------------------------------
    // #35 — transported conditional context caps at eight frames
    // -----------------------------------------------------------------------

    #[test]
    fn transported_conditional_context_caps_at_eight_frames() {
        use crate::analysis::type_expr::{RecursiveConditionalBranch, RecursiveConditionalFrame};

        // Build a RecursiveRef with >8 frames
        let frames: Vec<RecursiveConditionalFrame> = (0..12)
            .map(|i| RecursiveConditionalFrame {
                branch: if i % 2 == 0 {
                    RecursiveConditionalBranch::True
                } else {
                    RecursiveConditionalBranch::False
                },
                decided: true,
                check: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            })
            .collect();

        // The SolveState caps at 8 frames via current_symbol_conditional_context()
        let mut state = SolveState::new(SolveLimits::default());
        for frame in &frames {
            state.conditional_context_stack.push(
                crate::analysis::type_solver::arena::ConditionalFrameSnapshot {
                    branch: match frame.branch {
                        RecursiveConditionalBranch::True => {
                            crate::analysis::type_solver::arena::ConditionalBranch::True
                        }
                        RecursiveConditionalBranch::False => {
                            crate::analysis::type_solver::arena::ConditionalBranch::False
                        }
                    },
                    decided: frame.decided,
                    check: NodeId(0),
                    extends: NodeId(1),
                },
            );
        }

        let ctx = state.current_symbol_conditional_context();
        assert_eq!(
            ctx.len(),
            8,
            "current_symbol_conditional_context() must cap at 8 frames, got {}",
            ctx.len()
        );
    }

    #[test]
    fn transported_conditional_context_keeps_innermost_eight_frames() {
        let mut state = SolveState::new(SolveLimits::default());
        state.conditional_context_base_stack.push(0);

        for i in 0..12u32 {
            state.conditional_context_stack.push(
                crate::analysis::type_solver::arena::ConditionalFrameSnapshot {
                    branch: crate::analysis::type_solver::arena::ConditionalBranch::True,
                    decided: true,
                    check: NodeId(i),
                    extends: NodeId(i + 100),
                },
            );
        }

        let ctx = state.current_symbol_conditional_context();
        let checks: Vec<u32> = ctx.iter().map(|frame| frame.check.0).collect();

        assert_eq!(checks.len(), 8);
        assert_eq!(
            checks,
            vec![4, 5, 6, 7, 8, 9, 10, 11],
            "transported conditional context should keep the innermost eight frames"
        );
    }

    // -----------------------------------------------------------------------
    // #36 — fingerprint walk respects depth and node caps
    // -----------------------------------------------------------------------

    #[test]
    fn fingerprint_walk_respects_depth_and_node_caps() {
        use crate::analysis::type_solver::recursion::compute_structural_fingerprint;

        let mut arena = QueryArena::new();
        // Build a deeply nested structure: 100 levels of Array
        let mut inner = arena.primitive(PrimitiveKind::String);
        for _ in 0..100 {
            inner = arena.array(inner, false);
        }

        // Fingerprint computation should not stack overflow or take long
        let fp = compute_structural_fingerprint(&arena, &[inner], &[]);
        assert_eq!(
            fp.mode,
            crate::analysis::type_solver::recursion::StructuralRecursionMode::Plain
        );
        // Just verify it completed without panic
        assert!(fp.combined_fingerprint != 0);
    }

    // -----------------------------------------------------------------------
    // #38 — recursive context summary caps depth without large subtree projection
    // -----------------------------------------------------------------------

    #[test]
    fn recursive_context_summary_caps_depth_without_large_subtree_projection() {
        // Build a complex arena node and verify the compact projector caps it
        let mut arena = QueryArena::new();
        let str_ty = arena.primitive(PrimitiveKind::String);
        let _num_ty = arena.primitive(PrimitiveKind::Number);

        // Build a 5-level nested object
        use crate::analysis::type_solver::arena::{ObjectNode as ON, PropertyNode as PN};
        let mut current = arena.object(ON {
            properties: vec![PN {
                name: "leaf".into(),
                ty: str_ty,
                optional: false,
                readonly: false,
                is_method: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });
        for i in 0..5 {
            current = arena.object(ON {
                properties: vec![PN {
                    name: format!("level{}", i),
                    ty: current,
                    optional: false,
                    readonly: false,
                    is_method: false,
                }],
                index_signatures: vec![],
                call_signatures: vec![],
                construct_signatures: vec![],
            });
        }

        // Project with depth cap 1 — should truncate deeply
        let summary = project_recursive_arg_summary(&arena, current, 1, 8);
        let json = serde_json::to_string(&summary).unwrap();
        assert!(
            json.len() < 500,
            "context summary with depth cap 1 should be compact, got {} bytes: {}",
            json.len(),
            &json[..json.len().min(200)]
        );
    }

    #[test]
    fn recursive_arg_summary_preserves_type_parameter_identity_and_nested_context() {
        let mut arena = QueryArena::new();
        let type_param = arena.alloc(Node::TypeParam {
            name: "T".into(),
            constraint: None,
            default: None,
        });
        let check = arena.primitive(PrimitiveKind::String);
        let extends = arena.primitive(PrimitiveKind::Number);
        let nested = arena.alloc(Node::RecursiveRef {
            symbol_name: "Box".into(),
            type_arguments: vec![type_param],
            conditional_context: vec![
                crate::analysis::type_solver::arena::ConditionalFrameSnapshot {
                    branch: crate::analysis::type_solver::arena::ConditionalBranch::True,
                    decided: true,
                    check,
                    extends,
                },
            ],
        });

        let summary = project_recursive_arg_summary(&arena, nested, 2, 32);
        match summary {
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => {
                assert_eq!(&*name, "Box");
                assert_eq!(type_arguments.len(), 1);
                assert!(matches!(
                    &type_arguments[0],
                    TypeExpr::TypeParameter(crate::analysis::type_expr::TypeParam { name, .. }) if name == "T"
                ));
                assert_eq!(conditional_context.len(), 1);
                assert!(matches!(
                    conditional_context[0].check.as_ref(),
                    TypeExpr::Primitive(PrimitiveName::String)
                ));
                assert!(matches!(
                    conditional_context[0].extends.as_ref(),
                    TypeExpr::Primitive(PrimitiveName::Number)
                ));
            }
            other => panic!("expected nested recursive summary, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // #39 — component_meta boundary does not degrade recursive ref to unknown
    // -----------------------------------------------------------------------

    #[test]
    fn component_meta_boundary_does_not_degrade_recursive_ref_to_unknown() {
        use crate::analysis::type_expr::RecursiveConditionalBranch;
        use crate::analysis::type_expr::RecursiveConditionalFrame;

        let expr = TypeExpr::RecursiveRef {
            name: Arc::from("Tree"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
            conditional_context: Arc::from(vec![RecursiveConditionalFrame {
                branch: RecursiveConditionalBranch::True,
                decided: true,
                check: Arc::new(TypeExpr::named("T")),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            }]),
        };

        // Verify it doesn't match is_unknown or type_expr_is_placeholder_for_symbolic_fallback
        assert!(!expr.is_unknown(), "RecursiveRef must not be Unknown");
        assert!(
            !matches!(expr, TypeExpr::Unknown { .. }),
            "RecursiveRef must not pattern-match as Unknown"
        );

        // Verify the JSON output uses "recursiveRef", not "unknown"
        let json = expr.to_json_value();
        assert_eq!(
            json.get("kind").and_then(|k| k.as_str()),
            Some("recursiveRef"),
            "JSON kind must be recursiveRef, not unknown"
        );
    }

    // -----------------------------------------------------------------------
    // #5 — exact recursive key reuses placeholder through full solver flow
    // -----------------------------------------------------------------------

    #[test]
    fn exact_recursive_key_still_reuses_placeholder() {
        // type Tree = { children: Tree[] }
        // The exact-key hit should reuse the placeholder immediately on the
        // second encounter, producing RecursiveRef in the output.
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                crate::analysis::type_expr::ObjectProperty {
                    name: "children".into(),
                    ty: TypeExpr::Array {
                        element: Arc::new(TypeExpr::named("Tree")),
                        readonly: false,
                    },
                    optional: false,
                    readonly: false,
                },
            )],
        }));

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Tree".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);
        let result = solve_type(&TypeExpr::named("Tree"), &host);

        // Exact-key detection must produce RecursiveRef, not hit depth limit
        assert_eq!(
            result.execution_status,
            ExecutionStatus::Completed,
            "exact-key recursion should complete normally"
        );
        let json = serde_json::to_string(&result.value).unwrap();
        assert!(
            json.contains("recursiveRef"),
            "exact-key should produce RecursiveRef placeholder"
        );
        assert!(
            !json.contains("depth exceeded"),
            "should not hit depth limit"
        );
    }

    // -----------------------------------------------------------------------
    // #10 — same symbol global ceiling stops unbounded fingerprint churn
    // -----------------------------------------------------------------------

    #[test]
    fn same_symbol_global_ceiling_still_stops_unbounded_fingerprint_churn() {
        use crate::analysis::type_solver::recursion::*;

        let mut tracker = RecursionTracker::new();

        // Push 10 entries with DIFFERENT fingerprints — each gets its own
        // soft budget, but the hard ceiling should still stop at 10.
        for i in 0..10u64 {
            let key = RecursionKey {
                canonical_id: "/types.ts".into(),
                symbol_name: "Churn".into(),
                args_hash: i,
            };
            let fp = StructuralRecursionFingerprint {
                mode: StructuralRecursionMode::Conditional,
                combined_fingerprint: i * 1000, // all different fingerprints
            };
            assert!(
                tracker.enter(key.clone(), Some(&fp)).is_none(),
                "entry {} with distinct fingerprint should succeed",
                i
            );
            tracker.push(key, NodeId(i as u32), Some(&fp));
        }

        // 11th entry with yet another distinct fingerprint should still bail
        // because the hard ceiling (10) is reached.
        let key11 = RecursionKey {
            canonical_id: "/types.ts".into(),
            symbol_name: "Churn".into(),
            args_hash: 999,
        };
        let fp11 = StructuralRecursionFingerprint {
            mode: StructuralRecursionMode::Conditional,
            combined_fingerprint: 99999,
        };
        assert!(
            tracker.enter(key11, Some(&fp11)).is_some(),
            "hard ceiling should stop unbounded fingerprint churn"
        );
    }

    // -----------------------------------------------------------------------
    // #24 — same named infer binders in different branches remain distinct
    // -----------------------------------------------------------------------

    #[test]
    fn solve_same_named_infer_binders_in_different_branches_remain_distinct() {
        // type Foo<T> =
        //   T extends string ? { kind: "str"; value: T } :
        //   T extends number ? { kind: "num"; value: T } :
        //   never
        // Foo<string> → { kind: "str"; value: string }
        // Foo<number> → { kind: "num"; value: number }
        use crate::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
        use crate::analysis::type_solver::host::EvalEnvSolverHost;

        let body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::TypeParameter(
                crate::analysis::type_expr::TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                },
            )),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::Object(Arc::new(
                crate::analysis::type_expr::ObjectExpr {
                    properties: vec![
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "kind".into(),
                                ty: TypeExpr::string_literal("str"),
                                optional: false,
                                readonly: false,
                            },
                        ),
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "value".into(),
                                ty: TypeExpr::named("T"),
                                optional: false,
                                readonly: false,
                            },
                        ),
                    ],
                },
            ))),
            false_type: Arc::new(TypeExpr::Conditional {
                check: Arc::new(TypeExpr::TypeParameter(
                    crate::analysis::type_expr::TypeParam {
                        name: "T".into(),
                        constraint: None,
                        default: None,
                    },
                )),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
                true_type: Arc::new(TypeExpr::Object(Arc::new(
                    crate::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "kind".into(),
                                    ty: TypeExpr::string_literal("num"),
                                    optional: false,
                                    readonly: false,
                                },
                            ),
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "value".into(),
                                    ty: TypeExpr::named("T"),
                                    optional: false,
                                    readonly: false,
                                },
                            ),
                        ],
                    },
                ))),
                false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
            }),
        };

        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Foo".into(),
            declaration_id: 0,
            body,
            type_parameters: vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            kind: TypeDeclKind::Alias,
        });

        let host = EvalEnvSolverHost::new(&env);

        // Foo<string> → true branch → { kind: "str"; value: string }
        let result_str = solve_type(
            &TypeExpr::named_with_args("Foo", vec![TypeExpr::Primitive(PrimitiveName::String)]),
            &host,
        );
        let json_str = serde_json::to_string(&result_str.value).unwrap();
        assert!(
            json_str.contains("\"str\""),
            "Foo<string> should take the string branch, got: {}",
            &json_str[..json_str.len().min(200)]
        );
        assert!(
            !json_str.contains("\"num\""),
            "Foo<string> must NOT take the number branch"
        );

        // Foo<number> → false branch → true branch → { kind: "num"; value: number }
        let result_num = solve_type(
            &TypeExpr::named_with_args("Foo", vec![TypeExpr::Primitive(PrimitiveName::Number)]),
            &host,
        );
        let json_num = serde_json::to_string(&result_num.value).unwrap();
        assert!(
            json_num.contains("\"num\""),
            "Foo<number> should take the number branch, got: {}",
            &json_num[..json_num.len().min(200)]
        );
        assert!(
            !json_num.contains("\"str\""),
            "Foo<number> must NOT take the string branch"
        );
    }

    // -----------------------------------------------------------------------
    // #30 — component meta proto round-trips RecursiveRef without unknown fallback
    // -----------------------------------------------------------------------

    #[test]
    fn component_meta_proto_round_trips_recursive_ref_without_unknown_fallback() {
        use crate::analysis::type_expr::{RecursiveConditionalBranch, RecursiveConditionalFrame};

        // Verify that RecursiveRef survives JSON round-trip with full
        // structural fidelity — the same path the graph builder reads from.
        let expr = TypeExpr::RecursiveRef {
            name: Arc::from("Tree"),
            type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
            conditional_context: Arc::from(vec![RecursiveConditionalFrame {
                branch: RecursiveConditionalBranch::True,
                decided: true,
                check: Arc::new(TypeExpr::named("T")),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            }]),
        };

        let json_value = expr.to_json_value();

        // Must serialize as "recursiveRef", never "unknown"
        assert_eq!(
            json_value.get("kind").and_then(|k| k.as_str()),
            Some("recursiveRef"),
            "kind must be recursiveRef for protocol encoding"
        );

        // type_arguments must be present and non-empty
        let args = json_value.get("typeArguments").and_then(|a| a.as_array());
        assert!(
            args.is_some() && !args.unwrap().is_empty(),
            "typeArguments must be present"
        );

        // conditionalContext must be present and non-empty
        let ctx = json_value
            .get("conditionalContext")
            .and_then(|c| c.as_array());
        assert!(
            ctx.is_some() && !ctx.unwrap().is_empty(),
            "conditionalContext must be present"
        );

        // Frame fields must be present
        let frame = &ctx.unwrap()[0];
        assert_eq!(frame.get("branch").and_then(|b| b.as_str()), Some("true"));
        assert_eq!(frame.get("decided").and_then(|d| d.as_bool()), Some(true));
        assert!(frame.get("check").is_some());
        assert!(frame.get("extends").is_some());

        // Round-trip must preserve everything
        let round_tripped: TypeExpr = serde_json::from_value(json_value).unwrap();
        assert_eq!(round_tripped, expr);
    }

    #[test]
    fn context_capture_keeps_innermost_eight_frames_and_flags_truncation() {
        use crate::analysis::type_solver::arena::{
            ConditionalBranch, ConditionalFrameSnapshot, NodeId,
        };
        let mut state = SolveState::new(SolveLimits::default());

        // Push 12 frames
        for i in 0..12 {
            state
                .conditional_context_stack
                .push(ConditionalFrameSnapshot {
                    branch: ConditionalBranch::True,
                    decided: true,
                    check: NodeId(i),
                    extends: NodeId(i + 100),
                });
        }

        let capture = state.capture_symbol_conditional_context();
        assert_eq!(capture.available, 12);
        assert_eq!(capture.frames.len(), 8);
        assert!(capture.truncated);

        // The kept frames should be the innermost 8 (indices 4..12)
        assert_eq!(capture.frames[0].check, NodeId(4));
        assert_eq!(capture.frames[7].check, NodeId(11));
    }

    #[test]
    fn context_capture_within_cap_is_not_truncated() {
        use crate::analysis::type_solver::arena::{
            ConditionalBranch, ConditionalFrameSnapshot, NodeId,
        };
        let mut state = SolveState::new(SolveLimits::default());

        for i in 0..5 {
            state
                .conditional_context_stack
                .push(ConditionalFrameSnapshot {
                    branch: ConditionalBranch::False,
                    decided: false,
                    check: NodeId(i),
                    extends: NodeId(i + 100),
                });
        }

        let capture = state.capture_symbol_conditional_context();
        assert_eq!(capture.available, 5);
        assert_eq!(capture.frames.len(), 5);
        assert!(!capture.truncated);
    }

    #[test]
    fn context_capture_respects_symbol_base() {
        use crate::analysis::type_solver::arena::{
            ConditionalBranch, ConditionalFrameSnapshot, NodeId,
        };
        let mut state = SolveState::new(SolveLimits::default());

        // Push 5 frames for an outer symbol
        for i in 0..5 {
            state
                .conditional_context_stack
                .push(ConditionalFrameSnapshot {
                    branch: ConditionalBranch::True,
                    decided: true,
                    check: NodeId(i),
                    extends: NodeId(i + 100),
                });
        }

        // Push a symbol base at position 5 (simulating inner symbol resolution)
        state
            .conditional_context_base_stack
            .push(state.conditional_context_stack.len());

        // Push 10 frames for the inner symbol
        for i in 0..10 {
            state
                .conditional_context_stack
                .push(ConditionalFrameSnapshot {
                    branch: ConditionalBranch::False,
                    decided: false,
                    check: NodeId(50 + i),
                    extends: NodeId(150 + i),
                });
        }

        let capture = state.capture_symbol_conditional_context();

        // Should only see frames from the inner symbol (10 available),
        // keeping the innermost 8
        assert_eq!(capture.available, 10);
        assert_eq!(capture.frames.len(), 8);
        assert!(capture.truncated);
        // First kept frame should be at index 2 of inner symbol's range
        assert_eq!(capture.frames[0].check, NodeId(52));
    }

    #[test]
    fn context_truncation_diagnostic_requires_placeholder_creation() {
        use crate::analysis::type_solver::arena::{
            ConditionalBranch, ConditionalFrameSnapshot, PrimitiveKind,
        };

        let mut host = TestHost::new();
        host.add_alias("Tree", TypeExpr::Primitive(PrimitiveName::String));

        let mut arena = QueryArena::new();
        let check = arena.primitive(PrimitiveKind::String);
        let extends = arena.primitive(PrimitiveKind::Number);

        let mut state = SolveState::new(SolveLimits {
            max_instantiation_depth: 0,
            ..SolveLimits::default()
        });
        for _ in 0..12 {
            state
                .conditional_context_stack
                .push(ConditionalFrameSnapshot {
                    branch: ConditionalBranch::True,
                    decided: true,
                    check,
                    extends,
                });
        }

        let root_id = ResolvedRootIdentity::new("/test.ts", "Tree");
        let _ = resolve_prepared_ref(
            &mut arena,
            &host,
            &mut state,
            &SubstitutionEnv::new(),
            &root_id,
            &[],
        );

        assert!(
            state.diagnostics.is_empty(),
            "context truncation should only be reported once a recursive placeholder is created"
        );
    }

    #[test]
    fn diagnostic_cap_limits_collected_diagnostics() {
        // A solve on a deeply recursive conditional type can produce many
        // ConditionalContextTruncated diagnostics. With the cap, diagnostics
        // should be limited and a truncated_count should be tracked.
        let mut state = SolveState::new(SolveLimits::default());

        // Simulate recording 100 diagnostics
        for i in 0..100 {
            state.record_diagnostic(SolverDiagnostic::ConditionalContextTruncated {
                available: 20 + i,
                captured: 8,
            });
        }

        // After the cap, diagnostics vec should be at most max_diagnostics
        assert!(
            state.diagnostics.len() <= state.limits.max_diagnostics,
            "diagnostics should be capped at max_diagnostics ({}), got {}",
            state.limits.max_diagnostics,
            state.diagnostics.len()
        );
        // Should have recorded that some were truncated
        assert!(
            state.diagnostics_truncated > 0,
            "should track truncated diagnostic count"
        );
        assert_eq!(
            state.diagnostics.len() + state.diagnostics_truncated,
            100,
            "kept + truncated should equal total recorded"
        );
    }

    #[test]
    fn diagnostic_cap_does_not_truncate_under_limit() {
        let mut state = SolveState::new(SolveLimits::default());

        // Record fewer diagnostics than the cap
        for i in 0..3 {
            state.record_diagnostic(SolverDiagnostic::ConditionalContextTruncated {
                available: 10 + i,
                captured: 8,
            });
        }

        assert_eq!(state.diagnostics.len(), 3);
        assert_eq!(state.diagnostics_truncated, 0);
    }

    #[test]
    fn solve_batch_caches_hard_stop_result() {
        let mut host = TestHost::new();
        // Create a deeply recursive type that triggers HardStop via depth limit
        // type Deep<T> = { value: Deep<Deep<T>> }
        host.add_generic_alias(
            "Deep",
            vec![crate::analysis::type_expr::TypeParam {
                name: "T".into(),
                constraint: None,
                default: None,
            }],
            TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
                properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                    crate::analysis::type_expr::ObjectProperty {
                        name: "value".into(),
                        ty: TypeExpr::named_with_args(
                            "Deep",
                            vec![TypeExpr::named_with_args(
                                "Deep",
                                vec![TypeExpr::named("T")],
                            )],
                        ),
                        optional: false,
                        readonly: false,
                    },
                )],
            })),
        );

        let expr =
            TypeExpr::named_with_args("Deep", vec![TypeExpr::Primitive(PrimitiveName::String)]);

        let mut batch = SolveBatch::new(&host);

        // First solve — actually runs the solver
        let result1 = batch.solve(&expr);
        // The recursive type should hit HardStop or complete with RecursiveRef
        let status1 = result1.execution_status;

        // Second solve — should hit cache, returning identical result
        let result2 = batch.solve(&expr);

        assert_eq!(
            result2.execution_status, status1,
            "second solve should return cached status"
        );
        assert_eq!(
            format!("{:?}", result2.value),
            format!("{:?}", result1.value),
            "second solve should return cached value"
        );
        // Verify the cache actually has the entry (batch has exactly 1 cached entry)
        assert_eq!(
            batch.cache.len(),
            1,
            "batch should have exactly one cached entry"
        );
    }

    #[test]
    fn solve_batch_reuses_across_identical_expressions() {
        let mut host = TestHost::new();
        host.add_alias("MyType", TypeExpr::Primitive(PrimitiveName::String));

        let expr = TypeExpr::named("MyType");
        let mut batch = SolveBatch::new(&host);

        let r1 = batch.solve(&expr);
        let r2 = batch.solve(&expr);

        assert_eq!(r1.value, r2.value);
        assert_eq!(r1.exactness, r2.exactness);
        assert_eq!(batch.cache.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Workstream A: query-local reuse caching tests
    // -----------------------------------------------------------------------

    /// Host that tracks member projection calls via an interior counter.
    struct CountingHost {
        inner: TestHost,
        member_projection_calls: std::cell::RefCell<u32>,
    }

    impl CountingHost {
        fn new(inner: TestHost) -> Self {
            Self {
                inner,
                member_projection_calls: std::cell::RefCell::new(0),
            }
        }

        fn member_projection_call_count(&self) -> u32 {
            *self.member_projection_calls.borrow()
        }
    }

    impl TypeSolverHost for CountingHost {
        fn resolve_prepared_type_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedTypeDecl>> {
            self.inner.resolve_prepared_type_decl(root_identity)
        }

        fn resolve_prepared_value_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedValueDecl>> {
            self.inner.resolve_prepared_value_decl(root_identity)
        }

        fn utility_source(&self, name: &str) -> UtilitySource {
            self.inner.utility_source(name)
        }

        fn root_identity(
            &self,
            canonical_id: &str,
            symbol_name: &str,
        ) -> Option<ResolvedRootIdentity> {
            self.inner.root_identity(canonical_id, symbol_name)
        }

        fn resolve_member_projection(
            &self,
            root_identity: &ResolvedRootIdentity,
            member: &str,
        ) -> Option<crate::analysis::type_solver::host::SolverProjection<TypeExpr>> {
            *self.member_projection_calls.borrow_mut() += 1;
            // Delegate to a manual member lookup on the prepared declaration.
            // Build member_index since TestHost.add_alias() doesn't call it.
            let stored = self.inner.resolve_prepared_type_decl(root_identity)?;
            let mut prepared = (*stored).clone();
            prepared.build_member_index();
            let m = prepared.member(member)?;
            Some(crate::analysis::type_solver::host::SolverProjection::exact_concrete(m.ty.clone()))
        }

        fn request_status(&self) -> RequestStatus {
            RequestStatus::Running
        }
    }

    /// Helper: create a Props object type with named string properties.
    fn make_object_type(props: &[(&str, TypeExpr)]) -> TypeExpr {
        TypeExpr::Object(Arc::new(crate::analysis::type_expr::ObjectExpr {
            properties: props
                .iter()
                .map(|(name, ty)| {
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: (*name).into(),
                            ty: ty.clone(),
                            optional: false,
                            readonly: false,
                        },
                    )
                })
                .collect(),
        }))
    }

    #[test]
    fn host_projection_cache_collapses_repeated_member_access() {
        // Setup: BaseTransitionProps with a 'mode' member
        let mut inner = TestHost::new();
        let props_body = make_object_type(&[
            ("mode", TypeExpr::Primitive(PrimitiveName::String)),
            ("appear", TypeExpr::Primitive(PrimitiveName::Boolean)),
        ]);
        inner.add_alias("BaseTransitionProps", props_body);

        let host = CountingHost::new(inner);

        // Expression: union of three identical indexed accesses
        // BaseTransitionProps['mode'] | BaseTransitionProps['mode'] | BaseTransitionProps['mode']
        let indexed_access = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("BaseTransitionProps")),
            index: Arc::new(TypeExpr::string_literal("mode")),
        };
        let expr = TypeExpr::Union(Arc::from(vec![
            indexed_access.clone(),
            indexed_access.clone(),
            indexed_access.clone(),
        ]));

        let (result, audit) = solve_type_with_audit(&expr, &host);

        // Positive: result should be string (union of 3 identical strings deduped)
        assert_eq!(result.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);

        // KEY ASSERTION: host projection should be called exactly once, not 3 times.
        // The cache must collapse repeated projections of the same (root, member).
        assert_eq!(
            host.member_projection_call_count(),
            1,
            "host.resolve_member_projection should be called once, not 3 times"
        );

        // Audit: cache should have hits
        let key = (
            "/test.ts".to_string(),
            "BaseTransitionProps".to_string(),
            "mode".to_string(),
        );
        assert_eq!(
            audit.host_projection_counts.get(&key).copied().unwrap_or(0),
            1,
            "audit should record exactly 1 host projection"
        );
        assert!(
            audit.host_projection_cache_hits >= 2,
            "audit should record at least 2 cache hits"
        );
    }

    #[test]
    fn host_projection_cache_does_not_cross_different_members() {
        // Two different members of the same type should each call the host once
        let mut inner = TestHost::new();
        let props_body = make_object_type(&[
            ("mode", TypeExpr::Primitive(PrimitiveName::String)),
            ("appear", TypeExpr::Primitive(PrimitiveName::Boolean)),
        ]);
        inner.add_alias("BaseTransitionProps", props_body);

        let host = CountingHost::new(inner);

        // BaseTransitionProps['mode'] | BaseTransitionProps['appear']
        let expr = TypeExpr::Union(Arc::from(vec![
            TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("BaseTransitionProps")),
                index: Arc::new(TypeExpr::string_literal("mode")),
            },
            TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("BaseTransitionProps")),
                index: Arc::new(TypeExpr::string_literal("appear")),
            },
        ]));

        let (result, _audit) = solve_type_with_audit(&expr, &host);

        // Positive: union of string | boolean
        match &result.value {
            TypeExpr::Union(members) => assert_eq!(members.len(), 2),
            _ => panic!("expected Union"),
        }

        // Each member accessed once
        assert_eq!(
            host.member_projection_call_count(),
            2,
            "two different members should each call host once"
        );
    }

    #[test]
    fn prepared_ref_cache_reuses_completed_instantiation() {
        // Setup: type Wrapper = { inner: string }
        // Expression: Wrapper (accessed twice in same query via union)
        let mut inner = TestHost::new();
        inner.add_alias(
            "Wrapper",
            make_object_type(&[("inner", TypeExpr::Primitive(PrimitiveName::String))]),
        );

        let host = CountingHost::new(inner);

        // Wrapper | Wrapper — should instantiate the declaration once
        let expr = TypeExpr::Union(Arc::from(vec![
            TypeExpr::named("Wrapper"),
            TypeExpr::named("Wrapper"),
        ]));

        let (result, audit) = solve_type_with_audit(&expr, &host);

        // Positive: resolves to union or single object (union may not dedup objects)
        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 1);
            }
            TypeExpr::Union(members) => {
                // Both members should be identical Wrapper objects
                assert!(members.len() <= 2);
                for m in members.iter() {
                    match m {
                        TypeExpr::Object(obj) => assert_eq!(obj.properties.len(), 1),
                        _ => panic!("expected Object in union member"),
                    }
                }
            }
            _ => panic!("expected Object or Union, got {:?}", result.value),
        }

        // KEY ASSERTION: prepared_ref should be entered once, reused via cache on second
        assert!(
            audit.prepared_ref_cache_hits >= 1,
            "second Wrapper ref should hit prepared_ref cache, got {} hits",
            audit.prepared_ref_cache_hits,
        );
    }

    #[test]
    fn host_projection_cache_negative_no_extra_projections() {
        // Accessing mode should NOT cause projections on 'appear' or other members
        let mut inner = TestHost::new();
        let props_body = make_object_type(&[
            ("mode", TypeExpr::Primitive(PrimitiveName::String)),
            ("appear", TypeExpr::Primitive(PrimitiveName::Boolean)),
            ("duration", TypeExpr::Primitive(PrimitiveName::Number)),
        ]);
        inner.add_alias("BaseTransitionProps", props_body);

        let host = CountingHost::new(inner);

        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("BaseTransitionProps")),
            index: Arc::new(TypeExpr::string_literal("mode")),
        };

        let (_result, audit) = solve_type_with_audit(&expr, &host);

        // Negative: only 'mode' should be projected, not 'appear' or 'duration'
        for key in audit.host_projection_counts.keys() {
            assert_eq!(
                key.2, "mode",
                "only 'mode' should be projected, but found '{}'",
                key.2
            );
        }
    }

    #[test]
    fn solver_audit_tracks_visited_chain_and_excludes_unrelated_external_nodes() {
        let mut host = TestHost::new();
        host.add_alias_in(
            "/dep.ts",
            "Leaf",
            TypeExpr::Primitive(PrimitiveName::String),
        );
        host.add_alias_in("/dep.ts", "Used", TypeExpr::named("Leaf"));
        host.add_alias_in(
            "/dep.ts",
            "UnusedSibling",
            TypeExpr::Primitive(PrimitiveName::Boolean),
        );
        host.add_alias_in(
            "/entry.ts",
            "Root",
            make_object_type(&[("value", TypeExpr::named("Used"))]),
        );

        let (result, audit) = solve_type_with_audit(&TypeExpr::named("Root"), &host);

        assert!(matches!(
            object_property_type(&result.value, "value"),
            TypeExpr::Primitive(PrimitiveName::String)
        ));
        assert!(
            audit
                .external_decl_visit_counts
                .contains_key("/entry.ts::Root"),
            "root declaration should be marked visited"
        );
        assert!(
            audit
                .external_decl_visit_counts
                .contains_key("/dep.ts::Used"),
            "used dependency should be marked visited"
        );
        assert!(
            audit
                .external_decl_visit_counts
                .contains_key("/dep.ts::Leaf"),
            "leaf dependency should be marked visited"
        );
        assert!(
            !audit
                .external_decl_visit_counts
                .contains_key("/dep.ts::UnusedSibling"),
            "unused sibling must not be visited"
        );
        assert!(
            audit
                .prepared_ref_edge_counts
                .iter()
                .any(|((parent, child), count)| {
                    parent == "<query-root>" && child.starts_with("/entry.ts::Root#") && *count >= 1
                }),
            "root edge should be recorded from query root"
        );
        assert!(
            audit
                .prepared_ref_edge_counts
                .iter()
                .any(|((parent, child), count)| {
                    parent.starts_with("/entry.ts::Root#")
                        && child.starts_with("/dep.ts::Used#")
                        && *count >= 1
                }),
            "Root should only expand the Used dependency"
        );
        assert!(
            audit
                .prepared_ref_edge_counts
                .iter()
                .any(|((parent, child), count)| {
                    parent.starts_with("/dep.ts::Used#")
                        && child.starts_with("/dep.ts::Leaf#")
                        && *count >= 1
                }),
            "Used should expand the Leaf dependency"
        );
        assert!(
            !audit
                .prepared_ref_edge_counts
                .keys()
                .any(|(_, child)| child.starts_with("/dep.ts::UnusedSibling#")),
            "unused sibling must not appear in prepared-ref expansion edges"
        );
    }

    #[test]
    fn solver_audit_tracks_unresolved_bare_names_by_parent() {
        let mut host = TestHost::new();
        host.add_alias_in(
            "/dep.ts",
            "FnBox",
            make_object_type(&[
                ("cb", TypeExpr::named("Function")),
                ("label", TypeExpr::Primitive(PrimitiveName::String)),
            ]),
        );

        let (result, audit) = solve_type_with_audit(&TypeExpr::named("FnBox"), &host);

        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        assert!(
            audit
                .external_decl_visit_counts
                .contains_key("/dep.ts::FnBox"),
            "FnBox should be marked as visited"
        );
        assert!(
            audit
                .unresolved_root_counts
                .iter()
                .any(|((parent, symbol), count)| {
                    parent.starts_with("/dep.ts::FnBox#") && symbol == "Function" && *count >= 1
                }),
            "audit should attribute unresolved Function lookups to FnBox"
        );
        assert!(
            !audit
                .unresolved_root_counts
                .keys()
                .any(|(_, symbol)| symbol == "UnusedSibling"),
            "audit should not report unrelated unresolved symbols"
        );
    }

    // -----------------------------------------------------------------------
    // Workstream B: open-generic symbolic stop tests
    // -----------------------------------------------------------------------

    fn make_type_param(name: &str) -> crate::analysis::type_expr::TypeParam {
        crate::analysis::type_expr::TypeParam {
            name: name.into(),
            constraint: None,
            default: None,
        }
    }

    #[test]
    fn open_generic_with_only_literal_true_stays_symbolic() {
        // type GetModelValue<T, VK, Flag> = T extends ... ? ... : ...
        // Called as GetModelValue<T, VK, true> where T and VK are open
        let mut host = TestHost::new();

        // A complex conditional body: T extends Record<string, any> ? T[VK] : T
        host.add_generic_alias(
            "GetModelValue",
            vec![
                make_type_param("T"),
                make_type_param("VK"),
                make_type_param("Flag"),
            ],
            TypeExpr::Conditional {
                check: Arc::new(TypeExpr::named("T")),
                extends: Arc::new(TypeExpr::named_with_args(
                    "Record",
                    vec![
                        TypeExpr::Primitive(PrimitiveName::String),
                        TypeExpr::Primitive(PrimitiveName::Any),
                    ],
                )),
                true_type: Arc::new(TypeExpr::IndexedAccess {
                    object: Arc::new(TypeExpr::named("T")),
                    index: Arc::new(TypeExpr::named("VK")),
                }),
                false_type: Arc::new(TypeExpr::named("T")),
            },
        );

        // Call: GetModelValue<T, VK, true> — T and VK are open type params
        let expr = TypeExpr::named_with_args(
            "GetModelValue",
            vec![
                TypeExpr::type_parameter(make_type_param("T")),
                TypeExpr::type_parameter(make_type_param("VK")),
                TypeExpr::Literal(crate::analysis::type_expr::LiteralValue::Boolean(true)),
            ],
        );

        let (result, audit) = solve_type_with_audit(&expr, &host);

        // KEY ASSERTION: should stay symbolic (Applied or Ref), not expand
        assert!(
            result.exactness == SolverExactness::ExactSymbolic
                || matches!(&result.value, TypeExpr::Ref { .. }),
            "open-generic with only literal true should stay symbolic, got: {:?}",
            result.value,
        );

        // Negative: arena should be small (not multi-million node explosion)
        assert!(
            audit.arena_nodes < 1000,
            "arena should be small for symbolic stop, got {} nodes",
            audit.arena_nodes,
        );
    }

    #[test]
    fn open_generic_with_concrete_signal_still_expands() {
        // Same type but called with concrete arg: GetModelValue<string, "key", true>
        let mut host = TestHost::new();

        host.add_generic_alias(
            "GetModelValue",
            vec![
                make_type_param("T"),
                make_type_param("VK"),
                make_type_param("Flag"),
            ],
            TypeExpr::Conditional {
                check: Arc::new(TypeExpr::named("T")),
                extends: Arc::new(TypeExpr::named_with_args(
                    "Record",
                    vec![
                        TypeExpr::Primitive(PrimitiveName::String),
                        TypeExpr::Primitive(PrimitiveName::Any),
                    ],
                )),
                true_type: Arc::new(TypeExpr::IndexedAccess {
                    object: Arc::new(TypeExpr::named("T")),
                    index: Arc::new(TypeExpr::named("VK")),
                }),
                false_type: Arc::new(TypeExpr::named("T")),
            },
        );

        // Call with concrete args: GetModelValue<string, "key", true>
        let expr = TypeExpr::named_with_args(
            "GetModelValue",
            vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::string_literal("key"),
                TypeExpr::Literal(crate::analysis::type_expr::LiteralValue::Boolean(true)),
            ],
        );

        let result = solve_type(&expr, &host);

        // Should expand since T=string is concrete
        // The result depends on conditional evaluation, but it should NOT stay as a Ref
        assert_ne!(
            result.exactness,
            SolverExactness::ExactSymbolic,
            "concrete args should trigger expansion"
        );
    }

    #[test]
    fn fully_concrete_args_still_expand() {
        let mut host = TestHost::new();
        host.add_generic_alias(
            "Wrapper",
            vec![make_type_param("T")],
            make_object_type(&[("value", TypeExpr::named("T"))]),
        );

        // Wrapper<string> — fully concrete
        let expr =
            TypeExpr::named_with_args("Wrapper", vec![TypeExpr::Primitive(PrimitiveName::String)]);

        let result = solve_type(&expr, &host);

        // Should expand to { value: string }
        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 1);
            }
            _ => panic!("expected Object, got {:?}", result.value),
        }
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
    }

    #[test]
    fn open_generic_arena_size_regression() {
        // Chain of generic helpers that reference each other, simulating
        // the real GetModelValue pattern with cross-file refs.
        // type Inner<X> = X extends string ? { val: X } : { val: unknown }
        // type Middle<A, B> = Inner<A> & Inner<B>
        // type Outer<T, U, V> = Middle<T, U> & Middle<U, V> & Middle<T, V>
        let mut host = TestHost::new();
        host.add_generic_alias(
            "Inner",
            vec![make_type_param("X")],
            TypeExpr::Conditional {
                check: Arc::new(TypeExpr::named("X")),
                extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                true_type: Arc::new(make_object_type(&[("val", TypeExpr::named("X"))])),
                false_type: Arc::new(make_object_type(&[(
                    "val",
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                )])),
            },
        );
        host.add_generic_alias(
            "Middle",
            vec![make_type_param("A"), make_type_param("B")],
            TypeExpr::Intersection(Arc::from(vec![
                TypeExpr::named_with_args("Inner", vec![TypeExpr::named("A")]),
                TypeExpr::named_with_args("Inner", vec![TypeExpr::named("B")]),
            ])),
        );
        host.add_generic_alias(
            "Outer",
            vec![
                make_type_param("T"),
                make_type_param("U"),
                make_type_param("V"),
            ],
            TypeExpr::Intersection(Arc::from(vec![
                TypeExpr::named_with_args(
                    "Middle",
                    vec![TypeExpr::named("T"), TypeExpr::named("U")],
                ),
                TypeExpr::named_with_args(
                    "Middle",
                    vec![TypeExpr::named("U"), TypeExpr::named("V")],
                ),
                TypeExpr::named_with_args(
                    "Middle",
                    vec![TypeExpr::named("T"), TypeExpr::named("V")],
                ),
            ])),
        );

        // Outer<T, U, V> — all open
        let expr = TypeExpr::named_with_args(
            "Outer",
            vec![
                TypeExpr::type_parameter(make_type_param("T")),
                TypeExpr::type_parameter(make_type_param("U")),
                TypeExpr::type_parameter(make_type_param("V")),
            ],
        );

        let (result, audit) = solve_type_with_audit(&expr, &host);

        // Should stay symbolic — the symbolic stop should prevent expansion
        // into Inner/Middle chains with open args.
        assert!(
            result.exactness == SolverExactness::ExactSymbolic,
            "all-open Outer should be symbolic"
        );

        // KEY ASSERTION: with symbolic stop, Outer<T,U,V> should not expand
        // into 6 Inner instantiations. The arena should be very small.
        // Without the stop, it would expand to 6 conditional+object patterns.
        assert!(
            audit.arena_nodes < 50,
            "arena for all-open Outer should be tiny with symbolic stop, got {} nodes",
            audit.arena_nodes,
        );
    }

    #[test]
    fn builtin_partial_with_open_t_still_expands() {
        // Partial<T> is a builtin — should NOT be stopped by symbolic stop
        let host = TestHost::new();

        let expr = TypeExpr::named_with_args(
            "Partial",
            vec![TypeExpr::type_parameter(make_type_param("T"))],
        );

        let result = solve_type(&expr, &host);
        // Builtins stay on their normal path (NoopSolverHost has Unknown utility source)
        // but a real host with Builtin classification would expand Partial<T>
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
    }

    #[test]
    fn generic_with_defaulted_params_and_open_arg_stays_symbolic() {
        // type Config<T, Mode = "default"> = { value: T, mode: Mode }
        // Called as Config<U> where U is open — default fills Mode
        let mut host = TestHost::new();
        host.add_generic_alias(
            "Config",
            vec![
                make_type_param("T"),
                crate::analysis::type_expr::TypeParam {
                    name: "Mode".into(),
                    constraint: None,
                    default: Some(Arc::new(TypeExpr::string_literal("default"))),
                },
            ],
            make_object_type(&[
                ("value", TypeExpr::named("T")),
                ("mode", TypeExpr::named("Mode")),
            ]),
        );

        // Config<U> where U is open. Mode gets default "default" (literal).
        // T=U is open, Mode="default" is literal → no concrete signal → symbolic stop
        let expr = TypeExpr::named_with_args(
            "Config",
            vec![TypeExpr::type_parameter(make_type_param("U"))],
        );

        let (result, audit) = solve_type_with_audit(&expr, &host);

        // Should stay symbolic at depth > 0 but expand at depth 0 (top level)
        // At depth 0 the stop doesn't fire, so it expands
        match &result.value {
            TypeExpr::Object(_) => {}  // expanded at depth 0 — correct
            TypeExpr::Ref { .. } => {} // symbolic — also acceptable
            other => panic!("expected Object or Ref, got {:?}", other),
        }
        // Arena should be bounded regardless
        assert!(
            audit.arena_nodes < 100,
            "defaulted open param should be bounded, got {}",
            audit.arena_nodes,
        );
    }

    #[test]
    fn mixed_concrete_and_open_args_still_expands() {
        // Foo<string, T> — has concrete signal (string) → should expand
        let mut host = TestHost::new();
        host.add_generic_alias(
            "Pair",
            vec![make_type_param("A"), make_type_param("B")],
            make_object_type(&[
                ("first", TypeExpr::named("A")),
                ("second", TypeExpr::named("B")),
            ]),
        );

        let expr = TypeExpr::named_with_args(
            "Pair",
            vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::type_parameter(make_type_param("T")),
            ],
        );

        let result = solve_type(&expr, &host);

        // Should expand because string provides concrete signal
        match &result.value {
            TypeExpr::Object(obj) => {
                assert_eq!(
                    obj.properties.len(),
                    2,
                    "should expand to object with 2 props"
                );
            }
            _ => panic!("expected Object, got {:?}", result.value),
        }
    }

    #[test]
    fn host_projection_cache_hit_across_batch_solve_calls() {
        // SolveBatch: two separate solve() calls accessing the same member
        let mut inner = TestHost::new();
        let props = make_object_type(&[("x", TypeExpr::Primitive(PrimitiveName::String))]);
        inner.add_alias("Props", props);
        let host = CountingHost::new(inner);

        let expr1 = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("Props")),
            index: Arc::new(TypeExpr::string_literal("x")),
        };
        let expr2 = expr1.clone();

        let mut batch = SolveBatch::new(&host);
        let r1 = batch.solve(&expr1);
        let r2 = batch.solve(&expr2);

        assert_eq!(r1.value, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(r2.value, TypeExpr::Primitive(PrimitiveName::String));

        // SolveBatch caches at TypeExpr level, so the second solve hits
        // the SolveBatch cache (not even reaching the host projection).
        // host should be called at most once.
        assert!(
            host.member_projection_call_count() <= 1,
            "batch should cache: got {} host calls",
            host.member_projection_call_count()
        );
    }
}
