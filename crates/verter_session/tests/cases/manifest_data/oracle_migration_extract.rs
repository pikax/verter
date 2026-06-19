//! The closed `syn` migration-fidelity extractor + canonical fingerprint
//! (`docs/arch/u0-oracle-harness-design.md` §Q4 — `migration_fingerprint` /
//! `original_body_tokens`).
//!
//! This is the SOLE migration-fidelity authority for a lifted TS7
//! `TypeExpr`-projection row: it reads the ORIGINAL `#[test]` body (before the
//! `#[oracle_row]` replacement) and recovers the QUERY-IDENTITY fidelity tuple
//! the row authored — independent of the registry. The retained
//! `migration_fingerprint` (computed here at lift time) is what
//! `registry_payload_matches_migration_fingerprint` validates the registry
//! payload AGAINST, so a self-consistent-but-WRONG `(spec ∧ snapshot)` pair (the
//! registry + snapshot both query a different symbol/mode/type-args than the
//! original body) FAILS.
//!
//! CLOSED + STATIC by construction (`migration_fingerprint_extraction_is_static`):
//! every query argument must const-fold to a literal / closed enum path; a body
//! whose query is hidden behind a non-modeled wrapper, computes an argument
//! through a call / loop / closure, or carries an unmodeled obligation assertion
//! REJECTS (`ExtractError`) rather than approximating — the row then stays
//! without provenance (`migration: None`), NEVER an auto-lifted guessed
//! fingerprint.
//!
//! Scope of the v1 fingerprint (`verter.oracle.migration_fingerprint.v1`):
//!
//! - The per-query QUERY-IDENTITY tuple — `helper_kind`, `primary_canonical`
//!   (path-const folded at capture), `symbol_or_expression`, `type_arguments`,
//!   `projection_mode`, `host_setup_kind`, and the typed `source_locator`
//!   (`reference_canonical`, `reference_name`, `symbol_space`). All body-derivable:
//!   `Resolve*` / `ShallowSurfaceExpr` resolve the named symbol in TYPE space at
//!   the query's own `primary_canonical`; `EvaluateExpr` resolves the expression's
//!   leading binder in VALUE space at its scope.
//! - The row's `workspace_files` — each `{path, content_hash}`, SORTED by path,
//!   over the registry's upserted source bytes ([`content_hash`], the SAME `sha256`
//!   recipe as the snapshot's `identity.workspace_files`). NOT body-token-extractable
//!   (the source routes through file-local consts / `upsert*` wrappers), so it is
//!   CAPTURED from the lift/registry context and RETAINED in
//!   `LiftMigrationProvenance.workspace_files`; the hermetic re-check reads it back.
//! - Row identity, declared query count, and the proof-shape DISCRIMINANT
//!   (`Ts7Oracle` vs `OracleAndGuard:<guard>`; not the specific `OracleId`, which
//!   the body does not name).
//!
//! Both `workspace_files` and `source_locator` are now FINGERPRINT axes: a
//! self-consistent wrong `(registry ∧ snapshot)` pair that re-points the workspace
//! setup or the source locator and regenerates a matching snapshot — which
//! `snapshot_id` + `source_admission_digest_consistent` RECOMPUTE over and self-
//! agree on — FAILS the immutable retained fingerprint. The fingerprint is NOT a
//! `snapshot_id` input (the value-irrelevant `source_locator` deliberately stays
//! out of `identity`/`snapshot_id`). Genuinely DEFERRED to a future
//! `MIGRATION_FINGERPRINT_VERSION` bump: non-empty `type_arguments` (the
//! parameterized-printer spike) and proof-OBLIGATION modeling beyond the
//! `Ts7Oracle` discriminant (an obligation-bearing body defers loudly).

#![allow(dead_code)]

use serde_json::{json, Map, Value};
use syn::visit::Visit;
use syn::{Expr, ExprCall, Lit, Stmt};

/// The domain-separation tag for the migration fingerprint (v1). A change to the
/// fingerprint field set / shape bumps the `.vN` suffix AND
/// `MIGRATION_FINGERPRINT_VERSION`.
pub const MIGRATION_FINGERPRINT_DOMAIN: &str = "verter.oracle.migration_fingerprint.v1";

/// The migration-fingerprint algorithm version recorded alongside each
/// fingerprint (mirrored into the v3 snapshot). Bumped with the domain tag.
pub const MIGRATION_FINGERPRINT_VERSION: u32 = 1;

/// Why the closed extractor could not statically recover a row's fidelity tuple.
/// Every variant is a LOUD defer — the row stays without provenance, never an
/// approximate fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractError {
    /// The retained body token stream did not parse as a `syn::Block`.
    Parse(String),
    /// No modeled query helper call (`resolve_expr` / `resolve_with_mode` /
    /// `shallow_surface_expr` / `evaluate_expr`) was found.
    NoQueryCall,
    /// No modeled host-construction call (`make_host_with_footprint` /
    /// `make_host_with_workspace_files_footprint`) was found.
    NoHostSetup,
    /// A query argument is not a const-foldable literal / closed enum path (a
    /// computed call, a loop-bound, a closure, an unfolded identifier).
    NonConstArg(&'static str),
    /// A `projection_mode` enum path was not one of the four closed variants.
    UnsupportedMode(String),
    /// A non-empty `type_arguments` slice (the parameterized printer is a
    /// deferred spike; no lifted row carries one).
    UnsupportedTypeArgs,
    /// The query helper had the wrong arity for its modeled shape.
    BadArity(&'static str),
    /// An obligation-bearing assertion (footprint / audit / warm-cache /
    /// declared-dependency) was detected — the row's proof shape is
    /// `OracleAndGuard`, but proof-shape modeling beyond `Ts7Oracle` is a
    /// deferred capability, so such a body defers rather than mis-seating.
    UnmodeledObligation(String),
    /// A modeled query call appears INSIDE a control-flow construct rather than at
    /// top-level straight-line position. The extractor is CLOSED over control flow:
    /// it does NOT descend into and flatten such a call as if it were a top-level
    /// executed query (which could prove a query shape the body never executed in
    /// that flat form). The property is "ONLY a top-level straight-line executed
    /// query is admissible" — anything DEFERRED, CONDITIONAL, SHORT-CIRCUITED, DEAD
    /// (unreachable), or reached through an early-transfer construct REJECTS. The
    /// `&'static str` names the enclosing construct (or, for `"unreachable"`, that the
    /// position is dead):
    /// - `"loop"` — `for` / `while` / `while let` / `loop`,
    /// - `"closure"` — `|args| ...`,
    /// - `"conditional"` — `if` / `if let` / `match` arm or guard / a `let … else`
    ///   diverge,
    /// - `"short-circuit"` — the conditionally-evaluated RHS of `&&` / `||`,
    /// - `"compound-assign"` — either operand of a compound assignment (`+=`, `*=`, …),
    ///   whose evaluation order is type-dependent (the safe both-orders rule),
    /// - `"async"` — a deferred `async { … }` block,
    /// - `"const"` — a `const { … }` const-eval block,
    /// - `"try"` — the `?` operator or a `try { … }` block (fallible early transfer),
    /// - `"unreachable"` — a DEAD position. Reachability is computed by the single
    ///   [`EvalReach`] evaluation-order walker at both STATEMENT and OPERAND
    ///   granularity: a statement after a diverging statement in the same block is
    ///   dead, AND a later operand after an earlier operand diverges (a call argument,
    ///   array / tuple / struct element, index, binary operand, …) is dead. An
    ///   expression diverges iff some UNCONDITIONALLY-evaluated operand diverges, OR
    ///   the form cannot complete normally — a direct `return;` / `break;` /
    ///   `continue;`, divergence in a loop HEADER (`while`/`for` condition/iterator),
    ///   an infinite `loop` (no REACHABLE escaping `break`), an exhaustively-diverging
    ///   `if` / `match` (including a diverging match GUARD, evaluated before its body),
    ///   or divergence in any deep operand wrapper. A reachable macro STATEMENT is an
    ///   opaque barrier (it cuts following reachability). A diverging form's own
    ///   OPERANDS that evaluate before the transfer stay admissible — only the
    ///   positions that follow it are dead; Rust's `unreachable_code` warning is not a
    ///   proof rail.
    ///
    /// A bare `{ … }` / `unsafe { … }` / `const { … }` block is NOT a control-flow
    /// construct: its statements run unconditionally and straight-line, so a query
    /// scoped in one (e.g. `let expr = { …; resolve_expr(…) };`) stays admissible.
    ControlFlowAroundQuery(&'static str),
}

/// The typed source locator a query resolves through (§Q4) — a fidelity
/// coordinate of the ORIGINAL body. Body-extractable for every modeled helper:
/// `Resolve*` / `ShallowSurfaceExpr` resolve the named symbol in TYPE space at the
/// query's own `primary_canonical`; `EvaluateExpr` resolves the expression's
/// leading binder in VALUE space at its scope file. The registry projects the
/// SAME locator from `QuerySpec.source_locator`, so a lift that flips the
/// symbol-space or re-points the reference FAILS the fidelity guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocatorFidelity {
    pub reference_canonical: String,
    pub reference_name: String,
    /// `"Type"` | `"Value"` — the lookup table the name is resolved IN.
    pub symbol_space: String,
}

/// One workspace file's migration-fidelity coordinate: the canonical leading-slash
/// path + the `sha256:`-prefixed content hash of its registry source bytes, under
/// the SAME `{path, content_hash}` recipe the snapshot's `identity.workspace_files`
/// uses ([`content_hash`]). NOT body-token-extractable (the source routes through
/// file-local consts / `upsert*` wrappers absent from the `#[test]` body), so it is
/// CAPTURED from the lift/registry context and retained in
/// `LiftMigrationProvenance.workspace_files`; the hermetic re-check reads it back
/// from there, the registry guard re-derives it from the current registry source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileFidelity {
    pub path: String,
    pub content_hash: String,
}

/// One modeled query's recovered fidelity (the registry-comparable axes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryFidelity {
    pub helper_kind: String,
    pub primary_canonical: String,
    pub symbol_or_expression: String,
    pub type_arguments: Vec<Value>,
    pub projection_mode: String,
    pub host_setup_kind: String,
    pub source_locator: SourceLocatorFidelity,
}

/// The row's proof-shape discriminant (NOT the specific `OracleId`, which is not
/// present in the test body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofShape {
    Ts7Oracle,
    OracleAndGuard(String),
}

/// The full per-row fidelity tuple the fingerprint hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FidelityTuple {
    pub row_file: String,
    pub row_function: String,
    pub declared_query_count: u16,
    /// The row's workspace file set, each `{path, content_hash}`, SORTED by path
    /// (the canonical manifest ordering — upsert order is not an input). Supplied
    /// from the lift/registry context (NOT body-extractable); see
    /// [`WorkspaceFileFidelity`].
    pub workspace_files: Vec<WorkspaceFileFidelity>,
    pub queries: Vec<QueryFidelity>,
    pub proof_shape: ProofShape,
}

/// The modeled obligation-bearing assertion idents whose presence makes a row
/// `OracleAndGuard`. None of the initial auto-lift corpus carries one; a body
/// that does defers loudly (proof-shape modeling beyond `Ts7Oracle` is a later
/// capability), never silently mis-seated as bare `Ts7Oracle`.
const OBLIGATION_ASSERT_IDENTS: &[&str] = &[
    "assert_dependency_footprint",
    "assert_footprint",
    "assert_audit_record",
    "assert_warm_cache",
    "assert_cache_hit",
    "assert_declared_dependency",
];

/// Canonicalize an original `#[test]` fn body to its span-stripped,
/// comment-free, whitespace-insensitive token stream — the retained
/// `original_body_tokens`. The input is the `{ ... }` block source (path-consts
/// already folded at capture); the output is `quote!`'s normalized rendering,
/// which re-parses identically (so the audit re-extraction is hermetic).
pub fn canonicalize_body(fn_body_src: &str) -> Result<String, ExtractError> {
    let block: syn::Block =
        syn::parse_str(fn_body_src).map_err(|e| ExtractError::Parse(e.to_string()))?;
    Ok(quote::quote!(#block).to_string())
}

/// Extract the per-row fidelity tuple from a canonicalized body token stream plus
/// the row's `workspace_files` (the CAPTURED-from-context axis — not in the body
/// token stream). Hermetic: the same `(canonical_body, workspace_files)` always
/// yields the same tuple, so `original_extraction_input_auditable` re-derives the
/// fingerprint from the retained `original_body_tokens` + retained `workspace_files`
/// ALONE — with NO VCS archaeology and NO live registry read. Body-extractable
/// axes (queries + per-query `source_locator`) come from the tokens; the
/// `workspace_files` are passed through verbatim (sorted by path here so the
/// canonical ordering is independent of the caller's order).
pub fn extract_fidelity(
    canonical_body: &str,
    row_file: &str,
    row_function: &str,
    workspace_files: Vec<WorkspaceFileFidelity>,
) -> Result<FidelityTuple, ExtractError> {
    let block: syn::Block =
        syn::parse_str(canonical_body).map_err(|e| ExtractError::Parse(e.to_string()))?;

    // (1) Host setup — the single modeled host-construction call.
    let host_setup_kind = find_host_setup(&block.stmts).ok_or(ExtractError::NoHostSetup)?;

    // (2) Obligation detection — a modeled obligation assert defers (proof-shape
    //     modeling beyond Ts7Oracle is a later capability).
    if let Some(ident) = find_obligation_assert(&block) {
        return Err(ExtractError::UnmodeledObligation(ident));
    }

    // (3) The ordered modeled query calls — recovered by ONE evaluation-order
    //     reachability walker (`EvalReach`) that is CLOSED over control flow: a query
    //     reached through a deferred / conditional / short-circuited / dead position
    //     REJECTS rather than being flattened to a top-level executed query. The same
    //     walker drives divergence and break-escape, so reachability is shared.
    let mut walker = EvalReach::new(host_setup_kind);
    walker.walk_block(&block, false);
    if let Some(err) = walker.error {
        return Err(err);
    }
    if walker.queries.is_empty() {
        return Err(ExtractError::NoQueryCall);
    }

    Ok(FidelityTuple {
        row_file: row_file.to_string(),
        row_function: row_function.to_string(),
        declared_query_count: walker.queries.len() as u16,
        workspace_files: sorted_workspace_files(workspace_files),
        queries: walker.queries,
        // The initial auto-lift corpus is uniformly `Ts7Oracle`; an obligation
        // assert would have deferred above.
        proof_shape: ProofShape::Ts7Oracle,
    })
}

/// Canonicalize a workspace-file set: SORTED by path (the manifest-ordering rule),
/// so the fingerprint is independent of the order the caller assembled them.
fn sorted_workspace_files(mut files: Vec<WorkspaceFileFidelity>) -> Vec<WorkspaceFileFidelity> {
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    files
}

/// Build the fidelity tuple from the REGISTRY payload (the side the guard
/// validates). The caller maps its registry rows into `QueryFidelity` and passes
/// the manifest-declared `proof_shape`.
pub fn fidelity_from_registry(
    row_file: &str,
    row_function: &str,
    queries: Vec<QueryFidelity>,
    proof_shape: ProofShape,
    workspace_files: Vec<WorkspaceFileFidelity>,
) -> FidelityTuple {
    FidelityTuple {
        row_file: row_file.to_string(),
        row_function: row_function.to_string(),
        declared_query_count: queries.len() as u16,
        workspace_files: sorted_workspace_files(workspace_files),
        queries,
        proof_shape,
    }
}

/// The canonical-JSON document the fingerprint hashes. v1 axes (§Q4): row
/// identity, declared query count, the per-query QUERY-IDENTITY tuple INCLUDING
/// the typed `source_locator`, the row's `workspace_files` (each `{path,
/// content_hash}`, SORTED by path), and the proof-shape discriminant.
pub fn fidelity_to_canonical_json(t: &FidelityTuple) -> Value {
    let queries: Vec<Value> = t
        .queries
        .iter()
        .map(|q| {
            json!({
                "helper_kind": q.helper_kind,
                "primary_canonical": q.primary_canonical,
                "symbol_or_expression": q.symbol_or_expression,
                "type_arguments": q.type_arguments,
                "projection_mode": q.projection_mode,
                "host_setup_kind": q.host_setup_kind,
                "source_locator": {
                    "reference_canonical": q.source_locator.reference_canonical,
                    "reference_name": q.source_locator.reference_name,
                    "symbol_space": q.source_locator.symbol_space,
                },
            })
        })
        .collect();
    // Sort by path so the fingerprint is upsert-order-independent (the extractor /
    // registry projection already sort, but re-sort defensively here too).
    let mut files: Vec<&WorkspaceFileFidelity> = t.workspace_files.iter().collect();
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let workspace_files: Vec<Value> = files
        .iter()
        .map(|f| json!({ "path": f.path, "content_hash": f.content_hash }))
        .collect();
    let proof_shape = match &t.proof_shape {
        ProofShape::Ts7Oracle => json!({ "kind": "Ts7Oracle" }),
        ProofShape::OracleAndGuard(g) => json!({ "kind": "OracleAndGuard", "guard": g }),
    };
    json!({
        "row_file": t.row_file,
        "row_function": t.row_function,
        "declared_query_count": t.declared_query_count,
        "workspace_files": workspace_files,
        "queries": queries,
        "proof_shape": proof_shape,
    })
}

/// The `blake3:`-prefixed migration fingerprint over the domain-separated
/// canonical-JSON fidelity tuple.
pub fn fingerprint(t: &FidelityTuple) -> String {
    let canonical = canonical_json_string(&fidelity_to_canonical_json(t));
    let mut input = Vec::new();
    input.extend_from_slice(MIGRATION_FINGERPRINT_DOMAIN.as_bytes());
    input.extend_from_slice(canonical.as_bytes());
    format!("blake3:{}", blake3::hash(&input).to_hex())
}

/// The `sha256:`-prefixed content hash of a workspace file's source bytes — the
/// EXACT recipe `identity::content_hash` uses for the snapshot's
/// `identity.workspace_files` (`sha256` over [`canonical_content`]). Replicated
/// here because the lib's `pub(crate)` hasher is unreachable from the integration
/// test crate; pinned to the lib recipe by
/// `snapshot_workspace_files_match_retained_provenance` (a recipe drift would
/// surface as a mismatch against the generator-written snapshot hashes).
pub fn content_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let canonical = canonical_content(text);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    format!("sha256:{hex}")
}

/// Normalize file content to its canonical hashable form — pinned to match
/// `identity::canonical_content`: (1) every CRLF / lone CR → a single LF; (2) all
/// trailing newlines collapsed to EXACTLY ONE for non-empty content (an empty file
/// stays empty). Cross-platform: a CRLF checkout hashes identically to LF.
fn canonical_content(text: &str) -> String {
    let mut lf = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            lf.push('\n');
        } else {
            lf.push(c);
        }
    }
    let trimmed = lf.trim_end_matches('\n');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// The modeled host-construction calls → `host_setup_kind`.
fn find_host_setup(stmts: &[Stmt]) -> Option<String> {
    let mut found: Option<String> = None;
    let mut finder = HostFinder { found: &mut found };
    for stmt in stmts {
        finder.visit_stmt(stmt);
    }
    found
}

struct HostFinder<'a> {
    found: &'a mut Option<String>,
}

impl<'ast> Visit<'ast> for HostFinder<'_> {
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Some(ident) = call_ident(call) {
            match ident.as_str() {
                "make_host_with_footprint" => *self.found = Some("standalone".to_string()),
                "make_host_with_workspace_files_footprint" => {
                    *self.found = Some("workspace_footprint".to_string())
                }
                _ => {}
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// The modeled obligation-bearing assertion idents (call or method-call).
fn find_obligation_assert(block: &syn::Block) -> Option<String> {
    let mut found: Option<String> = None;
    let mut finder = ObligationFinder { found: &mut found };
    finder.visit_block(block);
    found
}

struct ObligationFinder<'a> {
    found: &'a mut Option<String>,
}

impl<'ast> Visit<'ast> for ObligationFinder<'_> {
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Some(ident) = call_ident(call) {
            if OBLIGATION_ASSERT_IDENTS.contains(&ident.as_str()) {
                *self.found = Some(ident);
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// The evaluation-order reachability result of a node: whether control can fall
/// through it (`MayComplete`) or it always transfers control before completing
/// normally (`AlwaysDiverges`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flow {
    MayComplete,
    AlwaysDiverges,
}

impl Flow {
    fn diverges(self) -> bool {
        matches!(self, Flow::AlwaysDiverges)
    }

    fn may_complete(self) -> bool {
        matches!(self, Flow::MayComplete)
    }
}

/// A control-flow frame a `break` can resolve to: a loop (the unlabeled-break target,
/// whose `escaped` flag decides whether the loop terminates) or a labeled block (a
/// labeled-break target that shadows an outer same-name label). Frames are identified
/// by their stack INDEX — stable for a frame's lifetime, since frames are pushed and
/// popped strictly LIFO — so resolution is binding-based and shadow-aware, not a
/// string-equality + depth scheme.
struct Frame {
    /// `Some("'lbl")` if the loop / block is labeled.
    label: Option<String>,
    /// Whether this frame is a loop (the only target of an UNLABELED break, and the
    /// only frame whose `escaped` flag is read to decide divergence).
    is_loop: bool,
    /// Set when a REACHABLE `break` (not dead, resolved within the current execution
    /// boundary, whose own value does not diverge first) resolves to this frame. A loop
    /// frame that is never escaped is an infinite loop → it diverges; a labeled-block
    /// frame that receives a reachable `break 'label` completes normally.
    escaped: bool,
}

/// An execution boundary the cursor is currently inside: a closure body, an `async`
/// block, a `const` block, or an immediately-invoked closure (IIFE) body. Two roles:
///
/// - **Break floor.** `floor` is the frame-stack length at boundary entry. A `break` /
///   `continue` may only resolve to a frame at index `>= floor` — a `break` that would
///   target an outer loop across a closure/async/const boundary is a compile error in
///   real Rust, so it is never counted. Frames pushed INSIDE the boundary (an inner
///   loop) stay reachable.
/// - **Local return.** An `immediate` boundary (a non-async IIFE body) treats a `return`
///   as LOCAL completion of the call rather than an outer transfer: a reachable `return`
///   sets `returned`, and the IIFE call then completes (`MayComplete`) instead of
///   diverging. A deferred boundary (closure / async / const) discards its body flow, so
///   `returned` is irrelevant there.
struct Boundary {
    floor: usize,
    immediate: bool,
    returned: bool,
}

/// The single evaluation-order reachability walker. ONE pass over the body drives the
/// three migration-fidelity concerns that previously ran as SEPARATE traversals:
///
/// - **divergence** — every walk method returns the node's [`Flow`]; a block /
///   operand list cuts the suffix after the first `AlwaysDiverges`, so reachability is
///   exact at both statement and operand granularity.
/// - **break-escape** — each loop / labeled block is a [`Frame`]; a REACHABLE `break`
///   sets its resolved frame's `escaped` flag, and a `loop` diverges iff its frame is
///   never escaped. Because the SAME reachability cursor gates the break, a break in a
///   dead subtree (a dead `if` branch, a dead operand, dead code after a transfer) is
///   never counted — closing the over-count hole by construction.
/// - **query-collection** — a modeled query call is recorded only from a LIVE,
///   un-gated position; a query reached through a deferred / conditional /
///   short-circuited / dead position is REJECTED with the enclosing reason (the gate,
///   or `"unreachable"` for dead code).
///
/// Sharing one walker closes the dead-code-admission class by construction: there is
/// no second traversal that could disagree with this one about what is reachable.
///
/// Soundness leans SAFE. The UNSOUND errors are UNDER-detecting divergence (admitting
/// a dead query as executed) and OVER-counting a break escape (making an infinite loop
/// look finite, admitting a dead query); OVER-detecting divergence / OVER-gating a
/// query only over-rejects a FOLLOWING query (the row defers — safe). Ambiguous
/// constructs resolve in the safe direction.
///
/// DEFER (sound, documented, out of scope — no corpus row): a `-> !` never-returning
/// function call and a macro in OPERAND position are SEMANTIC divergence not
/// syntactically determinable without type/macro awareness, so they read as
/// `MayComplete` here. The opaque-macro barrier is STATEMENT-level only (a
/// `Stmt::Macro` / macro expr-statement cuts following reachability) — STRUCTURAL,
/// never a macro-NAME match.
struct EvalReach {
    host_setup_kind: String,
    queries: Vec<QueryFidelity>,
    error: Option<ExtractError>,
    /// The loop / labeled-block frames currently enclosing the cursor (innermost
    /// last). Break resolution indexes into this stack.
    frames: Vec<Frame>,
    /// The deferred / conditional query gates currently enclosing the cursor
    /// (innermost last). A query reached with a non-empty stack is rejected with the
    /// innermost gate. ORTHOGONAL to reachability: a `"conditional"` gate rejects a
    /// query but does NOT suppress a break (an `if`-arm break can still escape).
    gates: Vec<&'static str>,
    /// The execution boundaries (closure / async / const / IIFE) enclosing the cursor,
    /// innermost last. The innermost boundary's `floor` bounds break resolution; an
    /// `immediate` (IIFE) boundary captures a reachable `return` as local completion.
    boundaries: Vec<Boundary>,
}

impl EvalReach {
    fn new(host_setup_kind: String) -> Self {
        EvalReach {
            host_setup_kind,
            queries: Vec::new(),
            error: None,
            frames: Vec::new(),
            gates: Vec::new(),
            boundaries: Vec::new(),
        }
    }

    /// Push a query gate for the duration of `f`, so a query reached inside `f` is
    /// rejected as enclosed by `gate`.
    fn within_gate<R>(&mut self, gate: &'static str, f: impl FnOnce(&mut Self) -> R) -> R {
        self.gates.push(gate);
        let r = f(self);
        self.gates.pop();
        r
    }

    /// The rejection reason a query at the current cursor carries, or `None` if it is
    /// a live straight-line (admissible) position. DEAD code takes precedence over an
    /// enclosing gate — a dead query's primary defect is that it never executes.
    fn query_gate(&self, dead: bool) -> Option<&'static str> {
        if dead {
            Some("unreachable")
        } else {
            self.gates.last().copied()
        }
    }

    /// Push a break-resolution frame; returns its stack index (its identity).
    fn push_frame(&mut self, label: Option<String>, is_loop: bool) -> usize {
        let idx = self.frames.len();
        self.frames.push(Frame {
            label,
            is_loop,
            escaped: false,
        });
        idx
    }

    fn pop_frame(&mut self) {
        self.frames.pop();
    }

    /// The lowest frame index a `break` / `continue` at the current cursor may resolve
    /// to — the innermost execution boundary's floor (0 at the function-body level). A
    /// break never crosses a closure / async / const / IIFE boundary to an outer loop.
    fn boundary_floor(&self) -> usize {
        self.boundaries.last().map_or(0, |b| b.floor)
    }

    /// Run `f` inside a fresh execution boundary. Records the break floor (the current
    /// frame count) and, for an `immediate` (IIFE) boundary, whether a reachable
    /// `return` completed it locally. Returns `f`'s result plus that `returned` flag.
    fn within_boundary<R>(&mut self, immediate: bool, f: impl FnOnce(&mut Self) -> R) -> (R, bool) {
        self.boundaries.push(Boundary {
            floor: self.frames.len(),
            immediate,
            returned: false,
        });
        let r = f(self);
        let returned = self.boundaries.pop().is_some_and(|b| b.returned);
        (r, returned)
    }

    /// The nearest enclosing LOOP frame within the current execution boundary — an
    /// unlabeled break's target.
    fn resolve_unlabeled(&self) -> Option<usize> {
        (self.boundary_floor()..self.frames.len())
            .rev()
            .find(|&i| self.frames[i].is_loop)
    }

    /// The nearest enclosing frame (loop OR labeled block) within the current execution
    /// boundary named `name` — innermost wins, so an inner same-name label SHADOWS the
    /// outer (a labeled block shadows a same-name loop label).
    fn resolve_labeled(&self, name: &str) -> Option<usize> {
        (self.boundary_floor()..self.frames.len())
            .rev()
            .find(|&i| self.frames[i].label.as_deref() == Some(name))
    }

    /// Record a modeled query call. A gated / dead position REJECTS (latches the
    /// error); a live straight-line position parses the query (which may itself defer
    /// on a non-const argument).
    fn record_query(&mut self, call: &ExprCall, gate: Option<&'static str>) {
        if self.error.is_some() {
            return;
        }
        if let Some(reason) = gate {
            self.error = Some(ExtractError::ControlFlowAroundQuery(reason));
            return;
        }
        let Some(ident) = call_ident(call) else {
            return;
        };
        let args: Vec<&Expr> = call.args.iter().collect();
        let parsed = match ident.as_str() {
            // resolve_expr(host, primary, symbol, &[type_args], mode)
            "resolve_expr" => parse_resolve_expr(&args, &self.host_setup_kind),
            // resolve_with_mode(host, primary, symbol, mode) — type_args = []
            "resolve_with_mode" => parse_resolve_with_mode(&args, &self.host_setup_kind),
            // shallow_surface_expr(host, primary, symbol) — Shallow, []
            "shallow_surface_expr" => parse_shallow_surface(&args, &self.host_setup_kind),
            // evaluate_expr(host, scope, expression, mode)
            "evaluate_expr" => parse_evaluate_expr(&args, &self.host_setup_kind),
            _ => None,
        };
        if let Some(result) = parsed {
            match result {
                Ok(q) => self.queries.push(q),
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// Walk a block's statements in order under a per-block reachability cursor: every
    /// statement after the first REACHABLE diverging statement is dead. Returns
    /// `AlwaysDiverges` iff a reachable statement diverges.
    fn walk_block(&mut self, block: &syn::Block, dead: bool) -> Flow {
        let mut cur_dead = dead;
        let mut diverged = false;
        for stmt in &block.stmts {
            if self.error.is_some() {
                break;
            }
            let flow = self.walk_stmt(stmt, cur_dead);
            if !cur_dead && flow.diverges() {
                diverged = true;
                cur_dead = true;
            }
        }
        if diverged {
            Flow::AlwaysDiverges
        } else {
            Flow::MayComplete
        }
    }

    /// Walk a statement; returns its `Flow`.
    fn walk_stmt(&mut self, stmt: &Stmt, dead: bool) -> Flow {
        match stmt {
            // An expression statement diverges iff its expression diverges. A macro
            // EXPRESSION-statement (`panic!()` as a tail/expr-statement) is the opaque
            // barrier, same as a `Stmt::Macro`.
            Stmt::Expr(expr, _) => {
                if matches!(peel_paren(expr), Expr::Macro(_)) {
                    return Flow::AlwaysDiverges;
                }
                self.walk_expr(expr, dead)
            }
            // `let PAT = <init> [else { … }];` diverges iff its INITIALIZER diverges —
            // control never reaches the binding. The `else` diverge arm is CONDITIONAL
            // (a refutable-pattern miss), gated `"conditional"`, and does not make the
            // `let` diverge — the init `expr` stays the unconditional query position.
            Stmt::Local(local) => {
                let Some(init) = &local.init else {
                    return Flow::MayComplete;
                };
                let init_flow = self.walk_expr(&init.expr, dead);
                if let Some((_, diverge)) = &init.diverge {
                    // The else arm runs only when the refutable pattern fails to bind —
                    // and never at all when the initializer diverged (control never
                    // reaches the bind). So it is DEAD when the init diverged, and a
                    // `break` there cannot escape an enclosing loop.
                    let else_dead = dead || init_flow.diverges();
                    self.within_gate("conditional", |s| s.walk_expr(diverge, else_dead));
                }
                init_flow
            }
            // A nested item declaration does not transfer control.
            Stmt::Item(_) => Flow::MayComplete,
            // The opaque macro-statement barrier (`panic!();`, `foo! { … }`):
            // STRUCTURAL, never a name match.
            Stmt::Macro(_) => Flow::AlwaysDiverges,
        }
    }

    /// Walk a sequence of operands in evaluation order, threading reachability: after
    /// an operand diverges, the rest are dead. Returns `AlwaysDiverges` iff a reachable
    /// operand diverges.
    fn walk_operands<'b>(
        &mut self,
        operands: impl IntoIterator<Item = &'b Expr>,
        dead: bool,
    ) -> Flow {
        let mut cur_dead = dead;
        let mut diverged = false;
        for op in operands {
            if self.error.is_some() {
                break;
            }
            let flow = self.walk_expr(op, cur_dead);
            if !cur_dead && flow.diverges() {
                diverged = true;
                cur_dead = true;
            }
        }
        if diverged {
            Flow::AlwaysDiverges
        } else {
            Flow::MayComplete
        }
    }

    /// Walk an expression; returns its `Flow`. Records any reachable query and marks
    /// any reachable break's target frame as escaped, as side effects.
    ///
    /// This match is the variant-table contract: EVERY `syn::Expr` variant is classified
    /// exactly once — each child position is UNCONDITIONAL (threaded under the
    /// reachability cursor, left-to-right), LAZY/CONDITIONAL/DEFERRED (gated), or a
    /// boundary/frame. `syn::Expr` is `#[non_exhaustive]`, so the trailing wildcard is
    /// mandatory; it is a SAFE barrier (never `MayComplete`) and is unreachable on the
    /// pinned `syn 2.0.117` (proven by `expr_variant_table_covers_every_syn_variant`;
    /// a syn bump is caught by `syn_version_is_pinned_for_the_expr_variant_table`).
    fn walk_expr(&mut self, expr: &Expr, dead: bool) -> Flow {
        if self.error.is_some() {
            return Flow::MayComplete;
        }
        match expr {
            // --- Direct unconditional transfers ----------------------------------
            // `return` / `break` evaluate their value FIRST (a query / nested break
            // there is reached before the transfer), then transfer regardless.
            Expr::Return(e) => {
                let value_diverges = match &e.expr {
                    Some(v) => self.walk_expr(v, dead).diverges(),
                    None => false,
                };
                // Inside an IMMEDIATE (IIFE) boundary a reachable `return` is LOCAL
                // completion: it returns control to the caller, so the call completes.
                // (For the function body and deferred closures there is no such
                // marking — a return diverges the enclosing frame / is discarded.)
                if !dead && !value_diverges {
                    if let Some(b) = self.boundaries.last_mut() {
                        if b.immediate {
                            b.returned = true;
                        }
                    }
                }
                Flow::AlwaysDiverges
            }
            Expr::Break(e) => {
                let value_diverges = match &e.expr {
                    Some(v) => self.walk_expr(v, dead).diverges(),
                    None => false,
                };
                // Count this break as escaping its target frame ONLY if it is reachable
                // (not dead) and its own value does not diverge first (`break { return; }`
                // diverges before transfer). Resolution honors the execution-boundary
                // floor, so a break that would cross a closure/async/const/IIFE boundary
                // to an outer loop resolves to nothing and is never counted.
                if !dead && !value_diverges {
                    let resolved = match &e.label {
                        Some(lbl) => self.resolve_labeled(&lbl.to_string()),
                        None => self.resolve_unlabeled(),
                    };
                    if let Some(idx) = resolved {
                        self.frames[idx].escaped = true;
                    }
                }
                Flow::AlwaysDiverges
            }
            Expr::Continue(_) => Flow::AlwaysDiverges,

            // --- Unconditional-operand wrappers (ordered, left-to-right) ----------
            Expr::Array(e) => self.walk_operands(e.elems.iter(), dead),
            Expr::Tuple(e) => self.walk_operands(e.elems.iter(), dead),
            Expr::Call(e) => {
                // An immediately-invoked closure (IIFE): a non-async closure callee,
                // possibly parenthesized. Its body is NOT deferred — it runs as an
                // immediate execution frame (see `walk_iife`).
                if let Expr::Closure(closure) = peel_paren(e.func.as_ref()) {
                    if closure.asyncness.is_none() {
                        return self.walk_iife(closure, e, dead);
                    }
                }
                // A call's operands (callee, then args left-to-right) are evaluated
                // BEFORE control enters the callee. Walk them first under this cursor:
                // if a reachable operand diverges, the call point is never reached. A
                // recognized query helper is therefore recorded only at a LIVE call
                // position — `dead || operands.diverges()` gates it `"unreachable"` (the
                // query never runs), exactly as a dead statement position would. The
                // identity parser ignores the host operand, so gating the record on the
                // post-operand reachability (not recording before the operands run) is
                // what rejects a query whose own argument diverges first.
                let operands =
                    self.walk_operands(std::iter::once(e.func.as_ref()).chain(e.args.iter()), dead);
                if is_query_call(e) {
                    let gate = self.query_gate(dead || operands.diverges());
                    self.record_query(e, gate);
                }
                operands
            }
            Expr::MethodCall(e) => self.walk_operands(
                std::iter::once(e.receiver.as_ref()).chain(e.args.iter()),
                dead,
            ),
            Expr::Struct(e) => self.walk_operands(
                e.fields.iter().map(|f| &f.expr).chain(e.rest.as_deref()),
                dead,
            ),
            // The repeated VALUE is an unconditional runtime operand; the LENGTH is a
            // const-eval boundary (gated `"const"`, breaks cannot escape), and being
            // const it does not contribute to runtime divergence — the repeat's flow is
            // the value's flow.
            Expr::Repeat(e) => {
                let value = self.walk_expr(&e.expr, dead);
                self.within_boundary(false, |s| {
                    s.within_gate("const", |s| s.walk_expr(&e.len, dead))
                });
                value
            }
            Expr::Index(e) => self.walk_operands([e.expr.as_ref(), e.index.as_ref()], dead),
            // Plain assignment evaluates the RHS before the LHS place expression.
            Expr::Assign(e) => self.walk_operands([e.right.as_ref(), e.left.as_ref()], dead),
            Expr::Range(e) => self.walk_operands(
                [e.start.as_deref(), e.end.as_deref()].into_iter().flatten(),
                dead,
            ),
            // Single-operand passthroughs.
            Expr::Unary(e) => self.walk_expr(&e.expr, dead),
            Expr::Cast(e) => self.walk_expr(&e.expr, dead),
            // `.await` of an arbitrary future is its base's flow (future-poll divergence
            // is semantic, not structural). But awaiting a SYNTACTIC `async { … }` block
            // or an async-closure IIFE FORCES the body to run as part of the await: a
            // `return` completes it, an infinite body diverges it
            // (`async { loop {} }.await` never completes). Queries/breaks stay async-gated.
            Expr::Await(e) => match peel_paren(&e.base) {
                Expr::Async(a) => {
                    self.walk_immediate_body(Some("async"), dead, |s, d| s.walk_block(&a.block, d))
                }
                Expr::Call(call) => {
                    if let Expr::Closure(c) = peel_paren(call.func.as_ref()) {
                        if c.asyncness.is_some() {
                            let args_flow = self.walk_operands(call.args.iter(), dead);
                            let body_dead = dead || args_flow.diverges();
                            let body =
                                self.walk_immediate_body(Some("async"), body_dead, |s, d| {
                                    s.walk_expr(&c.body, d)
                                });
                            return if args_flow.diverges() {
                                Flow::AlwaysDiverges
                            } else {
                                body
                            };
                        }
                    }
                    self.walk_expr(&e.base, dead)
                }
                _ => self.walk_expr(&e.base, dead),
            },
            Expr::Field(e) => self.walk_expr(&e.base, dead),
            Expr::Reference(e) => self.walk_expr(&e.expr, dead),
            Expr::RawAddr(e) => self.walk_expr(&e.expr, dead),
            Expr::Paren(e) => self.walk_expr(&e.expr, dead),
            Expr::Group(e) => self.walk_expr(&e.expr, dead),
            // The `if let` / `while let` scrutinee is unconditional.
            Expr::Let(e) => self.walk_expr(&e.expr, dead),

            // --- Short-circuit `&&` / `||` ---------------------------------------
            // The LEFT operand is unconditional; the RIGHT is a lazy boundary, gated
            // `"short-circuit"` for queries. The binary diverges iff the LEFT does.
            // Every other binary operator evaluates both operands unconditionally.
            Expr::Binary(e) => match e.op {
                // `&&` / `||`: the LEFT operand is unconditional; the RIGHT is a lazy
                // short-circuit boundary, gated for queries. The binary diverges iff the
                // LEFT does.
                syn::BinOp::And(_) | syn::BinOp::Or(_) => {
                    let left = self.walk_expr(&e.left, dead);
                    self.within_gate("short-circuit", |s| {
                        s.walk_expr(&e.right, dead || left.diverges())
                    });
                    left
                }
                // Compound assignment (`a += b`): evaluation order is TYPE-DEPENDENT (a
                // primitive evaluates LHS-then-RHS; an overloaded operator is a method
                // call with its own order). A structural walker cannot pick an order, so
                // the SAFE both-orders rule treats a query/break in EITHER operand as
                // not-guaranteed — both are gated `"compound-assign"` (queries reject)
                // inside a boundary (breaks cannot escape), and the form diverges if
                // either operand diverges.
                syn::BinOp::AddAssign(_)
                | syn::BinOp::SubAssign(_)
                | syn::BinOp::MulAssign(_)
                | syn::BinOp::DivAssign(_)
                | syn::BinOp::RemAssign(_)
                | syn::BinOp::BitXorAssign(_)
                | syn::BinOp::BitAndAssign(_)
                | syn::BinOp::BitOrAssign(_)
                | syn::BinOp::ShlAssign(_)
                | syn::BinOp::ShrAssign(_) => {
                    let (lf, _) = self.within_boundary(false, |s| {
                        s.within_gate("compound-assign", |s| s.walk_expr(&e.left, dead))
                    });
                    let (rf, _) = self.within_boundary(false, |s| {
                        s.within_gate("compound-assign", |s| s.walk_expr(&e.right, dead))
                    });
                    if lf.diverges() || rf.diverges() {
                        Flow::AlwaysDiverges
                    } else {
                        Flow::MayComplete
                    }
                }
                // Every other binary operator evaluates both operands unconditionally,
                // left to right.
                _ => self.walk_operands([e.left.as_ref(), e.right.as_ref()], dead),
            },

            // --- Conditional forms ------------------------------------------------
            Expr::If(e) => {
                // The CONDITION is unconditional (ungated); if it diverges the branches
                // are DEAD. The branches are gated `"conditional"` for queries but
                // may-reach for breaks. The `if` diverges iff the condition diverges, or
                // (with an `else`) BOTH arms diverge.
                let cond = self.walk_expr(&e.cond, dead);
                let branch_dead = dead || cond.diverges();
                let then =
                    self.within_gate("conditional", |s| s.walk_block(&e.then_branch, branch_dead));
                let els = e.else_branch.as_ref().map(|(_, els_expr)| {
                    self.within_gate("conditional", |s| s.walk_expr(els_expr, branch_dead))
                });
                let both_arms_diverge = then.diverges() && matches!(els, Some(f) if f.diverges());
                if cond.diverges() || both_arms_diverge {
                    Flow::AlwaysDiverges
                } else {
                    Flow::MayComplete
                }
            }
            Expr::Match(e) => {
                // The SCRUTINEE is unconditional (ungated); then each arm's GUARD runs
                // before its body, in source order. The match diverges if the scrutinee
                // diverges, OR ANY reachable guard diverges (a diverging guard transfers
                // control before any arm body — one-way safe), OR every arm body
                // diverges. A diverging guard makes that arm's body and all later arms
                // DEAD. Guards / bodies are gated `"conditional"`.
                let scrut = self.walk_expr(&e.expr, dead);
                let mut arms_dead = dead || scrut.diverges();
                let mut any_guard_diverges = false;
                let mut all_bodies_diverge = true;
                for arm in &e.arms {
                    if let Some((_, guard)) = &arm.guard {
                        let g = self.within_gate("conditional", |s| s.walk_expr(guard, arms_dead));
                        if !arms_dead && g.diverges() {
                            any_guard_diverges = true;
                            arms_dead = true;
                        }
                    }
                    let body =
                        self.within_gate("conditional", |s| s.walk_expr(&arm.body, arms_dead));
                    if !body.diverges() {
                        all_bodies_diverge = false;
                    }
                }
                if scrut.diverges() || any_guard_diverges || all_bodies_diverge {
                    Flow::AlwaysDiverges
                } else {
                    Flow::MayComplete
                }
            }

            // --- Loops ------------------------------------------------------------
            // A `while` / `for` diverges iff its HEADER (condition / iterator) diverges;
            // the BODY is a boundary (may not run / may break), descended for queries
            // (gated `"loop"`) and for breaks (the loop is a frame, so an unlabeled
            // break inside resolves HERE, not to an outer loop). A `loop` diverges iff
            // no REACHABLE break escapes it — its frame's `escaped` flag after the body.
            Expr::While(e) => {
                self.gates.push("loop");
                self.push_frame(label_name(&e.label), true);
                let cond = self.walk_expr(&e.cond, dead);
                self.walk_block(&e.body, dead || cond.diverges());
                self.pop_frame();
                self.gates.pop();
                cond
            }
            Expr::ForLoop(e) => {
                self.gates.push("loop");
                self.push_frame(label_name(&e.label), true);
                let iter = self.walk_expr(&e.expr, dead);
                self.walk_block(&e.body, dead || iter.diverges());
                self.pop_frame();
                self.gates.pop();
                iter
            }
            Expr::Loop(e) => {
                self.gates.push("loop");
                let idx = self.push_frame(label_name(&e.label), true);
                self.walk_block(&e.body, dead);
                let escaped = self.frames[idx].escaped;
                self.pop_frame();
                self.gates.pop();
                if escaped {
                    Flow::MayComplete
                } else {
                    Flow::AlwaysDiverges
                }
            }

            // --- Fallible early-transfer (`?` / `try { … }`) ----------------------
            // The operand evaluates unconditionally (so it contributes to `Flow`), but
            // a query entangled with the early transfer is gated `"try"`.
            Expr::Try(e) => self.within_gate("try", |s| s.walk_expr(&e.expr, dead)),
            Expr::TryBlock(e) => self.within_gate("try", |s| s.walk_block(&e.block, dead)),

            // --- Blocks -----------------------------------------------------------
            // An UNLABELED `{ … }` / `unsafe { … }` runs its statements straight-line
            // (no frame, not a boundary): block flow. A LABELED block `'lbl: { … }`
            // pushes a labeled-block FRAME so a `break 'lbl` can target it (shadowing a
            // same-name loop label); it completes if its body falls through OR a
            // reachable `break 'lbl` exits it, else it diverges with its body.
            Expr::Block(e) => {
                if e.label.is_some() {
                    let idx = self.push_frame(label_name(&e.label), false);
                    let body = self.walk_block(&e.block, dead);
                    let escaped = self.frames[idx].escaped;
                    self.pop_frame();
                    if body.may_complete() || escaped {
                        Flow::MayComplete
                    } else {
                        Flow::AlwaysDiverges
                    }
                } else {
                    self.walk_block(&e.block, dead)
                }
            }
            Expr::Unsafe(e) => self.walk_block(&e.block, dead),

            // --- Deferred / const boundaries (closure / async / const) ------------
            // A closure / `async` body never runs as part of the enclosing straight-line
            // flow; a `const { … }` block is evaluated in a separate const-eval context.
            // None makes the enclosing expr diverge, a break inside cannot escape an
            // outer runtime loop (the boundary floor), and queries inside are gated
            // (`"closure"` / `"async"` / `"const"`).
            Expr::Closure(e) => {
                self.within_boundary(false, |s| {
                    s.within_gate("closure", |s| s.walk_expr(&e.body, dead))
                });
                Flow::MayComplete
            }
            Expr::Async(e) => {
                self.within_boundary(false, |s| {
                    s.within_gate("async", |s| s.walk_block(&e.block, dead))
                });
                Flow::MayComplete
            }
            Expr::Const(e) => {
                self.within_boundary(false, |s| {
                    s.within_gate("const", |s| s.walk_block(&e.block, dead))
                });
                Flow::MayComplete
            }

            // `yield <value>` evaluates its value but does not diverge.
            Expr::Yield(e) => {
                if let Some(v) = &e.expr {
                    self.walk_expr(v, dead);
                }
                Flow::MayComplete
            }

            // --- Leaf / opaque positions ------------------------------------------
            // Literals, paths, and the inferred `_` carry no query / break and never
            // diverge. A macro in OPERAND position is a sanctioned semantic defer (its
            // expansion may diverge, but that is not syntactically determinable) — it
            // reads as `MayComplete`; the STATEMENT-level macro barrier is in `walk_stmt`.
            Expr::Lit(_) | Expr::Path(_) | Expr::Infer(_) | Expr::Macro(_) => Flow::MayComplete,
            // Tokens Syn could not interpret: a SAFE barrier (never `MayComplete`, so it
            // never admits a following dead query), per the variant table.
            Expr::Verbatim(_) => Flow::AlwaysDiverges,
            // `syn::Expr` is `#[non_exhaustive]`. A future variant Syn adds is not yet
            // modeled by the table / walker, so it is treated as the same safe barrier as
            // `Verbatim` (never `MayComplete`) until the table, this match, and a
            // discriminator are updated — forced by the `syn`-version pin guard
            // (`syn_version_is_pinned_for_the_expr_variant_table`) plus the
            // variant-coverage test. This arm is unreachable on the pinned `syn 2.0.117`
            // (every current variant is handled explicitly above).
            _ => Flow::AlwaysDiverges,
        }
    }

    /// Walk a body that runs as an IMMEDIATE execution frame — an IIFE body, or the
    /// body of a syntactic `async { … }` block / async-closure IIFE forced to run by
    /// `.await`. A local `return` completes the frame (returns control to the caller),
    /// so the frame completes; a body that neither falls through nor returns (e.g.
    /// `loop {}`) never returns to the caller, so the frame diverges. `gate` (if any)
    /// gates queries inside the body (`Some("async")` for an awaited async body). A
    /// `break` / `continue` cannot escape an outer frame (the boundary floor).
    fn walk_immediate_body(
        &mut self,
        gate: Option<&'static str>,
        dead: bool,
        walk: impl FnOnce(&mut Self, bool) -> Flow,
    ) -> Flow {
        let (body_flow, returned) = self.within_boundary(true, |s| match gate {
            Some(g) => s.within_gate(g, |s| walk(s, dead)),
            None => walk(s, dead),
        });
        if body_flow.may_complete() || returned {
            Flow::MayComplete
        } else {
            Flow::AlwaysDiverges
        }
    }

    /// Walk an immediately-invoked closure (IIFE) `(|args| body)(call args)`. Order: the
    /// closure value's creation (no runtime sub-expression), then the call arguments
    /// left-to-right, then the body as an immediate frame. The call diverges iff the
    /// arguments diverge, or the body never returns to the caller.
    fn walk_iife(&mut self, closure: &syn::ExprClosure, call: &ExprCall, dead: bool) -> Flow {
        let args_flow = self.walk_operands(call.args.iter(), dead);
        let body_dead = dead || args_flow.diverges();
        let body = self.walk_immediate_body(None, body_dead, |s, d| s.walk_expr(&closure.body, d));
        if args_flow.diverges() {
            Flow::AlwaysDiverges
        } else {
            body
        }
    }
}

/// The `'lbl` name of a loop / block label, or `None` if unlabeled.
fn label_name(label: &Option<syn::Label>) -> Option<String> {
    label.as_ref().map(|l| l.name.to_string())
}

/// Whether `call` is one of the four modeled query helpers.
fn is_query_call(call: &ExprCall) -> bool {
    matches!(
        call_ident(call).as_deref(),
        Some("resolve_expr" | "resolve_with_mode" | "shallow_surface_expr" | "evaluate_expr")
    )
}

fn parse_resolve_expr(args: &[&Expr], host: &str) -> Option<Result<QueryFidelity, ExtractError>> {
    if args.len() != 5 {
        return Some(Err(ExtractError::BadArity("resolve_expr")));
    }
    Some((|| {
        let primary_canonical = str_lit(args[1], "primary_canonical")?;
        let symbol = str_lit(args[2], "symbol")?;
        Ok(QueryFidelity {
            helper_kind: "ResolveExpr".to_string(),
            source_locator: type_space_locator(&primary_canonical, &symbol),
            primary_canonical,
            symbol_or_expression: symbol,
            type_arguments: empty_type_args(args[3])?,
            projection_mode: mode_path(args[4])?,
            host_setup_kind: host.to_string(),
        })
    })())
}

fn parse_resolve_with_mode(
    args: &[&Expr],
    host: &str,
) -> Option<Result<QueryFidelity, ExtractError>> {
    if args.len() != 4 {
        return Some(Err(ExtractError::BadArity("resolve_with_mode")));
    }
    Some((|| {
        let primary_canonical = str_lit(args[1], "primary_canonical")?;
        let symbol = str_lit(args[2], "symbol")?;
        Ok(QueryFidelity {
            helper_kind: "ResolveExpr".to_string(),
            source_locator: type_space_locator(&primary_canonical, &symbol),
            primary_canonical,
            symbol_or_expression: symbol,
            type_arguments: Vec::new(),
            projection_mode: mode_path(args[3])?,
            host_setup_kind: host.to_string(),
        })
    })())
}

fn parse_shallow_surface(
    args: &[&Expr],
    host: &str,
) -> Option<Result<QueryFidelity, ExtractError>> {
    if args.len() != 3 {
        return Some(Err(ExtractError::BadArity("shallow_surface_expr")));
    }
    Some((|| {
        let primary_canonical = str_lit(args[1], "primary_canonical")?;
        let symbol = str_lit(args[2], "symbol")?;
        Ok(QueryFidelity {
            helper_kind: "ShallowSurfaceExpr".to_string(),
            source_locator: type_space_locator(&primary_canonical, &symbol),
            primary_canonical,
            symbol_or_expression: symbol,
            type_arguments: Vec::new(),
            projection_mode: "Shallow".to_string(),
            host_setup_kind: host.to_string(),
        })
    })())
}

fn parse_evaluate_expr(args: &[&Expr], host: &str) -> Option<Result<QueryFidelity, ExtractError>> {
    if args.len() != 4 {
        return Some(Err(ExtractError::BadArity("evaluate_expr")));
    }
    Some((|| {
        let scope = str_lit(args[1], "scope")?;
        let expression = str_lit(args[2], "expression")?;
        // `EvaluateExpr` resolves the expression's LEADING BINDER in VALUE space at
        // its scope file (`typeof f` → `f`; §Q4). The reference_name is the binder,
        // not the whole expression text.
        let reference_name = evaluate_leading_binder(&expression)?;
        Ok(QueryFidelity {
            helper_kind: "EvaluateExpr".to_string(),
            source_locator: SourceLocatorFidelity {
                reference_canonical: scope.clone(),
                reference_name,
                symbol_space: "Value".to_string(),
            },
            primary_canonical: scope,
            symbol_or_expression: expression,
            type_arguments: Vec::new(),
            projection_mode: mode_path(args[3])?,
            host_setup_kind: host.to_string(),
        })
    })())
}

/// The TYPE-space locator a `Resolve*` / `ShallowSurfaceExpr` query resolves
/// through: the named symbol at the query's own `primary_canonical`. (Those
/// helpers are type-position resolutions by construction, so `symbol_space` is
/// always `Type` — derived from the helper, never an independent steering input.)
fn type_space_locator(primary_canonical: &str, symbol: &str) -> SourceLocatorFidelity {
    SourceLocatorFidelity {
        reference_canonical: primary_canonical.to_string(),
        reference_name: symbol.to_string(),
        symbol_space: "Type".to_string(),
    }
}

/// The leading binder of an `EvaluateExpr` expression — the identifier whose value
/// declaration the walk starts from. Strips an optional leading `typeof ` operator,
/// then takes the leading `[A-Za-z_$][A-Za-z0-9_$]*` identifier. A non-identifier
/// leading expression defers (`NonConstArg`) rather than guessing a binder.
///
/// TODO(follow-up): this is light leading-text parsing (strip `typeof`, scan one
/// identifier), the lone remaining text path in the extractor. It is safe ONLY
/// because the lifted corpus carries ZERO `EvaluateExpr` rows (it is exercised by
/// the unit test harness alone). Before any `EvaluateExpr` row is lifted, replace
/// it with a CLOSED parser over the expression's typed structure (the same
/// const-fold-or-defer discipline the query-argument path uses) so a non-trivial
/// `EvaluateExpr` expression can never seat an approximate binder.
fn evaluate_leading_binder(expression: &str) -> Result<String, ExtractError> {
    let trimmed = expression.trim_start();
    let rest = trimmed.strip_prefix("typeof").map_or(trimmed, |after| {
        // `typeof` must be a standalone keyword (followed by whitespace), not the
        // prefix of an identifier like `typeofThing`.
        match after.chars().next() {
            Some(c) if c.is_whitespace() => after.trim_start(),
            _ => trimmed,
        }
    });
    let ident: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if ident.is_empty() || ident.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Err(ExtractError::NonConstArg("expression"));
    }
    Ok(ident)
}

/// The single-segment call-ident of an `f(...)` expression (`None` for a method
/// call, a path with generics, or a non-path callee).
fn call_ident(call: &ExprCall) -> Option<String> {
    if let Expr::Path(p) = call.func.as_ref() {
        if p.qself.is_none() && p.path.segments.len() == 1 {
            return Some(p.path.segments[0].ident.to_string());
        }
    }
    None
}

/// A bare string literal, with leading `&` peeled (`&host` is never a string, so
/// this only matters for `&"x"`, which does not occur).
fn str_lit(expr: &Expr, what: &'static str) -> Result<String, ExtractError> {
    match peel_ref(expr) {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(s) => Ok(s.value()),
            _ => Err(ExtractError::NonConstArg(what)),
        },
        _ => Err(ExtractError::NonConstArg(what)),
    }
}

/// The closed `ProjectionMode` / `ProjectionModeTag` enum variant (last path
/// segment), validated against the four discriminants.
fn mode_path(expr: &Expr) -> Result<String, ExtractError> {
    let Expr::Path(p) = peel_ref(expr) else {
        return Err(ExtractError::NonConstArg("projection_mode"));
    };
    let Some(last) = p.path.segments.last() else {
        return Err(ExtractError::NonConstArg("projection_mode"));
    };
    let variant = last.ident.to_string();
    match variant.as_str() {
        "Shallow" | "Navigate" | "Expanded" | "Skeleton" => Ok(variant),
        other => Err(ExtractError::UnsupportedMode(other.to_string())),
    }
}

/// An EMPTY `&[]` type-argument slice. A non-empty slice is the deferred
/// parameterized-printer spike (no lifted row carries one).
fn empty_type_args(expr: &Expr) -> Result<Vec<Value>, ExtractError> {
    if let Expr::Reference(r) = expr {
        if let Expr::Array(arr) = r.expr.as_ref() {
            if arr.elems.is_empty() {
                return Ok(Vec::new());
            }
        }
    }
    // A bare `&[]` with no elements is the only modeled shape; anything else
    // (a non-empty slice, a const ref) defers.
    Err(ExtractError::UnsupportedTypeArgs)
}

fn peel_ref(expr: &Expr) -> &Expr {
    match expr {
        Expr::Reference(r) => peel_ref(r.expr.as_ref()),
        other => other,
    }
}

/// Peel `( … )` / invisible-group wrappers around an expression (NOT references —
/// that is [`peel_ref`]), so a parenthesized transfer statement (`(return);`) is
/// still recognized as diverging.
fn peel_paren(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(p) => peel_paren(p.expr.as_ref()),
        Expr::Group(g) => peel_paren(g.expr.as_ref()),
        other => other,
    }
}

/// Deterministic canonical JSON (recursively key-sorted) — independent of any
/// serde_json `preserve_order` feature, so the capture side and the guard side
/// hash identically.
pub fn canonical_json_string(value: &Value) -> String {
    canonical_value(value).to_string()
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted = Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonical_value(&map[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BODY: &str = r#"{
        // TS7 contract comment that must be stripped.
        let host = make_host_with_footprint();
        upsert(&host);
        let (expr, record) = resolve_expr(
            &host,
            "/fixtures/index_signatures.ts",
            "SymbolIndexed",
            &[],
            ProjectionMode::Expanded,
        );
        let sigs = object_index_signatures(&expr);
        assert_eq!(sigs.len(), 1);
        assert_query_mode(&record, ProjectionModeTag::Expanded);
    }"#;

    /// The SAMPLE_BODY row's workspace files (the CAPTURED-from-context axis — the
    /// body itself only names `upsert(&host)`, so the source bytes are supplied
    /// here, hashed under the canonical recipe).
    fn sample_workspace_files() -> Vec<WorkspaceFileFidelity> {
        vec![WorkspaceFileFidelity {
            path: "/fixtures/index_signatures.ts".to_string(),
            content_hash: content_hash("export type SymbolIndexed = { [k: symbol]: number };\n"),
        }]
    }

    fn extract_sample() -> FidelityTuple {
        let canonical = canonicalize_body(SAMPLE_BODY).expect("canonicalize");
        extract_fidelity(
            &canonical,
            "index_signatures.rs",
            "row_fn",
            sample_workspace_files(),
        )
        .expect("extract")
    }

    #[test]
    fn extracts_the_query_identity_tuple() {
        let t = extract_sample();
        assert_eq!(t.declared_query_count, 1);
        assert_eq!(t.queries.len(), 1);
        let q = &t.queries[0];
        assert_eq!(q.helper_kind, "ResolveExpr");
        assert_eq!(q.primary_canonical, "/fixtures/index_signatures.ts");
        assert_eq!(q.symbol_or_expression, "SymbolIndexed");
        assert_eq!(q.projection_mode, "Expanded");
        assert_eq!(q.host_setup_kind, "standalone");
        assert!(q.type_arguments.is_empty());
        // The source locator is the body-derived TYPE-space coordinate.
        assert_eq!(
            q.source_locator.reference_canonical,
            "/fixtures/index_signatures.ts"
        );
        assert_eq!(q.source_locator.reference_name, "SymbolIndexed");
        assert_eq!(q.source_locator.symbol_space, "Type");
        // The workspace files are carried through (sorted, hashed).
        assert_eq!(t.workspace_files.len(), 1);
        assert_eq!(t.workspace_files[0].path, "/fixtures/index_signatures.ts");
        assert!(t.workspace_files[0].content_hash.starts_with("sha256:"));
        assert_eq!(t.proof_shape, ProofShape::Ts7Oracle);
    }

    #[test]
    fn canonicalization_is_whitespace_and_comment_insensitive() {
        // Same TOKENS, different whitespace + comments → identical canonical form
        // → identical fingerprint. (Trailing commas are tokens, not whitespace, so
        // the reflowed `b` keeps SAMPLE_BODY's trailing comma after the mode arg.)
        let a = canonicalize_body(SAMPLE_BODY).unwrap();
        let b = canonicalize_body(
            "{ let host = make_host_with_footprint(); upsert(&host); /* reflowed */ \
             let (expr, record) = resolve_expr(&host, \"/fixtures/index_signatures.ts\", \
             \"SymbolIndexed\", &[], ProjectionMode::Expanded,); \
             let sigs = object_index_signatures(&expr); assert_eq!(sigs.len(), 1); \
             assert_query_mode(&record, ProjectionModeTag::Expanded); }",
        )
        .unwrap();
        assert_eq!(
            a, b,
            "canonical token streams must match modulo whitespace/comments"
        );
        let fa = fingerprint(&extract_fidelity(&a, "f.rs", "g", sample_workspace_files()).unwrap());
        let fb = fingerprint(&extract_fidelity(&b, "f.rs", "g", sample_workspace_files()).unwrap());
        assert_eq!(fa, fb);
    }

    #[test]
    fn fingerprint_is_domain_separated_blake3_and_symbol_sensitive() {
        let t = extract_sample();
        let fp = fingerprint(&t);
        assert!(
            fp.starts_with("blake3:"),
            "fingerprint is blake3-tagged: {fp}"
        );
        // A different symbol → a different fingerprint (the discriminating axis).
        let mut wrong = t.clone();
        wrong.queries[0].symbol_or_expression = "WrongSymbol".to_string();
        assert_ne!(
            fingerprint(&wrong),
            fp,
            "symbol drift must change the fingerprint"
        );
        // A different mode → a different fingerprint.
        let mut wrong_mode = t.clone();
        wrong_mode.queries[0].projection_mode = "Shallow".to_string();
        assert_ne!(fingerprint(&wrong_mode), fp);
    }

    #[test]
    fn fingerprint_is_sensitive_to_workspace_files_drift() {
        // A self-consistent WRONG pair re-points the row's workspace setup AND
        // regenerates a matching snapshot; only the IMMUTABLE retained fingerprint
        // anchors the original. So `workspace_files` (content AND path) MUST be a
        // fingerprint axis — drifting either changes the fingerprint.
        let t = extract_sample();
        let fp = fingerprint(&t);
        let mut content_drift = t.clone();
        content_drift.workspace_files[0].content_hash = content_hash("export type X = number;\n");
        assert_ne!(
            fingerprint(&content_drift),
            fp,
            "workspace_files CONTENT drift must change the fingerprint"
        );
        let mut path_drift = t.clone();
        path_drift.workspace_files[0].path = "/fixtures/WRONG.ts".to_string();
        assert_ne!(
            fingerprint(&path_drift),
            fp,
            "workspace_files PATH drift must change the fingerprint"
        );
        // Adding an extra workspace file also changes the fingerprint.
        let mut extra = t.clone();
        extra.workspace_files.push(WorkspaceFileFidelity {
            path: "/fixtures/extra.ts".to_string(),
            content_hash: content_hash("export type Y = 1;\n"),
        });
        assert_ne!(
            fingerprint(&extra),
            fp,
            "an extra file must change the fingerprint"
        );
    }

    #[test]
    fn fingerprint_is_insensitive_to_workspace_file_order() {
        // The canonical ordering is by path (upsert order is not an input): the
        // SAME file set in a DIFFERENT order fingerprints identically.
        let two: Vec<WorkspaceFileFidelity> = vec![
            WorkspaceFileFidelity {
                path: "/fixtures/b.ts".to_string(),
                content_hash: content_hash("export type B = 2;\n"),
            },
            WorkspaceFileFidelity {
                path: "/fixtures/a.ts".to_string(),
                content_hash: content_hash("export type A = 1;\n"),
            },
        ];
        let mut reversed = two.clone();
        reversed.reverse();
        let canonical = canonicalize_body(SAMPLE_BODY).unwrap();
        let fa = fingerprint(&extract_fidelity(&canonical, "f.rs", "g", two).unwrap());
        let fb = fingerprint(&extract_fidelity(&canonical, "f.rs", "g", reversed).unwrap());
        assert_eq!(
            fa, fb,
            "workspace_file order must not change the fingerprint"
        );
    }

    #[test]
    fn fingerprint_is_sensitive_to_source_locator_drift() {
        // The `source_locator` is a fidelity coordinate — a lift that flips
        // Type↔Value or re-points the reference must FAIL the guard.
        let t = extract_sample();
        let fp = fingerprint(&t);
        let mut space_flip = t.clone();
        space_flip.queries[0].source_locator.symbol_space = "Value".to_string();
        assert_ne!(
            fingerprint(&space_flip),
            fp,
            "symbol_space Type↔Value flip must change the fingerprint"
        );
        let mut name_drift = t.clone();
        name_drift.queries[0].source_locator.reference_name = "Other".to_string();
        assert_ne!(
            fingerprint(&name_drift),
            fp,
            "reference_name drift must change it"
        );
        let mut canon_drift = t.clone();
        canon_drift.queries[0].source_locator.reference_canonical =
            "/fixtures/other.ts".to_string();
        assert_ne!(
            fingerprint(&canon_drift),
            fp,
            "reference_canonical drift must change it"
        );
    }

    #[test]
    fn registry_projection_matches_body_extraction() {
        // The registry-side tuple (built from the queried axes) fingerprints to
        // the SAME value the body extraction produced — the property
        // `registry_payload_matches_migration_fingerprint` relies on.
        let body = extract_sample();
        let registry = fidelity_from_registry(
            "index_signatures.rs",
            "row_fn",
            vec![QueryFidelity {
                helper_kind: "ResolveExpr".to_string(),
                primary_canonical: "/fixtures/index_signatures.ts".to_string(),
                symbol_or_expression: "SymbolIndexed".to_string(),
                type_arguments: Vec::new(),
                projection_mode: "Expanded".to_string(),
                host_setup_kind: "standalone".to_string(),
                source_locator: SourceLocatorFidelity {
                    reference_canonical: "/fixtures/index_signatures.ts".to_string(),
                    reference_name: "SymbolIndexed".to_string(),
                    symbol_space: "Type".to_string(),
                },
            }],
            ProofShape::Ts7Oracle,
            sample_workspace_files(),
        );
        assert_eq!(fingerprint(&body), fingerprint(&registry));
    }

    #[test]
    fn rejects_a_computed_symbol_argument() {
        // STATIC extraction: a symbol computed through a call is not const-foldable
        // → defer, never an approximate fingerprint.
        let body = r#"{
            let host = make_host_with_footprint();
            let (expr, record) = resolve_expr(&host, "/f.ts", compute_symbol(), &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::NonConstArg("symbol"))
        );
    }

    #[test]
    fn rejects_a_const_arg_query_inside_a_loop() {
        // A query carrying a CONST symbol inside a loop must REJECT (the extractor
        // is CLOSED over control flow) — NOT be flattened into a top-level query.
        // A loop is rejected for the control-flow reason itself, not merely because
        // its arg happens to be non-const: a const arg must NOT slip through.
        let body = r#"{
            let host = make_host_with_footprint();
            for _ in 0..1 {
                let (expr, record) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
            }
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("loop"))
        );
    }

    #[test]
    fn rejects_a_const_arg_query_inside_a_closure() {
        // A query carrying a CONST symbol inside a closure must REJECT.
        let body = r#"{
            let host = make_host_with_footprint();
            let run = || {
                let (expr, record) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
            };
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("closure"))
        );
    }

    #[test]
    fn rejects_a_const_arg_query_inside_a_conditional() {
        // A query carrying a CONST symbol inside an `if` / `match` must REJECT —
        // the body did not unconditionally execute the flat query shape.
        let if_body = r#"{
            let host = make_host_with_footprint();
            if true {
                let (expr, record) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
            }
        }"#;
        let if_canonical = canonicalize_body(if_body).unwrap();
        assert_eq!(
            extract_fidelity(&if_canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("conditional"))
        );
        let match_body = r#"{
            let host = make_host_with_footprint();
            match 0 {
                _ => {
                    let (expr, record) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
                }
            }
        }"#;
        let match_canonical = canonicalize_body(match_body).unwrap();
        assert_eq!(
            extract_fidelity(&match_canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("conditional"))
        );
    }

    #[test]
    fn rejects_a_loop_computed_body_for_the_control_flow_reason() {
        // A non-const symbol built inside a loop REJECTS for the CONTROL-FLOW
        // reason (the loop is detected before the arg parse) — NOT the incidental
        // non-const-arg reason. The top-level computed-symbol case (above) still
        // defers via `NonConstArg`.
        let body = r#"{
            let host = make_host_with_footprint();
            for name in ["A", "B"] {
                let (expr, record) = resolve_expr(&host, "/f.ts", name, &[], ProjectionMode::Expanded);
            }
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("loop"))
        );
    }

    #[test]
    fn rejects_a_const_arg_query_inside_short_circuit_and() {
        // The RHS of `&&` is CONDITIONALLY executed (only when the LHS is true), so
        // a const-arg query reached through it must REJECT — it is not a top-level
        // straight-line executed query. The LHS is unconditional and not gated.
        let body = r#"{
            let host = make_host_with_footprint();
            let _ = false && { let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded); true };
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("short-circuit"))
        );
    }

    #[test]
    fn rejects_a_const_arg_query_inside_short_circuit_or() {
        // The RHS of `||` is CONDITIONALLY executed (only when the LHS is false).
        let body = r#"{
            let host = make_host_with_footprint();
            let _ = true || { let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded); false };
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("short-circuit"))
        );
    }

    #[test]
    fn rejects_a_const_arg_query_inside_an_async_block() {
        // `async { ... }` DEFERS execution — the body runs only when the future is
        // polled, never as part of the enclosing straight-line flow.
        let body = r#"{
            let host = make_host_with_footprint();
            let _fut = async { let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded); };
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("async"))
        );
    }

    #[test]
    fn rejects_a_const_arg_query_inside_a_try_operator() {
        // The `?` operator is a FALLIBLE early-transfer context: a query entangled
        // with `?` participates in an early return, so the body is not the simple
        // straight-line shape the fingerprint models.
        let body = r#"{
            let host = make_host_with_footprint();
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded)?;
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("try"))
        );
    }

    #[test]
    fn rejects_a_const_arg_query_inside_a_try_block() {
        // A `try { ... }` block is the same fallible early-transfer context.
        let body = r#"{
            let host = make_host_with_footprint();
            let _ = try { let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded); };
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("try"))
        );
    }

    #[test]
    fn rejects_a_const_arg_query_inside_a_let_else_diverge() {
        // The `else { ... }` diverge of a `let PAT = expr else { ... }` is
        // CONDITIONAL (it runs only when the refutable pattern fails to bind). A
        // query there must REJECT. The init expression stays the unconditional
        // straight-line query position (covered by the positive test below).
        let body = r#"{
            let host = make_host_with_footprint();
            let Some(_x) = maybe() else {
                let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
                panic!();
            };
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("conditional"))
        );
    }

    #[test]
    fn if_let_and_while_let_are_covered_by_the_conditional_and_loop_gates() {
        // `if let` lowers to `ExprIf` (gated "conditional"); `while let` lowers to
        // `ExprWhile` (gated "loop"). A query in either body REJECTS — this
        // confirms the brief's "verify they're covered" for the let-binding forms.
        let if_let = r#"{
            let host = make_host_with_footprint();
            if let Some(_x) = maybe() {
                let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
            }
        }"#;
        let if_let = canonicalize_body(if_let).unwrap();
        assert_eq!(
            extract_fidelity(&if_let, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("conditional"))
        );
        let while_let = r#"{
            let host = make_host_with_footprint();
            while let Some(_x) = next() {
                let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
            }
        }"#;
        let while_let = canonicalize_body(while_let).unwrap();
        assert_eq!(
            extract_fidelity(&while_let, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("loop"))
        );
    }

    #[test]
    fn admits_a_query_in_a_bare_nested_block() {
        // A bare `{ ... }` block executes its statements UNCONDITIONALLY and
        // straight-line — it neither defers nor conditionalizes. A query inside a
        // top-level-statement nested block is therefore a genuine straight-line
        // executed query and must be ADMITTED, NOT rejected. This mirrors the real
        // corpus row `utility_top_bottom.rs::Utb21NonNullableUnknown`, which scopes
        // its query as `let expr = { ...; let (e, r) = resolve_expr(...); e };`.
        // Gating bare blocks would over-reject it — a regression guard against that.
        let body = r#"{
            let expr = {
                let host = make_host_with_footprint();
                let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
                e
            };
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t =
            extract_fidelity(&canonical, "f.rs", "g", vec![]).expect("nested-block query admitted");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
        assert_eq!(t.queries[0].projection_mode, "Expanded");
    }

    #[test]
    fn admits_a_return_query_operand_with_no_dead_code() {
        // The OPERAND of `return <query>` is evaluated UNCONDITIONALLY, BEFORE the
        // control transfer — so a query there is a genuine straight-line executed
        // query and must be ADMITTED. Gating it would over-reject a valid row (the
        // codex ruling: `return`/`break`/`.await` OPERANDS are correct, not a hole).
        // No statement follows the transfer, so reachability never cuts. This
        // POSITIVE guard proves the reachability fix does not over-gate.
        let body = r#"{
            let host = make_host_with_footprint();
            return resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a return-operand query with no dead code is straight-line and must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
        assert_eq!(t.queries[0].projection_mode, "Expanded");
    }

    #[test]
    fn rejects_a_dead_query_after_an_unconditional_return() {
        // A query in a statement syntactically AFTER an unconditional `return;` is
        // UNREACHABLE (dead) — it never executes, so it must NOT be collected as a
        // top-level executed query. A naive walk that ignores reachability would
        // collect the dead query as straight-line; the walker's per-block reachability
        // cursor rejects it as `"unreachable"`.
        let body = r#"{
            let host = make_host_with_footprint();
            return;
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_an_unconditional_break() {
        // `break;` is the loop-exit analogue of `return;`: the statements after it
        // in the same block are dead. A query there must REJECT as `"unreachable"`.
        let body = r#"{
            let host = make_host_with_footprint();
            break;
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_an_unconditional_continue() {
        // `continue;` is the loop-restart analogue: statements after it in the same
        // block are dead. A query there must REJECT as `"unreachable"`.
        let body = r#"{
            let host = make_host_with_footprint();
            continue;
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_a_return_that_carried_an_operand() {
        // The transfer statement is visited FIRST (so its operand query, if any, is
        // admissible), THEN subsequent statements are unreachable. Here the transfer
        // carries a benign non-query operand, and the DEAD query after it must still
        // REJECT — i.e. the reachability cut survives a `return <expr>;` form, not
        // only a bare `return;`.
        let body = r#"{
            let host = make_host_with_footprint();
            return ();
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn a_conditional_return_does_not_cut_sibling_reachability() {
        // Only an UNCONDITIONAL transfer at STATEMENT position cuts reachability. A
        // `return` nested inside a top-level `if` is CONDITIONAL — it runs only when
        // the condition holds — so it does NOT make sibling top-level statements
        // unreachable. The query in the following top-level statement is still a
        // genuine straight-line executed query and must be ADMITTED. This guards the
        // reachability fix against over-rejecting (the precision rule from the brief).
        let body = r#"{
            let host = make_host_with_footprint();
            if false { return; }
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query after a CONDITIONAL (if-nested) return must still extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    // --- Complete recursive divergence analysis -------------------------------
    // Reachability is cut after ANY unconditionally-diverging statement, including
    // divergence reached through an unconditional WRAPPER — a bare block, a `let`
    // initializer, an exhaustively-diverging `if`/`match`, an infinite `loop` — at
    // every nesting depth (the `EvalReach` walker's recursive `Flow`). Each
    // `rejects_*` asserts a DEAD query after such a divergence REJECTS as
    // `"unreachable"`; each `admits_*` asserts a query after a construct that may
    // FALL THROUGH is NOT over-cut.

    #[test]
    fn rejects_a_dead_query_after_a_bare_block_that_diverges() {
        // A bare `{ … }` block whose statements unconditionally diverge transfers
        // control, so the following sibling is dead. The wrapper is an `Expr::Block`
        // statement, not a direct transfer, so divergence must propagate through it.
        let body = r#"{
            let host = make_host_with_footprint();
            { return; }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_a_let_init_that_diverges() {
        // A `let _ = { return; };` diverges through its INITIALIZER — control never
        // reaches the binding, so the following statement is dead. The statement is a
        // `Stmt::Local`, not a direct transfer, so divergence must propagate through it.
        let body = r#"{
            let host = make_host_with_footprint();
            let _ = { return; };
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_a_match_where_all_arms_diverge() {
        // A `match` whose EVERY arm body diverges always transfers control —
        // whichever arm runs, it diverges — so the following statement is dead.
        let body = r#"{
            let host = make_host_with_footprint();
            match 0 { _ => return }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn admits_a_query_after_a_match_with_a_live_arm() {
        // A `match` with at least ONE non-diverging arm may fall through, so the
        // following statement is REACHABLE — the query there is a genuine
        // straight-line executed query and must be ADMITTED (no over-cut).
        let body = r#"{
            let host = make_host_with_footprint();
            match 0 { 1 => return, _ => () }
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query after a match with a live arm must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn rejects_a_dead_query_after_an_if_else_where_both_arms_diverge() {
        // `if c { return } else { return }` diverges on BOTH arms → it always
        // transfers control → the following statement is dead.
        let body = r#"{
            let host = make_host_with_footprint();
            if true { return; } else { return; }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn admits_a_query_after_an_if_with_one_live_arm() {
        // An `if` with one NON-diverging arm may fall through, so the following
        // statement is reachable → ADMIT (the existing conditional-return test
        // covers the no-`else` form; this covers a present-but-live `else`).
        let body = r#"{
            let host = make_host_with_footprint();
            if true { return; } else { () }
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query after an if with a live else arm must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn rejects_a_dead_query_after_a_doubly_nested_diverging_block() {
        // Divergence propagates through MULTIPLE wrapper levels: `{ { return; } }`
        // is a bare block whose only statement is a bare block that diverges → the
        // outer block diverges → the following statement is dead. No nesting leaks.
        let body = r#"{
            let host = make_host_with_footprint();
            { { return; } }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_an_infinite_loop() {
        // A `loop { … }` with NO `break` that escapes it is a genuine infinite
        // (`!`-typed) loop → the following statement is dead. (`while`/`for` never
        // unconditionally diverge — their condition / iterator may be empty.)
        let body = r#"{
            let host = make_host_with_footprint();
            loop { do_work(); }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn admits_a_query_after_a_loop_with_an_escaping_break() {
        // A `loop { … break … }` whose `break` exits it terminates, so the
        // following statement is REACHABLE → ADMIT (no over-cut). The break is
        // unlabeled and not inside a nested loop, so it escapes THIS loop.
        let body = r#"{
            let host = make_host_with_footprint();
            loop { if done() { break; } }
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query after a loop with an escaping break must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn rejects_a_dead_query_after_a_loop_whose_only_break_targets_a_nested_loop() {
        // The `break` belongs to the INNER loop, so the OUTER loop never exits — it
        // is infinite and the following statement is dead. This discriminates the
        // break-escape analysis from a naive "any break token ⇒ terminates" check.
        let body = r#"{
            let host = make_host_with_footprint();
            loop { loop { break; } }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn admits_a_query_after_a_loop_exited_by_a_labeled_break_from_a_nested_loop() {
        // A labeled `break 'outer` from inside a nested loop DOES exit the outer
        // loop (labeled breaks pierce nesting), so the outer loop terminates and the
        // following statement is reachable → ADMIT. Discriminates label handling.
        let body = r#"{
            let host = make_host_with_footprint();
            'outer: loop { loop { break 'outer; } }
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query after a loop exited by a labeled break must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn rejects_a_dead_query_after_a_loop_whose_only_break_is_dead_code() {
        // The loop body diverges (`return`) BEFORE its `break`, so the `break` is
        // unreachable and never exits the loop — the loop is infinite and the
        // following statement is dead. The break-escape analysis is
        // reachability-aware: a `break` in dead code within the body does NOT count
        // as escaping (a purely syntactic "any break ⇒ terminates" check would wrongly
        // admit the dead query here).
        let body = r#"{
            let host = make_host_with_footprint();
            loop { return; break; }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_body_with_no_host_setup() {
        let body =
            r#"{ let (e, r) = resolve_expr(&host, "/f.ts", "X", &[], ProjectionMode::Expanded); }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::NoHostSetup)
        );
    }

    #[test]
    fn rejects_an_obligation_bearing_body() {
        let body = r#"{
            let host = make_host_with_footprint();
            let (expr, record) = resolve_expr(&host, "/f.ts", "X", &[], ProjectionMode::Expanded);
            assert_dependency_footprint(&record, &["/f.ts"]);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert!(matches!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::UnmodeledObligation(_))
        ));
    }

    #[test]
    fn evaluate_leading_binder_strips_typeof_and_takes_the_binder() {
        assert_eq!(evaluate_leading_binder("typeof f").unwrap(), "f");
        assert_eq!(evaluate_leading_binder("  typeof   obj").unwrap(), "obj");
        assert_eq!(evaluate_leading_binder("bare").unwrap(), "bare");
        // `typeofThing` is an identifier, not the `typeof` operator.
        assert_eq!(
            evaluate_leading_binder("typeofThing").unwrap(),
            "typeofThing"
        );
        // A non-identifier leading expression defers.
        assert_eq!(
            evaluate_leading_binder("123"),
            Err(ExtractError::NonConstArg("expression"))
        );
    }

    #[test]
    fn content_hash_matches_canonical_recipe() {
        // The content hash is `sha256` over canonicalized content — pinned to
        // `identity::content_hash`: line endings → LF, trailing newlines → one.
        assert!(content_hash("export type A = 1;").starts_with("sha256:"));
        // CRLF vs LF vs no-trailing-newline vs many-trailing-newlines all hash
        // identically (the cross-platform + trailing-newline normalization).
        let base = content_hash("a\nb\n");
        assert_eq!(content_hash("a\r\nb\r\n"), base);
        assert_eq!(content_hash("a\nb"), base);
        assert_eq!(content_hash("a\nb\n\n\n"), base);
        // Different content → different hash (discriminating).
        assert_ne!(content_hash("a\nb\n"), content_hash("a\nc\n"));
    }

    #[test]
    fn re_extraction_from_canonical_tokens_is_stable() {
        // `original_extraction_input_auditable`: re-canonicalizing the canonical
        // form is idempotent, and re-extraction yields the same fingerprint.
        let canonical = canonicalize_body(SAMPLE_BODY).unwrap();
        let again = canonicalize_body(&canonical).unwrap();
        assert_eq!(canonical, again, "canonicalization is idempotent");
        let f1 = fingerprint(
            &extract_fidelity(&canonical, "a.rs", "b", sample_workspace_files()).unwrap(),
        );
        let f2 =
            fingerprint(&extract_fidelity(&again, "a.rs", "b", sample_workspace_files()).unwrap());
        assert_eq!(f1, f2);
    }

    // --- General divergence -----------------------------------------------------
    // The `EvalReach` walker computes divergence uniformly: an expression diverges iff
    // any UNCONDITIONALLY-evaluated operand diverges, or the form itself cannot
    // complete normally. These tests cover the cases a per-form case-list would miss —
    // loop HEADERS, break VALUES, deep operand wrappers, label SHADOWING, and the
    // opaque macro barrier — handled as ONE consequence, plus the precision boundary
    // (no over-cut).

    #[test]
    fn rejects_a_dead_query_after_a_while_whose_condition_diverges() {
        // A `while` whose CONDITION diverges (`while ({ return; }) {}`) is itself
        // diverging: the general predicate evaluates the loop HEADER (condition)
        // unconditionally → the while diverges → the following query is dead → REJECT.
        // (The body is a boundary; only the header cuts.)
        let body = r#"{
            let host = make_host_with_footprint();
            while ({ return; }) {}
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_a_for_whose_iterator_diverges() {
        // The `for` ITERATOR expr evaluates unconditionally (the body is a boundary).
        // `for _ in ({ return; }) {}` diverges in its header → the following query is
        // dead → REJECT.
        let body = r#"{
            let host = make_host_with_footprint();
            for _ in ({ return; }) {}
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_a_loop_whose_only_break_value_diverges() {
        // `loop { break { return; }; }` — the break VALUE diverges, so the break never
        // transfers out of the loop (it diverges first). The binding-based escape
        // analysis evaluates the break value first and does not count a diverging-value
        // break as escaping → the loop is infinite → the following query is dead →
        // REJECT.
        let body = r#"{
            let host = make_host_with_footprint();
            loop { break { return; }; }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_divergence_in_a_deep_operand_wrapper() {
        // A `return` buried in ANY unconditionally-evaluated operand makes the
        // enclosing statement diverge — the general predicate recurses into call args,
        // array / tuple / struct elements, index base+index, the method receiver, and
        // binary operands. The dead query after each such form must REJECT (a per-form
        // case-list that did not recurse into operands would wrongly admit it).
        for (label, stmt) in [
            ("call-arg", "recv({ return; });"),
            ("array-elem", "let _ = [{ return; }];"),
            ("tuple-elem", "let _ = ({ return; },);"),
            ("struct-field", "let _ = Wrap { a: { return; } };"),
            ("index-index", "let _ = arr[{ return; }];"),
            ("method-receiver", "let _ = ({ return; }).foo();"),
            ("binary-operand", "let _ = ({ return; }) + 1;"),
        ] {
            let body = format!(
                "{{ let host = make_host_with_footprint(); {stmt} \
                 let _ = resolve_expr(&host, \"/f.ts\", \"ConstSym\", &[], ProjectionMode::Expanded); }}"
            );
            let canonical = canonicalize_body(&body).unwrap();
            assert_eq!(
                extract_fidelity(&canonical, "f.rs", "g", vec![]),
                Err(ExtractError::ControlFlowAroundQuery("unreachable")),
                "a dead query after divergence in the {label} operand must REJECT",
            );
        }
    }

    #[test]
    fn rejects_a_dead_query_after_a_label_shadowed_inner_break() {
        // `'outer: loop { 'outer: loop { break 'outer; } }` — the INNER `'outer` label
        // shadows the outer, so `break 'outer` targets the INNER loop and does NOT
        // escape the outer → the outer loop is infinite → the following query is dead →
        // REJECT. Label resolution is binding-based and shadow-aware: `break 'outer`
        // resolves to the NEAREST in-scope `'outer` frame (the inner loop), so it does
        // NOT escape the outer loop — a string-equality label match would wrongly
        // match it to the outer loop and ADMIT the dead query (the unsound case).
        let body = r#"{
            let host = make_host_with_footprint();
            'outer: loop { 'outer: loop { break 'outer; } }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_dead_query_after_a_macro_statement_barrier() {
        // §3: `syn::visit` cannot see inside macro tokens, so a macro STATEMENT that
        // might diverge (`panic!();`) is an OPAQUE control-flow barrier — it
        // conservatively cuts following reachability. The detection is STRUCTURAL (the
        // statement is a `Stmt::Macro`), NOT a name match on `panic!`. The following
        // dead query REJECTS as `"unreachable"`.
        let body = r#"{
            let host = make_host_with_footprint();
            panic!();
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn macro_barrier_is_structural_not_a_name_match() {
        // The barrier is structural, not `panic!`-specific: ANY macro statement —
        // `unreachable!()`, `todo!()`, or an arbitrary user macro — cuts the following
        // dead query identically. This discriminates the structural barrier from a
        // forbidden macro-NAME heuristic (a `panic!`/`unreachable!` text match).
        for mac in ["unreachable!()", "todo!()", "some_user_macro!(a, b)"] {
            let body = format!(
                "{{ let host = make_host_with_footprint(); {mac}; \
                 let _ = resolve_expr(&host, \"/f.ts\", \"ConstSym\", &[], ProjectionMode::Expanded); }}"
            );
            let canonical = canonicalize_body(&body).unwrap();
            assert_eq!(
                extract_fidelity(&canonical, "f.rs", "g", vec![]),
                Err(ExtractError::ControlFlowAroundQuery("unreachable")),
                "any macro statement ({mac}) must barrier following reachability",
            );
        }
    }

    // --- Precision: the general predicate must NOT over-cut --------------------

    #[test]
    fn admits_a_query_after_a_while_whose_condition_is_benign() {
        // A `while flag {}` whose CONDITION does not diverge may not run its body and
        // certainly falls through — the following query is reachable → ADMIT. The
        // loop-header rule is precise: only a DIVERGING header cuts, a benign one does
        // not (the converse of the diverging-condition reject above).
        let body = r#"{
            let host = make_host_with_footprint();
            while flag {}
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query after a benign while must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn admits_a_query_followed_by_a_benign_macro_assertion() {
        // The critical no-over-cut guard for §3: the corpus shape places result
        // assertions (`assert_eq!`, `assert_query_mode`) AFTER the query. The query is
        // collected at its reachable position FIRST, THEN the trailing macro barriers
        // the (irrelevant) remainder — extraction still succeeds with the query. A
        // macro barrier must never retroactively reject an already-collected query.
        let body = r#"{
            let host = make_host_with_footprint();
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
            assert_eq!(1, 1);
            assert_query_mode(&r, ProjectionModeTag::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query followed by a benign macro assertion must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
        assert_eq!(t.queries[0].projection_mode, "Expanded");
    }

    // --- Unified reachability: one walker for divergence + break-escape + queries ---
    // Divergence, break-escape, and query-collection share ONE evaluation-order
    // reachability walker, so a break reached only through a DEAD subtree is never
    // counted and a query reached only through a DEAD operand is rejected
    // `"unreachable"`. The two `rejects_a_query_after_a_loop_whose_only_break_is_*`
    // tests exercise break-escape's reachability (a dead break does not terminate the
    // loop); the dead-operand test exercises query-collection's operand-level
    // reachability; the match-guard tests exercise guard-before-body evaluation order.

    #[test]
    fn rejects_a_query_after_a_loop_whose_only_break_is_in_a_dead_if_branch() {
        // `loop { if ({ return; }) { break; } }` — the `if` CONDITION diverges, so the
        // then-branch holding the `break` is DEAD and never reached. The break never
        // escapes → the loop is infinite → the following query is dead → REJECT. The
        // unified walker reaches the break only through the dead branch, so the
        // break-escape consumer never counts it (the default `syn::visit` escape scan
        // walked the branch unconditionally and wrongly counted the dead break).
        let body = r#"{
            let host = make_host_with_footprint();
            loop { if ({ return; }) { break; } }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_query_after_a_loop_whose_only_break_is_in_a_dead_operand() {
        // `loop { f({ return; }, { break; }); }` — the FIRST call operand
        // (`{ return; }`) diverges, so the second operand (`{ break; }`) is a DEAD
        // position never reached. The break never escapes → the loop is infinite → the
        // following query is dead → REJECT. Break-escape uses the SAME operand-level
        // reachability the query walk does, so the dead break is never counted (the
        // default `syn::visit` scan descended into every operand and wrongly counted
        // it).
        let body = r#"{
            let host = make_host_with_footprint();
            loop { f({ return; }, { break; }); }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_query_in_a_dead_operand_after_a_diverging_operand() {
        // A query as a LATER call operand after an earlier operand diverges is in a
        // DEAD position — the earlier `{ return; }` operand transfers control before the
        // query operand is evaluated. The query must REJECT as `"unreachable"`, not be
        // collected as a live straight-line query. The unified walker cuts reachability
        // at OPERAND granularity; statement-level-only reachability would miss it and
        // wrongly collect the dead operand query.
        let body = r#"{
            let host = make_host_with_footprint();
            f({ return; }, resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded));
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    // --- The query call's OWN operands run before the helper is entered --------
    // Rust evaluates a call's operands (callee, then args left-to-right) BEFORE
    // control enters the callee. A recognized query helper therefore executes only
    // once every one of its OWN arguments has completed: if an argument diverges
    // first, the helper is never entered and the query never runs. The host argument
    // (args[0]) is the sharpest case — the identity parser ignores it entirely and
    // reads only args[1..] (path / symbol / type-args / mode) — so recording the
    // query from the identity args BEFORE confirming the call's operands complete
    // admits a query that never executes. The operand-first walk REJECTS it.

    #[test]
    fn rejects_a_query_whose_own_host_operand_diverges() {
        // `resolve_expr({ return; }, "/f.ts", "ConstSym", &[], Expanded)` — the host
        // argument diverges before `resolve_expr` is entered, so the query never runs.
        // Pre-fix the extractor recorded it from the identity args alone (which never
        // touch args[0]); the operand-first walk gates it `"unreachable"`.
        let body = r#"{
            let host = make_host_with_footprint();
            let _ = resolve_expr({ return; }, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn rejects_a_query_whose_own_operand_diverges_for_each_helper() {
        // The operand-ordering rule holds for ALL four modeled helpers: a divergence in
        // the host argument (the position the identity parser ignores) means the call is
        // never reached, so the query is dead → REJECT `"unreachable"`. Each `call`
        // below is the only query statement; pre-fix the extractor recorded it from the
        // identity args alone — the unsound admission this round closes.
        for (label, call) in [
            (
                "resolve_expr",
                "resolve_expr({ return; }, \"/f.ts\", \"ConstSym\", &[], ProjectionMode::Expanded)",
            ),
            (
                "resolve_with_mode",
                "resolve_with_mode({ return; }, \"/f.ts\", \"ConstSym\", ProjectionMode::Expanded)",
            ),
            (
                "shallow_surface_expr",
                "shallow_surface_expr({ return; }, \"/f.ts\", \"ConstSym\")",
            ),
            (
                "evaluate_expr",
                "evaluate_expr({ return; }, \"/f.ts\", \"foo\", ProjectionMode::Expanded)",
            ),
        ] {
            let body = format!("{{ let host = make_host_with_footprint(); let _ = {call}; }}");
            let canonical = canonicalize_body(&body).unwrap();
            assert_eq!(
                extract_fidelity(&canonical, "f.rs", "g", vec![]),
                Err(ExtractError::ControlFlowAroundQuery("unreachable")),
                "{label}: a diverging host operand makes the query unreachable → REJECT",
            );
        }
    }

    #[test]
    fn admits_a_query_with_benign_own_operands_for_each_helper() {
        // The converse must NOT over-reject: benign operands (a `&host` ref, string
        // literals, `&[]`, a mode path) do not diverge, so the call is reached and each
        // helper's query is recorded exactly once. This is the shape of every lifted
        // corpus row — the operand-first walk leaves it admissible.
        for (label, call, symbol) in [
            (
                "resolve_expr",
                "resolve_expr(&host, \"/f.ts\", \"ConstSym\", &[], ProjectionMode::Expanded)",
                "ConstSym",
            ),
            (
                "resolve_with_mode",
                "resolve_with_mode(&host, \"/f.ts\", \"ConstSym\", ProjectionMode::Expanded)",
                "ConstSym",
            ),
            (
                "shallow_surface_expr",
                "shallow_surface_expr(&host, \"/f.ts\", \"ConstSym\")",
                "ConstSym",
            ),
            (
                "evaluate_expr",
                "evaluate_expr(&host, \"/f.ts\", \"foo\", ProjectionMode::Expanded)",
                "foo",
            ),
        ] {
            let body = format!("{{ let host = make_host_with_footprint(); let _ = {call}; }}");
            let canonical = canonicalize_body(&body).unwrap();
            let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
                .unwrap_or_else(|e| panic!("{label}: a benign query must extract: {e:?}"));
            assert_eq!(t.queries.len(), 1, "{label}: exactly one query recorded");
            assert_eq!(t.queries[0].symbol_or_expression, symbol, "{label}");
        }
    }

    #[test]
    fn rejects_a_dead_query_after_a_match_whose_first_guard_diverges() {
        // A `match` whose first arm GUARD diverges (`_ if { return; } => …`) always
        // transfers control: the scrutinee is matched, then the guard runs and diverges
        // before any arm body. So the match diverges → the following query is dead →
        // REJECT. The unified walker evaluates each arm's guard before its body, so a
        // diverging guard cuts the following reachability (the arm-body-only divergence
        // rule missed it).
        let body = r#"{
            let host = make_host_with_footprint();
            match 0 { _ if { return; } => (), _ => () }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn admits_a_query_after_a_match_with_a_benign_guard() {
        // The match-guard divergence rule must NOT over-reject: a benign guard (`cond`)
        // does not diverge, and with non-diverging arm bodies the match may fall
        // through, so the following query is reachable → ADMIT. Guard-diverges is a
        // one-way (safe-direction) rule; a benign guard never cuts.
        let body = r#"{
            let host = make_host_with_footprint();
            match 0 { 1 if cond => (), _ => () }
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query after a match with a benign guard must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    // --- The three mandatory terminating discriminators ------------------------
    // The codex variant-table ruling names three structural holes a per-bug walker
    // missed. Each fixture below is RED against a walker that (1) walks a `let … else`
    // diverge arm LIVE when the initializer diverged, (2) treats an immediately-invoked
    // closure body as a deferred boundary, or (3) drops an `ExprBlock` label — and GREEN
    // against the table contract.

    #[test]
    fn let_else_dead_break_does_not_escape_an_outer_loop() {
        // `loop { let Some(_) = ({ return; }) else { break; }; }` — the let INITIALIZER
        // (`({ return; })`) diverges, so control never reaches the refutable bind or its
        // `else` arm: the else `break` is DEAD and must NOT escape the loop. With the
        // else walked under `dead || init.diverges()`, the loop has no reachable break →
        // it is infinite → the following query is dead → REJECT `"unreachable"`. A walk
        // that walked the else arm live would mark the loop escaped and wrongly ADMIT.
        // (A let-else initializer may not be a bare block ending in `}`, so the diverging
        // block is parenthesized — `({ return; })` — which the table treats identically.)
        let body = r#"{
            let host = make_host_with_footprint();
            loop { let Some(_) = ({ return; }) else { break; }; }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn an_iife_with_an_infinite_loop_body_kills_a_following_query() {
        // `(|| loop {})();` — a non-async closure invoked immediately. Its body is NOT
        // deferred: it runs as part of the straight-line flow. The body is an infinite
        // `loop {}` that never returns to the caller, so the call diverges → the
        // following query is dead → REJECT `"unreachable"`. A walk that treated the
        // closure callee as a deferred boundary would read the call as `MayComplete` and
        // wrongly ADMIT the dead query.
        let body = r#"{
            let host = make_host_with_footprint();
            (|| loop {})();
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn an_inner_labeled_block_shadows_a_same_name_loop_label() {
        // `'outer: loop { 'outer: { break 'outer; } }` — the inner `'outer:` LABELED
        // BLOCK shadows the outer `'outer:` loop. `break 'outer` resolves to the nearest
        // `'outer` frame (the block), exiting the block, NOT the loop. The loop has no
        // reachable break → it is infinite → the following query is dead → REJECT. A walk
        // that dropped the block label would resolve `break 'outer` to the outer loop and
        // wrongly ADMIT the dead query.
        let body = r#"{
            let host = make_host_with_footprint();
            'outer: loop { 'outer: { break 'outer; } }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    // --- Per-position coverage: every remaining unconditional operand ----------
    // The variant table classifies each operand child position as UNCONDITIONAL or
    // deferred/conditional. The earlier `…deep_operand_wrapper` test covers the call
    // arg / array / tuple / struct-field / index-index / method-receiver / ordinary
    // binary-left positions; this completes the table by proving divergence threads
    // through EVERY remaining unconditional operand position — the following query is
    // dead → REJECT `"unreachable"`. (A per-form case-list that failed to recurse into
    // any one of these would wrongly ADMIT.)
    #[test]
    fn rejects_a_dead_query_after_divergence_in_each_remaining_operand_position() {
        for (label, stmt) in [
            ("call-func", "({ return; })();"),
            ("methodcall-arg", "recv.m({ return; });"),
            ("struct-rest", "let _ = Foo { ..({ return; }) };"),
            ("index-base", "let _ = ({ return; })[0];"),
            ("range-start", "let _ = ({ return; })..1;"),
            ("range-end", "let _ = 0..({ return; });"),
            ("repeat-value", "let _ = [{ return; }; 4];"),
            ("assign-right", "x = { return; };"),
            ("unary-operand", "let _ = !({ return; });"),
            ("cast-operand", "let _ = ({ return; }) as u8;"),
            ("reference-operand", "let _ = &({ return; });"),
            ("raw-addr-operand", "let _ = &raw const ({ return; });"),
            ("field-base", "let _ = ({ return; }).field;"),
            ("await-base", "let _ = ({ return; }).await;"),
            ("let-scrutinee", "if let Some(_) = ({ return; }) {}"),
            ("paren-inner", "let _ = ({ return; });"),
            ("binary-right", "let _ = 1 + ({ return; });"),
        ] {
            let body = format!(
                "{{ let host = make_host_with_footprint(); {stmt} \
                 let _ = resolve_expr(&host, \"/f.ts\", \"ConstSym\", &[], ProjectionMode::Expanded); }}"
            );
            let canonical = canonicalize_body(&body).unwrap();
            assert_eq!(
                extract_fidelity(&canonical, "f.rs", "g", vec![]),
                Err(ExtractError::ControlFlowAroundQuery("unreachable")),
                "a dead query after divergence in the {label} position must REJECT",
            );
        }
    }

    // --- Closure-as-callee (IIFE) positive positions --------------------------
    // The mandatory `(|| loop {})()` discriminator proves an IIFE body diverges. These
    // prove the converse table contract: the IIFE body is an UNCONDITIONAL runtime
    // child (a query inside is admitted), a local `return` completes the call, and an
    // inner loop's break is scoped to the IIFE (not suppressed by the boundary floor).

    #[test]
    fn admits_a_query_in_an_iife_body() {
        // `(|| resolve_expr(…))();` — the closure body runs immediately as part of the
        // straight-line flow, so the query is a genuine executed query → ADMIT (the
        // table classifies the IIFE body as unconditional, not deferred).
        let body = r#"{
            let host = make_host_with_footprint();
            (|| resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded))();
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query in an IIFE body is unconditional and must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn an_iife_local_return_completes_the_call() {
        // `(|| { return; })();` — the `return` is LOCAL closure completion, so the call
        // returns to the caller and COMPLETES. The following query is reachable → ADMIT.
        // (A walk that treated the IIFE return as an outer transfer would kill it.)
        let body = r#"{
            let host = make_host_with_footprint();
            (|| { return; })();
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query after an IIFE whose body returns locally must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn an_iife_inner_loop_break_is_scoped_to_the_iife() {
        // `(|| { loop { break; } })();` — the inner loop's `break` targets THAT loop
        // (inside the IIFE), so the loop terminates, the body completes, and the call
        // completes → the following query is reachable → ADMIT. The boundary floor must
        // NOT suppress a break that targets a frame INSIDE the boundary (only one that
        // would cross it to an outer loop).
        let body = r#"{
            let host = make_host_with_footprint();
            (|| { loop { break; } })();
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("an IIFE whose inner loop breaks must complete and admit a following query");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn an_iife_with_a_diverging_argument_kills_a_following_query() {
        // `(|x| x)({ return; });` — the call ARGUMENT diverges before the body runs, so
        // the call diverges → the following query is dead → REJECT.
        let body = r#"{
            let host = make_host_with_footprint();
            (|x| x)({ return; });
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    // --- Labeled block (frame) positions --------------------------------------

    #[test]
    fn a_labeled_block_completes_via_its_own_break() {
        // `'b: { break 'b; }` — the `break 'b` exits the block normally, so the block
        // COMPLETES and the following query is reachable → ADMIT. (The block body
        // diverges straight-line via the break, but the break targets THIS block frame,
        // so the block completes.)
        let body = r#"{
            let host = make_host_with_footprint();
            'b: { break 'b; }
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a labeled block exited by its own break must complete and admit a query");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn a_labeled_block_with_no_escaping_break_propagates_divergence() {
        // `'b: { loop {} }` — the block body is an infinite loop with no `break 'b`, so
        // the block never falls through and never breaks → it diverges → the following
        // query is dead → REJECT. (A labeled block is not unconditionally `MayComplete`.)
        let body = r#"{
            let host = make_host_with_footprint();
            'b: { loop {} }
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    // --- Const block, nested item, awaited async body, compound assign, repeat --

    #[test]
    fn rejects_a_query_inside_a_const_block() {
        // A `const { … }` block is a const-eval boundary: a query inside is NOT a runtime
        // straight-line query → gated `"const"` → REJECT. (The table classifies the const
        // body as a deferred const-eval position.)
        let body = r#"{
            let host = make_host_with_footprint();
            const { let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded); };
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("const"))
        );
    }

    #[test]
    fn defers_when_the_only_query_is_inside_a_nested_item() {
        // A nested `fn` is a `Stmt::Item`: its body is a separate item, NOT descended for
        // queries. A body whose only query lives inside one has NO top-level query →
        // defer `NoQueryCall`. (A walk that descended into items would wrongly collect it.)
        let body = r#"{
            let host = make_host_with_footprint();
            fn inner() { let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded); }
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::NoQueryCall)
        );
    }

    #[test]
    fn awaiting_a_syntactic_async_block_forces_its_body_flow() {
        // `(async { loop {} }).await;` — awaiting a SYNTACTIC async block forces its body
        // to run as part of the await; an infinite body never completes → the await
        // diverges → the following query is dead → REJECT. (The non-awaited async block
        // stays deferred; only the `.await` forces it.)
        let body = r#"{
            let host = make_host_with_footprint();
            (async { loop {} }).await;
            let _ = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("unreachable"))
        );
    }

    #[test]
    fn awaiting_an_async_block_that_returns_completes() {
        // `(async { return; }).await;` — the `return` completes the async body, so the
        // future resolves and the await COMPLETES → the following query is reachable →
        // ADMIT. (The forced async body treats a `return` as local completion.)
        let body = r#"{
            let host = make_host_with_footprint();
            let _ = (async { return; }).await;
            let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query after awaiting an async body that returns must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn rejects_a_query_inside_an_awaited_async_block() {
        // A query inside an awaited async block is gated `"async"` (the body runs, but it
        // is not a runtime straight-line query) → REJECT.
        let body = r#"{
            let host = make_host_with_footprint();
            let _ = (async { let (e, r) = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded); }).await;
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("async"))
        );
    }

    #[test]
    fn rejects_a_query_in_a_compound_assignment_operand() {
        // `a += resolve_expr(…);` — a compound assignment's evaluation order is
        // type-dependent, so a query in either operand is not-guaranteed → gated
        // `"compound-assign"` → REJECT (the safe both-orders rule).
        let body = r#"{
            let host = make_host_with_footprint();
            let mut a = A;
            a += resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("compound-assign"))
        );
    }

    #[test]
    fn rejects_a_query_in_a_repeat_length() {
        // `[0; resolve_expr(…)]` — the repeat LENGTH is a const-eval position, not a
        // runtime straight-line query → gated `"const"` → REJECT. (The repeated VALUE is
        // unconditional; the earlier operand test covers a divergence there.)
        let body = r#"{
            let host = make_host_with_footprint();
            let _ = [0; resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded)];
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        assert_eq!(
            extract_fidelity(&canonical, "f.rs", "g", vec![]),
            Err(ExtractError::ControlFlowAroundQuery("const"))
        );
    }

    // --- Stmt child positions -------------------------------------------------

    #[test]
    fn admits_a_query_in_a_let_init_position() {
        // `LocalInit.expr` is the primary admissible straight-line position: a query as a
        // `let` initializer is collected. (The earlier let-init divergence test covers
        // the dead-after path; this is the live admit.)
        let body = r#"{
            let host = make_host_with_footprint();
            let _x = resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query in a let-init position must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    #[test]
    fn admits_a_query_in_an_expression_statement_position() {
        // `Stmt::Expr` (the query as a bare expression statement) is admissible.
        let body = r#"{
            let host = make_host_with_footprint();
            resolve_expr(&host, "/f.ts", "ConstSym", &[], ProjectionMode::Expanded);
        }"#;
        let canonical = canonicalize_body(body).unwrap();
        let t = extract_fidelity(&canonical, "f.rs", "g", vec![])
            .expect("a query in an expression-statement position must extract");
        assert_eq!(t.queries.len(), 1);
        assert_eq!(t.queries[0].symbol_or_expression, "ConstSym");
    }

    // --- The exhaustive `syn 2.0.117` variant guard ---------------------------
    // `syn::Expr` is `#[non_exhaustive]` and the `non_exhaustive_omitted_patterns` lint
    // that would make an omitted variant a COMPILE error is unstable on the pinned stable
    // toolchain — so the compile-time mechanism the syn docs suggest is unavailable here.
    // The guard is therefore TWO cooperating tests:
    //   (1) `expr_variant_table_covers_every_syn_variant` — a 1:1 classifier
    //       (`expr_variant_tag`, mirroring `walk_expr`'s arm list) plus a parse-driven
    //       corpus proving every modeled variant is individually recognized (never the
    //       `Unrecognized` wildcard);
    //   (2) `syn_version_is_pinned_for_the_expr_variant_table` — a Cargo.lock version pin
    //       that FAILS when `syn` 2.x drifts from the reviewed pin, forcing a re-audit of
    //       the table + `walk_expr` + classifier + corpus (the only way a NEW Expr variant
    //       can arrive is a syn bump).
    // `syn::Stmt` is NOT `#[non_exhaustive]`, so `walk_stmt` (and `stmt_variant_tag`) are
    // exhaustive with NO wildcard — a new Stmt variant is a COMPILE error directly.

    /// The reviewed `syn` 2.x version the Expr variant table was audited against. Bump
    /// ONLY after re-auditing `syn::Expr`'s variants against the table in this file.
    const REVIEWED_SYN_VERSION: &str = "2.0.117";
    /// The number of `syn::Expr` variants modeled (excluding the `Unrecognized` mirror).
    const KNOWN_EXPR_VARIANT_COUNT: usize = 40;

    /// A 1:1 tag for every modeled `syn::Expr` variant, plus `Unrecognized` for a future
    /// `#[non_exhaustive]` variant. `expr_variant_tag`'s match MUST mirror `walk_expr`'s
    /// arm list — the coverage test fails if any modeled variant slips to `Unrecognized`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum ExprVariantTag {
        Array,
        Assign,
        Async,
        Await,
        Binary,
        Block,
        Break,
        Call,
        Cast,
        Closure,
        Const,
        Continue,
        Field,
        ForLoop,
        Group,
        If,
        Index,
        Infer,
        Let,
        Lit,
        Loop,
        Macro,
        Match,
        MethodCall,
        Paren,
        Path,
        Range,
        RawAddr,
        Reference,
        Repeat,
        Return,
        Struct,
        Try,
        TryBlock,
        Tuple,
        Unary,
        Unsafe,
        Verbatim,
        While,
        Yield,
        Unrecognized,
    }

    /// All modeled tags (no `Unrecognized`). Asserted to have `KNOWN_EXPR_VARIANT_COUNT`
    /// distinct entries; the corpus must cover all of them except the two proc-macro-only
    /// variants (`Group`, `Verbatim`).
    const ALL_EXPR_VARIANT_TAGS: &[ExprVariantTag] = &[
        ExprVariantTag::Array,
        ExprVariantTag::Assign,
        ExprVariantTag::Async,
        ExprVariantTag::Await,
        ExprVariantTag::Binary,
        ExprVariantTag::Block,
        ExprVariantTag::Break,
        ExprVariantTag::Call,
        ExprVariantTag::Cast,
        ExprVariantTag::Closure,
        ExprVariantTag::Const,
        ExprVariantTag::Continue,
        ExprVariantTag::Field,
        ExprVariantTag::ForLoop,
        ExprVariantTag::Group,
        ExprVariantTag::If,
        ExprVariantTag::Index,
        ExprVariantTag::Infer,
        ExprVariantTag::Let,
        ExprVariantTag::Lit,
        ExprVariantTag::Loop,
        ExprVariantTag::Macro,
        ExprVariantTag::Match,
        ExprVariantTag::MethodCall,
        ExprVariantTag::Paren,
        ExprVariantTag::Path,
        ExprVariantTag::Range,
        ExprVariantTag::RawAddr,
        ExprVariantTag::Reference,
        ExprVariantTag::Repeat,
        ExprVariantTag::Return,
        ExprVariantTag::Struct,
        ExprVariantTag::Try,
        ExprVariantTag::TryBlock,
        ExprVariantTag::Tuple,
        ExprVariantTag::Unary,
        ExprVariantTag::Unsafe,
        ExprVariantTag::Verbatim,
        ExprVariantTag::While,
        ExprVariantTag::Yield,
    ];

    /// Classify a `syn::Expr` by variant. The arm list mirrors `walk_expr`; the wildcard
    /// maps a future non_exhaustive variant to `Unrecognized` (the coverage test proves
    /// no modeled variant lands there on the pinned syn).
    fn expr_variant_tag(expr: &Expr) -> ExprVariantTag {
        match expr {
            Expr::Array(_) => ExprVariantTag::Array,
            Expr::Assign(_) => ExprVariantTag::Assign,
            Expr::Async(_) => ExprVariantTag::Async,
            Expr::Await(_) => ExprVariantTag::Await,
            Expr::Binary(_) => ExprVariantTag::Binary,
            Expr::Block(_) => ExprVariantTag::Block,
            Expr::Break(_) => ExprVariantTag::Break,
            Expr::Call(_) => ExprVariantTag::Call,
            Expr::Cast(_) => ExprVariantTag::Cast,
            Expr::Closure(_) => ExprVariantTag::Closure,
            Expr::Const(_) => ExprVariantTag::Const,
            Expr::Continue(_) => ExprVariantTag::Continue,
            Expr::Field(_) => ExprVariantTag::Field,
            Expr::ForLoop(_) => ExprVariantTag::ForLoop,
            Expr::Group(_) => ExprVariantTag::Group,
            Expr::If(_) => ExprVariantTag::If,
            Expr::Index(_) => ExprVariantTag::Index,
            Expr::Infer(_) => ExprVariantTag::Infer,
            Expr::Let(_) => ExprVariantTag::Let,
            Expr::Lit(_) => ExprVariantTag::Lit,
            Expr::Loop(_) => ExprVariantTag::Loop,
            Expr::Macro(_) => ExprVariantTag::Macro,
            Expr::Match(_) => ExprVariantTag::Match,
            Expr::MethodCall(_) => ExprVariantTag::MethodCall,
            Expr::Paren(_) => ExprVariantTag::Paren,
            Expr::Path(_) => ExprVariantTag::Path,
            Expr::Range(_) => ExprVariantTag::Range,
            Expr::RawAddr(_) => ExprVariantTag::RawAddr,
            Expr::Reference(_) => ExprVariantTag::Reference,
            Expr::Repeat(_) => ExprVariantTag::Repeat,
            Expr::Return(_) => ExprVariantTag::Return,
            Expr::Struct(_) => ExprVariantTag::Struct,
            Expr::Try(_) => ExprVariantTag::Try,
            Expr::TryBlock(_) => ExprVariantTag::TryBlock,
            Expr::Tuple(_) => ExprVariantTag::Tuple,
            Expr::Unary(_) => ExprVariantTag::Unary,
            Expr::Unsafe(_) => ExprVariantTag::Unsafe,
            Expr::Verbatim(_) => ExprVariantTag::Verbatim,
            Expr::While(_) => ExprVariantTag::While,
            Expr::Yield(_) => ExprVariantTag::Yield,
            _ => ExprVariantTag::Unrecognized,
        }
    }

    #[test]
    fn expr_variant_table_covers_every_syn_variant() {
        use std::collections::HashSet;
        // `ALL_EXPR_VARIANT_TAGS` is the full modeled set: distinct, of the pinned size.
        let all: HashSet<ExprVariantTag> = ALL_EXPR_VARIANT_TAGS.iter().copied().collect();
        assert_eq!(
            all.len(),
            KNOWN_EXPR_VARIANT_COUNT,
            "ALL_EXPR_VARIANT_TAGS must list {KNOWN_EXPR_VARIANT_COUNT} distinct tags",
        );
        assert!(!all.contains(&ExprVariantTag::Unrecognized));

        // One parseable snippet per modeled variant. `Group` (invisible-delimiter group)
        // and `Verbatim` (uninterpreted tokens) arise ONLY from proc-macro token streams,
        // never from `syn::parse_str`, so they are covered by explicit `walk_expr` /
        // classifier arms + the version pin, but cannot appear in this parse-driven corpus.
        let corpus: &[(ExprVariantTag, &str)] = &[
            (ExprVariantTag::Array, "[1, 2]"),
            (ExprVariantTag::Assign, "a = b"),
            (ExprVariantTag::Async, "async { 1 }"),
            (ExprVariantTag::Await, "f.await"),
            (ExprVariantTag::Binary, "a + b"),
            (ExprVariantTag::Block, "{ 1 }"),
            (ExprVariantTag::Break, "break"),
            (ExprVariantTag::Call, "f(1)"),
            (ExprVariantTag::Cast, "x as u8"),
            (ExprVariantTag::Closure, "|| 1"),
            (ExprVariantTag::Const, "const { 1 }"),
            (ExprVariantTag::Continue, "continue"),
            (ExprVariantTag::Field, "a.b"),
            (ExprVariantTag::ForLoop, "for _ in x {}"),
            (ExprVariantTag::If, "if c {}"),
            (ExprVariantTag::Index, "a[0]"),
            (ExprVariantTag::Infer, "_"),
            (ExprVariantTag::Let, "let Some(x) = y"),
            (ExprVariantTag::Lit, "1"),
            (ExprVariantTag::Loop, "loop {}"),
            (ExprVariantTag::Macro, "vec![1]"),
            (ExprVariantTag::Match, "match x { _ => 1 }"),
            (ExprVariantTag::MethodCall, "a.b()"),
            (ExprVariantTag::Paren, "(1)"),
            (ExprVariantTag::Path, "a::b"),
            (ExprVariantTag::Range, "1..2"),
            (ExprVariantTag::RawAddr, "&raw const x"),
            (ExprVariantTag::Reference, "&x"),
            (ExprVariantTag::Repeat, "[0; 4]"),
            (ExprVariantTag::Return, "return"),
            (ExprVariantTag::Struct, "Foo { a: 1 }"),
            (ExprVariantTag::Try, "x?"),
            (ExprVariantTag::TryBlock, "try { 1 }"),
            (ExprVariantTag::Tuple, "(1, 2)"),
            (ExprVariantTag::Unary, "!x"),
            (ExprVariantTag::Unsafe, "unsafe { 1 }"),
            (ExprVariantTag::While, "while c {}"),
            (ExprVariantTag::Yield, "yield 1"),
        ];
        let unproducible = [ExprVariantTag::Group, ExprVariantTag::Verbatim];

        let mut covered: HashSet<ExprVariantTag> = HashSet::new();
        for (want, src) in corpus {
            let parsed: Expr =
                syn::parse_str(src).unwrap_or_else(|e| panic!("corpus snippet {src:?}: {e}"));
            let got = expr_variant_tag(&parsed);
            assert_ne!(
                got,
                ExprVariantTag::Unrecognized,
                "snippet {src:?} hit the Unrecognized wildcard — walk_expr / expr_variant_tag \
                 is missing an arm for this variant",
            );
            assert_eq!(
                got, *want,
                "snippet {src:?} classified as {got:?}, expected {want:?}",
            );
            assert!(covered.insert(*want), "duplicate corpus tag {want:?}");
        }

        // The corpus covers exactly the parse-producible modeled variants: all but the
        // two proc-macro-only variants. A NEW modeled variant with no corpus snippet (or
        // a snippet that no longer classifies) fails one of the assertions above; a NEW
        // syn variant trips the version pin. Either way the table must be updated.
        for tag in ALL_EXPR_VARIANT_TAGS {
            if unproducible.contains(tag) {
                assert!(
                    !covered.contains(tag),
                    "{tag:?} is documented proc-macro-only but a corpus snippet produced it",
                );
            } else {
                assert!(
                    covered.contains(tag),
                    "modeled variant {tag:?} has no corpus snippet — add a discriminating one",
                );
            }
        }
        assert_eq!(covered.len(), KNOWN_EXPR_VARIANT_COUNT - unproducible.len());
    }

    /// Every `syn::Stmt` variant — exhaustive with NO wildcard, so a new Stmt variant is a
    /// COMPILE error (the `syn::Stmt`-is-not-non_exhaustive mechanism), mirroring
    /// `walk_stmt`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum StmtVariantTag {
        Local,
        Item,
        Expr,
        Macro,
    }

    fn stmt_variant_tag(stmt: &Stmt) -> StmtVariantTag {
        match stmt {
            Stmt::Local(_) => StmtVariantTag::Local,
            Stmt::Item(_) => StmtVariantTag::Item,
            Stmt::Expr(_, _) => StmtVariantTag::Expr,
            Stmt::Macro(_) => StmtVariantTag::Macro,
        }
    }

    #[test]
    fn stmt_variant_table_covers_every_syn_variant() {
        use std::collections::HashSet;
        let corpus: &[(StmtVariantTag, &str)] = &[
            (StmtVariantTag::Local, "let x = 1;"),
            (StmtVariantTag::Item, "fn f() {}"),
            (StmtVariantTag::Expr, "1"),
            (StmtVariantTag::Macro, "m!();"),
        ];
        let mut covered: HashSet<StmtVariantTag> = HashSet::new();
        for (want, src) in corpus {
            let block: syn::Block = syn::parse_str(&format!("{{ {src} }}"))
                .unwrap_or_else(|e| panic!("stmt snippet {src:?}: {e}"));
            let stmt = block.stmts.last().expect("one statement");
            assert_eq!(stmt_variant_tag(stmt), *want, "snippet {src:?}");
            covered.insert(*want);
        }
        // `walk_stmt` and `stmt_variant_tag` are exhaustive over all four `Stmt` variants
        // with no wildcard; this corpus confirms each is reachable and distinct.
        assert_eq!(covered.len(), 4);
    }

    #[test]
    fn syn_version_is_pinned_for_the_expr_variant_table() {
        // The Expr variant table + `walk_expr` + classifier + corpus are audited against
        // an EXACT syn version. Because `syn::Expr` is `#[non_exhaustive]` and the
        // `non_exhaustive_omitted_patterns` lint is unstable on the pinned stable
        // toolchain, a new Expr variant cannot be caught at compile time — this guard
        // catches it instead: it FAILS when the resolved `syn` 2.x version drifts.
        let lock = read_workspace_cargo_lock();
        let resolved = resolved_syn_2x_version(&lock);
        assert_eq!(
            resolved, REVIEWED_SYN_VERSION,
            "syn 2.x resolved to {resolved} but the Expr variant table was audited against \
             {REVIEWED_SYN_VERSION}. Re-audit syn::Expr's variants against the per-variant \
             arms in walk_expr (the variant-table contract) plus expr_variant_tag and the \
             coverage corpus; if a variant was added or removed, update walk_expr, the \
             classifier, ALL_EXPR_VARIANT_TAGS, KNOWN_EXPR_VARIANT_COUNT, and a discriminating \
             test, then set REVIEWED_SYN_VERSION to the new version.",
        );
    }

    /// Read the workspace `Cargo.lock` by walking up from this crate's manifest dir.
    fn read_workspace_cargo_lock() -> String {
        let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        loop {
            let candidate = dir.join("Cargo.lock");
            if candidate.is_file() {
                return std::fs::read_to_string(&candidate)
                    .unwrap_or_else(|e| panic!("read {}: {e}", candidate.display()));
            }
            assert!(
                dir.pop(),
                "no Cargo.lock found above {}",
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    }

    /// The single resolved `syn` 2.x version in a Cargo.lock (the workspace also pins a
    /// distinct `syn 1.x` for legacy macro deps; this picks the 2.x line).
    fn resolved_syn_2x_version(lock: &str) -> String {
        let mut in_syn = false;
        let mut versions: Vec<String> = Vec::new();
        for line in lock.lines() {
            let line = line.trim();
            if line == "[[package]]" {
                in_syn = false;
            } else if line == "name = \"syn\"" {
                in_syn = true;
            } else if in_syn {
                if let Some(rest) = line.strip_prefix("version = \"") {
                    if let Some(v) = rest.strip_suffix('"') {
                        versions.push(v.to_string());
                        in_syn = false;
                    }
                }
            }
        }
        let two_x: Vec<String> = versions
            .into_iter()
            .filter(|v| v.starts_with("2."))
            .collect();
        assert_eq!(
            two_x.len(),
            1,
            "expected exactly one syn 2.x package in Cargo.lock, found {two_x:?}",
        );
        two_x.into_iter().next().unwrap()
    }
}
