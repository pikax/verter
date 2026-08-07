//! THE flow-return shape corpus — one table every round and every reviewer
//! runs instead of rebuilding its own fixture set.
//!
//! # Why this file exists
//!
//! Eleven review rounds each authored their own fixtures for the same
//! surface, and each reviewer rebuilt the shape space by hand. That work was
//! discarded every round, so the tested surface never GREW — it was rebuilt.
//! Four consecutive rounds then oscillated on one guard, each shipping a fix
//! whose fixture set structurally could not reach the neighbouring failure.
//!
//! This table is the fence that ends that. It is APPEND-ONLY by design:
//! adding a shape is one [`Row`] literal, and every lane (`checker`, runtime
//! bytes, TSX bytes, flow graph node, Svelte twin) is driven from that one
//! literal by the shared drivers below. If adding a shape requires editing a
//! driver, the driver is wrong.
//!
//! # What a row carries
//!
//! * `script` — the authored `<script setup>` body, spliced verbatim.
//! * `checker` — what tsgo `7.0.0-dev.20260526.1`
//!   (`--noEmit --strict --ignoreConfig`, CHECKER only, never `.d.ts`) prints
//!   for the row's `probe` type. Verified live by
//!   [`corpus_checker_column_matches_tsgo`] whenever the pinned binary is
//!   resolvable.
//! * `runtime` — the expected EMITTED option value (`props: {…}` /
//!   `emits: […]`), BRACKET-MATCHED out of the rendered `CompileTarget::BUNDLER`
//!   module.
//! * `tsx` — the `CompileTarget::IDE | TEMPLATE_DATA` lane outcome, reached
//!   through `ensure_ide_compiled` + `get_ide`.
//! * `flow` — for a flow-level shape, the GRAPH NODE, the `degradation`, and
//!   the `slot_candidate_count`.
//! * `svelte` — the `.svelte` twin's `FrameworkSurfaceKind::Props` member set.
//! * `verdict` — [`Verdict`], the row's relationship to the checker.
//!
//! # Two assertion rules this file exists to enforce
//!
//! 1. **A `contains("propname")` assertion is FORBIDDEN.** A rendered
//!    `<script setup>` module splices the AUTHORED script verbatim into
//!    `setup(__props)`, so `code.contains("label")` is satisfied by the helper
//!    source whatever the props block says — such a check passes against
//!    `props: {}`. Every runtime assertion here runs against the
//!    BRACKET-MATCHED option value ([`emitted_option`]).
//! 2. **Assert on the GRAPH NODE, never the projected `TypeExpr`.**
//!    `TypeParam` / `DeclRef` / `BareRef` all project to `Ref { name }`, so a
//!    `TypeExpr`-level assertion cannot tell them apart. [`NodeShape`] reads
//!    `SemanticNodeData` directly.
//!
//! # CONTRIBUTOR NOTE — read this before you build your own fixtures
//!
//! **If you measured a shape, add it here.** Passing or failing, agreeing with
//! the checker or not. A reviewer who measured a shape in a scratch worktree,
//! reported it, and did not add a row has thrown the measurement away — that
//! is the exact waste this table exists to end. Landed coverage grows
//! monotonically; measurement corpora do not, unless you append.
//!
//! ## Adding a row — the whole procedure
//!
//! 1. **Write the row.** One [`Row`] literal appended to [`CORPUS`], with
//!    `..Row::BLANK` filling the lanes your shape does not exercise. `script`
//!    is the authored `<script setup>` body; `macro_call` defaults to
//!    `defineProps<ReturnType<typeof makeProps>>()`; `probe` defaults to
//!    `ReturnType<typeof makeProps>`. Nothing outside the table changes.
//! 2. **Measure it, do not guess it.**
//!    ```text
//!    U6_CORPUS_DUMP=1 cargo test -p verter_session --lib u6_flow_shape_corpus \
//!        -- --nocapture --test-threads=1 2>&1 | grep <your_row_id>
//!    ```
//!    Every lane prints its MEASURED value. Transcribe them into the row.
//! 3. **Record the checker's answer.** `checker` is what tsgo prints for
//!    `probe`; leave it empty and the row is rejected. Do not hand-write it —
//!    run the suite, and [`oracle::corpus_checker_column_matches_tsgo`]
//!    regenerates the probe from your own `script` + `probe` and byte-compares
//!    against the pinned binary. If the row is `any`, set `checker_is_any`:
//!    `any` is assignable to `null`, so the shape probe alone cannot see it.
//! 4. **Pick the verdict.** [`Verdict`] is the row's relationship to the
//!    CHECKER, and [`verdict_consistency`] enforces each claim:
//!    * [`Verdict::MatchesChecker`] — the published member set equals the
//!      checker's. No erased member, no refusal.
//!    * [`Verdict::Degraded`] — the member SET is right and some member TYPE
//!      is erased to `type: null`. An honest weaker answer.
//!    * [`Verdict::FailsClosed`] — production REFUSES, and refusing is the
//!      DESIGNED answer because the root's key set is genuinely unknowable
//!      (spread of `any`, of an index signature, of a class instance).
//!    * [`Verdict::KnownOwed`] — production DISAGREES with the checker and the
//!      divergence is a debt. Name the `owner`, and put in `owed_absent` the
//!      needles that would APPEAR if the debt were repaired. Also append the
//!      row's id to [`OPEN_DEBTS`]. This makes the row a tripwire in BOTH
//!      directions: it fails if the shape degrades further AND it fails the
//!      moment the owner fixes it, so the fix is visible instead of silent.
//! 5. **Run the suite.** It is 8 tests and about 5 seconds once the crate is
//!    built; no other crate and no other suite is involved.
//!
//! ## Two ways to read a failure
//!
//! Every failure prints the authored shape, the checker's answer, EXPECTED,
//! ACTUAL, the verdict, the owner, and the one command that re-measures that
//! single row. You should never need to re-derive a row to understand why it
//! failed. A failure is either
//! * a **regression** — production got worse; fix production; or
//! * a **repair** — a pinned `KnownOwed` / `Tsx::Faults` row started behaving
//!   correctly. That failure is the INTENDED signal. Re-pin the row and drop
//!   its id from [`OPEN_DEBTS`] in the same change.
//!
//! # Adding a row
//!
//! One [`Row`] literal appended to [`CORPUS`], with `..Row::BLANK` filling the
//! lanes that shape does not exercise. Nothing else.

use std::sync::Arc;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    FlowReturnDegradation, QueryError, SemanticNodeData, SemanticQueryApi, SemanticQueryKey,
};
use crate::types::{CompileProfile, HostConfig, UpsertRequest, VirtualNodeKind, VirtualQuery};
use crate::{FileLanguage, VerterHost};
use verter_compiler::compile::CompileTarget;
use verter_type_expr::facts::FunctionPartIdentity;

// ─────────────────────────────────────────────────────────────────────────
// Row vocabulary
// ─────────────────────────────────────────────────────────────────────────

/// What the RUNTIME (bundler) lane must do with a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Runtime {
    /// The row does not drive the runtime lane.
    Skip,
    /// The lane REFUSES with `XUnavailableMacroSemanticResult` — no bytes.
    Refused,
    /// The lane emits, and the BRACKET-MATCHED option value must contain every
    /// `has` needle and none of the `hasnt` ones.
    Emitted {
        has: &'static [&'static str],
        hasnt: &'static [&'static str],
    },
    /// The lane emits bytes and declares NO such option (a macro with no
    /// runtime option surface, e.g. `defineSlots`).
    NoOption,
    /// The lane emits bytes, and the compile carries this diagnostic code.
    Diagnostic(&'static str),
}

/// What the TSX (IDE) lane must do with a row.
///
/// The vocabulary is deliberately TOTAL: a future row must be able to name
/// its outcome without touching a driver, so a variant no current row uses
/// still belongs here.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tsx {
    /// The row does not drive the TSX lane.
    Skip,
    /// The lane projects, and the projection splices the authored script and
    /// the authored macro call. A body-derived degradation says NOTHING about
    /// whether the TSX is the full surface, so the lane must never fault.
    Projects,
    /// The lane FAULTS: `ensure_ide_compiled` returns an error and the file
    /// loses its whole type-check surface.
    ///
    /// This is never a designed outcome for a program the checker types. Every
    /// `Tsx::Faults` row is either a `KnownOwed` debt or a row whose source
    /// genuinely does not compile (`XInvalidMacroScopeReference`), and the
    /// pinned code is what discriminates the two.
    Faults(&'static str),
}

/// The `SemanticNodeData` discriminant a flow-level row's return node carries.
///
/// This is deliberately the GRAPH-NODE discriminant, not a projected
/// `TypeExpr`: `TypeParam` / `DeclRef` / `BareRef` all project to
/// `Ref { name }`, so a `TypeExpr` assertion cannot discriminate them.
/// The vocabulary is deliberately TOTAL — see [`Tsx`].
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeShape {
    Object,
    ObjectSpreadProgram,
    Union,
    Intersection,
    Primitive,
    Literal,
    Alias,
    TypeOf,
    Array,
    Tuple,
    IndexedAccess,
    Reference,
    /// `SemanticNodeData::Opaque(QueryError::UnmodeledPosition)` — the typed
    /// unmodelled-position MARKER, distinct from a cache miss.
    OpaqueUnmodeledPosition,
    /// `SemanticNodeData::Opaque(_)` carrying any other query error.
    OpaqueOther,
    /// Any node kind not enumerated above.
    Other,
}

/// The typed degradation a flow-level row's result must carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Degr {
    None,
    UnmodeledPosition,
    UnappliedWriteEffect,
    ConditionalVarDefinition,
    NonCallableBinding,
    UnrepresentableCallee,
    FailedBindingInitializer,
    UnreducedDeclaredUnion,
    UnresolvedValue,
}

/// The flow-graph lane expectation for one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    /// The row does not drive the flow lane.
    Skip,
    /// `FlowReturn` on `fn` produced a complete result with this node shape,
    /// this degradation, and this warm-candidate count.
    Result {
        function: &'static str,
        node: NodeShape,
        degradation: Degr,
        /// `slot_candidate_count_for_tests` for the row's `FlowReturn` key.
        /// A DEGRADED success is `ReturnOnly` — nothing warms — so this is
        /// `0` for every degraded row and non-zero only for clean ones.
        candidates: usize,
    },
    /// `FlowReturn` answered with a typed non-value (a `Miss`, a `ReturnOnly`
    /// with no value). The row pins the refusal, not a fabricated shape.
    NoValue,
}

/// The `.svelte` twin's `FrameworkSurfaceKind::Props` expectation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Svelte {
    /// The shape has no `.svelte` twin (Vue-macro-specific).
    Skip,
    /// The framework surface resolves and PROPS carries exactly these member
    /// names (order-insensitive, compared as a set).
    Props(&'static [&'static str]),
}

/// A row's relationship to the CHECKER.
///
/// The point of the enum is that a row which is deliberately imperfect is
/// pinned as EXACTLY that imperfection, so any drift — in either direction —
/// fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Production's published surface agrees with the checker.
    MatchesChecker,
    /// Production REFUSES rather than publish a surface it cannot justify.
    /// Loud, and strictly better than a silently wrong one.
    FailsClosed,
    /// Production publishes an HONEST WEAKER surface — the member set is
    /// right and some member types are erased.
    Degraded(&'static str),
    /// Production DISAGREES with the checker and this row pins the
    /// disagreement.
    ///
    /// A `KnownOwed` row is a TRIPWIRE IN BOTH DIRECTIONS: `runtime.has`
    /// pins the current (wrong) behaviour so a further degradation fails,
    /// and `owed_absent` pins what the CORRECT answer would look like so the
    /// owner's fix ALSO fails the row — visibly, at the moment it lands,
    /// rather than silently.
    KnownOwed {
        owner: &'static str,
        /// Needles that must stay ABSENT while the debt is open. When the
        /// owner fixes the shape, one of these appears and the row fails,
        /// which is the intended signal to re-pin the row.
        owed_absent: &'static [&'static str],
        note: &'static str,
    },
}

/// One corpus shape.
#[derive(Clone, Copy)]
pub(crate) struct Row {
    /// Stable identity. Doubles as the fixture stem and the oracle probe file
    /// name, so it must be a valid file stem.
    pub(crate) id: &'static str,
    /// A sibling module's source for a cross-file shape, or `""`.
    ///
    /// Upserted at `<dir>/<id>__aux.ts` (Vue lane) and imported from the
    /// script as `./<id>__aux`.
    pub(crate) aux: &'static str,
    /// The authored script body — spliced verbatim into `<script setup>`,
    /// into the `.svelte` instance script, and into the flow-lane `.ts`.
    pub(crate) script: &'static str,
    /// The macro call line, verbatim.
    pub(crate) macro_call: &'static str,
    /// The rendered runtime option to bracket-match.
    pub(crate) option_key: &'static str,
    /// The type the ORACLE probe declares. `""` means the row is not probed
    /// (it has no single type the checker can name).
    pub(crate) probe: &'static str,
    /// The type text the CHECKER prints for `probe`.
    pub(crate) checker: &'static str,
    /// Whether the checker says `probe` is `any`
    /// (`type IsAny<T> = 0 extends 1 & T ? true : false`). A row that is
    /// `any` to the checker is a fail-closed row for us by construction.
    pub(crate) checker_is_any: bool,
    pub(crate) runtime: Runtime,
    pub(crate) tsx: Tsx,
    pub(crate) flow: Flow,
    pub(crate) svelte: Svelte,
    pub(crate) verdict: Verdict,
}

impl Row {
    /// The lanes a new row does not exercise. Append a row as
    /// `Row { id: …, script: …, …, ..Row::BLANK }`.
    pub(crate) const BLANK: Row = Row {
        id: "",
        aux: "",
        script: "",
        macro_call: "defineProps<ReturnType<typeof makeProps>>()",
        option_key: "props: ",
        probe: "ReturnType<typeof makeProps>",
        checker: "",
        checker_is_any: false,
        runtime: Runtime::Skip,
        tsx: Tsx::Projects,
        flow: Flow::Skip,
        svelte: Svelte::Skip,
        verdict: Verdict::MatchesChecker,
    };
}

// ─────────────────────────────────────────────────────────────────────────
// Drivers — every lane, shared by every row
// ─────────────────────────────────────────────────────────────────────────

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    ))
}

fn upsert(host: &VerterHost, id: &str, source: &str, language: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(id.to_owned()),
            input_id: id.to_owned(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|err| panic!("upsert `{id}`: {err:?}"));
}

/// The EMITTED runtime option value (`props: { … }` / `emits: [ … ]`),
/// bracket-matched out of a rendered module.
///
/// The assertion target has to be this object and nothing else — see the
/// module docs' rule 1.
fn emitted_option(code: &str, option_key: &str) -> Option<String> {
    let key = code.find(option_key)?;
    let open = key + option_key.len();
    let (opener, closer) = match *code.as_bytes().get(open)? {
        b'{' => (b'{', b'}'),
        b'[' => (b'[', b']'),
        _ => return None,
    };
    let mut depth = 0usize;
    for (offset, byte) in code.as_bytes()[open..].iter().enumerate() {
        if *byte == opener {
            depth += 1;
        } else if *byte == closer {
            depth -= 1;
            if depth == 0 {
                return Some(code[open..=open + offset].to_owned());
            }
        }
    }
    None
}

/// The `.vue` carrier for a row.
fn vue_source(row: &Row) -> String {
    format!(
        "<script setup lang=\"ts\">\n{}\n{}\n</script>\n<template><div /></template>",
        row.script, row.macro_call
    )
}

/// What the runtime lane actually did.
#[derive(Debug)]
enum RuntimeOutcome {
    /// The module emitted; the payload is the bracket-matched option value
    /// (or `None` when the module declares no such option) plus the compile's
    /// diagnostic codes.
    Emitted(Option<String>, Vec<String>),
    /// The lane refused with `XUnavailableMacroSemanticResult`.
    Refused,
}

fn drive_runtime(row: &Row) -> RuntimeOutcome {
    let host = make_host();
    let dir = "/src";
    if !row.aux.is_empty() {
        upsert(
            &host,
            &format!("{dir}/{}__aux.ts", row.id),
            row.aux,
            FileLanguage::script_ts(),
        );
    }
    let canonical = format!("{dir}/{}.vue", row.id);
    upsert(&host, &canonical, &vue_source(row), FileLanguage::vue());
    let response = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.clone()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: CompileProfile {
            target: CompileTarget::BUNDLER,
            ..CompileProfile::default()
        },
    });
    match response {
        Ok(response) => {
            let codes: Vec<String> = response
                .diagnostics
                .diagnostics
                .iter()
                .map(|d| d.code.clone())
                .collect();
            if codes.iter().any(|c| c == "XUnavailableMacroSemanticResult") {
                return RuntimeOutcome::Refused;
            }
            RuntimeOutcome::Emitted(emitted_option(&response.code, row.option_key), codes)
        }
        Err(crate::types::HostError::CompileError(failure)) => {
            let codes: Vec<String> = failure
                .diagnostics
                .diagnostics
                .iter()
                .map(|d| d.code.clone())
                .collect();
            if codes.iter().any(|c| c == "XUnavailableMacroSemanticResult") {
                RuntimeOutcome::Refused
            } else {
                RuntimeOutcome::Emitted(None, codes)
            }
        }
        Err(other) => panic!("{}: unexpected host failure {other:?}", row.id),
    }
}

/// Drive the TSX lane through `ensure_ide_compiled` + `get_ide`.
///
/// This is the ONLY way to reach the `CachedTsx` projection: `get_virtual_file`
/// with `VirtualNodeKind::Main` and `CompileTarget::IDE` returns the RUNTIME
/// module under a names-only demand, so a test written that way measures the
/// runtime lane twice and reports the TSX lane healthy no matter what it does.
fn drive_tsx(row: &Row) -> Result<String, String> {
    let host = make_host();
    let dir = "/src";
    if !row.aux.is_empty() {
        upsert(
            &host,
            &format!("{dir}/{}__aux.ts", row.id),
            row.aux,
            FileLanguage::script_ts(),
        );
    }
    let canonical = format!("{dir}/{}.vue", row.id);
    upsert(&host, &canonical, &vue_source(row), FileLanguage::vue());
    // The LSP's own IDE profile (`Documents::tsx_profile`). A default
    // (BUNDLER) profile normalized with the TSX bit still demands runtime
    // PROP CONSTRUCTORS, so a test written that way measures the runtime
    // lane's constructor demand and calls it the TSX lane.
    let profile = CompileProfile {
        source_map: true,
        target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
        ..CompileProfile::default()
    };
    match host.ensure_ide_compiled(&canonical, &profile) {
        Ok(true) => match host.get_ide(&canonical, &profile) {
            Some(ide) => Ok(ide.code.to_string()),
            None => Err("no TSX projection was cached".to_owned()),
        },
        Ok(false) => Err("carrier has no IDE projection surface".to_owned()),
        Err(err) => Err(format!("{err:?}")),
    }
}

/// The macro identifier a row's `macro_call` invokes (`defineProps`,
/// `defineEmits`, …), independent of its type argument and of any
/// `withDefaults` wrapper.
fn macro_base_name(macro_call: &str) -> &str {
    let start = macro_call
        .find("define")
        .expect("every corpus macro_call invokes a `define*` macro");
    let rest = &macro_call[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    &rest[..end]
}

/// The top-level declaration names a row's script authors. The TSX projection
/// SPLICES the authored script, so every one of them must survive into it.
fn authored_helpers(script: &str) -> Vec<&str> {
    let mut names = Vec::new();
    for line in script.lines() {
        let line = line.trim();
        for prefix in ["function ", "class ", "const ", "interface "] {
            if let Some(rest) = line.strip_prefix(prefix) {
                let end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$')
                    .unwrap_or(rest.len());
                if end > 0 {
                    names.push(&rest[..end]);
                }
                break;
            }
        }
    }
    names
}

fn node_shape(data: Option<&SemanticNodeData>) -> NodeShape {
    match data {
        Some(SemanticNodeData::Object(_)) => NodeShape::Object,
        Some(SemanticNodeData::ObjectSpreadProgram(_)) => NodeShape::ObjectSpreadProgram,
        Some(SemanticNodeData::Union(_)) => NodeShape::Union,
        Some(SemanticNodeData::Intersection(_)) => NodeShape::Intersection,
        Some(SemanticNodeData::Primitive(_)) => NodeShape::Primitive,
        Some(SemanticNodeData::Literal(_)) => NodeShape::Literal,
        Some(SemanticNodeData::Alias(_)) => NodeShape::Alias,
        Some(SemanticNodeData::TypeOf(_)) => NodeShape::TypeOf,
        Some(SemanticNodeData::Array { .. }) => NodeShape::Array,
        Some(SemanticNodeData::Tuple { .. }) => NodeShape::Tuple,
        Some(SemanticNodeData::IndexedAccess { .. }) => NodeShape::IndexedAccess,
        Some(SemanticNodeData::Opaque(QueryError::UnmodeledPosition)) => {
            NodeShape::OpaqueUnmodeledPosition
        }
        Some(SemanticNodeData::Opaque(_)) => NodeShape::OpaqueOther,
        _ => NodeShape::Other,
    }
}

fn degr_of(reason: Option<FlowReturnDegradation>) -> Degr {
    match reason {
        None => Degr::None,
        Some(FlowReturnDegradation::UnmodeledPosition) => Degr::UnmodeledPosition,
        Some(FlowReturnDegradation::UnappliedWriteEffect) => Degr::UnappliedWriteEffect,
        Some(FlowReturnDegradation::ConditionalVarDefinition) => Degr::ConditionalVarDefinition,
        Some(FlowReturnDegradation::NonCallableBinding) => Degr::NonCallableBinding,
        Some(FlowReturnDegradation::UnrepresentableCallee) => Degr::UnrepresentableCallee,
        Some(FlowReturnDegradation::FailedBindingInitializer) => Degr::FailedBindingInitializer,
        Some(FlowReturnDegradation::UnreducedDeclaredUnion) => Degr::UnreducedDeclaredUnion,
        Some(FlowReturnDegradation::UnresolvedValue) => Degr::UnresolvedValue,
    }
}

/// Drive the FLOW lane: demand `FlowReturn` on the row's named function and
/// read back the GRAPH NODE, the typed degradation, and the warm-candidate
/// count for that key.
fn drive_flow(row: &Row, function: &str) -> Flow {
    let host = make_host();
    let dir = "/ws";
    if !row.aux.is_empty() {
        upsert(
            &host,
            &format!("{dir}/{}__aux.ts", row.id),
            row.aux,
            FileLanguage::script_ts(),
        );
    }
    let canonical = format!("{dir}/{}.ts", row.id);
    upsert(&host, &canonical, row.script, FileLanguage::script_ts());

    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let key = crate::semantic_query::FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical.as_str()),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(function),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(&canonical),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
    };
    let outcome = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
    let candidates = dispatch
        .graph()
        .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
    match outcome {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: crate::semantic_query::SemanticQueryValue::FlowReturn(result),
            ..
        }) => {
            let data = dispatch.graph().node_data(result.return_type());
            Flow::Result {
                function: "",
                node: node_shape(data.as_deref()),
                degradation: degr_of(result.degradation()),
                candidates,
            }
        }
        _ => Flow::NoValue,
    }
}

/// The `.svelte` twin for a row.
///
/// A `.svelte` carrier is classified with [`FileLanguage::svelte`]; a
/// rest-only destructure with the row's own annotation is what makes the
/// twin's prop surface come from the SAME type the Vue macro consumes,
/// without the row having to spell its member names twice.
fn svelte_source(row: &Row) -> String {
    format!(
        "<script lang=\"ts\">\n{}\nlet {{ ...rest }}: {} = $props();\nvoid rest;\n</script>\n<div />\n",
        row.script, row.probe
    )
}

fn drive_svelte(row: &Row) -> Result<Vec<String>, String> {
    use verter_protocol::typeinfo::graph::{self as wire, FrameworkSurfaceKind};
    use verter_protocol::verter::v1::{
        type_info_graph_request as wire_request, type_info_graph_response,
    };

    let host = make_host();
    let dir = "/svelte";
    if !row.aux.is_empty() {
        upsert(
            &host,
            &format!("{dir}/{}__aux.ts", row.id),
            row.aux,
            FileLanguage::script_ts(),
        );
    }
    let canonical = format!("{dir}/{}.svelte", row.id);
    upsert(
        &host,
        &canonical,
        &svelte_source(row),
        FileLanguage::svelte(),
    );

    let envelope = wire::TypeInfoGraphRequest {
        schema_version: 3,
        operation: wire::Operation::FrameworkSurfaces as i32,
        payload: Some(wire_request::Payload::FrameworkSurface(
            wire::FrameworkSurfaceRequest {
                selector: Some(wire::ComponentSelector {
                    canonical_id: canonical.clone(),
                    export_name: String::new(),
                    has_export_name: false,
                    framework_adapter_id: "svelte".to_string(),
                }),
                context: Some(wire::ProjectionReductionContext {
                    mode: wire::ProjectionMode::Expanded as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(
                        verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                            wire::ClosureOneLevel {},
                        ),
                    ),
                }),
                display_policy: Some(wire::DisplayPolicy {
                    qualification: wire::DisplayQualification::Qualified as i32,
                    branding: wire::DisplayBranding::On as i32,
                    budgets: Some(wire::DisplayBudgets {
                        max_string_length: 4096,
                        max_depth: 16,
                    }),
                }),
                include_provenance: false,
                include_diagnostics: false,
                include_projection: vec![],
                schema_version: 3,
            },
        )),
    };
    let result = host.resolve_framework_surface_with_audit(envelope);
    let response = match result.as_result() {
        Ok(response) => response,
        Err(err) => return Err(format!("{err:?}")),
    };
    let payload = match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => return Err(format!("not a framework_surface arm: {other:?}")),
    };
    let strings = payload
        .graph
        .as_ref()
        .and_then(|g| g.strings.as_ref())
        .map(|t| t.entries.clone())
        .unwrap_or_default();
    let mut names: Vec<String> = payload
        .surfaces
        .iter()
        .find(|e| e.kind == FrameworkSurfaceKind::Props as i32)
        .map(|e| {
            e.members
                .iter()
                .filter_map(|m| strings.get(m.name_id as usize).cloned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    Ok(names)
}

// ─────────────────────────────────────────────────────────────────────────
// THE TABLE
// ─────────────────────────────────────────────────────────────────────────

include!("u6_flow_shape_corpus_table.rs");

// ─────────────────────────────────────────────────────────────────────────
// The suite
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod corpus_suite {
    use super::*;

    /// Set `U6_CORPUS_DUMP=1` to print every row's MEASURED lane values
    /// instead of asserting them. This is how a NEW row's expectations are
    /// first obtained; it is never how a row is verified.
    fn dump_mode() -> bool {
        std::env::var("U6_CORPUS_DUMP").is_ok_and(|v| v == "1")
    }

    /// A self-contained failure report for one row.
    ///
    /// The failure text alone must be enough for the next agent to act: the
    /// authored shape, the checker's answer, the expectation, the measurement,
    /// the owner, and what to do. A reviewer must never have to re-derive the
    /// row to understand why it failed — re-derivation is the waste this
    /// corpus exists to end.
    fn report(row: &Row, lane: &str, expected: &str, actual: &str, what: &str) -> String {
        let (verdict, owner) = match row.verdict {
            Verdict::MatchesChecker => ("MatchesChecker".to_owned(), "—".to_owned()),
            Verdict::FailsClosed => ("FailsClosed".to_owned(), "—".to_owned()),
            Verdict::Degraded(reason) => (format!("Degraded({reason})"), "—".to_owned()),
            Verdict::KnownOwed { owner, note, .. } => {
                (format!("KnownOwed — {note}"), owner.to_owned())
            }
        };
        format!(
            "\n\
             ┌── {id}  [{lane}]\n\
             │ WHAT     {what}\n\
             │ SHAPE    {script}\n\
             │ MACRO    {macro_call}\n\
             │ CHECKER  {checker}{any}\n\
             │ EXPECTED {expected}\n\
             │ ACTUAL   {actual}\n\
             │ VERDICT  {verdict}\n\
             │ OWNER    {owner}\n\
             └── re-measure this ONE row with:\n\
             \x20     U6_CORPUS_DUMP=1 cargo test -p verter_session --lib u6_flow_shape_corpus \
             -- --nocapture --test-threads=1 2>&1 | grep {id}\n\
             \x20   then re-pin the row (and, if a debt CLOSED, drop its id from OPEN_DEBTS).",
            id = row.id,
            script = row.script.replace('\n', "  ⏎  "),
            macro_call = row.macro_call,
            checker = row.checker,
            any = if row.checker_is_any {
                "   (IsAny = true)"
            } else {
                ""
            },
        )
    }

    #[test]
    fn corpus_ids_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for row in CORPUS {
            assert!(
                seen.insert(row.id),
                "duplicate corpus id `{}` — a row id is the fixture stem AND the oracle probe \
                 file name, so it must be unique",
                row.id
            );
            assert!(
                !row.id.is_empty()
                    && row
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "corpus id `{}` must be a portable file stem",
                row.id
            );
            assert!(
                !row.script.is_empty() && !row.checker.is_empty(),
                "corpus row `{}` must carry an authored script AND the checker's answer — a row \
                 without the checker's answer records no ground truth",
                row.id
            );
        }
        assert!(
            CORPUS.len() >= 73,
            "the corpus is APPEND-ONLY: it landed with 73 rows, and a change that shrinks it is \
             deleting measured coverage, not refactoring it (got {})",
            CORPUS.len()
        );
    }

    #[test]
    fn corpus_runtime_lane() {
        let dump = dump_mode();
        let mut failures = Vec::new();
        for row in CORPUS {
            if matches!(row.runtime, Runtime::Skip) && !dump {
                continue;
            }
            let outcome = drive_runtime(row);
            if dump {
                println!("RT {} => {outcome:?}", row.id);
                continue;
            }
            let actual = match &outcome {
                RuntimeOutcome::Refused => "REFUSED (XUnavailableMacroSemanticResult)".to_owned(),
                RuntimeOutcome::Emitted(None, codes) => {
                    format!(
                        "emitted, no `{}` option; diagnostics {codes:?}",
                        row.option_key
                    )
                }
                RuntimeOutcome::Emitted(Some(value), codes) => {
                    format!("{}{value}   diagnostics {codes:?}", row.option_key)
                }
            };
            match (row.runtime, &outcome) {
                (Runtime::Refused, RuntimeOutcome::Refused) => {}
                (Runtime::Refused, RuntimeOutcome::Emitted(..)) => failures.push(report(
                    row,
                    "runtime",
                    "REFUSED — the substrate could not type the ROOT, so there is no member set",
                    &actual,
                    "the lane PUBLISHED a surface for a root it cannot type. If this is a FIX, \
                     re-pin the row: the refusal was the pinned behaviour.",
                )),
                (Runtime::Emitted { has, hasnt }, RuntimeOutcome::Emitted(value, _)) => {
                    let Some(value) = value else {
                        failures.push(report(
                            row,
                            "runtime",
                            &format!("an emitted `{}` option containing {has:?}", row.option_key),
                            &actual,
                            "the rendered module declares no such option at all",
                        ));
                        continue;
                    };
                    let missing: Vec<_> = has.iter().filter(|n| !value.contains(**n)).collect();
                    if !missing.is_empty() {
                        failures.push(report(
                            row,
                            "runtime",
                            &format!("{}{{ … }} containing {missing:?}", row.option_key),
                            &actual,
                            "a pinned member is MISSING from the bracket-matched option value",
                        ));
                    }
                    let present: Vec<_> = hasnt.iter().filter(|n| value.contains(**n)).collect();
                    if !present.is_empty() {
                        failures.push(report(
                            row,
                            "runtime",
                            &format!("{present:?} ABSENT from {}{{ … }}", row.option_key),
                            &actual,
                            "a forbidden fragment APPEARED — a member the substrate typed \
                             exactly has been erased, or a fabricated constructor was published",
                        ));
                    }
                    // A `KnownOwed` row is a tripwire in BOTH directions: the
                    // owner's fix must fail it too, visibly, rather than
                    // silently turning a pinned debt into a pass.
                    if let Verdict::KnownOwed { owed_absent, .. } = row.verdict {
                        let appeared: Vec<_> =
                            owed_absent.iter().filter(|n| value.contains(**n)).collect();
                        if !appeared.is_empty() {
                            failures.push(report(
                                row,
                                "runtime",
                                &format!("{owed_absent:?} still ABSENT (the debt is still open)"),
                                &actual,
                                "the debt looks REPAIRED — the owed members have APPEARED. This \
                                 failure is the INTENDED signal, not a regression: re-pin the \
                                 row (verdict → MatchesChecker / Degraded, new `has` needles) \
                                 and remove its id from OPEN_DEBTS in the same change.",
                            ));
                        }
                    }
                }
                (Runtime::Emitted { has, .. }, RuntimeOutcome::Refused) => failures.push(report(
                    row,
                    "runtime",
                    &format!("{}{{ … }} containing {has:?}", row.option_key),
                    &actual,
                    "the row has a publishable member set and the lane DELETED the module",
                )),
                (Runtime::NoOption, RuntimeOutcome::Emitted(value, _)) => {
                    if value.is_some() {
                        failures.push(report(
                            row,
                            "runtime",
                            &format!("an emitting module with NO `{}` option", row.option_key),
                            &actual,
                            "this macro has no runtime option surface and one appeared",
                        ));
                    }
                }
                (Runtime::NoOption, RuntimeOutcome::Refused) => failures.push(report(
                    row,
                    "runtime",
                    &format!("an emitting module with NO `{}` option", row.option_key),
                    &actual,
                    "the lane refused instead of emitting",
                )),
                (Runtime::Diagnostic(code), RuntimeOutcome::Emitted(_, codes)) => {
                    if !codes.iter().any(|c| c == code) {
                        failures.push(report(
                            row,
                            "runtime",
                            &format!("diagnostic `{code}`"),
                            &actual,
                            "the pinned diagnostic was not reported",
                        ));
                    }
                }
                (Runtime::Diagnostic(code), RuntimeOutcome::Refused) => failures.push(report(
                    row,
                    "runtime",
                    &format!("diagnostic `{code}`"),
                    &actual,
                    "the lane refused instead of reporting the pinned diagnostic",
                )),
                (Runtime::Skip, _) => {}
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn corpus_tsx_lane() {
        let dump = dump_mode();
        let mut failures = Vec::new();
        for row in CORPUS {
            if matches!(row.tsx, Tsx::Skip) && !dump {
                continue;
            }
            let outcome = drive_tsx(row);
            if dump {
                println!(
                    "TSX {} => {}",
                    row.id,
                    match &outcome {
                        Ok(_) => "Projects".to_owned(),
                        Err(e) => format!("FAULT {e}"),
                    }
                );
                continue;
            }
            match (row.tsx, outcome) {
                (Tsx::Projects, Ok(code)) => {
                    // The TSC projection SPLICES the authored declaration and
                    // the authored type argument and lets the external checker
                    // compute the member types. A body-derived return this
                    // substrate could not infer, could not verify, or could not
                    // produce at all therefore says NOTHING about whether the
                    // TSX is the full surface — so both the authored macro AND
                    // every authored top-level declaration must survive into it.
                    let macro_name = macro_base_name(row.macro_call);
                    let mut absent: Vec<&str> = Vec::new();
                    if !code.contains(macro_name) {
                        absent.push(macro_name);
                    }
                    for helper in authored_helpers(row.script) {
                        if !code.contains(helper) {
                            absent.push(helper);
                        }
                    }
                    if !absent.is_empty() {
                        failures.push(report(
                            row,
                            "tsx",
                            &format!("a projection splicing {absent:?}"),
                            "a projection MISSING them",
                            "the TSX lane splices the AUTHORED script and macro — an authored \
                             declaration that does not survive into it is a lost type-check \
                             surface",
                        ));
                    }
                }
                (Tsx::Projects, Err(err)) => failures.push(report(
                    row,
                    "tsx",
                    "a projection (the lane must never delete the file's type-check surface)",
                    &format!("FAULT {err}"),
                    "the TSX lane splices the authored declaration and is UNAFFECTED by a \
                     body-derived degradation; faulting deletes every byte of type-checking \
                     for a program the checker types without difficulty",
                )),
                (Tsx::Faults(code), Err(err)) => {
                    if !err.contains(code) {
                        failures.push(report(
                            row,
                            "tsx",
                            &format!("FAULT carrying `{code}`"),
                            &format!("FAULT {err}"),
                            "the pinned fault code changed",
                        ));
                    }
                }
                (Tsx::Faults(code), Ok(_)) => failures.push(report(
                    row,
                    "tsx",
                    &format!("FAULT carrying `{code}` (the debt is still open)"),
                    "a clean projection",
                    "the debt looks REPAIRED — the TSX lane now PROJECTS. This failure is the \
                     INTENDED signal, not a regression: re-pin the row to `Tsx::Projects` and \
                     remove its id from OPEN_DEBTS in the same change.",
                )),
                (Tsx::Skip, _) => {}
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn corpus_flow_lane() {
        let dump = dump_mode();
        let mut failures = Vec::new();
        for row in CORPUS {
            let function = match row.flow {
                Flow::Skip if !dump => continue,
                Flow::Result { function, .. } => function,
                _ => "makeProps",
            };
            let measured = drive_flow(row, function);
            if dump {
                println!("FLOW {} [{function}] => {measured:?}", row.id);
                continue;
            }
            match (row.flow, measured) {
                (
                    Flow::Result {
                        node,
                        degradation,
                        candidates,
                        ..
                    },
                    Flow::Result {
                        node: got_node,
                        degradation: got_degr,
                        candidates: got_candidates,
                        ..
                    },
                ) => {
                    if node != got_node {
                        failures.push(report(
                            row,
                            &format!("flow GRAPH NODE of `{function}`"),
                            &format!("{node:?}"),
                            &format!("{got_node:?}"),
                            "the flow return's SemanticNodeData discriminant changed. This is \
                             asserted on the GRAPH NODE, never the projected TypeExpr, because \
                             TypeParam / DeclRef / BareRef all project to `Ref { name }`.",
                        ));
                    }
                    if degradation != got_degr {
                        failures.push(report(
                            row,
                            &format!("flow degradation of `{function}`"),
                            &format!("{degradation:?}"),
                            &format!("{got_degr:?}"),
                            "the typed degradation reason changed",
                        ));
                    }
                    if candidates != got_candidates {
                        failures.push(report(
                            row,
                            &format!("flow slot_candidate_count of `{function}`"),
                            &format!("{candidates}"),
                            &format!("{got_candidates}"),
                            "a DEGRADED success is ReturnOnly — nothing warms — so a non-zero \
                             count on a degraded row means a torn result was promoted warm",
                        ));
                    }
                }
                (Flow::NoValue, Flow::NoValue) => {}
                (expected, got) => failures.push(report(
                    row,
                    &format!("flow lane of `{function}`"),
                    &format!("{expected:?}"),
                    &format!("{got:?}"),
                    "the flow lane's OUTCOME class changed (value ↔ no-value)",
                )),
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn corpus_svelte_twins() {
        let dump = dump_mode();
        let mut failures = Vec::new();
        for row in CORPUS {
            if matches!(row.svelte, Svelte::Skip) && !dump {
                continue;
            }
            let measured = drive_svelte(row);
            if dump {
                println!("SVELTE {} => {measured:?}", row.id);
                continue;
            }
            match (row.svelte, measured) {
                (Svelte::Props(expected), Ok(got)) => {
                    let mut want: Vec<&str> = expected.to_vec();
                    want.sort_unstable();
                    let got_refs: Vec<&str> = got.iter().map(String::as_str).collect();
                    if want != got_refs {
                        failures.push(report(
                            row,
                            "svelte FrameworkSurfaceKind::Props",
                            &format!("{want:?}"),
                            &format!("{got_refs:?}"),
                            "the `.svelte` twin's prop member set changed. NOTE: Svelte props \
                             are served by `resolve_framework_surface_with_audit`, NOT by \
                             `ComponentMetaAnalysis.props` — a harness driving the latter \
                             reports every Svelte row empty and proves nothing.",
                        ));
                    }
                }
                (Svelte::Props(expected), Err(err)) => failures.push(report(
                    row,
                    "svelte FrameworkSurfaceKind::Props",
                    &format!("{expected:?}"),
                    &format!("the framework surface did not resolve: {err}"),
                    "the `.svelte` twin must resolve through the registered Svelte adapter",
                )),
                (Svelte::Skip, _) => {}
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

// ─────────────────────────────────────────────────────────────────────────
// The ORACLE — the `checker` column, verified against the pinned tsgo
// ─────────────────────────────────────────────────────────────────────────

/// The pinned checker. CHECKER only, never `.d.ts`.
#[cfg(test)]
const TSGO_VERSION: &str = "7.0.0-dev.20260526.1";

/// Resolve the pinned `tsgo`, or `None`.
///
/// Resolution order: `VERTER_TSGO_BIN`, then the pnpm hoisted bin dir, then
/// the platform package's own `lib/tsgo`. A miss SKIPS (and passes) — the
/// corpus stays hermetic on a runner with no `node_modules`.
#[cfg(test)]
fn resolve_tsgo() -> Option<std::path::PathBuf> {
    if let Ok(explicit) = std::env::var("VERTER_TSGO_BIN") {
        let path = std::path::PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
        return None;
    }
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?;
    let hoisted = workspace.join("node_modules/.pnpm/node_modules/.bin/tsgo");
    if hoisted.is_file() {
        return Some(hoisted);
    }
    let store = workspace.join("node_modules/.pnpm");
    for entry in std::fs::read_dir(store).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("@typescript+native-preview-") {
            continue;
        }
        // `@typescript+native-preview-<platform>@<ver>/node_modules/@typescript/native-preview-<platform>/lib/tsgo`
        let inner = entry.path().join("node_modules/@typescript");
        let Ok(dirs) = std::fs::read_dir(inner) else {
            continue;
        };
        for dir in dirs.flatten() {
            for candidate in ["lib/tsgo", "lib/tsgo.exe", "bin/tsgo", "bin/tsgo.exe"] {
                let path = dir.path().join(candidate);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// One generated probe program.
///
/// Two steps, deliberately. A one-step `const x: null = f(…)` reports NOTHING
/// when the contextual type feeds inference, and a raw call bound to `const`
/// reads UNWIDENED literals — both silently turn the probe into a no-op. A
/// `declare const` of the probe type followed by an assignment to `null`
/// never feeds inference back into the call, so the checker prints the type
/// it actually computed.
///
/// The `IsAny` half is the second trap: `any` IS assignable to `null`, so an
/// `any` row emits NOTHING from the shape probe. `0 extends 1 & T` is the
/// only reliable `any` detector.
#[cfg(test)]
fn probe_program(probe: &str, script: &str) -> String {
    format!(
        "type IsAny<T> = 0 extends 1 & T ? true : false;\n\
         {script}\n\
         declare const __v: {probe};\n\
         export const __shape: null = __v;\n\
         declare const __a: IsAny<{probe}>;\n\
         export const __isany: null = __a;\n"
    )
}

/// Parse `<file>(<line>,<col>): error TS2322: Type 'X' is not assignable to
/// type 'null'.` into `stem -> (shape, is_any)`.
#[cfg(test)]
fn parse_tsgo(output: &str) -> std::collections::HashMap<String, (String, bool)> {
    let mut by: std::collections::HashMap<String, (Option<String>, Option<String>)> =
        std::collections::HashMap::new();
    for line in output.lines() {
        let Some((file, rest)) = line.split_once('(') else {
            continue;
        };
        let Some(message) = rest.split_once("error TS2322: ").map(|(_, m)| m) else {
            continue;
        };
        let Some(inner) = message
            .strip_prefix("Type '")
            .and_then(|m| m.strip_suffix("' is not assignable to type 'null'."))
        else {
            continue;
        };
        let stem = file.trim_end_matches(".ts").to_owned();
        let slot = by.entry(stem).or_default();
        if slot.0.is_none() {
            slot.0 = Some(inner.to_owned());
        } else if slot.1.is_none() {
            slot.1 = Some(inner.to_owned());
        }
    }
    by.into_iter()
        .map(|(stem, (first, second))| {
            // `any` is assignable to `null`, so an `any` row's SHAPE probe
            // emits nothing and the first captured diagnostic is the IsAny
            // probe reporting `true`.
            match (first, second) {
                (Some(a), None) if a == "true" => (stem, ("any".to_owned(), true)),
                (Some(a), _) => (stem, (a, false)),
                (None, _) => (stem, (String::new(), false)),
            }
        })
        .collect()
}

#[cfg(test)]
mod oracle {
    use super::*;

    /// A probe whose recorded answer is RIGHT, and one whose recorded answer
    /// is deliberately WRONG. Asserting that the first matches and the second
    /// does NOT is what proves the probe discriminates in both directions —
    /// a parse that silently produced `None` for everything would pass a
    /// one-sided check.
    const CONTROL_POSITIVE: (&str, &str, &str) = (
        "__control_positive",
        "function makeProps() { return { label: \"x\" } }",
        "{ label: string; }",
    );
    const CONTROL_NEGATIVE: (&str, &str, &str) = (
        "__control_negative",
        "function makeProps() { return { label: \"x\" } }",
        "{ THIS_IS_NOT_WHAT_TSGO_PRINTS: never; }",
    );

    /// THE `checker` column is verified, not asserted.
    ///
    /// Every row's `checker` text is regenerated from the row's own `script`
    /// and `probe` and byte-compared against the pinned tsgo. A row added
    /// with a guessed `checker` fails here.
    #[test]
    fn corpus_checker_column_matches_tsgo() {
        let Some(tsgo) = resolve_tsgo() else {
            // Hermetic: a runner with no `node_modules` skips and passes.
            // The corpus's own lanes still run.
            eprintln!(
                "u6 corpus: pinned tsgo {TSGO_VERSION} not resolvable — the checker column is \
                 not re-verified on this runner (set VERTER_TSGO_BIN to force it)"
            );
            return;
        };
        let dir = std::env::temp_dir().join(format!(
            "verter-u6-shape-corpus-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("probe dir");

        let mut files: Vec<String> = Vec::new();
        let mut expected: Vec<(String, String, bool)> = Vec::new();
        for row in CORPUS {
            if row.probe.is_empty() {
                continue;
            }
            if !row.aux.is_empty() {
                std::fs::write(dir.join(format!("{}__aux.ts", row.id)), row.aux)
                    .expect("aux probe");
            }
            let name = format!("{}.ts", row.id);
            std::fs::write(dir.join(&name), probe_program(row.probe, row.script))
                .expect("probe file");
            files.push(name);
            expected.push((
                row.id.to_owned(),
                row.checker.to_owned(),
                row.checker_is_any,
            ));
        }
        for (id, script, _) in [CONTROL_POSITIVE, CONTROL_NEGATIVE] {
            let name = format!("{id}.ts");
            std::fs::write(
                dir.join(&name),
                probe_program("ReturnType<typeof makeProps>", script),
            )
            .expect("control probe");
            files.push(name);
        }

        let output = std::process::Command::new(&tsgo)
            .current_dir(&dir)
            .arg("--noEmit")
            .arg("--strict")
            .arg("--ignoreConfig")
            .arg("--pretty")
            .arg("false")
            .args(&files)
            .output()
            .expect("run tsgo");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let measured = parse_tsgo(&combined);
        let _ = std::fs::remove_dir_all(&dir);

        // NEGATIVE CONTROL, both directions. If either half is wrong the probe
        // is a no-op and every row below it is meaningless.
        let positive = measured
            .get(CONTROL_POSITIVE.0)
            .unwrap_or_else(|| panic!("the positive control produced no diagnostic:\n{combined}"));
        assert_eq!(
            positive.0, CONTROL_POSITIVE.2,
            "the positive control must MATCH its recorded answer — the probe is not measuring \
             the checker"
        );
        let negative = measured
            .get(CONTROL_NEGATIVE.0)
            .unwrap_or_else(|| panic!("the negative control produced no diagnostic:\n{combined}"));
        assert_ne!(
            negative.0, CONTROL_NEGATIVE.2,
            "the negative control must MISMATCH its deliberately-wrong recorded answer — a probe \
             that 'matches' everything discriminates nothing"
        );

        let mut drift = Vec::new();
        for (id, checker, is_any) in expected {
            match measured.get(&id) {
                Some((got, got_any)) => {
                    if got != &checker || got_any != &is_any {
                        drift.push(format!(
                            "{id}: recorded `{checker}` (is_any={is_any}), tsgo {TSGO_VERSION} \
                             prints `{got}` (is_any={got_any})"
                        ));
                    }
                }
                None => drift.push(format!(
                    "{id}: tsgo emitted no probe diagnostic at all — the row's script or probe \
                     does not compile"
                )),
            }
        }
        assert!(
            drift.is_empty(),
            "the `checker` column has drifted from the pinned checker:\n{}",
            drift.join("\n")
        );
    }
}

#[cfg(test)]
mod verdict_consistency {
    use super::*;

    /// The `verdict` column is LOAD-BEARING, not a comment.
    ///
    /// Every verdict makes a claim the row's other pinned columns must
    /// corroborate. Without this, a reviewer could relabel a genuine debt as
    /// `MatchesChecker` and nothing would notice — which is exactly how the
    /// oscillating rounds stayed invisible.
    #[test]
    fn every_verdict_is_corroborated_by_the_row_it_labels() {
        let mut failures = Vec::new();
        for row in CORPUS {
            let publishes_marker = matches!(row.runtime, Runtime::Emitted { has, .. }
                if has.iter().any(|n| n.contains("type: null")));
            match row.verdict {
                Verdict::MatchesChecker => {
                    if matches!(row.runtime, Runtime::Refused) {
                        failures.push(format!(
                            "{}: labelled MatchesChecker but the runtime lane REFUSES — a \
                             refusal cannot agree with a checker that names a member set",
                            row.id
                        ));
                    }
                    if publishes_marker {
                        failures.push(format!(
                            "{}: labelled MatchesChecker but publishes an erased member \
                             (`type: null`) — that is `Degraded`, not agreement",
                            row.id
                        ));
                    }
                    // A `MatchesChecker` row may only fault the TSX lane when
                    // the SOURCE genuinely does not compile — i.e. the runtime
                    // lane reports the SAME diagnostic. Otherwise a deleted
                    // type-check surface is a debt, never agreement.
                    if let Tsx::Faults(code) = row.tsx {
                        if row.runtime != Runtime::Diagnostic(code) {
                            failures.push(format!(
                                "{}: labelled MatchesChecker with a TSX fault `{code}` that \
                                 the runtime lane does NOT also report — a deleted \
                                 type-check surface for a compiling source is a debt",
                                row.id
                            ));
                        }
                    }
                }
                Verdict::Degraded(reason) => {
                    if reason.is_empty() {
                        failures.push(format!("{}: Degraded with no reason", row.id));
                    }
                    if !publishes_marker {
                        failures.push(format!(
                            "{}: labelled Degraded but no member publishes the typed marker \
                             (`type: null`) — nothing is actually degraded",
                            row.id
                        ));
                    }
                }
                Verdict::FailsClosed => {
                    if !matches!(row.runtime, Runtime::Refused) {
                        failures.push(format!(
                            "{}: labelled FailsClosed but the runtime lane does not refuse",
                            row.id
                        ));
                    }
                }
                Verdict::KnownOwed {
                    owner,
                    owed_absent,
                    note,
                } => {
                    if owner.is_empty() || note.is_empty() {
                        failures.push(format!("{}: KnownOwed with no owner or no note", row.id));
                    }
                    // The debt must be OBSERVABLE from this row, otherwise the
                    // label is decorative: either the lane refuses, or the TSX
                    // lane faults, or the row names the needles whose
                    // appearance means the debt was repaired.
                    let observable = matches!(row.runtime, Runtime::Refused)
                        || matches!(row.tsx, Tsx::Faults(_))
                        || !owed_absent.is_empty();
                    if !observable {
                        failures.push(format!(
                            "{}: labelled KnownOwed but the row pins nothing that would change \
                             if the debt were repaired — a KnownOwed row must be a tripwire in \
                             BOTH directions",
                            row.id
                        ));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "verdict column:\n{}",
            failures.join("\n")
        );
    }

    /// The corpus's own debt ledger has a floor: the shapes this table landed
    /// with as OPEN debts stay pinned until an owner closes them.
    ///
    /// A change that quietly relabels debts as agreement without changing
    /// production fails here.
    #[test]
    fn the_open_debt_ledger_is_pinned() {
        let owed: Vec<&str> = CORPUS
            .iter()
            .filter(|r| matches!(r.verdict, Verdict::KnownOwed { .. }))
            .map(|r| r.id)
            .collect();
        assert_eq!(
            owed.len(),
            OPEN_DEBTS.len(),
            "the open-debt ledger changed: pinned {:?}, measured {owed:?} — if an owner CLOSED \
             a debt, update OPEN_DEBTS in the same change that re-pins the row",
            OPEN_DEBTS
        );
        for id in OPEN_DEBTS {
            assert!(
                owed.contains(id),
                "`{id}` is in the open-debt ledger but is no longer labelled KnownOwed"
            );
        }
    }
}

/// The shapes this corpus landed with as OPEN debts — production disagrees
/// with the checker, or deletes a type-check surface the checker types.
///
/// This is the block's remaining debt in ONE place. Closing a debt means
/// re-pinning its row AND removing its id here, in the same change.
#[cfg(test)]
const OPEN_DEBTS: &[&str] = &[
    // Leaf-fallback spread: a CALL-sourced spread beside a computed key, a
    // numeric key, `as const`, or `satisfies` refuses the module. The
    // IDENTIFIER-sourced twins (B07/B08/B10) publish the same shapes.
    "B01_computed_after_call",
    "B02_numeric_key_after_call",
    "B03_as_const_call",
    "B04_as_const_spread_only",
    "B05_satisfies_object",
    "B06_two_calls_computed",
    // A numeric literal key is silently dropped from the published surface.
    "B09_numeric_key_ident",
    // An intersection / heritage arm whose flow return is WHOLLY unmodelled is
    // silently DROPPED instead of failing closed. This is the family the 15
    // existing `ReturnType<typeof …>` tests structurally could not reach:
    // none of them uses `&` or `extends`.
    "C04_emits_intersection_degraded",
    "C11_props_intersection_unmodelled_arm",
    "C12_heritage_unmodelled_clause",
    // A mapped type over a flow-return heritage clause publishes ZERO props.
    "C09_heritage_members_clean",
    "C10_heritage_members_degraded",
    // Multi-return join: `switch` / `try` arms that each return an object
    // literal produce NO VALUE and the module is deleted.
    "D06_switch_return",
    "D07_try_return",
    // Overloaded callee: refused at runtime AND the TSX lane faults.
    "D08_overloaded_callee",
    // The TSX lane FAULTS — the file loses its whole type-check surface for
    // programs tsgo types without difficulty.
    "D10_callee_new_spread_only",
    "D11_callee_new_spread_key",
    "E01_spread_any",
    "E02_spread_index_signature",
    "E03_spread_array",
];
