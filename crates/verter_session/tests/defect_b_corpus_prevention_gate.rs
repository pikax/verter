//! Open-generic expansion-storm real-corpus acceptance gate.
//!
//! The runaway fuse is ARMED by default (`projection_op_budget == 0` ⇒
//! effective cap 2000): it is the genuine termination backstop for the
//! open-generic expansion-storm class. The invariant this gate guards
//! is the partial-taint SCOPING: a budget-tripped partial in
//! one consumer must NOT poison a genuinely-COMPLETE sibling's warm entry
//! through a request-wide sticky suppress. A request-wide sticky would
//! collapse sibling warm reuse repo-wide; instead each cold compute carries
//! its OWN completeness so complete siblings warm cleanly.
//!
//! It asserts on the REAL corpus via the actual host (the same path the
//! `packages/benchmark` `bench:meta:ui` harness drives), NOT a hermetic
//! fixture. Four assertions, split across two component sets:
//!
//! - #1 Per-component no-timeout — every component in
//!   `NO_TIMEOUT_MANDATORY_COMPONENTS` resolves within a hard per-component
//!   budget (a watchdog thread aborts the run if any component hangs).
//!   `Table.vue` is here: it trips the armed fuse and resolves
//!   degraded-but-terminating (~5s), which #1 requires.
//! - #2 Warm-cache non-regression — the warm (2nd) pass COLLAPSES the
//!   audited per-request `RequestContext.cold_builds` by ≥90% for every
//!   GENUINELY-COMPLETE component (`COMPLETE_MANDATORY_COMPONENTS`),
//!   measured on the same `cold_builds` axis the `bench:meta:ui` accounts
//!   and the Defect-B bisect use (Table 0→5101) — NOT the
//!   `ComponentMetaResultDb` hit counter, which is structurally always 0 for
//!   these components on both this branch and af35 (an unsatisfiable
//!   oracle). This is the direct points-3-6 witness: a complete sibling
//!   warms despite the open-generic siblings tripping the fuse on the same
//!   host.
//! - #3 Perf-budget regression — first-pass aggregate elapsed vs a
//!   COMMITTED post-fix baseline, ~15% threshold.
//! - #4 No `BudgetExceeded` on a `demand: Published` key — no
//!   GENUINELY-COMPLETE component (`COMPLETE_MANDATORY_COMPONENTS`) carries
//!   a budget-tripped partial (`synthesis_should_suppress` OR a leaked
//!   sentinel) on its published surface.
//!
//! `Table.vue` is DEFERRED from #2/#4: under the armed fuse it is
//! degraded-but-terminating and legitimately carries
//! `Partial(BUDGET_EXCEEDED)` on its published surface until the
//! route/mode-INDEPENDENT L1 open-domain carrier-stop lands as a
//! tracked structural follow-up — see the `TODO(structural-block)`
//! on `COMPLETE_MANDATORY_COMPONENTS`. Asserting Table satisfies #2/#4
//! today would be a hollow/false gate.
//!
//! **Testing-Hermeticity rule (HARD).** This file is gated behind the
//! `external-corpus` cargo feature and is therefore NOT compiled by the
//! default `cargo nextest run --workspace` / `cargo test -p verter_session
//! --tests` gate — it runs only in CI / with the live corpus present. The
//! architecture guard `external_corpus_paths_not_present_outside_gated_tests`
//! enforces the `#![cfg(feature = ...)]` line below.
//!
//! Run with:
//! ```text
//! cargo test -p verter_session --features external-corpus \
//!   --test defect_b_corpus_prevention_gate
//! ```

#![cfg(feature = "external-corpus")]

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use verter_session::{HostConfig, VerterHost};
use verter_workspace::{
    FilesystemOptions, FilesystemWorkspace, IdeProjectCompilerOptions, ProjectMembership,
    ProjectRank, VfsProjectConfig, WorkspaceAccess,
};

/// Components that MUST resolve without timeout (#1) under the
/// armed-by-default runaway fuse. `Table.vue` belongs here: it is an
/// open-generic surface that trips the fuse and resolves
/// degraded-but-terminating (~5s) — the genuine termination backstop. It
/// does NOT belong to the genuinely-complete set below (its degraded
/// result legitimately carries `Partial(BUDGET_EXCEEDED)` until the
/// route/mode-independent L1 open-domain carrier-stop lands as a tracked
/// structural follow-up). `ChatMessages.vue` is
/// intentionally ABSENT — it is an Expanded-route storm case
/// that is EXPECTED to still hang; see
/// `chat_messages_no_timeout_is_deferred_to_defect_a`.
const NO_TIMEOUT_MANDATORY_COMPONENTS: &[&str] = &[
    "Button.vue",
    "Badge.vue",
    "Avatar.vue",
    "Modal.vue",
    // Corpus-validated warm-recovering generic-heavy sibling (promoted into
    // the complete set — see `COMPLETE_MANDATORY_COMPONENTS`). Resolves fast
    // + terminating + COMPLETE on a solo cold host (no published partial),
    // so #1 (no-timeout), #3 (perf), and #4 (no published partial) cover it.
    "Calendar.vue",
    "Table.vue",
];

/// Components confirmed to resolve fast + COMPLETE under the armed fuse —
/// the set the warm-recovery (#2) and no-published-partial (#4)
/// assertions enforce. The partial-taint SCOPING invariant guarantees a
/// genuinely-complete sibling warms with
/// zero cold rebuilds and carries no `BudgetExceeded` on its published
/// surface — even if a sibling's budget-tripped partial would otherwise
/// poison a shared cold compute through a request-wide sticky.
///
/// `Table.vue` is DEFERRED from #2/#4: under the armed fuse it is
/// degraded-but-terminating and legitimately carries
/// `Partial(BUDGET_EXCEEDED)` on its published surface, so it neither
/// warms nor stays free of the sentinel today. It is promoted into this
/// set only when the route/mode-independent L1 carrier-stop lands (mirrors
/// the ChatMessages Expanded-route-storm deferral).
///
/// `Calendar.vue` is corpus-validated and promoted: on the real nuxt-ui
/// corpus its 2nd-pass `RequestContext.cold_builds` collapses ≥90% of the
/// cold value (279→8 = 97.1%) AND it resolves COMPLETE on a solo cold host
/// (no `BudgetExceeded` on its published surface — it passes #4), witnessing
/// the points-3-6 fix on a real generic-heavy component.
///
/// TODO(structural-block): promote `Table.vue` into the #2/#4
/// genuinely-complete set once the route/mode-INDEPENDENT L1 open-domain
/// carrier-stop covering the structural decl-body-lowering memo-cycle
/// route (`raise.rs` / `lower.rs`) lands — the separate escalated block
/// (`/tmp/mom/REGFIXB/ESCALATION.md`), sequenced with/after MappedTemplate.
const COMPLETE_MANDATORY_COMPONENTS: &[&str] = &[
    "Button.vue",
    "Badge.vue",
    "Avatar.vue",
    "Modal.vue",
    "Calendar.vue",
];

/// Generic-heavy siblings probed on the real corpus that DO NOT qualify for
/// `COMPLETE_MANDATORY_COMPONENTS` today, with the honest reason per
/// candidate. None are Table-class refused-warm on the shared host, but each
/// fails at least one genuinely-complete gate:
///
/// - `SelectMenu.vue` — collapses ≥90% on the SHARED host (394→32, 91.9%),
///   but on a SOLO cold host it trips the armed fuse and carries a
///   budget-tripped partial on its published surface (fails #4). Its #2
///   shared-host collapse rides warmed transitive deps from earlier
///   components, not a clean complete-entry warm admission, so admitting it
///   would make #4 a false gate.
/// - `Select.vue`    (cold 268 → warm 32, 88.1% collapse) and
/// - `InputMenu.vue` (cold 232 → warm 32, 86.2% collapse) warm-recover
///   substantially but their stable 32-build residual stays just below the
///   principled ≥90%-collapse bar (`COLD_BUILD_RESIDUAL_FRACTION`); admitting
///   them would require lowering the threshold below the level that catches
///   the 0%-collapse regression class — forbidden (a tuned-to-pass gate).
///
/// TODO(structural-block): re-probe and promote once the route/mode-
/// INDEPENDENT L1 open-domain carrier-stop lands and these surfaces resolve
/// complete on a solo host with a warm residual under
/// `max(COLD_BUILD_RESIDUAL_FLOOR, cold * COLD_BUILD_RESIDUAL_FRACTION)`.
#[allow(dead_code)]
const DEFERRED_NON_COMPLETE_SIBLINGS: &[&str] = &["SelectMenu.vue", "Select.vue", "InputMenu.vue"];

/// Hard per-component HANG / non-termination watchdog budget. This is a
/// non-termination detector, NOT a perf assertion — perf is assertion #3's
/// job (it owns the committed baseline + the 1.15 ceiling). The budget is
/// deliberately GENEROUS: a genuine hang or stack-overflow regression (the
/// Expanded-route storm class) either aborts the process instantly or blows
/// ANY finite budget, so the exact value never decides whether a true hang
/// is caught — it only governs how much slack a slow-but-TERMINATING
/// degraded cold build (e.g. `Table.vue`'s armed-fuse path, thousands of
/// cold builds) is given before a false #1 timeout under CI / concurrent
/// build load. 60s gives ample headroom over the observed isolated worst
/// case while still tripping on genuine non-termination. Do NOT tune this
/// down to chase perf — that is #3's committed baseline.
const PER_COMPONENT_HARD_BUDGET: Duration = Duration::from_secs(60);

/// Perf-budget regression threshold (#3): first-pass aggregate elapsed
/// may exceed the committed baseline by at most this fraction.
const PERF_REGRESSION_THRESHOLD: f64 = 0.15;

/// #2 warm-collapse threshold: the residual fraction of cold builds a
/// genuinely-warming component may still rebuild on the warm (2nd) pass.
/// A warming cache must collapse ≥90% of the cold builds, so the warm pass
/// may carry at most 10% of the cold `RequestContext.cold_builds`.
///
/// This is chosen to catch the 0%-collapse regression class, NOT tuned to
/// pass: the Table-class refused-warm bug leaves warm `cold_builds` ≈ (or,
/// observed, slightly ABOVE) cold — Table on the real corpus is 5029 cold →
/// 5099 warm (≈101% — NO collapse), which trips this bound by a ~10×
/// margin. A genuinely-warming component collapses far below 10%: on the
/// real corpus Button 295→14 (4.7%), SelectMenu 394→32 (8.1%), Calendar
/// 279→8 (2.9%), and Badge/Avatar/Modal reach exactly 0. The bar sits
/// between the warm-recovering population (≤8.1% residual) and the
/// refused-warm regression (~101% residual), so no constant in this range
/// could be "tuned" to admit Table while excluding a real warming
/// component — the two populations are an order of magnitude apart.
const COLD_BUILD_RESIDUAL_FRACTION: f64 = 0.10;

/// #2 absolute floor on the warm-collapse ceiling. A component with a small
/// cold-build count (e.g. Badge at 81) would otherwise face a near-zero
/// proportional ceiling (8.1) that a legitimate tiny residual could exceed.
/// The floor of 20 tolerates the small legitimate residual a warming
/// component may keep (the largest observed promoted residual is 32 for the
/// DEFERRED siblings, which this floor still EXCLUDES — Button's 14 and
/// SelectMenu's 32 are gated by the proportional term, not this floor) while
/// staying ~250× below Table's ~5099 refused-warm residual, so it never
/// masks the 0%-collapse regression class.
const COLD_BUILD_RESIDUAL_FLOOR: u64 = 20;

/// Committed perf baseline (#3). Captured on reference development
/// hardware against the mandatory set under the armed-fuse contract. The
/// +28%/2× UTILMETA regression would exceed `baseline * 1.15`. Lives in
/// `packages/benchmark/baselines/` per the bench-baseline convention.
fn perf_baseline_path() -> PathBuf {
    workspace_root()
        .join("packages")
        .join("benchmark")
        .join("baselines")
        .join("defect-b-corpus-prevention-gate-baseline.json")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Locate the live `nuxt-ui` corpus (the same corpus `bench:meta:ui`
/// resolves at `.integration-tests/repos/nuxt-ui`).
fn locate_corpus_root() -> PathBuf {
    let corpus = workspace_root()
        .join(".integration-tests")
        .join("repos")
        .join("nuxt-ui");
    assert!(
        corpus.exists(),
        "corpus path missing: {} — the external-corpus gate requires the nuxt-ui corpus \
         checked out at the recorded baseline commit",
        corpus.display()
    );
    corpus
}

/// Build a host backed by the live filesystem corpus (mirrors the
/// `repo_first_pass_diagnosis_corpus` builder).
fn build_corpus_host(corpus_root: &Path) -> Arc<VerterHost> {
    build_corpus_host_with_config(corpus_root, HostConfig::default())
}

/// Build a corpus-backed host with audit + footprint capture enabled, so
/// each resolve publishes a `RequestAuditRecord` whose footprint exposes
/// the per-request `RequestContext.cold_builds` counter (the faithful warm
/// oracle for #2 — the same `cold_builds` axis the `bench:meta:ui` accounts
/// and the Defect-B bisect (Table 0→5101) use).
fn build_audit_corpus_host(corpus_root: &Path) -> Arc<VerterHost> {
    build_corpus_host_with_config(
        corpus_root,
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
    )
}

fn build_corpus_host_with_config(corpus_root: &Path, config: HostConfig) -> Arc<VerterHost> {
    let ws_root_str = corpus_root.to_string_lossy().to_string();
    let tsconfig_path = corpus_root
        .join("tsconfig.json")
        .to_string_lossy()
        .to_string();
    #[allow(deprecated)]
    let project_graph = verter_workspace::ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: ws_root_str.clone(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some(tsconfig_path.clone()),
        root_files: vec![],
        extensions: vec![],
        workspace_root: ws_root_str.clone(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: ProjectMembership::MatchAll,
    }]);
    let workspace = Arc::new(FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![ws_root_str.clone()],
        eager_preload: false,
    }));
    workspace.set_project_graph(project_graph);
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new(config, ws_access);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            ws_root_str.clone(),
            ws_root_str,
            Some(tsconfig_path),
        ),
    ]);
    Arc::new(host)
}

/// Resolve `canonical` once on the audit-enabled host and return the
/// per-request `RequestContext.cold_builds` mined onto the published audit
/// footprint. This is the SAME counter the bench's
/// `cache_outcomes.cold_builds` surfaces and the Defect-B bisect's 0→5101
/// axis tracks: it is bumped once per cold `SemanticGraphStore`
/// `execute_cooperative` build, so a fully-warming 2nd pass collapses it
/// toward zero while a refused-warm (Table-class) regression leaves it ≈
/// the cold value.
fn resolve_cold_builds(host: &Arc<VerterHost>, canonical: &str) -> u64 {
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution(canonical)
        .unwrap_or_else(|| panic!("resolve of {canonical} returned None (no component meta)"));
    let record = host
        .take_audit_record(resolution.request_id)
        .unwrap_or_else(|| {
            panic!(
                "audit record missing for {canonical} (request_id {})",
                resolution.request_id
            )
        });
    record
        .footprint
        .as_ref()
        .unwrap_or_else(|| {
            panic!("audit record for {canonical} carried no footprint (footprint_capture off?)")
        })
        .cache_outcomes
        .cold_builds as u64
}

fn locate_component(corpus_root: &Path, basename: &str) -> PathBuf {
    let direct = corpus_root.join("src/runtime/components").join(basename);
    if direct.exists() {
        return direct;
    }
    walk_for(corpus_root.join("src/runtime/components"), basename)
        .unwrap_or_else(|| panic!("component `{basename}` not found in corpus"))
}

fn walk_for(root: PathBuf, basename: &str) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }
    let entries = std::fs::read_dir(&root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = walk_for(path, basename) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some(basename) {
            return Some(path);
        }
    }
    None
}

/// One component's measured outcome over a single resolve.
struct ComponentOutcome {
    /// Whether the resolve completed within the hard budget.
    timed_out: bool,
    /// Elapsed wall-clock for the resolve.
    elapsed: Duration,
    /// Whether the resolve succeeded (Some) and, if so, whether it
    /// surfaced a budget-tripped false partial on its published surface.
    resolved_ok: bool,
    /// `synthesis_should_suppress` for a successful resolve — `true` means
    /// a budget-tripped / partial `demand: Published` result.
    published_partial: bool,
    /// Whether the actual PUBLISHED `ComponentMetaAnalysis` surface (props,
    /// models, events, slots) leaks a `BudgetExceeded` partial sentinel in
    /// any typed published field. Walked structurally over the typed-IR
    /// (`TypeExpr`), NOT via `synthesis_should_suppress` — a leaked sentinel
    /// can ride a published field WITHOUT the request-level suppress flag
    /// being set (the masking case assertion #4 must catch).
    published_surface_budget_exceeded: bool,
}

/// Structural typed-IR walk for a leaked budget-exceeded partial
/// sentinel. A budget-tripped / partial early-exit that leaks onto a
/// published surface surfaces as a `TypeExpr::Unknown { raw }` carrying
/// the production `budgetExceeded(<Domain>)` spelling
/// `semantic_query_error_raw` emits. The sentinel is recognized by the
/// SAME shared `pub(crate)` production recognizer (re-exported via
/// `verter_session::test_only::budget_sentinel`), so the test's spelling
/// can never drift from the producer's. This walks every type-bearing
/// arm of `TypeExpr` (NOT string matching on a rendered display string —
/// the marker is read off the typed `Unknown.raw` field via the shared
/// recognizer).
fn type_expr_mentions_budget_exceeded(expr: &verter_type_expr::TypeExpr) -> bool {
    use verter_session::test_only::budget_sentinel::is_budget_exceeded_sentinel;
    use verter_type_expr::{ObjectMember, TypeExpr, TypeParam};

    // The sole production spelling of a leaked budget sentinel is a
    // `TypeExpr::Unknown { raw: "budgetExceeded(...)" }`; recognize it
    // here through the shared production recognizer before structural
    // recursion.
    if is_budget_exceeded_sentinel(expr) {
        return true;
    }

    fn type_param_mentions(p: &TypeParam) -> bool {
        p.constraint
            .as_ref()
            .is_some_and(|c| type_expr_mentions_budget_exceeded(c))
            || p.default
                .as_ref()
                .is_some_and(|d| type_expr_mentions_budget_exceeded(d))
    }

    fn function_mentions(f: &verter_type_expr::FunctionExpr) -> bool {
        f.type_parameters.iter().any(type_param_mentions)
            || f.parameters
                .iter()
                .any(|p| type_expr_mentions_budget_exceeded(&p.ty))
            || f.return_type
                .as_ref()
                .is_some_and(|r| type_expr_mentions_budget_exceeded(r))
    }

    match expr {
        TypeExpr::Ref { type_arguments, .. } | TypeExpr::RecursiveRef { type_arguments, .. } => {
            type_arguments
                .iter()
                .any(type_expr_mentions_budget_exceeded)
        }
        // The leaked sentinel is recognized above via the shared
        // recognizer; a non-sentinel `Unknown` carries no nested
        // type-bearing surface.
        TypeExpr::Unknown { .. } => false,
        TypeExpr::TypeParameter(param) => type_param_mentions(param),
        TypeExpr::Object(object) => object.properties.iter().any(|m| match m {
            ObjectMember::Property(p) => type_expr_mentions_budget_exceeded(&p.ty),
            ObjectMember::IndexSignature(sig) => {
                type_expr_mentions_budget_exceeded(&sig.key_type)
                    || type_expr_mentions_budget_exceeded(&sig.value_type)
            }
            ObjectMember::CallSignature(f) | ObjectMember::ConstructSignature(f) => {
                function_mentions(f)
            }
            ObjectMember::Method(m) => function_mentions(&m.function),
        }),
        TypeExpr::Function(f) | TypeExpr::ConstructorType(f) => function_mentions(f),
        TypeExpr::Array { element, .. } => type_expr_mentions_budget_exceeded(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|e| type_expr_mentions_budget_exceeded(&e.ty)),
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
            arms.iter().any(type_expr_mentions_budget_exceeded)
        }
        TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) | TypeExpr::Parenthesized(inner) => {
            type_expr_mentions_budget_exceeded(inner)
        }
        TypeExpr::IndexedAccess { object, index } => {
            type_expr_mentions_budget_exceeded(object) || type_expr_mentions_budget_exceeded(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            type_expr_mentions_budget_exceeded(check)
                || type_expr_mentions_budget_exceeded(extends)
                || type_expr_mentions_budget_exceeded(true_type)
                || type_expr_mentions_budget_exceeded(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            type_expr_mentions_budget_exceeded(source)
                || type_expr_mentions_budget_exceeded(value)
                || name_type
                    .as_ref()
                    .is_some_and(|n| type_expr_mentions_budget_exceeded(n))
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().any(type_expr_mentions_budget_exceeded)
        }
        // Terminals with no nested TypeExpr to carry the sentinel.
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::SyntheticSlotBinding(_) => false,
    }
}

/// Pins the gate-#4 walker to the REAL production spelling and proves it
/// reaches a sentinel borne on a function type parameter's
/// `constraint`/`default` (the traversal extended for FINDING 2). NON-hollow:
/// every assertion would fail against the pre-fix walker (which keyed on
/// capital-B `"BudgetExceeded"` and treated type parameters as terminal).
#[test]
fn gate4_walker_detects_production_sentinel_including_in_type_parameters() {
    use std::sync::Arc;
    use verter_type_expr::{FunctionExpr, TypeExpr, TypeParam};

    let real_sentinel = || TypeExpr::Unknown {
        raw: "budgetExceeded(ProjectionOperation)".into(),
    };

    // Bare production sentinel.
    assert!(
        type_expr_mentions_budget_exceeded(&real_sentinel()),
        "walker MUST detect the production `budgetExceeded(...)` sentinel"
    );

    // The stale capital-B spelling never occurs in production and must NOT
    // be what the walker keys on.
    assert!(
        !type_expr_mentions_budget_exceeded(&TypeExpr::Unknown {
            raw: "BudgetExceeded".into()
        }),
        "walker must key on the production prefix, not the stale capital-B literal"
    );

    // Clean `Unknown` text does not fire.
    assert!(
        !type_expr_mentions_budget_exceeded(&TypeExpr::Unknown {
            raw: "string".into()
        }),
        "walker MUST NOT fire on clean `Unknown` text"
    );

    // Sentinel borne on a function type parameter's CONSTRAINT (FINDING 2).
    let fn_constraint = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        Vec::new(),
        None,
        vec![TypeParam {
            name: "T".into(),
            constraint: Some(Arc::new(real_sentinel())),
            default: None,
        }],
    )));
    assert!(
        type_expr_mentions_budget_exceeded(&fn_constraint),
        "walker MUST detect a sentinel borne on a function type-parameter constraint"
    );

    // Sentinel borne on a type parameter's DEFAULT (FINDING 2).
    let param_default = TypeExpr::TypeParameter(TypeParam {
        name: "U".into(),
        constraint: None,
        default: Some(Arc::new(real_sentinel())),
    });
    assert!(
        type_expr_mentions_budget_exceeded(&param_default),
        "walker MUST detect a sentinel borne on a type-parameter default"
    );

    // A clean function with clean type parameters must not fire.
    let clean_fn = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        Vec::new(),
        None,
        vec![TypeParam {
            name: "T".into(),
            constraint: Some(Arc::new(TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String,
            ))),
            default: None,
        }],
    )));
    assert!(
        !type_expr_mentions_budget_exceeded(&clean_fn),
        "walker MUST NOT fire on a clean function / clean type parameters"
    );
}

/// Walk every PUBLISHED type-bearing surface of a resolved
/// `ComponentMetaAnalysis` (props + models + events + slots) for a leaked
/// budget-exceeded partial sentinel. `true` means at least one
/// `demand: Published` field carries the marker — a silent correctness
/// regression even when `synthesis_should_suppress` is `false`.
fn published_surface_carries_budget_exceeded(
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> bool {
    let props = meta
        .props
        .iter()
        .any(|p| type_expr_mentions_budget_exceeded(&p.type_expr));
    let models = meta
        .models
        .iter()
        .any(|m| type_expr_mentions_budget_exceeded(&m.type_expr));
    let events = meta
        .events
        .iter()
        .any(|e| type_expr_mentions_budget_exceeded(&e.payload));
    let slots = meta.slots.iter().any(|s| {
        s.return_expr
            .as_ref()
            .is_some_and(type_expr_mentions_budget_exceeded)
            || s.bindings
                .iter()
                .any(|b| type_expr_mentions_budget_exceeded(&b.type_expr))
    });
    props || models || events || slots
}

/// Resolve one component on a fresh thread guarded by a hard budget. The
/// watchdog returns `timed_out = true` (without joining the worker, which
/// may be genuinely non-terminating — the Expanded-route storm class) when the budget
/// elapses. A successful resolve reports its `synthesis_should_suppress`.
fn resolve_with_hard_budget(corpus_root: &Path, basename: &str) -> ComponentOutcome {
    let canonical = locate_component(corpus_root, basename)
        .to_string_lossy()
        .to_string();
    let corpus_owned = corpus_root.to_path_buf();
    // (resolved_ok, synthesis_should_suppress, published_surface_budget_exceeded)
    let (tx, rx) = mpsc::channel::<(bool, bool, bool)>();
    let start = Instant::now();
    // The host is built INSIDE the worker so the watchdog can abandon a
    // non-terminating build without a poisoned shared host. The published
    // `ComponentMetaAnalysis` surface is walked here (the worker) before
    // sending the booleans back — the typed meta itself is not `Send`-
    // friendly across the channel and must be inspected in-thread.
    let _worker = std::thread::spawn(move || {
        let host = build_corpus_host(&corpus_owned);
        let resolved = host.get_component_meta_with_resolution(&canonical);
        let (ok, published_partial, surface_budget_exceeded) = match resolved {
            Some((meta, resolution)) => (
                true,
                resolution.synthesis_should_suppress,
                published_surface_carries_budget_exceeded(&meta),
            ),
            None => (false, false, false),
        };
        // Receiver may already be gone (timeout) — ignore send error.
        let _ = tx.send((ok, published_partial, surface_budget_exceeded));
    });
    match rx.recv_timeout(PER_COMPONENT_HARD_BUDGET) {
        Ok((resolved_ok, published_partial, published_surface_budget_exceeded)) => {
            ComponentOutcome {
                timed_out: false,
                elapsed: start.elapsed(),
                resolved_ok,
                published_partial,
                published_surface_budget_exceeded,
            }
        }
        Err(_) => ComponentOutcome {
            timed_out: true,
            elapsed: start.elapsed(),
            resolved_ok: false,
            published_partial: false,
            published_surface_budget_exceeded: false,
        },
    }
}

/// Assertions #1 + #3 + #4 on a fresh host per component (the
/// per-component hard-budget watchdog needs an isolated host it can
/// abandon on non-termination). Aggregates first-pass elapsed for the
/// perf-budget check (#3).
///
/// #1 (no-timeout) + #3 (perf) span `NO_TIMEOUT_MANDATORY_COMPONENTS`
/// (incl. `Table.vue`, which trips the armed fuse but terminates). #4
/// (no published `BudgetExceeded`) applies ONLY to the
/// genuinely-COMPLETE set (`COMPLETE_MANDATORY_COMPONENTS`) — `Table.vue`
/// legitimately carries a degraded `Partial(BUDGET_EXCEEDED)` on its
/// published surface today and is DEFERRED from #4 to the structural
/// block.
#[test]
fn mandatory_components_resolve_without_timeout_or_false_partial_and_within_perf_budget() {
    let corpus_root = locate_corpus_root();

    let mut total_elapsed = Duration::ZERO;
    let mut timeouts: Vec<&str> = Vec::new();
    let mut false_partials: Vec<&str> = Vec::new();
    let mut unresolved: Vec<&str> = Vec::new();

    for &component in NO_TIMEOUT_MANDATORY_COMPONENTS {
        let outcome = resolve_with_hard_budget(&corpus_root, component);
        total_elapsed += outcome.elapsed;
        // #1 — no component may time out.
        if outcome.timed_out {
            timeouts.push(component);
            continue;
        }
        if !outcome.resolved_ok {
            unresolved.push(component);
            continue;
        }
        // #4 — a GENUINELY-COMPLETE resolve must NOT carry a budget-tripped
        // partial on its `demand: Published` surface. Two independent
        // signals: the request-level `synthesis_should_suppress` flag, AND
        // a leaked `BudgetExceeded` sentinel on the actual published typed
        // surface (props + models + events + slots). The latter catches the
        // masking case the flag misses — a sentinel riding a published
        // field while `synthesis_should_suppress` is `false`. #4 is scoped
        // to the genuinely-complete set: `Table.vue` is degraded under the
        // armed fuse and is DEFERRED from #4 (it legitimately carries the
        // sentinel until the structural block lands).
        if COMPLETE_MANDATORY_COMPONENTS.contains(&component)
            && (outcome.published_partial || outcome.published_surface_budget_exceeded)
        {
            false_partials.push(component);
        }
    }

    // #1
    assert!(
        timeouts.is_empty(),
        "#1 per-component no-timeout: components exceeded the {}s hard budget (timeout / \
         non-termination): {:?}. `Table.vue` here means it no longer terminates degraded under \
         the armed fuse (a regression below the af35 backstop).",
        PER_COMPONENT_HARD_BUDGET.as_secs(),
        timeouts
    );
    assert!(
        unresolved.is_empty(),
        "#1 per-component resolution: components failed to resolve at all: {unresolved:?}"
    );
    // #4
    assert!(
        false_partials.is_empty(),
        "#4 no `BudgetExceeded` on a `demand: Published` key (genuinely-complete set only): \
         components surfaced a budget-tripped partial — either `synthesis_should_suppress=true` \
         OR a `BudgetExceeded` sentinel leaked onto a published typed field \
         (props/models/events/slots) — on their published surface: {false_partials:?}. A \
         genuinely-complete component must NOT manufacture a partial, and no published field may \
         carry the sentinel even when the request suppress flag is clear (the masking case). \
         `Table.vue` is intentionally excluded from #4 (deferred to the structural block)."
    );

    // #3 — perf-budget regression vs the committed post-fix baseline.
    let baseline_ms = read_perf_baseline_ms();
    let measured_ms = total_elapsed.as_secs_f64() * 1000.0;
    let ceiling_ms = baseline_ms * (1.0 + PERF_REGRESSION_THRESHOLD);
    assert!(
        measured_ms <= ceiling_ms,
        "#3 perf-budget regression: first-pass aggregate elapsed {measured_ms:.1}ms exceeds the \
         committed baseline {baseline_ms:.1}ms by more than {:.0}% (ceiling {ceiling_ms:.1}ms). \
         The UTILMETA +28%/2× regression would trip this. If this is an INTENTIONAL perf change, \
         recapture {} after confirming the change is sound.",
        PERF_REGRESSION_THRESHOLD * 100.0,
        perf_baseline_path().display()
    );
}

/// Assertion #2 — warm-cache non-regression for the GENUINELY-COMPLETE
/// set, measured on the FAITHFUL audited `RequestContext.cold_builds`
/// counter (the bisect's 0→5101 axis), NOT the structurally-empty
/// `ComponentMetaResultDb` hit counter.
///
/// Why `cold_builds`, not the final-result hit counter: the
/// `ComponentMetaResultDb` final-result cache is consulted once at the top
/// of `get_component_meta` and ALWAYS MISSES for these components on BOTH
/// this branch and the af35 baseline (the entry's read-set revalidates
/// against the live `StoreView` and the fall-through cold resolver runs).
/// Its hit-delta is therefore structurally 0 on every pass for every
/// component here — a hit-delta oracle is unsatisfiable and proves nothing
/// about warmth. The faithful warmth signal is the per-request
/// `RequestContext.cold_builds` counter — bumped once per cold
/// `SemanticGraphStore::execute_cooperative` build — surfaced on the audit
/// footprint's `cache_outcomes.cold_builds` (the exact axis the
/// `bench:meta:ui` accounts and the Defect-B bisect use, Table 0→5101).
///
/// A single audit-enabled host resolves every genuinely-complete component
/// twice. The cold (1st) pass populates the shared semantic caches; the
/// warm (2nd) pass must COLLAPSE its cold builds by ≥90% (warm `cold_builds`
/// ≤ `max(COLD_BUILD_RESIDUAL_FLOOR, cold * COLD_BUILD_RESIDUAL_FRACTION)`).
/// This is the direct points-3-6 witness: pre-fix the request-wide sticky
/// suppress (raised by the open-generic siblings tripping the fuse)
/// collapsed these complete siblings' warm reuse, so their 2nd pass
/// cold-rebuilt; post-fix each cold compute carries its OWN completeness, so
/// the complete entries warm cleanly and `cold_builds` collapses.
///
/// This uses ONE shared host (warm reuse is the point) and therefore does
/// not apply the per-component watchdog — the complete set is the fast +
/// terminating subset. `Table.vue` is EXCLUDED: under the armed fuse it is
/// degraded-but-terminating and its result is correctly refused warm
/// admission (the no-poison invariant), so its 2nd-pass `cold_builds` does
/// NOT collapse (real corpus: 5029 cold → 5099 warm, ≈0% collapse — exactly
/// the regression this assertion catches) until the structural block lands
/// (see the `TODO(structural-block)` on `COMPLETE_MANDATORY_COMPONENTS`).
/// `ChatMessages.vue` is excluded too — it is the Expanded-route storm class.
#[test]
fn warm_pass_does_zero_cold_rebuilds_for_complete_components() {
    let corpus_root = locate_corpus_root();
    let host = build_audit_corpus_host(&corpus_root);

    let canonicals: Vec<String> = COMPLETE_MANDATORY_COMPONENTS
        .iter()
        .map(|c| {
            locate_component(&corpus_root, c)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    // Cold pass — populates the shared semantic caches and records each
    // component's cold `RequestContext.cold_builds`.
    let mut cold_builds: Vec<u64> = Vec::with_capacity(canonicals.len());
    for canonical in &canonicals {
        cold_builds.push(resolve_cold_builds(&host, canonical));
    }

    // Warm pass — each 2nd resolve's `cold_builds` must collapse ≥90% of
    // the cold value (or fall under the absolute floor). A component whose
    // warm `cold_builds` stays ≈ cold (Table-class refused-warm) trips the
    // bound by a ~10× margin.
    let mut regressed: Vec<(&str, u64, u64, u64)> = Vec::new();
    for (component, canonical) in COMPLETE_MANDATORY_COMPONENTS.iter().zip(&canonicals) {
        let idx = canonicals.iter().position(|c| c == canonical).unwrap();
        let cold = cold_builds[idx];
        let warm = resolve_cold_builds(&host, canonical);
        let ceiling = COLD_BUILD_RESIDUAL_FLOOR
            .max((cold as f64 * COLD_BUILD_RESIDUAL_FRACTION).ceil() as u64);
        if warm > ceiling {
            regressed.push((component, cold, warm, ceiling));
        }
    }

    assert!(
        regressed.is_empty(),
        "#2 warm-cache non-regression (genuinely-complete set, faithful \
         `RequestContext.cold_builds` oracle — the bisect's 0→5101 axis): the warm (2nd) pass \
         did NOT collapse cold builds by ≥{:.0}% for (component, cold, warm, ceiling): \
         {regressed:?}. A warm `cold_builds` that stays ≈ cold is the Table-class refused-warm \
         regression (real corpus Table: 5029 cold → 5099 warm, ~0% collapse). This is the \
         points-3-6 failure mode: a request-wide sticky suppress (raised by an open-generic \
         sibling's budget trip) collapsed a COMPLETE sibling's warm reuse instead of letting its \
         own per-cold-compute completeness admit it.",
        (1.0 - COLD_BUILD_RESIDUAL_FRACTION) * 100.0
    );
}

/// `ChatMessages.vue` no-timeout is DEFERRED to the route/mode-independent
/// L1 open-domain carrier-stop (the Expanded route-candidate storm in
/// `raise.rs`). It is EXPECTED to still hang under the armed-fuse-only
/// backstop, so this entry is `#[ignore]`d and must NOT gate the suite.
/// When that carrier-stop lands, remove the `#[ignore]` and fold
/// `ChatMessages.vue` into `NO_TIMEOUT_MANDATORY_COMPONENTS`.
///
/// TODO(l1-route-independent): un-ignore + add `ChatMessages.vue` to the
/// mandatory set
/// once the Expanded-route storm fix lands (the route/mode-independent L1
/// carrier-stop touching `project_semantic_dispatch/raise.rs`, `lower.rs`,
/// and the Expanded route-candidate path).
#[test]
#[ignore = "deferred: ChatMessages.vue Expanded-route storm still hangs; \
            un-ignore when the raise.rs/L1-carrier-stop fix lands"]
fn chat_messages_no_timeout_is_deferred_to_defect_a() {
    let corpus_root = locate_corpus_root();
    let outcome = resolve_with_hard_budget(&corpus_root, "ChatMessages.vue");
    assert!(
        !outcome.timed_out && outcome.resolved_ok && !outcome.published_partial,
        "ChatMessages.vue must resolve without timeout or false partial once the \
         route/mode-independent L1 carrier-stop lands \
         (timed_out={}, resolved_ok={}, published_partial={})",
        outcome.timed_out,
        outcome.resolved_ok,
        outcome.published_partial
    );
}

/// Read the committed perf baseline (#3). The baseline file holds a single
/// JSON object with a `firstPassAggregateMs` number. A missing baseline is
/// a hard failure (the gate must run against a committed baseline, never a
/// silent zero).
///
/// FAIL-CLOSED on a provisional baseline: a baseline carrying
/// `"firstPassAggregateMsProvisional": true` is a placeholder, NOT a
/// committed measurement, so the #3 perf assertion would be
/// non-authoritative (it could pass on a fabricated number). This panics
/// with a clear recapture instruction rather than silently skipping — the
/// gate must not pass on a provisional value.
fn read_perf_baseline_ms() -> f64 {
    let path = perf_baseline_path();
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "#3 perf baseline missing at {} ({e}). The gate requires a COMMITTED post-fix \
             baseline; capture one and commit it.",
            path.display()
        )
    });
    assert!(
        !baseline_is_provisional(&raw),
        "#3 Defect-B perf baseline is provisional; recapture `firstPassAggregateMs` on corpus \
         hardware and set `firstPassAggregateMsProvisional: false` in {} before this assertion \
         is authoritative. A provisional value is a placeholder, not a committed measurement — \
         the gate must NOT pass on a fabricated number.",
        path.display()
    );
    parse_first_pass_aggregate_ms(&raw).unwrap_or_else(|| {
        panic!(
            "#3 perf baseline at {} has no parseable `firstPassAggregateMs` number",
            path.display()
        )
    })
}

/// Whether the baseline carries `"firstPassAggregateMsProvisional": true`.
/// A missing flag is treated as committed (`false`) so a future baseline
/// that drops the flag entirely (already recaptured) is authoritative.
/// Minimal hand-parser (no serde dep on the external-corpus path,
/// mirroring `parse_first_pass_aggregate_ms`).
fn baseline_is_provisional(raw: &str) -> bool {
    let key = "\"firstPassAggregateMsProvisional\"";
    let Some(idx) = raw.find(key) else {
        return false;
    };
    let after = &raw[idx + key.len()..];
    let Some(colon) = after.find(':') else {
        return false;
    };
    after[colon + 1..].trim_start().starts_with("true")
}

/// Minimal hand-parser for the single `firstPassAggregateMs` field (no
/// serde dep on the external-corpus path, mirroring the diagnosis corpus
/// test's manual JSON handling).
fn parse_first_pass_aggregate_ms(raw: &str) -> Option<f64> {
    let key = "\"firstPassAggregateMs\"";
    let idx = raw.find(key)?;
    let after = &raw[idx + key.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e'))
        .unwrap_or(rest.len());
    rest[..end].parse::<f64>().ok()
}
