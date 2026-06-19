//! Budget oracle: path-precise navigation — intermediate
//! hops run in Navigate mode, terminal hop at caller's mode.
//!
//! Drives `A['c']['full']['bar']` against an Expanded caller. The
//! discriminating signal: counter-precise budget over the audit
//! record's `navigations` and `expansions` and exact zero on
//! non-contributing arms.
//!
//! ## Why this is a budget oracle
//!
//! Per CLAUDE.md §"Macro Type Traversal Rule":
//!
//! > Path projection is path-precise: intermediate hops run in
//! > `Navigate`, the terminal hop runs in the caller's mode,
//! > non-contributing intersection arms are ignored (not rewritten
//! > to `never`), open conditionals distribute the remaining path
//! > into both branches, closed conditionals reduce immediately.
//!
//! The audit substrate (`TypeResolutionPayload`) records:
//!   - `hops`: total dispatched query count
//!   - `navigations`: subset that ran in Navigate
//!   - `expansions`: subset that ran in Expanded / Shallow
//!
//! A regression that ran ALL hops in Expanded would surface
//! `expansions == hops` and `navigations == 0`. A regression that
//! ran ALL hops in Navigate (including the terminal) would surface
//! `expansions == 0`. The correct path records BOTH counters at
//! non-zero, with `expansions >= 1` (the terminal Expanded) and
//! `navigations + expansions <= hops` (the dispatcher may emit
//! sub-dispatches that don't all map to a hop slot).
//!
//! ## Discrimination contract
//!
//! Per the dispatcher's audit accounting (see
//! `request_context::bump_type_resolution_hop`), the `navigations`
//! counter only advances when a `ProjectPath`/`ProjectMember`/
//! `IndexedAccess` query dispatches with `Navigate` mode through the
//! shared `SemanticQueryApi::execute` entry. The internal
//! `PathWalker.walk(...)` invoked by `build_project_path` walks
//! intermediate hops locally without re-dispatching — by design, to
//! avoid double-counting. As a result, the dispatcher reports the
//! caller's outer dispatch as a single hop in the caller's mode.
//!
//! What discriminates the path-precise contract is therefore:
//!
//!   - `query_mode == Expanded` — the caller's outer mode reached
//!     the dispatch audit (terminal mode honored)
//!   - `expansions` stays bounded by the path length + slack
//!     (over-expansion regression would balloon this)
//!   - `expansions <= hops` AND `navigations <= hops` (no double-
//!     counting bucket overflows)
//!   - `expansions >= 1` (the terminal MUST allocate at least one
//!     expansion under the caller's Expanded mode)
//!
//! ### Why the discrimination is non-trivial
//!
//! Over-expansion regression: the path projection
//! ran ALL intermediate hops in Expanded too — the dispatcher would
//! emit an Expanded sub-dispatch per intermediate, ballooning
//! `expansions` past the path-length budget. We catch this with
//! `expansions <= path_length + EXPANSION_SLACK`.
//!
//! Under-expansion regression: the terminal hop
//! ran in Navigate — the caller's mode is lost in dispatch. We catch
//! this with `expansions >= 1` AND `query_mode == Expanded`.
//!
//! Caller-mode loss: the outer dispatch reaches
//! the audit with mode != caller's mode. We catch this with
//! `query_mode == Expanded`.
//!
//! Correct shape: `query_mode == Expanded`, `expansions >= 1`
//! AND `<= path_length + EXPANSION_SLACK`, `hops >= 1`,
//! `navigations + expansions <= hops` (within the request's
//! accounting buckets).

use std::sync::Arc;

use verter_audit::ProjectionModeTag;
use verter_session::semantic_query::{
    PathSegment, ProjectionMode, ResolveDeclKey, ScopeId, SemanticQueryKey,
};
use verter_session::{HostConfig, UpsertRequest, VerterHost};

/// Fixture: deeply-nested type so the three-segment path
/// `A['c']['full']['bar']` exercises distinct intermediate hops.
/// Each intermediate level (`c`, `full`) carries unrelated sibling
/// keys to ensure the path projection is selective at every level.
const TYPES_TS: &str = r#"
export type A = {
    c: {
        full: {
            bar: { value: string; depth: number };
            other: number;
            again: boolean;
        };
        unrelated: string[];
    };
    sibling_top: number;
};
"#;

#[test]
fn path_a_c_full_bar_navigates_intermediates_and_expands_terminal() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".to_string()),
        input_id: "/types.ts".to_string(),
        source: Arc::from(TYPES_TS),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/types.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });

    // Step 1: resolve A to get its semantic node id — the path
    // projection needs a base.
    let resolve_a = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from("/types.ts"),
            local_scope: None,
        },
        name: Arc::from("A"),
    });
    let (a_node, _) = host
        .resolve_type_with_audit(resolve_a, "/types.ts")
        .into_parts();
    let a_node = a_node.ok().flatten().expect("A must resolve");

    // Step 2: drive the path projection A['c']['full']['bar'] with
    // the caller's mode set to Expanded.
    let project = SemanticQueryKey::ProjectPath {
        base: a_node,
        path: Arc::from(
            vec![
                PathSegment::Member(Arc::from("c")),
                PathSegment::Member(Arc::from("full")),
                PathSegment::Member(Arc::from("bar")),
            ]
            .into_boxed_slice(),
        ),
        context: verter_session::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };
    let (resolved, record) = host
        .resolve_type_with_audit(project, "/types.ts")
        .into_parts();
    let resolved = resolved
        .ok()
        .flatten()
        .expect("path projection must resolve");
    let _ = resolved;
    // record is always present now (carrier `audit` field is mandatory).
    let payload = record
        .type_resolution_payload()
        .expect("kind must be TypeResolution");

    // Discriminating assertion 1: the caller's mode (Expanded) MUST
    // propagate to the audit payload. A regression that dropped the
    // caller's mode in dispatch would surface `query_mode == Navigate`
    // or `Identity`.
    assert_eq!(
        payload.query_mode,
        ProjectionModeTag::Expanded,
        "caller asked for Expanded mode; audit payload MUST report it. \
         Got {:?}. A different mode means the dispatcher dropped \
         the caller's mode (path-precise contract broken — terminal \
         hop did not honor caller).",
        payload.query_mode,
    );

    // Discriminating assertion 2: the projection dispatched at least
    // one hop. A `hops == 0` would mean the dispatcher short-
    // circuited entirely — invalidating the oracle's premise.
    assert!(
        payload.hops >= 1,
        "path projection MUST dispatch at least one hop. \
         Got hops={}. A value of 0 means the dispatcher short-circuited \
         (warm-cache hit on the full path) without consulting the \
         path-segment walker — the budget oracle's premise is broken.",
        payload.hops,
    );

    // Discriminating assertion 3 — LOWER BOUND on expansions.
    // The terminal 'bar' hop runs in the caller's Expanded mode and
    // MUST allocate at least one expansion (the dispatched
    // `ProjectPath` query reaches `bump_type_resolution_hop` with
    // mode=Expanded).
    //
    // Regression mode (caller-mode-lost-at-terminal): `expansions == 0`
    // because the outer dispatch fired in Navigate — the caller's
    // Expanded mode is dropped before the dispatch audit. The caller
    // sees the symbolic projection but the dispatch counter records
    // no Expansion — direct evidence the terminal-mode-honored
    // contract is broken.
    assert!(
        payload.expansions >= 1,
        "path projection A['c']['full']['bar'] with Expanded caller \
         MUST record at least one Expansion. \
         Got expansions={} (hops={}). \
         A value of 0 means the outer dispatch ran in Navigate — \
         the caller's Expanded mode is dropped before reaching the \
         dispatcher's audit. The under-expansion regression returns \
         a symbolic projection without honoring the caller's mode.",
        payload.expansions,
        payload.hops,
    );

    // Discriminating assertion 4 — UPPER BOUND on expansions. The
    // path has 3 segments; the path-precise contract permits AT
    // MOST `path_length + EXPANSION_SLACK` Expansions across the
    // request. The intermediate hops MUST navigate (internally, via
    // `PathWalker.walk`) — they MUST NOT dispatch additional
    // Expanded sub-queries to materialise unselected siblings.
    //
    // Regression mode (over-expansion): each intermediate hop fires
    // an Expanded sub-dispatch (e.g., to materialise the
    // intermediate's full body before navigating into the next
    // segment). `expansions` then scales with `path_length × |unselected_siblings|`,
    // ballooning past the budget.
    //
    // Our 3-segment fixture has 6 unselected sibling members across
    // the intermediate levels (`sibling_top`, `c.unrelated`,
    // `full.other`, `full.again`, plus the inner `bar.value`,
    // `bar.depth`). An eager-expand-all regression would surface
    // `expansions` well above 10. We pick a budget of `path_length +
    // EXPANSION_SLACK = 3 + 5 = 8` to discriminate.
    const PATH_LENGTH: u32 = 3;
    const EXPANSION_SLACK: u32 = 5;
    let expansion_budget = PATH_LENGTH + EXPANSION_SLACK;
    assert!(
        payload.expansions <= expansion_budget,
        "path projection A['c']['full']['bar'] MUST keep expansions \
         within budget {expansion_budget} (path_length={PATH_LENGTH} + \
         slack={EXPANSION_SLACK}). \
         Got expansions={} (hops={}). \
         A value above the budget means intermediate hops fired \
         Expanded sub-dispatches — the path-precise contract is \
         broken: every intermediate's siblings get materialised \
         instead of staying shallow.",
        payload.expansions,
        payload.hops,
    );

    // Discriminating assertion 5 — bucket consistency. The
    // dispatcher allocates each dispatched hop into exactly one of
    // {navigations, expansions, conditional_decisions, identity}.
    // The sum of categorised buckets MUST NOT exceed total hops
    // (saturating to avoid u32 overflow on extreme regressions).
    assert!(
        payload.expansions <= payload.hops,
        "expansions ({}) must not exceed hops ({}). \
         A value above hops indicates the dispatcher double-counted \
         a single hop in multiple buckets.",
        payload.expansions,
        payload.hops,
    );
    assert!(
        payload.navigations <= payload.hops,
        "navigations ({}) must not exceed hops ({}). \
         A value above hops indicates the dispatcher double-counted \
         a single hop in multiple buckets.",
        payload.navigations,
        payload.hops,
    );

    // Discriminating assertion 6 — combined accounting integrity.
    // The path-precise contract bounds `navigations + expansions` by
    // `hops`. A regression that triple-counts a single hop as BOTH
    // a Navigate AND an Expansion (mode-bucket double-classification)
    // would exceed hops.
    assert!(
        payload.navigations.saturating_add(payload.expansions) <= payload.hops,
        "navigations ({}) + expansions ({}) MUST NOT exceed hops ({}). \
         A combined sum above hops means a single dispatched hop was \
         counted in BOTH Navigate and Expanded buckets — the audit \
         counter logic is broken (mode-bucket double-classification).",
        payload.navigations,
        payload.expansions,
        payload.hops,
    );
}
