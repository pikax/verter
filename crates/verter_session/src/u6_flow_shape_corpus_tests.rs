//! Flow-return shape corpus: one append-only table every lane is driven
//! from. Adding a shape is one [`Row`] literal; if that requires editing
//! a driver, the driver is wrong.
//!
//! Primary columns:
//!
//! * `script` — authored program, spliced verbatim into every lane.
//! * `checker` — recorded tsgo `7.0.0-dev.20260526.1` (`--noEmit
//!   --strict --ignoreConfig`, checker only, never `.d.ts`) print for
//!   `probe`. The suite never invokes tsgo. The column must be
//!   non-empty, the probe derivable
//!   ([`oracle::every_row_records_the_checkers_answer`]), and every
//!   deep-pinned row compared semantically against the live graph
//!   ([`corpus_suite::deep_pinned_rows_semantic_equality_follows_their_verdict`]).
//! * `flow` — production composition: graph node, per-member shapes,
//!   typed `degradation`, `slot_candidate_count`. An authored return
//!   annotation is [`Flow::Declared`]; the body-derived answer is not
//!   what consumers see.
//! * `owner` — [`Owner`]; drives per-owner conformance.
//! * `subject`, `demand`, `verdict`.
//!
//! Secondary (optional, `Skip` by default):
//!
//! * `runtime` — bracket-matched emitted option value.
//! * `tsx` — `ensure_ide_compiled` + `get_ide`.
//! * `svelte` — `FrameworkSurfaceKind::Props` via
//!   `resolve_framework_surface_with_audit`, not
//!   `ComponentMetaAnalysis.props` (that path reports every Svelte row
//!   empty).
//!
//! Two assertion rules:
//!
//! 1. Never `contains("propname")`. A rendered `<script setup>` splices
//!    the authored script into `setup(__props)`, so `code.contains("label")`
//!    passes against `props: {}`. Runtime asserts against
//!    [`emitted_option`].
//! 2. Assert on the graph node, never the projected `TypeExpr`.
//!    `TypeParam` / `DeclRef` / `BareRef` all project to `Ref { name }`.
//!    [`NodeShape`] reads `SemanticNodeData` at the node and each member.
//!
//! This is a TypeScript semantics corpus. Framework columns mean "the
//! answer reached a consumer"; most rows carry none.
//!
//! Adding a row: append a [`Row`] with `..Row::BLANK`, measure with
//! `U6_CORPUS_DUMP=1`, transcribe, record the checker's print (dump the
//! probe and run the pinned tsgo; do not hand-write). Pin member shapes,
//! not just the enclosing `Object`. Set `checker_is_any` when the row is
//! `any` (`any` is assignable to `null`). [`Verdict::KnownOwed`] names
//! the `owner` and appends the id to [`OPEN_DEBTS`] so the row fails if
//! the shape degrades or the owner fixes it. Narrowing rows set
//! [`Demand::Narrowing`]. Framework-only shapes set
//! [`Subject::FrameworkOnly`].

use std::sync::Arc;

use crate::host_flow_return_audit::FlowReturnError;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    FlowGap, FlowReturnDegradation, FlowReturnFailure, FlowReturnUnsupported, PrimitiveKind,
    QueryError, SemanticNodeData, SemanticQueryKey,
};
use crate::types::{CompileProfile, HostConfig, UpsertRequest, VirtualNodeKind, VirtualQuery};
use crate::CompileTarget;
use crate::{FileLanguage, VerterHost};

// The strengthening layer: recursive expectations, the public cold/warm
// boundary companion, negative controls, and the crossed capture-write
// matrix. Declared as a CHILD of this module (not in `lib.rs`) so the
// corpus and its strengthening travel as one unit.
#[path = "u6_flow_expect_tests.rs"]
pub(crate) mod u6_flow_expect_tests;

#[path = "flow_gap_retraction_tests.rs"]
mod flow_gap_retraction_tests;
use self::u6_flow_expect_tests::{Boundary, Expect, ExpectedNode, Lit};

// Row vocabulary

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
    FlowGap(crate::semantic_query::FlowGap),
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
    /// `fn`'s return is BODY-DERIVED, and the `FlowReturn` producer —
    /// reached through the ONE sealed function-return consumer entry, which
    /// the driver asserts really is the rail the served signature fact
    /// selects — produced a complete result with this node shape, this
    /// degradation, and this warm-candidate count.
    Result {
        function: &'static str,
        node: NodeShape,
        /// The GRAPH-NODE shape of named members of the returned surface.
        ///
        /// This is the row's PRIMARY semantic assertion for everything that
        /// depends on a member's computed type — narrowing above all. A
        /// `typeof` guard that stopped applying shows up here as
        /// `Union` where the checker says `Primitive`, and it shows up
        /// NOWHERE else: the enclosing node is `Object` either way.
        ///
        /// Read at the GRAPH-NODE level on purpose. `TypeParam` / `DeclRef` /
        /// `BareRef` all project to `TypeExpr::Ref { name }`, so a
        /// `TypeExpr`-level member assertion cannot discriminate them.
        ///
        /// `&[]` asserts nothing about members.
        members: &'static [(&'static str, NodeShape)],
        degradation: Degr,
        /// `slot_candidate_count_for_tests` for the row's `FlowReturn` key.
        /// A DEGRADED success is `ReturnOnly` — nothing warms — so this is
        /// `0` for every degraded row and non-zero only for clean ones.
        candidates: usize,
    },
    /// `fn`'s return is DECLARED (an authored annotation): the authorship
    /// gate routes it to the memoized locator rail and the body-derived
    /// producer never runs for it, so the lane asserts the composed
    /// answer's node and member shapes through the same sealed
    /// function-return consumer entry. Pinning [`Flow::Result`] for an
    /// annotated function measures a body-only evaluation NO consumer ever
    /// demands (fresh literals widen where the annotation holds them) —
    /// the driver fails such a row, so the off-contract measurement is
    /// unspellable. The declared rail carries no `FlowReturnDegradation`
    /// and no flow slot, so the variant pins neither.
    Declared {
        function: &'static str,
        node: NodeShape,
        /// Same GRAPH-NODE member semantics as [`Flow::Result::members`].
        members: &'static [(&'static str, NodeShape)],
    },
    /// `FlowReturn` answered with a typed non-value (a `Miss`, a `ReturnOnly`
    /// with no value). The row pins the refusal, not a fabricated shape.
    /// Only a GENUINE producer refusal satisfies this pin: a served signature
    /// fact carrying no return source (the extractor dropped it) measures as
    /// [`MeasuredFlow::Absent`] and FAILS the row, so the pin can never pass
    /// on a function the producer never evaluated.
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

/// The block that OWNS a row — the team the row's answer is attributed to.
///
/// Every row has an owner, not only the failing ones, because the conformance
/// number is `matching ÷ total` PER OWNER and a denominator built only from
/// failures is meaningless. Merging this branch back to `main` is gated on
/// every PARKED row (every [`Verdict::KnownOwed`]) being green, so this
/// attribution is the go/no-go signal and shows which owner is blocking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Owner {
    U2IndexedAccess,
    U2MappedTemplate,
    U6CallResolve,
    U6ValueInference,
    U6LoopClosure,
    U6ContextualCore,
    U6FlowReturnSubstrate,
    U6NarrowTypeof,
    U6NarrowInstanceof,
    U6NarrowLattice,
    U6NarrowSubstitution,
    U6NarrowInvalidation,
    /// **NO U-BLOCK.** The shared surface reducer (intersection / heritage
    /// arms, PathWalker hops) — reusable type-resolution machinery that no
    /// `U*` block is scheduled to touch. Rows here have nobody assigned and
    /// will otherwise be the last thing blocking the merge.
    SharedTypeResolution,
    /// **NO U-BLOCK.** The compile / virtual-file pipeline: the TSX (IDE) lane
    /// deleting a file's type-check surface. Same warning as above.
    SharedCompilePipeline,
    /// Framework-specific POLICY, scoped to the project owner's post-merge
    /// pass. Not a semantic debt.
    FrameworkOnly,
}

impl Owner {
    pub(crate) const ALL: &'static [Owner] = &[
        Owner::U2IndexedAccess,
        Owner::U2MappedTemplate,
        Owner::U6CallResolve,
        Owner::U6ValueInference,
        Owner::U6LoopClosure,
        Owner::U6ContextualCore,
        Owner::U6FlowReturnSubstrate,
        Owner::U6NarrowTypeof,
        Owner::U6NarrowInstanceof,
        Owner::U6NarrowLattice,
        Owner::U6NarrowSubstitution,
        Owner::U6NarrowInvalidation,
        Owner::SharedTypeResolution,
        Owner::SharedCompilePipeline,
        Owner::FrameworkOnly,
    ];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::U2IndexedAccess => "U2.INDEXED_ACCESS",
            Self::U2MappedTemplate => "U2.MAPPED_TEMPLATE",
            Self::U6CallResolve => "U6.CALL_RESOLVE",
            Self::U6ValueInference => "U6.VALUE_INFERENCE",
            Self::U6LoopClosure => "U6.LOOP_CLOSURE",
            Self::U6ContextualCore => "U6.CONTEXTUAL_CORE",
            Self::U6FlowReturnSubstrate => "U6.FLOW_RETURN_SUBSTRATE",
            Self::U6NarrowTypeof => "U6.NARROW_TYPEOF",
            Self::U6NarrowInstanceof => "U6.NARROW_INSTANCEOF",
            Self::U6NarrowLattice => "U6.NARROW_LATTICE",
            Self::U6NarrowSubstitution => "U6.NARROW_SUBSTITUTION",
            Self::U6NarrowInvalidation => "U6.NARROW_INVALIDATION",
            Self::SharedTypeResolution => "SHARED.TYPE_RESOLUTION  (no U-block)",
            Self::SharedCompilePipeline => "SHARED.COMPILE_PIPELINE (no U-block)",
            Self::FrameworkOnly => "FRAMEWORK_ONLY          (owner post-merge)",
        }
    }

    /// Whether the owner names a semantic authority whose deferred rows ride
    /// the shared convergence gate. These owners are NOT independently
    /// scheduled blocks — none has its own schedulable unit; a deferred row
    /// under one is an obligation of the convergence proof that must retire
    /// it deliberately. `false` means nobody is assigned at all, which is
    /// exactly the class that silently blocks a merge.
    pub(crate) const fn has_convergence_owner(self) -> bool {
        !matches!(
            self,
            Self::SharedTypeResolution | Self::SharedCompilePipeline | Self::FrameworkOnly
        )
    }
}

/// WHO owns a row.
///
/// The corpus is a **TypeScript semantics** corpus first. A row's identity is
/// the semantic answer the substrate computes for a plain `.ts` program,
/// measured against the checker: the flow-return GRAPH NODE, its member
/// shapes, the typed `degradation`, and the `slot_candidate_count`. Framework
/// emission is SECONDARY evidence — it answers "did the semantic answer reach
/// a consumer intact", which is a real and separately-catchable defect, but it
/// is never the subject.
///
/// A row must therefore be expressible with NO framework column at all, and
/// [`Row::BLANK`] defaults every framework lane to its `Skip` variant so that
/// a plain `.ts` row is the SHORT literal and a framework row is the long one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Subject {
    /// Plain TypeScript semantics. Any framework column on the row is
    /// secondary evidence that the answer survived the trip to a consumer.
    TypeScript,
    /// The shape EXISTS only as a framework shape — a macro payload spelling,
    /// `withDefaults`, `defineModel`, `defineSlots`, the runtime-form macros,
    /// Vue's own scope policy. Framework-specific POLICY for these rows is
    /// scoped to the project owner's post-merge pass; this marker is what
    /// makes that worklist a single query
    /// (see `the_framework_only_worklist_is_pinned`).
    FrameworkOnly,
}

/// What a row demands — named rather than implied.
///
/// Narrowing is the class where a weakening passes every test not
/// written for it (silent widen, `typeof` that stops applying, a
/// narrowed type that stays warm after the predicate's dependency
/// changes). [`Demand::Narrowing`] labels a returned-member type that
/// already rides the existing drivers.
///
/// Type-at-an-arbitrary-position under a predicate is not built: add a
/// `Demand` variant, a `drive_*` through `ProjectSemanticDispatch`
/// (never a second resolver), and a lane test. [`Row`] / [`Verdict`] /
/// the oracle do not change for that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Demand {
    /// The published macro surface: the emitted runtime option value, the TSX
    /// projection, the flow-return graph node, and the `.svelte` twin.
    MacroSurface,
    /// Narrowing shape, labelled so the population is countable.
    /// Driven through the same lanes as [`Self::MacroSurface`].
    Narrowing(NarrowBlock),
}

/// The closed set of narrowing blocks that can own a [`Demand::Narrowing`]
/// row. Closed on purpose: a new owner is a deliberate edit, not a typo.
// The variants mirror the PLAN's block ids one-for-one; the shared prefix is
// the point, not an accident.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NarrowBlock {
    /// `typeof` / `instanceof` / literal guards.
    NarrowTypeof,
    /// The `instanceof` arm rule: derived-arm selection, nullish
    /// stripping, and the whole-subject intersection fallback.
    NarrowInstanceof,
    /// Branch joins — the lattice over two or more arms.
    NarrowLattice,
    /// A guard applied ACROSS a call boundary (user-defined type predicates,
    /// assertion functions, substituted generics).
    NarrowSubstitution,
    /// A narrowed value read after an intervening write, and warm-cache
    /// invalidation of a narrowed result.
    NarrowInvalidation,
}

impl NarrowBlock {
    /// The block id, exactly as it is spelled in the plan.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::NarrowTypeof => "U6.NARROW_TYPEOF",
            Self::NarrowInstanceof => "U6.NARROW_INSTANCEOF",
            Self::NarrowLattice => "U6.NARROW_LATTICE",
            Self::NarrowSubstitution => "U6.NARROW_SUBSTITUTION",
            Self::NarrowInvalidation => "U6.NARROW_INVALIDATION",
        }
    }
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
        /// Needles that must stay ABSENT while the debt is open. When the
        /// owner fixes the shape, one of these appears and the row fails,
        /// which is the intended signal to re-pin the row. A row with no
        /// framework column pins its [`Flow::Result::members`] instead.
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
    /// The BLOCK this row is attributed to. Drives the per-owner
    /// conformance number, which is the merge go/no-go.
    pub(crate) owner: Owner,
    /// WHO owns this row — TypeScript semantics, or a framework-only shape.
    pub(crate) subject: Subject,
    /// WHAT this row demands. Defaults to [`Demand::MacroSurface`]; a
    /// narrowing row sets [`Demand::Narrowing`] and nothing else changes.
    pub(crate) demand: Demand,
    pub(crate) runtime: Runtime,
    pub(crate) tsx: Tsx,
    pub(crate) flow: Flow,
    pub(crate) svelte: Svelte,
    pub(crate) verdict: Verdict,
    /// Recursive graph-node expectation through the public audited
    /// boundary. Distinguishes `() => "a"` from `() => "b"` where
    /// [`NodeShape`] cannot. See [`u6_flow_expect_tests::ExpectedNode`].
    pub(crate) expect: Expect,
    /// Public cold/warm companion: `get_flow_return_type_with_audit`
    /// twice, pinning JSON, degradation, and replay. See
    /// [`u6_flow_expect_tests::Boundary`].
    pub(crate) boundary: Boundary,
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
        owner: Owner::U6FlowReturnSubstrate,
        subject: Subject::TypeScript,
        demand: Demand::MacroSurface,
        runtime: Runtime::Skip,
        tsx: Tsx::Skip,
        flow: Flow::Skip,
        svelte: Svelte::Skip,
        verdict: Verdict::MatchesChecker,
        expect: Expect::Skip,
        boundary: Boundary::Skip,
    };
}

// Drivers — every lane, shared by every row

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

/// The plain-`.ts` materialization of a row's script for the flow lanes:
/// an ES MODULE — exactly the scope the row's other lanes give the same
/// text. The runtime lane splices the script into a `<script setup>`
/// block (a module by construction) and the checker probe program
/// exports its probes, so a bare `.ts` script would be the ONE lane
/// reading the text as a global-scope file — where a same-file helper
/// is never a provably closed callee for control-call certification and
/// predicate selection, and the lanes would measure different programs.
pub(crate) fn module_script(script: &str) -> String {
    format!("{script}\nexport {{}};\n")
}

pub(crate) fn upsert(host: &VerterHost, id: &str, source: &str, language: FileLanguage) {
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

pub(crate) fn degr_of(reason: Option<FlowReturnDegradation>) -> Degr {
    match reason {
        None => Degr::None,
        Some(FlowReturnDegradation::FlowGap(gap)) => Degr::FlowGap(gap),
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

/// The GRAPH-NODE member shapes of one returned surface. Shared by both
/// rails: only a closed `Object` surface has named members; a spread
/// PROGRAM is a construction plan and reports none.
fn member_shapes(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
) -> Vec<(String, NodeShape)> {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Object(surface)) => surface
            .positive_members()
            .iter()
            .filter_map(|member| {
                let name = member.key.as_string()?.to_owned();
                let value = dispatch.graph().node_data(member.value);
                Some((name, node_shape(value.as_deref())))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Drive the FLOW lane: demand the row's named function's return the way
/// PRODUCTION composes it — through the ONE sealed function-return
/// consumer entry ([`ProjectSemanticDispatch::execute_function_return_source`]),
/// with the served signature fact's `return_source` picking the rail. An
/// authored return annotation selects the DECLARED locator rail (the
/// body-derived producer never runs for an annotated function — the
/// authorship gate routes it); anything else selects the `FlowReturn`
/// producer. A row's [`Flow`] variant declares which rail it expects, and
/// the lane fails a row whose pin names the other rail — the off-contract
/// measurement (the body-only evaluation of an annotated function) is
/// unspellable here.
fn drive_flow(row: &Row, function: &str) -> MeasuredFlow {
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
    upsert(
        &host,
        &canonical,
        &module_script(row.script),
        FileLanguage::script_ts(),
    );

    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let mut source = match host.prepared_value_decl_in(&canonical, owner, function) {
        Some(prepared) => match prepared.signatures.as_slice() {
            [signature] => signature.return_source.clone(),
            _ => return MeasuredFlow::NoSignature,
        },
        None => return MeasuredFlow::NoSignature,
    };
    // Anchor fill mirrors the signature-composition consumers: the
    // extractor stamps the declaration name; canonical / owner come from
    // the serving scope.
    match &mut source {
        verter_type_expr::facts::FunctionReturnSource::Declared(locator) => {
            let slot = match locator {
                verter_type_expr::locators::FunctionReturnLocator::Authored(slot)
                | verter_type_expr::locators::FunctionReturnLocator::Jsdoc(slot) => slot,
            };
            slot.anchor.canonical_id = Arc::from(canonical.as_str());
            slot.anchor.owner = owner;
        }
        verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
            identity.anchor.canonical_id = Arc::from(canonical.as_str());
            identity.anchor.owner = owner;
        }
        verter_type_expr::facts::FunctionReturnSource::Absent => {}
    }
    let flow_identity = match &source {
        verter_type_expr::facts::FunctionReturnSource::Flow(identity) => Some(identity.clone()),
        _ => None,
    };
    match dispatch.execute_function_return_source(&source, &canonical) {
        crate::project_semantic_dispatch::flow_return::FunctionReturnNode::Declared(hot) => {
            MeasuredFlow::Declared {
                node: node_shape(dispatch.graph().node_data(hot.node()).as_deref()),
                members: member_shapes(&dispatch, hot.node()),
            }
        }
        crate::project_semantic_dispatch::flow_return::FunctionReturnNode::Flow(result) => {
            let identity = flow_identity.expect("the Flow arm carries the identity");
            let candidates =
                dispatch
                    .graph()
                    .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                        dispatch.flow_return_key_for(&identity),
                    )));
            MeasuredFlow::Result {
                node: node_shape(dispatch.graph().node_data(result.return_type()).as_deref()),
                members: member_shapes(&dispatch, result.return_type()),
                degradation: degr_of(result.degradation()),
                candidates,
            }
        }
        crate::project_semantic_dispatch::flow_return::FunctionReturnNode::DeclaredMiss => {
            MeasuredFlow::DeclaredMiss
        }
        crate::project_semantic_dispatch::flow_return::FunctionReturnNode::NoValue(_) => {
            MeasuredFlow::NoValue
        }
        crate::project_semantic_dispatch::flow_return::FunctionReturnNode::Absent => {
            MeasuredFlow::Absent
        }
    }
}

/// What the flow lane actually produced. Distinct from the row's declarative
/// [`Flow`] expectation because the measured member list is owned, not static.
#[derive(Debug)]
enum MeasuredFlow {
    Result {
        node: NodeShape,
        members: Vec<(String, NodeShape)>,
        degradation: Degr,
        candidates: usize,
    },
    /// The DECLARED locator rail's answer (an annotated function's return):
    /// node + member shapes, no degradation axis, no flow slot.
    Declared {
        node: NodeShape,
        members: Vec<(String, NodeShape)>,
    },
    /// A DECLARED locator whose raise missed.
    DeclaredMiss,
    /// The function has no served single-signature return carrier at all.
    NoSignature,
    /// A genuine producer refusal: the `FlowReturn` producer ran and
    /// answered with a typed non-value. This is the ONLY measurement a
    /// [`Flow::NoValue`] pin accepts.
    NoValue,
    /// The served signature fact carried NO return source (a bodiless
    /// overload or a synthesized signature), so the producer never ran.
    /// Distinct from [`MeasuredFlow::NoValue`]: collapsing the two would let
    /// a [`Flow::NoValue`] pin pass when the extractor dropped the return
    /// rather than the producer genuinely refusing it.
    Absent,
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

// The table

include!("u6_flow_shape_corpus_rows_tests.rs");

/// Locked baseline rows that were checker-correct, complete, and singly admitted.
///
/// The cohort is derived mechanically from the table: it is EXACTLY the rows
/// labelled [`Verdict::MatchesChecker`] whose flow lane is complete
/// (`Degr::None`) and singly admitted (`candidates: 1`), and
/// `flow_gap_retraction_preserves_clean_checker_matches` asserts that set
/// equality in both directions — so a row that stops matching the checker,
/// stops warming, or newly starts doing either fails here until the cohort is
/// re-derived. Each SHA-256 fingerprint is length-delimited over
/// `(script, probe, checker)`, so changing the authored fixture, its probe, or
/// its recorded checker answer cannot be hidden by re-pinning the remaining
/// row fields.
#[cfg(test)]
const CLEAN_CHECKER_MATCH_PRESERVATION_COHORT: &[(&str, &str)] = &[
    (
        "A01_spread_call_key",
        "d8a8f42a671509c0a02b379c89644b21c3a8e7ce69d381354571b93ef980b9d0",
    ),
    (
        "A02_spread_only",
        "3db59c6b2c9592e6a8a1041d23e48f2e5fdf79196abab0ce27bb6f05ef5fd13b",
    ),
    (
        "A03_two_spreads",
        "9af5cffc5b6b0400b9a53c15bb9c9e77be8a199eaab433dbefd81cfc614e14f2",
    ),
    (
        "A04_arrow_const_spread",
        "da669a75623648178a4f480ece639a38443df2106dc793d5531cb74f205b5c63",
    ),
    (
        "A05_module_const_spread",
        "af512e90ff6055e5c4a7939d3e3a2dc452357f6ec20c43409fc4a54ff7b21915",
    ),
    (
        "A06_plain_literal",
        "fb9b47c475cb7fc34202495244b5977f052c0b1492e73aa6e64dc95ce327aca8",
    ),
    (
        "A07_nested_spread",
        "c2fd98aeec489ba74b458087297f0f996710d6684f0d3f6a103ebe4a4183be2a",
    ),
    (
        "A08_cross_file_spread",
        "533076313fdd168c37330b38653fa493c0218345b0dd9c0d9d40c927a558fd6e",
    ),
    (
        "A09_iife_spread",
        "97c7ece5646952510cd0146c87c1027e4e07fdb2632d1cc4e50107a1f5d2e591",
    ),
    (
        "A10_spread_literal_arm",
        "9c0678cd03271901dbeeffa34cafc7d6af28de0f1cc0adf56198bb37c91a6c7c",
    ),
    (
        "A11_spread_empty_literal",
        "8be9ffc55188501ce69b7c974844f585aca770f1393593829441077bdfa02671",
    ),
    (
        "A12_override_after",
        "9f434e83799fa2b4fc007d3ad03bac21f4b8f864038d68e1c49b990e86f442e5",
    ),
    (
        "A13_override_before",
        "1d0e8abde00b7d47b3eb9441adad5432489eaa2360fa1c5620230f8208676ef1",
    ),
    (
        "A14_two_spreads_same_key",
        "f68536b0c2aa99c0077c48134c4463ea9d6f6d22a35f2a451495ca998c564188",
    ),
    (
        "A15_method_member",
        "381ab854ba91fb50f549b3d2ed07a1a1906151f87700fc808a66336761532b91",
    ),
    (
        "A16_getter_member",
        "6c3185695d8ccd9789fc11d44e6282cbc6e659f9dc22d0f41a296a72ec7ea058",
    ),
    (
        "A17_param_spread",
        "12954be31af7288278350482eba3af6f0a6244790a26ca8d0064e026aef4c598",
    ),
    (
        "A18_local_const_spread",
        "3f29d883b4a7de5c07fad9d6d791af628eb09a132d4b3fa11af050416b1d8836",
    ),
    (
        "B01_computed_after_call",
        "7c4f3b03b93cf8db633f72902e5678d40ad3d088c5073d25f1a2eef588a4f8b2",
    ),
    (
        "B02_numeric_key_after_call",
        "3ad9839b4051425accfd869e7414c75a56b798c9a9e66901fca444583151b9cb",
    ),
    (
        "B03_as_const_call",
        "0c6c96908cd1fd03ecde940f14c1d72ef317a12842f2a0d11d5411e8db90eebf",
    ),
    (
        "B04_as_const_spread_only",
        "e56087c67a873bbeed1842b47bc7fda4189bed7d5d327447084d73a24a4b430b",
    ),
    (
        "B05_satisfies_object",
        "ce1fe7fb18011f8f6b005f043d99849dd54574c7e743b943280db5346919c674",
    ),
    (
        "B06_two_calls_computed",
        "2d14d9d50752005536401531e44041f51515cc0a34e03c3f4633048175fa5c68",
    ),
    (
        "B07_computed_after_ident",
        "624488652a1357ec6e87efb5f150ef7a473aee633984f33ff1c3aa834f4e22e5",
    ),
    (
        "B08_computed_before_ident",
        "90e82531c179c5bb427a92b39d83631e56faaa7814510a6bf1ad764a222b74c1",
    ),
    (
        "B09_numeric_key_ident",
        "11944c4c0aaa48a63a2c6828de3b8f5b924dda5229f16550138529221680b94d",
    ),
    (
        "B10_as_const_ident",
        "c6bf05a6989852f0195adc0c13a71f73964a60c0d7cbb9f601936785c9b99275",
    ),
    (
        "C01_intersection_clean",
        "da6864ff976a6791bbd413e38dec200f534fb266699920567265b838a0000397",
    ),
    (
        "C03_emits_intersection_clean",
        "e5275f5110a60b2b581c4fd8160e9554239dccb3a06ff2f7a25056dbd3a37183",
    ),
    (
        "C05_withdefaults_intersection_clean",
        "f48a7cd3aa9458c025578349aa3e4fea122d949e67fa13d3de24593937471663",
    ),
    (
        "C07_heritage_extends_clean",
        "868308ec5bb4cba373e9a81e6a2fa933ff01ab45e7ba11a9dd8f92cc9b9f0d63",
    ),
    (
        "C09_heritage_members_clean",
        "e7391ffa9791e9064fca070d4494f2d00c8169ddbaeff0109cc8dcd4343194db",
    ),
    (
        "C13_emits_heritage_clean",
        "5a47b2cfe65c26e6a913289e4805974dfc8ed125dfcc6bd2f1f134a23f3cf249",
    ),
    (
        "CC03_as_const_plain_return",
        "7e730b8d0bcbb37eaf8a7f46630d08e828afdf5a14fc9da5574f39821b02820e",
    ),
    (
        "CC04_as_const_member",
        "f11f2a1f10757ad8334def70c1b21f6e961b8419ad07f9734448143ff170fc09",
    ),
    (
        "CC05_satisfies_literal_union",
        "aec83bc5654ab8d03e5f552cf3cfed3abc9dd886b94387da9e303faf644def53",
    ),
    (
        "CC07_annotated_spread_source",
        "c93c1afc2a56465c8bdde1ace16813094845a6e6bc8c9dffad1c8e4dfaa14283",
    ),
    (
        "CC08_contextual_through_call_return",
        "42820c30d07fe3a6bbe4cb033e24a35b4f38f3d53fbf462613c5b55f89c4b7d2",
    ),
    (
        "D02_param_reassign",
        "45dd83b6d25a0dcc47846814ce8928b9cdec12407cb2b0f6f56d9d5bbf711e93",
    ),
    (
        "D03_conditional_var",
        "f0a4b2404ecf8a8ea5ff231d890ad27965aeff526559511b08e6c342db2403a0",
    ),
    (
        "D04_destructured_param_write",
        "f4709dba61ebaac5df4635e845cfa81deda2487ddc9d3c1a91e85b7001bac666",
    ),
    (
        "D06_switch_return",
        "976de41e15c20e6bb63f258b0effe6bc96bb8cb4d168200fa4b91e73c8e011cb",
    ),
    (
        "D07_try_return",
        "d1caa663e0eac5cb7345193d7867e0495a85cea0527091e8463cc3536b0b4201",
    ),
    (
        "D08_overloaded_callee",
        "094addee6d5b49b37da84c00789b93f9bf2199ae708427f46814ba089fa2a407",
    ),
    (
        "E05_empty_literal",
        "954bd988d8bcf4862f58c7d2cf598d8debda29850d2e60ac6d1b031d09dc4d08",
    ),
    (
        "E06_write_then_empty",
        "1606cf6d9258d8e6b5e3cb14cc33ba85f109af469fa2f75b5c70bfd94509868b",
    ),
    (
        "F02_definemodel",
        "3d16614f0879fd7c0cf36f66368850b40c1c5ffc8159fc23dba25e6f8e57c2a0",
    ),
    (
        "F03_defineslots",
        "c8fe73525c50bcd0cf5e0044700bf79dd03920e1fb9146f4eb17dad713b7f007",
    ),
    (
        "F05_defineoptions_runtime_spread",
        "7b114f4e64129b071588adb16e0d01b5eab126bcafd32d60ccc49cbb2395f3ee",
    ),
    (
        "F06_path_projection",
        "392b4ebcc37d56e2ddb274272115b3647d21bdedbce94860136bdef5c6bf137b",
    ),
    (
        "G01_emits_spread_key",
        "a1912bcbdac07cbebc17a874b9ff767aebbdee5ab4719b5c13dbc8669b5be321",
    ),
    (
        "G02_emits_spread_only",
        "eff356446acfa776011b696977f11fe5c09bfe4c68ca04523f0031707d7441a2",
    ),
    (
        "G04_emits_cross_file",
        "241b26f80d2d9e3c1df1738f0000448ec029eb7940fc092229300378b57e4bcc",
    ),
    (
        "G05_emits_empty",
        "74331175227c26a42cd6f39a81328c0bb436ad1f7d94915fd3023a2cd60c5d6b",
    ),
    (
        "G06_emits_plain",
        "b35a15d3c017d4cefca5999425981c107abd2771ad6bdc2dd275e6547135d823",
    ),
    (
        "H01_generic_callee_spread",
        "4d9180489cf02f0db202f9358ee0fa74542d53590c7c58dad23630d5f5e80f15",
    ),
    (
        "H02_union_spread_source",
        "06297105fab2f3af96383561d9afc77910297d9a90af842aeeb2417919fe26a2",
    ),
    (
        "H03_self_recursive_helper",
        "bb7d08c4ab86ccca6c0e74cb096cf76eda456abf6ca2653d7fa517741729444f",
    ),
    (
        "N01_typeof_guard_ternary",
        "9aa50ec0801ec0520c07105ed551abc0ff6181599ab52cdb3302f193dbbbd8d8",
    ),
    (
        "N02_typeof_guard_block",
        "c8f823e583e1044fcb64dc553bd25f1750aa79e0261debeece73bdabc4085329",
    ),
    (
        "N03_truthiness_guard",
        "5373413a8a2857e17878d39974cbdc78d32996815af11e02ba8de115995bd16f",
    ),
    (
        "N04_discriminated_union",
        "bfd4550b08587ce5947f4e1f67f85dc9b0671895997963caeb5cb30bbf187db7",
    ),
    (
        "N05_in_operator_guard",
        "719576721133ad069f6b50e4f76f4bb6fd524ec0455eae1cda6f90ba02ba5fb8",
    ),
    (
        "N06_instanceof_guard",
        "813bb79b3bca159438343b49f636614830e943475d1961a5cddddb3f1844be9a",
    ),
    (
        "N07_branch_join_widens",
        "ffebd8822e84d0f6f963bd5a06380795d235d5f8ce925aa28458b7c8fc366693",
    ),
    (
        "N08_predicate_across_call",
        "1fe3e8096ef87dc97ebe90f4de989557ad3fb7541df369b6a578bc3ed78e8eac",
    ),
    (
        "N10_assertion_signature",
        "d5df30bcdb839bddeedd0fa447035ff14a4f6f14c7bd4975a64f12850c1a858c",
    ),
    (
        "N11_narrow_survives_call",
        "a4bed3076db8008d6231b32050c88106291cfbcb475c1dc36c38dca12bdf42a2",
    ),
    (
        "N12_literal_union_narrow",
        "396d3492770b69c3dc2dd6a5c56b1def66dc34b93fd10f606cb6480862fc8651",
    ),
    (
        "N13_nested_property_guard",
        "984f7aeccd7334c9e70a0e5fdf35018a32beb912847e256f8c88f2b45e788611",
    ),
    (
        "N14_negated_typeof_guard",
        "490f4549969280b64337a8811b694c94e91f07f61b4950b068209b9dc24333b2",
    ),
    (
        "N15_terminating_typeof_guard_negated_edge",
        "b665e479092ed2a3b77bb3d127cce5dcf36c9251af262c1a0d49b68c9856961a",
    ),
    (
        "N16_terminating_predicate_guard_negated_edge",
        "f40a64b5596b2bda505284e25cc6572b0ed4a12cc6990ee30afcf86c0679404b",
    ),
    (
        "N17_targetless_asserts_drops_falsy_arm",
        "05db4eb3de4a0f72f036cc1520062aff7f4862c8d34d73efe556c18d32da761c",
    ),
    (
        "N18_logical_and_opaque_false_edge_keeps_union",
        "ce4c6d7b35763aeb7d45ad206507d60bf4aac043f09f44d8bd68a13dc9eb66c0",
    ),
    (
        "N19_logical_or_opaque_false_edge_negates_modelled",
        "eff2c2e2e5d8b35ac4eb0ff9761d90f6979d01a945ce1cd02a3d2eaf7c98256c",
    ),
    (
        "N20_negated_conjunction_recovers_predicate",
        "625279b5457dc903accb6322989a447461fddd25cbc766b55324556e4432f836",
    ),
    (
        "N21_guard_union_uses_final_alternative_overlay",
        "c439f2566c473af6c3b8b029b6c63f1e915d388fa53ffa8dee17a9c94cf07a34",
    ),
    (
        "N22_double_negated_guard_union_uses_final_alternative_overlay",
        "ecdd6ca1f3441f10c6f649b2e256b25b34664d506c0aabdd29d1bec34ccf6694",
    ),
    (
        "N23_impossible_conjunction_drops_dead_disjunction_alternative",
        "6912ee53c2ff8f51576c96c1ed4a9979040af88a277a7058e85e928d21d19761",
    ),
    (
        "N24_impossible_predicate_ternary_omits_dead_contributor",
        "dc9516ff00fb032afd1c7e4fc9844b95e3b18a681381bbcb015ab675cace97a3",
    ),
    (
        "N26_structurally_possible_predicate_intersection_survives",
        "ca413199ab0bff1dde03fc9e6c7a418890599f4325a608716909e5c50cac0c68",
    ),
    (
        "N31_discriminated_union_switch_positive_control",
        "82864336129b447c34bf97d99dd936464a993c1c7a6112ab35892ffa151ca93c",
    ),
    (
        "N38_postfix_non_null_wrapped_guard",
        "ea4d706da28a3e569aefa75f45af245cd496d8a55295927c5b7d9d9d58bfabe8",
    ),
    (
        "N40_as_wrapped_guard",
        "c641157ff36c975ad319a9643cbbf6f2e1e6f9a31a0c5dc09122d66502bdf4a3",
    ),
    (
        "N45_destructured_parameter_discriminant",
        "a80e5dc02aaf92fc332064af44e9debe551db7e7e0cfbbaf6cf917f279c5330e",
    ),
    (
        "N49_closure_narrows_own_parameter",
        "7c3c9f675623866d1625c5ddbda540c416b751080fcefc6983d9c8c97e9ee4f0",
    ),
    (
        "X01_spread_narrow_arm_source",
        "affeead1e071a2ba50875d5bfcfd86a842b79d4139751a55b03a172d5a33a25f",
    ),
    (
        "X02_spread_with_narrow_member",
        "41a2489e8207b86373c98f2c2f54fe2d90537696304075bcefb00295f2d1316d",
    ),
    (
        "X03_narrow_member_spread_sibling",
        "fc0690148d35b4798acc4ee0014ebe2c5787b88df0ee7edf281a82b5117e7bf6",
    ),
    (
        "X04_try_catch_join",
        "0077a76ce1f41b30f530e5d2d6bd6e693294f87fe00c0b3ac8cd2309e17e203d",
    ),
    (
        "X05_catch_return_fallthrough",
        "4cfa3405c9157f1e18ca5b6440146f993e40e978375229443acd4a051e8203d3",
    ),
    (
        "X06_iife_return",
        "cc0b45c70f68f733f8b6a45b4e472d57b356a94df697894b3e02e7425586e71b",
    ),
    (
        "X07_local_arrow_return",
        "d7e4b747ae06c9a98501eb2bfc02cbe7f1c39b514793a1838d557af9a0f3b5c5",
    ),
    (
        "X08_generic_two_instantiations",
        "80566aa919d692de9f5e5e7df4e0b8ac82ed9097532b92bc1173689aeba7a6bb",
    ),
    (
        "X09_generic_wrap_return",
        "338375339d6dcdedd10c9764ecdcac896b9ca3700c5f5f8e472b92e3dd142a6b",
    ),
    (
        "X10_destructured_default_conditional",
        "0287338e681f2a3272daa33188661401def01c8e02ad17bd1b71fa2b5d95f2eb",
    ),
    (
        "X11_class_static_method_return",
        "062c14da3e3b4da3b0a3df5dd891a821ccaca1dc35f47a032ee002892a36b084",
    ),
    (
        "X13_proto_entry",
        "cb01ba30111e946baf65f4464db509350d8d20b324e41d0417ad447fdccde942",
    ),
    (
        "X15_labelled_block_return",
        "8e2e1c8361cd81914345df51e2bca7ab7504ae3679c5a0d3f95d6d0e506382aa",
    ),
    (
        "X16_switch_fallthrough",
        "da6e096ca75e743743cf48d63f121f803c598115fec9705b8472acde93f03aab",
    ),
    (
        "X20_as_const_spread_source",
        "8d96e558b8acd65d989e885644bc0821703409eaac8bdbf38bf4704fe22ba549",
    ),
    (
        "X22_switch_break_case_entry",
        "c76dfe56935d20c486ad3695ee27b779e464d342d25d67178c8c7b892636f6be",
    ),
    (
        "X25_try_assertion_catch_scope",
        "095993c48c8f8979211c6c00984ccb8b4a8516d601f2e1fea448eafaaffb6ad8",
    ),
    (
        "X27_finally_fallthrough_break_override",
        "d2288851e3a19b88563f2253ed7722f95191df0682650547e190f8dd3b58013c",
    ),
    (
        "X28_destructured_default_undefined",
        "78d7fad70e8c43d76373c2a0f5071eb22b510e93a09999dd11e562d5be1b6ffd",
    ),
    (
        "X29_write_annotated_union_write_widens",
        "f218f5f4b7e5b1d0aa5712ca5e5faecb89d88de617e5fa4f654f9013fffadc58",
    ),
    (
        "X30_switch_terminating_arm_write_fallthrough",
        "dad8c18d9ca02b4cd7734af7d8a517198f03e8f5efe3b5182e59dfed346c262c",
    ),
    (
        "X31_switch_default_break_state",
        "7c9a1e226b3220d42b762835dfc7e4b6cdc2c558bce1589915e5b1902e8b52f2",
    ),
    (
        "X32_switch_exhaustive_single_case",
        "8059dbe313344addd581530cd3151e0549212d73565c061ffc75f4a20c282088",
    ),
    (
        "X33_switch_case_narrows_discriminant",
        "edbc739e7f7d079a4ccbea1f708c8237307e2ae2efdb9b167a4c8746d89b030e",
    ),
    (
        "X34_switch_exhaustive_union_no_implicit_undefined",
        "afbcc9b359791620e69a38bba8509b03f0c0aa0aba7164717bf7e162d391d0f6",
    ),
    (
        "X35_labeled_break_carries_write_state",
        "2dd449fce9efd3e5cc8118b1e9095c288579fd996f47d9bf466e3085cb397242",
    ),
    (
        "X37_labeled_conditional_break_write",
        "6d60cab979362bebab7da91d67f85471c25d0d69f8294d22653128a6e89b9f2d",
    ),
    (
        "X38_switch_conditional_break_write",
        "1829d8babb7153432c4f63a983ba1f92f6e12ec4247e7f76888efb677d470e5b",
    ),
    (
        "X40_finally_write_to_outer_let",
        "04ae6d2e3017acb7e1c16195e4e79bc4676e76b2159061d9ea695ba8ef75201a",
    ),
    (
        "X41_try_write_survives_finally",
        "3efdc46cbc6161b6aa463e25e5e9a7134c25529bb4e8a5227256797b7a6f6b23",
    ),
    (
        "X42_destructured_default_alias_undefined",
        "c43b3adc4eee73bdfe04010e2ddad65425791a5ac62e935ddba6eff1fa52954b",
    ),
    (
        "X43_plain_block_write_survives",
        "2865d6d01d95b78a56d9c7a9541ccf86a519ebb7d482916b181165dbaf535811",
    ),
    (
        "X44_switch_exhaustive_boolean",
        "34a9b1a6c4c4d5beece191a0befbe3c1e3bbe6c52176d154fe8869f06c72da1a",
    ),
    (
        "X50_switch_break_exit_closes_crossed_scope",
        "b4f88a4c10802e517d8273751c23123168dc82d85b4bb1c947745fc05ad1bfe9",
    ),
    (
        "X51_finally_write_not_on_abrupt_edge",
        "7788f7e8640c12051a56378d1636e025ab5d0a52341929e9fcb8e6901128ce17",
    ),
    (
        "X53_terminated_if_arm_contributes_nothing",
        "96e5c602603d248508f332c76865a42f6d10283be4c666571925b26ad8e6035f",
    ),
    (
        "X54_switch_live_fallthrough_reaches_default",
        "8cbaee0ee56f97419445580fcf503d33040fe46e45d7f68eb3424994f8318cc8",
    ),
    (
        "X55_finally_entry_joins_pending_return",
        "a6df0726392860562e15dc03905b422d7c33bca188bb8375a4165b69421fcb4d",
    ),
    (
        "X56_finally_return_preserves_try_return",
        "149610904ebc89a0d10914f76044acd8db25ea734b139425095ad4d4b1768b7b",
    ),
    (
        "X57_if_arm_closes_lexical_shadow",
        "a4fd3c1f7a89d38b66d1ae4c22994f939b2ea656dcad6089000125fd4722ad43",
    ),
    (
        "X61_finally_break_preserves_own_exit",
        "d3ead9f133fafd4379a26ec537f64f88f15bc01866a6404a422e6e8a928ed86e",
    ),
    (
        "X63_hoisted_var_no_init_preserves_write",
        "18baab1bafad589daac5f3395adbc41f41c1b83801d3852efe9de4ba2f1d5c4d",
    ),
    (
        "X64_inert_arithmetic_loop_stays_transparent",
        "eb9f486ba8a0951c662246b67e5e7ec796ed0225e3e3062a3bb84746f694785c",
    ),
    (
        "X65_object_assignment_declared_union_selects_matching_arm",
        "45a75dc56bb6bc90b1af0b566df25e3969eede7a6c7e2f6f4b8473b4867af487",
    ),
    (
        "X66_hoisted_annotated_var_authority_serves_forward_read",
        "e8bfcb32b5183ec851d366e4912ce939b84011e121df973c622c521573374f78",
    ),
    (
        "X67_destructured_parameter_authority_precedes_writes",
        "be3bd8c02927656eb820d725ab99382412f84525beb21ec15dc176156e683726",
    ),
    (
        "X68_finally_return_over_labelled_break_keeps_undefined",
        "5c368cf5703cf131c0edf3176d4b9f65927d51ef16f890ebbed359e57dccfac3",
    ),
    (
        "X69_overlapping_object_union_assignment_selects_narrow_arm",
        "352e71421171ceb0fc69ca5ea7afdc6d25073b7684a23db0255d3d958ba2d7fc",
    ),
    (
        "X70_loop_callback_argument_is_not_invoked_closure",
        "466661520fde20758c2ee1ed81e343651d1f790ff7ba034879ed8511d44e8793",
    ),
    (
        "X71_loop_member_write_compares_full_selected_path",
        "b787466e01699546f873ec83c4e126bed767d2a32323e1872ab540fb7cc5f6e5",
    ),
    (
        "X72_loop_unreachable_write_does_not_trigger_refusal",
        "fda9dd71faba9e328f21ef18061121951626801c9d34655d41ad6639a71889b0",
    ),
    (
        "X75_fresh_object_assignment_selects_optional_member_arm",
        "aa43bf8c938a971ea596b817b45a63d353b5adc2b3f074f8e698590ba5b327be",
    ),
    (
        "X76_fresh_computed_object_assignment_selects_optional_member_arm",
        "c53d678a5dd9dcbce5f0af0aeab12c1b6ea354dbaf54e59f7fbc5616cc67749a",
    ),
    (
        "X77_spread_object_assignment_preserves_declared_union",
        "d000c6c704fd9dc15616e02997ccec819e82d27158ee541f14588a616a51817a",
    ),
    (
        "X78_mutable_var_capture_uses_declared_authority",
        "ba51ed57ebbd9515212c8ce00374ef64c5c25178873b153436fe9c92aabf848f",
    ),
    (
        "X79_forward_let_capture_uses_declared_authority",
        "b5c55f6a6d8e1a9b1e4849598400054ec392eefbe11d7af43966c8306eb20993",
    ),
    (
        "X80_wrapped_labelled_try_finally_keeps_undefined",
        "423a15e43f1c9b598cc706b8c4d58f5764e2c44aca16e23eaa3bce09067c1233",
    ),
    (
        "X81_while_false_body_write_is_inert",
        "c1c61c4d0855907ba23fa8b3ef7400f20c4e8d441229274144c189535d8e2851",
    ),
    (
        "X84_required_property_assignment_preserves_optional_union_arm",
        "195e600cceaf21a1ecce17c510bf0f9ee43d46ba4a48521c7e7fd0c7682aa9be",
    ),
    (
        "X85_nested_closure_write_updates_captured_binding",
        "481ba4c6ad6475ca2b6b34c0986751479c9010a1d23c40c347ec6e72ddbd09e0",
    ),
    (
        "X86_destructured_parameter_capture_retains_declared_union",
        "c1f61c49cd5893520ada185cf82a2d09398f24acd6131495ea90850a23813f13",
    ),
    (
        "X87_read_only_let_capture_keeps_reaching_literal",
        "95300d65ae6acc0d45ec9d604731000ba1907e187c1aa12fbcf366e3d1c026f3",
    ),
    (
        "X88_nested_label_inherits_enclosing_suffix_return",
        "76fa69e05ef1bc843a3d649da249b426144ad4a8f37a93c8265d4be987b3a66d",
    ),
    (
        "N58_predicate_targets_second_parameter",
        "67937178ac7fa11755183b7de413da72c13210550a6606d39e40cd8e160a87b9",
    ),
    (
        "N63_two_discriminants_conjunction",
        "b7d1e5359cb077889bb12447e27a7590144da48791ce7e3270d27a845dc10d5c",
    ),
    (
        "N64_boolean_literal_discriminant",
        "c13f0591d4c8cbad6f9598e8f0416e78ccd0fa510563558e51bafc52240f2f8d",
    ),
    (
        "N66_shared_nonliteral_property_is_not_a_discriminant",
        "c79aa1eef3b0955b01462e490e414915f527e0e73f7c81dd6835be7eda251523",
    ),
    (
        "N67_intersection_arm_discriminant",
        "fa77124287966e04ecd8b1f5e1554c68b83216157ef1a3b560ef208f859eec25",
    ),
    (
        "N68_template_literal_discriminant",
        "d0cac39054be89c7afdb7cce4bae21d3fc5fc280af6b3039073a3fd7f63ab981",
    ),
    (
        "N71_in_operator_optional_member_keeps_undefined",
        "216313b7c5a1ee161a92125b9e43c98bb3478ce97fcb255af8dd11b768382eb9",
    ),
    (
        "N73_typeof_object_keeps_null",
        "95343cc9fd971495876ad1406249ffc7167c107bea92a77eade69daaa71d4d54",
    ),
    (
        "N77_strict_not_null_keeps_undefined",
        "64a93578211c164bb4f545454879b4d63be0bace35905d5f22e30629b66ea1f8",
    ),
    (
        "N78_strict_not_undefined_keeps_null",
        "501aee0440f7bfb526a1235fe6e75f55e6e9d314f31cded0cca0ca22c0bd497e",
    ),
    (
        "N82_falsy_branch_keeps_empty_string_literal",
        "74493204e6a526ecf7c6b24c59635076e3b8f3c7e241cda0623723f229ff73bb",
    ),
    (
        "N83_optional_property_truthiness",
        "efdc012efc005f13011543f60358f17e89b529cd7cd29701231e972d01b1f111",
    ),
    (
        "X89_never_returning_call_terminates_branch",
        "9a4b4247b867ee52ca9b88aeeb0d0699ba0600899668f133f4f5ade8977d2ffa",
    ),
    (
        "X92_if_false_branch_still_contributes_return",
        "d9f042edc82799e267929c202c6a63b2956f4ec2cf8fdc8711d2dcd7982f2b70",
    ),
    (
        "X93_if_true_branch_keeps_fallthrough_return",
        "d7b616ee9e42758ab6c958c3f50839790cbd088d0441663fa5c4a7c6d64ac479",
    ),
    (
        "X99_nested_try_finally_collects_every_return",
        "46933021df276dc95d823af15b07d58e2c20f7dc05ce61b5d55cc897b4703772",
    ),
    (
        "X100_switch_default_between_cases_source_order",
        "2ca64ebeb8d8025b0823a9374a44c5fc57fc542f7a691dc794b543a3edbd6274",
    ),
    (
        "X102_literal_return_absorbed_by_string_sibling",
        "2973459cfe70ee6277fa671f27597f13e11f7cc5bb0fcc93a7d647c8fa101095",
    ),
    (
        "X104_void_arm_not_absorbed_in_union",
        "e845a45b10ce2a1e02d00459d845fcf3513277f16da200970e4d48a9a97f1555",
    ),
    (
        "X106_triple_nested_closure_return",
        "2e3602b0461a1531ef1c06bfd59b5ba3dd124c83222ac2b01d2d331f4d953340",
    ),
    (
        "X111_guard_clause_return_then_use",
        "29b1b1b45391fb6e864797a6de8fec98ed65489300d35db1ac6d1cd12f68ff42",
    ),
    (
        "X115_union_alias_passthrough_keeps_alias",
        "9f2c707f961ff0630cb1633a1ec3eec7abf371845f3126b5918e2e8866b01ca4",
    ),
    (
        "Y01_union_never_arm_collapses",
        "fce25953d5f1e1ab45d1e4826885061aed0f21ec9e5128d745b9525f3edd8969",
    ),
    (
        "Y02_union_idempotent_switch_join",
        "24d2efa40c539c51ff636d111a18c90418964f425add1c69967267118e50bf2b",
    ),
    (
        "Y03_disjoint_scalar_intersection_member",
        "30f97e989e158f33f9f20ce2af9f7b55bad03e663803b4c6ae336bcbeb290735",
    ),
    (
        "N85_uninhabited_conjunct_keeps_sibling_subject_contributor",
        "04d1f801c7a70decc002101c5e131908ce6746d64d2d5194fab2c6a3b17288c5",
    ),
    (
        "N86_uninhabited_conjunct_keeps_sibling_in_ternary",
        "3ed99725de4ca09d9646f2061ac4342e02639478dd1808eb08f4b68774142367",
    ),
    (
        "N87_uninhabited_negated_disjunct_keeps_fallthrough",
        "6283bce4221d024df9396e470284aed4f60e8b26ebde3287ef8618b51d6e48b0",
    ),
    (
        "N89_in_known_key_filters_arms_exactly",
        "1491c269e3bf8a82857f8c06d2169180b8e21d1536c7ce7da633be31efeaf37a",
    ),
    (
        "N91_typeof_function_over_member_surface_reads_never",
        "dbd07ccc18231dafc886ff3256758ae19486616b6b4cde25a12ad44400d87b5a",
    ),
    (
        "N92_instanceof_unrelated_class_intersects_whole_subject",
        "5b07bcbff3b37bec6f99a9d10cbbb302a49abb3f48136249fe5acd2f0f1189bf",
    ),
    (
        "N93_instanceof_strips_nullish_before_the_intersection",
        "67a7ac428ba6b57aefdd4a9d249f5298cff4aaf9f50e449cc010f2c6bee808ee",
    ),
    (
        "N95_instanceof_related_arm_drops_unrelated_class_arm",
        "50706288d26387ef6e464f1b79e31e783bff2bb561539cc26e0a24c37357217e",
    ),
    (
        "N96_branch_join_mixed_pinned_arm",
        "cb79cc4a3063944f86f890e1450c0a2ee0a9c30c6136be2705e3fb0a74e3afbc",
    ),
    (
        "N97_widening_const_read_through_let_initializer",
        "1caafb1623f5e48fea49e924dd69609a23d678693a10bf8e70e23fff1797f157",
    ),
    (
        "N98_fresh_call_join_sibling_pin_number",
        "167a1298a202e5b84e7653503842a755f8d10a3653967a110490b7bf6270c5d2",
    ),
    (
        "N99_fresh_call_join_sibling_pin_string",
        "e07427226c7817232be8773c115f09f489cc5eb6f061701d4098f9a08461e288",
    ),
    (
        "N100_fresh_call_join_sibling_pin_boolean",
        "73a488a8622baa3e8f379b6d7f39bfbcce1b86d97049ea7945983e398ad857f0",
    ),
    (
        "N101_fresh_call_join_sibling_annotated_const",
        "aa83cbf9492ad6a69955f9cddd598ae272e909720fc1edd4cb5b0d4968b4633c",
    ),
    (
        "N102_fresh_call_join_both_arms_widen",
        "a9dc78d4b6e884fb76f15a783bf99814af77370340196f90c7986c3e7a96afd9",
    ),
    (
        "N103_fresh_call_join_distinct_literal_arms",
        "d2f6e5f3d73f3f4e102b13df9eefb3ff6bc0e3debe546b4b822e24c62ace7c1c",
    ),
    (
        "N104_declared_union_null_keeps_literal",
        "689c37e2ebb842cd2d183e364e987589128a22c1b6faf2c24aaefc945a627848",
    ),
    (
        "N105_declared_union_undefined_keeps_literal",
        "288f820f2838008e888ed32300f5491273093421f68a3fc70b58b5ff69a995c9",
    ),
    (
        "N107_declared_union_join_with_pinned_arm",
        "d6b57f1e9493fa43db04d5345693e65734eef30be7b8fd60e22e60786ad688c5",
    ),
    (
        "N108_flow_inferred_union_keeps_literal",
        "f00249216ff49425be836597d07ffbe535033a71ec3386e6c832fb23fc5fae4a",
    ),
    (
        "N109_declared_union_explicit_type_argument",
        "4014b768a705ef0f7a1dc67be225114c24589e7776e35b62d37290341bdda379",
    ),
    (
        "N110_member_fresh_call_widens",
        "7bb9ef2cb6d3f91db73bcb064a0ffbd2bbe83e43439410da04f05415deda9e4a",
    ),
    (
        "N111_member_fresh_call_binding_widens",
        "33f929f2a20bd44ab99a2f0aaa4a7db9f4ad48d67670c66ac1858607a9d854a6",
    ),
    (
        "N112_member_fresh_call_nested_widens",
        "fbe4785c5b81e6eeb0148562593aa16f3db6c33bfb4d062520f149d49e971aff",
    ),
    (
        "N113_member_union_call_fresh_arm_widens",
        "0275f1d63d147cad7b5aacb87ab5cea9b049b882f82e586ec8f9e7f5d43e2791",
    ),
    (
        "N114_member_union_binding_fresh_arm_widens",
        "4fc67e22ad714259b7eccbca71856c16444f928410ef8c167f5159466514c93d",
    ),
    (
        "N116_binding_fresh_call_const_widening_read",
        "e3d55514de38ce18f68aa113a412a677bc1d1792179fbbccf70ff4ec07274c48",
    ),
    (
        "N117_binding_fresh_call_let_widens_at_decl",
        "eee481bb472bedb1822f599e15233c5ae25347d86992aeb6e5ee10c2579fafb3",
    ),
    (
        "N118_binding_union_call_const_return_pinned",
        "39e44ff409e42563045a21e38968d375f4b884ed4e14ba487c3752ae5adca759",
    ),
    (
        "N119_binding_union_call_let_widens",
        "f7a26551c6a84eba01426272f591d99d7402ddd5ccacecfa9e8ac3f0dff3d77d",
    ),
    (
        "N120_membership_through_const_initializer",
        "92d8ba410f7220598da6805a8eaf504aee92cfe5c93c8b74f5b2fbccfc49ff52",
    ),
    (
        "N121_partial_membership_through_const_initializer",
        "70d988ead7561445735c5f83937388dd1266ca06125f2565e6965a35161b34e5",
    ),
    (
        "N122_membership_through_let_initializer_return",
        "a41c746c0447cb4204efe4c68e095d549aebac9b0201a432e6506d21de150b78",
    ),
    (
        "N123_mixed_pinned_same_literal_stays_pinned",
        "192375a983c1b95f322735a12999e83b5f465c43020cddce455c28fa7939e6e9",
    ),
    (
        "N124_mixed_fresh_and_call_arm_member_read",
        "efa1cf34c335c4c63ac3267c06050c70c3601058cbc12998a7a4fb0212b04d84",
    ),
    (
        "N125_mixed_pinned_arm_return_read_stays_pinned",
        "03fa1a0181fff39c32b1ba8c44cb33ac071fa5adf50925e230e7e373d1af7fd9",
    ),
    (
        "N126_all_fresh_conditional_return_read_stays_union",
        "1305271c5a32b029c7bf278529408ccac71eaf00f1beffd46e5744c884be9df3",
    ),
];

// The suite

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
            Verdict::MatchesChecker => ("MatchesChecker".to_owned(), row.owner.id().to_owned()),
            Verdict::FailsClosed => ("FailsClosed".to_owned(), row.owner.id().to_owned()),
            Verdict::Degraded(reason) => (format!("Degraded({reason})"), row.owner.id().to_owned()),
            Verdict::KnownOwed { note, .. } => {
                (format!("KnownOwed — {note}"), row.owner.id().to_owned())
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

    /// Under `U6_CORPUS_DUMP=1`, print the exact probe program for every row
    /// so the `checker` column can be REFRESHED without hand-writing anything.
    /// Piped through the pinned tsgo, its output IS the column.
    #[test]
    fn corpus_probe_programs() {
        if !dump_mode() {
            return;
        }
        println!(
            "PROBE ── write each block below as <id>.ts, then run:\n\
             PROBE     ls *.ts | xargs <tsgo {TSGO_VERSION}> --noEmit --strict \
             --ignoreConfig --pretty false"
        );
        for row in CORPUS {
            if !row.aux.is_empty() {
                println!("PROBE FILE {}__aux.ts\n{}", row.id, row.aux);
            }
            println!(
                "PROBE FILE {}.ts\n{}",
                row.id,
                probe_program(row.probe, row.script)
            );
        }
        panic!(
            "U6_CORPUS_DUMP=1: probe programs dumped above; corpus_probe_programs EVALUATED \
             NO PINS in this mode. A dump run is measurement, never evidence — re-run \
             without U6_CORPUS_DUMP for a verdict."
        );
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
            CORPUS.len() >= 87,
            "the corpus is APPEND-ONLY: it landed with 87 rows, and a change that shrinks it is \
             deleting measured coverage, not refactoring it (got {})",
            CORPUS.len()
        );
    }

    #[test]
    fn flow_gap_retraction_preserves_clean_checker_matches() {
        use sha2::{Digest, Sha256};
        use std::collections::BTreeMap;
        use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
        use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

        fn fingerprint(row: &Row) -> String {
            use std::fmt::Write;

            let mut hasher = Sha256::new();
            for field in [row.script, row.probe, row.checker] {
                hasher.update((field.len() as u64).to_le_bytes());
                hasher.update(field.as_bytes());
            }
            let mut encoded = String::with_capacity(64);
            for byte in hasher.finalize() {
                write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
            }
            encoded
        }

        fn identity(canonical: &str, symbol: &str) -> FlowFunctionReturnIdentity {
            FlowFunctionReturnIdentity {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from(canonical),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: Arc::from(symbol),
                    space: LocatorSymbolSpace::Value,
                },
                function_part: FunctionPartIdentity::DeclarationBody,
                overload_ordinal: 0,
            }
        }

        fn candidate_count(host: &Arc<VerterHost>, canonical: &str, function: &str) -> usize {
            use crate::semantic_query::{FlowInputContext, FlowReturnKey, ReturnProjectionDemand};

            let store_view = host.resolver_store_view_read().into_owned_view();
            let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            let host_ctx =
                crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
            let dispatch = ProjectSemanticDispatch::new(&host_ctx);
            let key = FlowReturnKey {
                function: dispatch.flow_function_slot_for(
                    Arc::from(canonical),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    Arc::from(function),
                    FunctionPartIdentity::DeclarationBody,
                    0,
                ),
                normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
                context: dispatch.flow_return_context_for(canonical),
                demand: ReturnProjectionDemand::whole_return(),
                input: FlowInputContext::empty(),
                result_contract:
                    crate::project_semantic_dispatch::flow_solve::flow_return_result_contract_id(),
            };
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)))
        }

        let locked: BTreeMap<&str, &str> = CLEAN_CHECKER_MATCH_PRESERVATION_COHORT
            .iter()
            .copied()
            .collect();
        assert_eq!(
            locked.len(),
            CLEAN_CHECKER_MATCH_PRESERVATION_COHORT.len(),
            "the locked baseline preservation cohort contains a duplicate row id"
        );

        let mut current = BTreeMap::new();
        for row in CORPUS.iter().filter(|row| {
            matches!(row.verdict, Verdict::MatchesChecker)
                && matches!(
                    row.flow,
                    Flow::Result {
                        degradation: Degr::None,
                        candidates: 1,
                        ..
                    }
                )
        }) {
            assert!(
                current.insert(row.id, fingerprint(row)).is_none(),
                "duplicate current cohort row id `{}`",
                row.id
            );
        }

        let locked_ids: Vec<_> = locked.keys().copied().collect();
        let current_ids: Vec<_> = current.keys().copied().collect();
        assert_eq!(
            locked_ids, current_ids,
            "locked baseline clean checker-match cohort changed; locked={locked_ids:?}, current={current:?}"
        );

        for (id, locked_fingerprint) in locked {
            let row = CORPUS
                .iter()
                .find(|row| row.id == id)
                .unwrap_or_else(|| panic!("locked baseline row `{id}` is missing"));
            assert_eq!(
                current.get(id).map(String::as_str),
                Some(locked_fingerprint),
                "locked row `{id}` changed script, probe, or checker fingerprint"
            );
            let (function, expected_node, expected_members) = match row.flow {
                Flow::Result {
                    function,
                    node,
                    members,
                    degradation: Degr::None,
                    candidates: 1,
                } if matches!(row.verdict, Verdict::MatchesChecker) => (function, node, members),
                _ => panic!(
                    "locked row `{id}` must remain MatchesChecker with Degr::None and one candidate"
                ),
            };

            let host = u6_flow_expect_tests::make_audit_host();
            let dir = "/flow-gap-preservation";
            if !row.aux.is_empty() {
                upsert(
                    &host,
                    &format!("{dir}/{id}__aux.ts"),
                    row.aux,
                    FileLanguage::script_ts(),
                );
            }
            let canonical = format!("{dir}/{id}.ts");
            upsert(
                &host,
                &canonical,
                &module_script(row.script),
                FileLanguage::script_ts(),
            );
            let ident = identity(&canonical, function);

            let first = host.get_flow_return_type_with_audit(
                &ident,
                crate::semantic_query::ReturnProjectionDemand::whole_return(),
            );
            let first_audit = first.audit();
            let first_payload = first_audit
                .flow_return_inference_payload()
                .expect("first public call carries flow audit payload");
            let first_result = first
                .as_result()
                .unwrap_or_else(|error| panic!("locked row `{id}` first call refused: {error:?}"));
            assert_eq!(
                first_result.degradation(),
                None,
                "locked row `{id}` first call degraded"
            );
            assert!(
                !first_audit.from_cache,
                "locked row `{id}` first call must be cold"
            );
            assert!(
                first_payload.cold_computes >= 1,
                "locked row `{id}` first call must perform cold work"
            );
            assert_eq!(
                candidate_count(&host, &canonical, function),
                1,
                "locked row `{id}` first call must admit exactly one candidate"
            );

            let first_node = first_result.return_type();
            let first_json = host
                .project_node_to_type_expr_json_bytes(first_node)
                .map(|bytes| String::from_utf8(bytes).expect("TypeExpr JSON is UTF-8"));
            {
                let store_view = host.resolver_store_view_read().into_owned_view();
                let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
                let host_ctx =
                    crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
                let dispatch = ProjectSemanticDispatch::new(&host_ctx);
                assert_eq!(
                    node_shape(dispatch.graph().node_data(first_node).as_deref()),
                    expected_node,
                    "locked row `{id}` root node pin changed"
                );
                assert_eq!(
                    member_shapes(&dispatch, first_node),
                    expected_members
                        .iter()
                        .map(|(name, shape)| ((*name).to_owned(), *shape))
                        .collect::<Vec<_>>(),
                    "locked row `{id}` member-shape pins changed"
                );
                if let Expect::Node(expected) = row.expect {
                    let failures =
                        u6_flow_expect_tests::check_node(&dispatch, first_node, expected);
                    assert!(
                        failures.is_empty(),
                        "locked row `{id}` recursive semantic pin failed: {failures:?}"
                    );
                }
            }

            let second = host.get_flow_return_type_with_audit(
                &ident,
                crate::semantic_query::ReturnProjectionDemand::whole_return(),
            );
            let second_audit = second.audit();
            let second_payload = second_audit
                .flow_return_inference_payload()
                .expect("second public call carries flow audit payload");
            let second_result = second
                .as_result()
                .unwrap_or_else(|error| panic!("locked row `{id}` second call refused: {error:?}"));
            assert_eq!(
                second_result.degradation(),
                None,
                "locked row `{id}` second call degraded"
            );
            assert!(
                second_audit.from_cache,
                "locked row `{id}` second call must be warm"
            );
            assert_eq!(
                second_payload.cold_computes, 0,
                "locked row `{id}` second call must perform no cold work"
            );
            assert_eq!(
                candidate_count(&host, &canonical, function),
                1,
                "locked row `{id}` second call must retain exactly one candidate"
            );
            let second_json = host
                .project_node_to_type_expr_json_bytes(second_result.return_type())
                .map(|bytes| String::from_utf8(bytes).expect("TypeExpr JSON is UTF-8"));
            assert_eq!(
                second_json, first_json,
                "locked row `{id}` warm output changed"
            );

            match row.boundary {
                Boundary::Audit {
                    json,
                    degradation: Degr::None,
                    warm_replay: true,
                } => assert_eq!(
                    first_json.as_deref(),
                    Some(json),
                    "locked row `{id}` exact public output pin changed"
                ),
                Boundary::Skip => {}
                other => {
                    panic!("locked row `{id}` carries an incompatible boundary pin: {other:?}")
                }
            }
        }
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
        if dump {
            panic!(
                "U6_CORPUS_DUMP=1: measurements dumped above; corpus_runtime_lane EVALUATED \
                 NO PINS in this mode. A dump run is measurement, never evidence — re-run \
                 without U6_CORPUS_DUMP for a verdict."
            );
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
        if dump {
            panic!(
                "U6_CORPUS_DUMP=1: measurements dumped above; corpus_tsx_lane EVALUATED NO \
                 PINS in this mode. A dump run is measurement, never evidence — re-run \
                 without U6_CORPUS_DUMP for a verdict."
            );
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
                Flow::Result { function, .. } | Flow::Declared { function, .. } => function,
                _ => "makeProps",
            };
            let measured = drive_flow(row, function);
            if dump {
                println!("FLOW {} [{function}] => {measured:?}", row.id);
                continue;
            }
            // The PRIMARY semantic assertion, shared by both rails: each
            // named member's own graph-node shape. A narrowing that stopped
            // applying is visible ONLY here — the enclosing node is `Object`
            // whether or not the guard was honoured.
            let check_surface = |node: NodeShape,
                                 got_node: &NodeShape,
                                 members: &[(&str, NodeShape)],
                                 got_members: &[(String, NodeShape)],
                                 failures: &mut Vec<String>| {
                if node != *got_node {
                    failures.push(report(
                        row,
                        &format!("flow GRAPH NODE of `{function}`"),
                        &format!("{node:?}"),
                        &format!("{got_node:?}"),
                        "the return's SemanticNodeData discriminant changed. This is \
                             asserted on the GRAPH NODE, never the projected TypeExpr, because \
                             TypeParam / DeclRef / BareRef all project to `Ref { name }`.",
                    ));
                }
                for (name, want) in members {
                    match got_members.iter().find(|(got, _)| got == name) {
                        Some((_, got)) if got == want => {}
                        Some((_, got)) => failures.push(report(
                            row,
                            &format!("member `{name}` of `{function}`'s return"),
                            &format!("{want:?}"),
                            &format!("{got:?}"),
                            "the MEMBER's computed type changed. For a narrowing row this \
                                 is the whole assertion: `Union` where the checker says \
                                 `Primitive` means the guard stopped applying, and the \
                                 enclosing node looks identical either way.",
                        )),
                        None => failures.push(report(
                            row,
                            &format!("member `{name}` of `{function}`'s return"),
                            &format!("{want:?}"),
                            &format!("no such member; the surface carries {got_members:?}"),
                            "the pinned member is absent from the returned surface",
                        )),
                    }
                }
            };
            match (row.flow, &measured) {
                (
                    Flow::Result {
                        node,
                        members,
                        degradation,
                        candidates,
                        ..
                    },
                    MeasuredFlow::Result {
                        node: got_node,
                        members: got_members,
                        degradation: got_degr,
                        candidates: got_candidates,
                    },
                ) => {
                    check_surface(node, got_node, members, got_members, &mut failures);
                    if degradation != *got_degr {
                        failures.push(report(
                            row,
                            &format!("flow degradation of `{function}`"),
                            &format!("{degradation:?}"),
                            &format!("{got_degr:?}"),
                            "the typed degradation reason changed",
                        ));
                    }
                    if candidates != *got_candidates {
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
                (
                    Flow::Declared { node, members, .. },
                    MeasuredFlow::Declared {
                        node: got_node,
                        members: got_members,
                    },
                ) => {
                    check_surface(node, got_node, members, got_members, &mut failures);
                }
                (Flow::Result { .. }, MeasuredFlow::Declared { .. }) => failures.push(report(
                    row,
                    &format!("flow lane of `{function}`"),
                    "Flow::Result (a body-derived return)",
                    "the DECLARED rail",
                    "the function's return carries an AUTHORED ANNOTATION, so production serves \
                     it from the declared locator rail and the body-derived producer never runs \
                     for it. Re-pin the row as `Flow::Declared` — a `Flow::Result` pin here \
                     measures a body-only evaluation no consumer ever demands.",
                )),
                (Flow::Declared { .. }, MeasuredFlow::Result { .. }) => failures.push(report(
                    row,
                    &format!("flow lane of `{function}`"),
                    "Flow::Declared (an authored-annotation return)",
                    "the body-derived FlowReturn producer",
                    "the function's return is BODY-DERIVED, not annotated. Re-pin the row as \
                     `Flow::Result`.",
                )),
                (Flow::NoValue, MeasuredFlow::NoValue) => {}
                (Flow::NoValue, MeasuredFlow::Absent) => failures.push(report(
                    row,
                    &format!("flow lane of `{function}`"),
                    "Flow::NoValue (a genuine producer refusal)",
                    "Absent (the served signature fact carried NO return source)",
                    "the pin demands a refusal the `FlowReturn` producer actually computed; \
                     this function's return source was dropped BEFORE the producer ran (a \
                     bodiless overload or a synthesized signature), so nothing was ever \
                     refused. The pin is measuring the extractor, not the producer.",
                )),
                (expected, got) => failures.push(report(
                    row,
                    &format!("flow lane of `{function}`"),
                    &format!("{expected:?}"),
                    &format!("{got:?}"),
                    "the flow lane's OUTCOME class changed (value \u{2194} no-value)",
                )),
            }
        }
        if dump {
            panic!(
                "U6_CORPUS_DUMP=1: measurements dumped above; corpus_flow_lane EVALUATED NO \
                 PINS in this mode. A dump run is measurement, never evidence — re-run \
                 without U6_CORPUS_DUMP for a verdict."
            );
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The RECURSIVE-expectation + PUBLIC-BOUNDARY lane.
    ///
    /// For every row carrying an [`Expect::Node`], [`Boundary::Audit`],
    /// or [`Boundary::AuditRefusal`] pin, drive
    /// `get_flow_return_type_with_audit` TWICE on a fresh host: the
    /// recursive expectation is matched against the first call's result
    /// node at the GRAPH-NODE level, and the boundary pin models BOTH
    /// calls — result class, typed degradation, exact projected JSON,
    /// `from_cache`, and cold-compute count — in both directions. A
    /// refusal row ([`Flow::NoValue`] + [`Boundary::AuditRefusal`])
    /// asserts the typed non-admission contract: both calls refuse,
    /// cold, never warm-served.
    #[test]
    fn corpus_expect_and_boundary_lane() {
        use super::u6_flow_expect_tests::{
            check_boundary, check_boundary_refusal, drive_expect_boundary,
        };
        let dump = dump_mode();
        let mut failures = Vec::new();
        for row in CORPUS {
            let wants =
                !matches!(row.expect, Expect::Skip) || !matches!(row.boundary, Boundary::Skip);
            if !wants && !dump {
                continue;
            }
            let function = match (row.flow, row.boundary) {
                (Flow::Result { function, .. }, _) => function,
                // A refusal row: Flow::NoValue names no function; every
                // corpus refusal program uses the `makeProps` convention
                // (the same default `drive_flow` applies).
                (Flow::NoValue, Boundary::AuditRefusal { .. }) => "makeProps",
                _ => {
                    if wants {
                        failures.push(report(
                            row,
                            "expect/boundary lane",
                            "a Flow::Result (body-derived) row, or Flow::NoValue with \
                             Boundary::AuditRefusal",
                            "a row on another rail",
                            "the recursive expectation and the value boundary drive the \
                             body-derived FlowReturn rail; the refusal boundary rides only \
                             on a Flow::NoValue row; a declared-rail or skipped row cannot \
                             carry these pins",
                        ));
                    }
                    continue;
                }
            };
            let expected = match row.expect {
                Expect::Node(node) => Some(node),
                Expect::Skip => None,
            };
            let measured = drive_expect_boundary(row.aux, row.id, row.script, function, expected);
            if dump {
                println!(
                    "EXPECT {} => node {:?}  degr {:?}  cold1 {}  fc1 {}  fc2 {}  cold2 {}  \
                     err {:?}  json1 {:?}",
                    row.id,
                    measured.rendered,
                    measured.boundary.degradation,
                    measured.boundary.first_cold_computes,
                    measured.boundary.first_from_cache,
                    measured.boundary.second_from_cache,
                    measured.boundary.second_cold_computes,
                    measured.boundary.error,
                    measured.boundary.json,
                );
                continue;
            }
            if let Some(fails) = &measured.expect_failures {
                for failure in fails {
                    failures.push(report(
                        row,
                        &format!("recursive expectation on `{function}`'s return"),
                        failure,
                        measured.rendered.as_deref().unwrap_or("<no value>"),
                        "the recursive graph-node expectation did not match. This lane is \
                         what distinguishes `() => \"a\"` from `() => \"b\"` — re-measure \
                         with the dump before re-pinning.",
                    ));
                }
            }
            match row.boundary {
                Boundary::Audit {
                    json,
                    degradation,
                    warm_replay,
                } => {
                    for failure in
                        check_boundary(json, degradation, warm_replay, &measured.boundary)
                    {
                        failures.push(report(
                            row,
                            &format!("public cold/warm boundary on `{function}`"),
                            &failure,
                            "see the clause above",
                            "get_flow_return_type_with_audit, invoked twice: call 1 must be \
                             cold with the pinned typed degradation and EXACT projected JSON; \
                             call 2 must keep the result class and typed degradation, project \
                             identically, and hold the pinned cache-replay state in both \
                             directions (a cold replay must genuinely recompute).",
                        ));
                    }
                }
                Boundary::AuditRefusal { error } => {
                    for failure in check_boundary_refusal(error, &measured.boundary) {
                        failures.push(report(
                            row,
                            &format!("public refusal boundary on `{function}`"),
                            &failure,
                            "see the clause above",
                            "get_flow_return_type_with_audit, invoked twice: both calls must \
                             REFUSE with exactly the pinned typed refusal, cold each time, \
                             keeping the same refusal identity across the calls; the refusal \
                             is never admitted warm and recomputes on every demand — the \
                             typed non-admission contract.",
                        ));
                    }
                }
                Boundary::Skip => {}
            }
        }
        if dump {
            panic!(
                "U6_CORPUS_DUMP=1: measurements dumped above; corpus_expect_and_boundary_lane \
                 EVALUATED NO PINS in this mode. A dump run is measurement, never evidence — \
                 re-run without U6_CORPUS_DUMP for a verdict."
            );
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The `checker` column of a deep-pinned row is cross-validated at
    /// the PRESENTATION-BYTE level: wherever the checker's printed
    /// syntax coincides with the harness renderer's syntax, the LIVE
    /// recursive rendering of the row's flow return must equal the
    /// row's `checker` text verbatim.
    ///
    /// `RENDER_INCOMPARABLE` exempts PRESENTATION BYTES ONLY — never
    /// semantic equality, which
    /// [`deep_pinned_rows_semantic_equality_follows_their_verdict`]
    /// compares for EVERY deep-pinned row regardless of these lists.
    /// The incomparability claim is itself VERIFIED: an incomparable
    /// row's live rendering must genuinely BYTE-DIFFER from its checker
    /// text, so reclassifying a comparable row into
    /// `RENDER_INCOMPARABLE` with a bogus reason fails here instead of
    /// silently dropping the byte comparison. Both lists are exhaustive
    /// over the deep-pinned (`Expect::Node`) population and
    /// stale-failing.
    #[test]
    fn checker_column_cross_validates_against_live_rendering() {
        use super::u6_flow_expect_tests::drive_expect_boundary;
        /// Deep-pinned rows whose `checker` text IS renderer syntax.
        const RENDER_COMPARABLE: &[&str] = &[
            "X85_nested_closure_write_updates_captured_binding",
            "X87_read_only_let_capture_keeps_reaching_literal",
            "X106_triple_nested_closure_return",
            "N91_typeof_function_over_member_surface_reads_never",
            "N98_fresh_call_join_sibling_pin_number",
            "N99_fresh_call_join_sibling_pin_string",
            "N100_fresh_call_join_sibling_pin_boolean",
            "N101_fresh_call_join_sibling_annotated_const",
            "N102_fresh_call_join_both_arms_widen",
            "N116_binding_fresh_call_const_widening_read",
            "N117_binding_fresh_call_let_widens_at_decl",
            "N122_membership_through_let_initializer_return",
        ];
        /// Deep-pinned rows whose `checker` text is NOT byte-comparable
        /// to the renderer, each with the PRESENTATION reason. Semantic
        /// equality is NOT exempted — it is compared for every entry by
        /// the verdict-directed semantic test, and the byte-divergence
        /// claimed here is asserted live below.
        const RENDER_INCOMPARABLE: &[(&str, &str)] = &[
            (
                "Y01_union_never_arm_collapses",
                "checker prints `{ v: string; }`; the renderer spells the same \
                 surface `{ v: string }` — object members print without the \
                 trailing `;` terminator",
            ),
            (
                "Y02_union_idempotent_switch_join",
                "checker prints `{ v: string; }`; the renderer spells the same \
                 surface `{ v: string }` — object members print without the \
                 trailing `;` terminator",
            ),
            (
                "Y03_disjoint_scalar_intersection_member",
                "checker prints `{ v: never; }`; the renderer spells the same \
                 surface `{ v: never }` — object members print without the \
                 trailing `;` terminator",
            ),
            (
                "X22_switch_break_case_entry",
                "checker prints `{ v: \"a\"; } | { v: \"b\"; }`; the renderer spells \
                 the same node `Union({ v: \"a\" } | { v: \"b\" })` — union spelling \
                 and member terminators differ",
            ),
            (
                "X88_nested_label_inherits_enclosing_suffix_return",
                "checker prints `\"a\" | \"b\"`; the renderer spells the same node \
                 `Union(\"a\" | \"b\")`",
            ),
            (
                "N25_impossible_predicate_statement_keeps_dead_contributor",
                "the renderer spells the (KnownOwed-divergent) union \
                 `{ v: Union(…) }` where the checker prints `{ v: \"no\" | \"ok\"; }` — \
                 print syntax AND semantics differ; the semantic divergence is held by \
                 the KnownOwed arm of the semantic test",
            ),
            (
                "N31_discriminated_union_switch_positive_control",
                "checker prints `{ v: string | number; }`; the renderer spells \
                 `{ v: Union(string | number) }`",
            ),
            (
                "N71_in_operator_optional_member_keeps_undefined",
                "checker prints `{ v: string | undefined; }`; the renderer spells the same node `{ v: Union(string | undefined) }`",
            ),
            (
                "N40_as_wrapped_guard",
                "checker prints `{ v: string | number; }`; the renderer spells \
                 `{ v: Union(string | number) }`",
            ),
            (
                "N49_closure_narrows_own_parameter",
                "checker prints `{ result: { kind: \"s\"; val: string; } | \
                 { kind: \"n\"; val: number; }; }`; the renderer spells the same node \
                 `{ result: Union({ kind: \"s\", val: string } | \
                 { kind: \"n\", val: number }) }` — object members print `,`-separated \
                 and the union carries the `Union(…)` wrapper",
            ),
            (
                "N27_switch_true_guard_dispatch",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(string | number) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N28_switch_typeof_dispatch",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(string | number) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N30_switch_true_negated_guard_dispatch",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(string | number) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N34_non_null_asserted_property_discriminant",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(Opaque(UnmodeledPosition) | string) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N35_const_aliased_condition",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(string | number) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N42_comma_sequence_guard",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(string | number) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N44_typeof_over_unknown",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(unknown | string) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N46_typeof_over_any",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(any | string) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N47_correlated_tuple_discriminant",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(string | Opaque(UnmodeledPosition)) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N48_closure_narrows_captured_binding",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) surface `{ v: Union(string | number) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N26_structurally_possible_predicate_intersection_survives",
                "checker prints `{ v: string | (A & B); }` (object print with a \
                 parenthesised intersection); the renderer spells \
                 `{ v: Union(string | Intersection(DeclRef(A) & DeclRef(B))) }`",
            ),
            (
                "D01_helper_new",
                "checker prints `{ label: string; made: Box; }`; the renderer spells \
                 `{ label: string, made: Opaque(UnmodeledPosition) }` — print syntax AND \
                 semantics differ; the Degraded divergence is held by the semantic test",
            ),
            (
                "N09_narrow_then_write",
                "checker prints `{ label: string; }`; the renderer spells \
                 `{ label: Union(string | Opaque(UnmodeledPosition)) }` — the retained \
                 unmodelled-marker arm the `typeof` test cannot classify; print syntax \
                 AND semantics differ; the KnownOwed divergence is held by the semantic \
                 test",
            ),
            (
                "N56_arrow_predicate_annotated_binding",
                "checker prints `{ v: string | A; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(DeclRef(A) | DeclRef(B) | string) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N57_object_literal_method_predicate",
                "checker prints `{ v: string | A; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(DeclRef(A) | DeclRef(B) | string) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N59_generic_predicate_instantiated_at_call",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(unknown | string) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N60_class_method_assertion_narrows",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(string | number) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N62_annotated_const_assertion_narrows",
                "checker prints `{ v: string; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(string | number) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N64_boolean_literal_discriminant",
                "checker prints `{ v: string | number; }`; the renderer spells the same node `{ v: Union(number | string) }`",
            ),
            (
                "N66_shared_nonliteral_property_is_not_a_discriminant",
                "checker prints `{ v: N1 | N2; }`; the renderer spells the same node `{ v: Union(DeclRef(N1) | DeclRef(N2)) }`",
            ),
            (
                "N69_in_operator_const_literal_key",
                "checker prints `{ v: string | { a: string; }; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(string | { a: string } | { b: number }) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N70_in_operator_numeric_key",
                "checker prints `{ v: string | boolean; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(Opaque(UnmodeledPosition) | string) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N72_typeof_function_guard",
                "checker prints `{ v: string | number; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(string | any) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N73_typeof_object_keeps_null",
                "checker prints `{ v: { a: number; } | null; }`; the renderer spells the same node `{ v: Union({ a: number } | null) }`",
            ),
            (
                // Equality-guard forms: the loose `== null` operator, and equality
    // against a const-typed literal binding or comparison target.
    "N76_loose_equality_null_removes_both",
                "checker prints `{ v: string | number; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(string | null | undefined | number) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N77_strict_not_null_keeps_undefined",
                "checker prints `{ v: string | undefined; }`; the renderer spells the same node `{ v: Union(string | undefined) }`",
            ),
            (
                "N78_strict_not_undefined_keeps_null",
                "checker prints `{ v: string | null; }`; the renderer spells the same node `{ v: Union(string | null) }`",
            ),
            (
                "N79_equality_against_const_literal_binding",
                "checker prints `{ v: 5 | 15; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(5 | 10 | 15) }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N80_equality_against_const_literal_target_narrows",
                "checker prints `{ v: \"a\"; }`; the renderer spells the (KnownOwed-divergent) node `{ v: Union(\"a\" | \"b\") }` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N82_falsy_branch_keeps_empty_string_literal",
                "checker prints `{ v: \"\"; }`; the renderer spells the same node `{ v: \"\" }`",
            ),
            (
                "X92_if_false_branch_still_contributes_return",
                "checker prints `\"a1\" | \"a2\"`; the renderer spells the same node `Union(\"a1\" | \"a2\")`",
            ),
            (
                "X93_if_true_branch_keeps_fallthrough_return",
                "checker prints `\"b1\" | \"b2\"`; the renderer spells the same node `Union(\"b1\" | \"b2\")`",
            ),
            (
                "X94_evolving_let_one_branch_keeps_undefined",
                "the renderer spells the (KnownOwed-divergent) node `{ v: \"q\" }` where the checker prints `{ v: \"q\" | undefined; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "X97_evolving_let_switch_without_default_keeps_undefined",
                "the renderer spells the (KnownOwed-divergent) node `{ v: Union(\"s\" | 3) }` where the checker prints `{ v: \"s\" | 3 | undefined; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "X99_nested_try_finally_collects_every_return",
                "checker prints `\"z3\" | \"z4\" | \"z5\" | \"z6\"`; the renderer spells the same node `Union(\"z3\" | \"z4\" | \"z5\" | \"z6\")`",
            ),
            (
                "X100_switch_default_between_cases_source_order",
                "checker prints `{ v: \"w1\"; } | { v: \"w2\"; } | { v: \"w3\"; }`; the renderer spells the same node `Union({ v: \"w1\" } | { v: \"w2\" } | { v: \"w3\" })`",
            ),
            (
                "X104_void_arm_not_absorbed_in_union",
                "checker prints `{ v: void | number; }`; the renderer spells the same node `{ v: Union(void | number) }`",
            ),
            (
                "X105_closure_captures_narrowed_binding_in_guarded_arm",
                "checker prints `() => string`; the renderer spells the (KnownOwed-divergent) node `Union(() => Union(string | undefined) | () => string)` — print syntax AND semantics differ; the divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "X111_guard_clause_return_then_use",
                "checker prints `{ v: number; } | { v: string; }`; the renderer spells the same node `Union({ v: number } | { v: string })`",
            ),
            (
                "X115_union_alias_passthrough_keeps_alias",
                "checker prints `Shape`; the renderer spells the same node `DeclRef(Shape)`",
            ),
            (
                "N85_uninhabited_conjunct_keeps_sibling_subject_contributor",
                "checker prints `string | 0`; the renderer spells the same node `Union(string | 0)`",
            ),
            (
                "N86_uninhabited_conjunct_keeps_sibling_in_ternary",
                "checker prints `string | 0`; the renderer spells the same node `Union(string | 0)`",
            ),
            (
                "N87_uninhabited_negated_disjunct_keeps_fallthrough",
                "checker prints `string | 0`; the renderer spells the same node `Union(string | 0)`",
            ),
            (
                "N89_in_known_key_filters_arms_exactly",
                "checker prints `0 | { a: number; }`; the renderer spells the same node `Union({ a: number } | 0)`",
            ),
            (
                "N90_typeof_function_over_object_narrows_to_function",
                "the renderer spells the (KnownOwed-divergent) node `Union(object | 0)` where the checker prints `0 | Function` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N07_branch_join_widens",
                "checker prints `{ label: string | number; }`; the renderer spells the same surface `{ label: Union(string | number) }`",
            ),
            (
                "X09_generic_wrap_return",
                "checker prints `{ box: string; }`; the renderer spells the same surface `{ box: string }` — object members print without the trailing `;` terminator",
            ),
            (
                "X36_labeled_break_drops_arm_assertion",
                "the renderer spells the (KnownOwed-divergent) node `Union({ v: string } | { v: Union(string | number) })` where the checker prints `{ v: string | number; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "X39_try_catch_throw_point_join",
                "the renderer spells the (KnownOwed-divergent) node `Union({ v: Union(string | number) } | { v: number })` where the checker prints `{ v: string | number; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "X45_switch_fallthrough_case_narrows_by_chain_tests",
                "the renderer spells the (KnownOwed-divergent) node `Union({ v: Union(\"a\" | \"b\") } | { v: string })` where the checker prints `{ v: string; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "X46_try_catch_template_throw_point",
                "the renderer spells the (KnownOwed-divergent) node `Union({ v: Union(string | number) } | { v: number })` where the checker prints `{ v: string | number; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "X47_try_catch_sequence_throw_point",
                "the renderer spells the (KnownOwed-divergent) node `Union({ v: Union(string | number) } | { v: number })` where the checker prints `{ v: string | number; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "X48_try_catch_if_guard_throw_point",
                "the renderer spells the (KnownOwed-divergent) node `Union({ v: Union(string | number) } | { v: number })` where the checker prints `{ v: string | number; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "X49_try_catch_new_callee_throw_point",
                "the renderer spells the (KnownOwed-divergent) node `Union({ v: Union(string | number) } | { v: number })` where the checker prints `{ v: string | number; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N92_instanceof_unrelated_class_intersects_whole_subject",
                "checker prints `{ v: (string | L) & K; } | { v: number; }`; the renderer spells the same node `Union({ v: Intersection(Union(string | DeclRef(L)) & DeclRef(K)) } | { v: number })`",
            ),
            (
                "N93_instanceof_strips_nullish_before_the_intersection",
                "checker prints `{ v: L & K; } | { v: number; }`; the renderer spells the same node `Union({ v: Intersection(DeclRef(L) & DeclRef(K)) } | { v: number })`",
            ),
            (
                "N94_instanceof_subclass_test_over_base_gaps",
                "the renderer spells the (KnownOwed-divergent) node `Union({ v: DeclRef(K) } | { v: number })` where the checker prints `{ v: KSub; } | { v: number; }` — print syntax AND semantics differ; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N95_instanceof_related_arm_drops_unrelated_class_arm",
                "checker prints `{ v: K; } | { v: number; }`; the renderer spells the same node `Union({ v: DeclRef(K) } | { v: number })`",
            ),
            (
                "N96_branch_join_mixed_pinned_arm",
                "checker prints `{ label: number | \"s\"; }`; the renderer spells the same node `{ label: Union(number | \"s\") }` — union spelling and member terminators differ",
            ),
            (
                "N97_widening_const_read_through_let_initializer",
                "checker prints `{ label: number; }`; the renderer spells the same surface `{ label: number }` — object members print without the trailing `;` terminator",
            ),
            (
                "X26_switch_assertion_case_scope",
                "checker prints `{ c: string | number; }`; the renderer spells the (KnownOwed-divergent) node `Union({ c: string } | { c: string | number })` — the checker's reunion absorbs the subtype arm; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N103_fresh_call_join_distinct_literal_arms",
                "checker prints `1 | 2`; the renderer spells the same node `Union(2 | 1)` — constituent order differs",
            ),
            (
                "N104_declared_union_null_keeps_literal",
                "checker prints `\"x\" | null`; the renderer spells the same node `Union(\"x\" | null)`",
            ),
            (
                "N105_declared_union_undefined_keeps_literal",
                "checker prints `\"x\" | undefined`; the renderer spells the same node `Union(\"x\" | undefined)`",
            ),
            (
                "N107_declared_union_join_with_pinned_arm",
                "checker prints `1 | null`; the renderer spells the same node `Union(1 | null)`",
            ),
            (
                "N108_flow_inferred_union_keeps_literal",
                "checker prints `\"x\" | null`; the renderer spells the same node `Union(\"x\" | null)`",
            ),
            (
                "N109_declared_union_explicit_type_argument",
                "checker prints `string | null`; the renderer spells the same node `Union(string | null)`",
            ),
            (
                "N118_binding_union_call_const_return_pinned",
                "checker prints `\"x\" | null`; the renderer spells the same node `Union(\"x\" | null)`",
            ),
            (
                "N119_binding_union_call_let_widens",
                "checker prints `string | null`; the renderer spells the same node `Union(string | null)`",
            ),
            (
                "N125_mixed_pinned_arm_return_read_stays_pinned",
                "checker prints `\"s\" | 1`; the renderer spells the same node `Union(1 | \"s\")` — constituent order differs",
            ),
            (
                "N126_all_fresh_conditional_return_read_stays_union",
                "checker prints `1 | 2`; the renderer spells the same node `Union(1 | 2)`",
            ),
            (
                "N110_member_fresh_call_widens",
                "checker prints `{ a: number; }`; the renderer spells the same surface `{ a: number }` — object members print without the trailing `;` terminator",
            ),
            (
                "N111_member_fresh_call_binding_widens",
                "checker prints `{ a: number; }`; the renderer spells the same surface `{ a: number }` — object members print without the trailing `;` terminator",
            ),
            (
                "N112_member_fresh_call_nested_widens",
                "checker prints `{ x: { a: number; }; }`; the renderer spells the same surface `{ x: { a: number } }` — object members print without the trailing `;` terminator",
            ),
            (
                "N113_member_union_call_fresh_arm_widens",
                "checker prints `{ a: string | null; }`; the renderer spells the same node `{ a: Union(string | null) }` — union spelling and member terminators differ",
            ),
            (
                "N114_member_union_binding_fresh_arm_widens",
                "checker prints `{ a: string | null; }`; the renderer spells the same node `{ a: Union(string | null) }` — union spelling and member terminators differ",
            ),
            (
                "N115_member_intersection_call_stays_pinned",
                "the renderer spells the (KnownOwed-divergent) surface `{ a: {  } & \"x\" }` where the checker prints `{ a: \"x\"; }` — the checker reduces the intersection; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N106_declared_intersection_keeps_literal",
                "the renderer spells the (KnownOwed-divergent) node `{  } & \"x\"` where the checker prints `\"x\"` — the checker reduces the intersection; the semantic divergence is held by the KnownOwed arm of the semantic test",
            ),
            (
                "N120_membership_through_const_initializer",
                "checker prints `{ label: number; }`; the renderer spells the same surface `{ label: number }` — object members print without the trailing `;` terminator",
            ),
            (
                "N121_partial_membership_through_const_initializer",
                "checker prints `{ label: number | \"s\"; }`; the renderer spells the same node `{ label: Union(number | \"s\") }` — union spelling and member terminators differ",
            ),
            (
                "N123_mixed_pinned_same_literal_stays_pinned",
                "checker prints `{ label: 1; }`; the renderer spells the same surface `{ label: 1 }` — object members print without the trailing `;` terminator",
            ),
            (
                "N124_mixed_fresh_and_call_arm_member_read",
                "checker prints `{ label: number | boolean; }`; the renderer spells the same node `{ label: Union(number | boolean) }` — union spelling and member terminators differ",
            ),
        ];
        let mut failures = Vec::new();
        for row in CORPUS {
            let comparable = RENDER_COMPARABLE.contains(&row.id);
            let incomparable = RENDER_INCOMPARABLE.iter().any(|(id, _)| *id == row.id);
            let deep_pinned = matches!(row.expect, Expect::Node(_));
            if deep_pinned && !(comparable ^ incomparable) {
                failures.push(format!(
                    "{}: every deep-pinned row must appear in EXACTLY ONE of \
                     RENDER_COMPARABLE / RENDER_INCOMPARABLE (comparable={comparable}, \
                     incomparable={incomparable}) — classify it deliberately",
                    row.id
                ));
            }
            if !deep_pinned && (comparable || incomparable) {
                failures.push(format!(
                    "{}: named in a cross-validation list but carries no Expect::Node pin — \
                     stale entry",
                    row.id
                ));
            }
            if !deep_pinned || !(comparable || incomparable) {
                continue;
            }
            let Flow::Result { function, .. } = row.flow else {
                failures.push(format!(
                    "{}: cross-validated rows ride the body-derived rail",
                    row.id
                ));
                continue;
            };
            let measured = drive_expect_boundary(row.aux, row.id, row.script, function, None);
            let rendered = measured.rendered.as_deref();
            if comparable && rendered != Some(row.checker) {
                failures.push(format!(
                    "{}: the `checker` column must EQUAL the live rendering for a \
                     render-comparable row — checker `{}`, rendered `{}`. One of the two \
                     changed without the other; re-measure against the pinned oracle before \
                     re-pinning.",
                    row.id,
                    row.checker,
                    rendered.unwrap_or("<no value>")
                ));
            }
            if incomparable && rendered == Some(row.checker) {
                failures.push(format!(
                    "{}: listed RENDER_INCOMPARABLE but the live rendering EQUALS the \
                     checker byte-for-byte (`{}`) — the incomparability claim is FALSE; \
                     move the row to RENDER_COMPARABLE. A reclassification may never drop \
                     the byte comparison for a row that satisfies it.",
                    row.id, row.checker
                ));
            }
        }
        let named_ids = RENDER_COMPARABLE
            .iter()
            .copied()
            .chain(RENDER_INCOMPARABLE.iter().map(|(id, _)| *id));
        for id in named_ids {
            if !CORPUS.iter().any(|row| row.id == id) {
                failures.push(format!(
                    "{id}: named in a cross-validation list but absent from the corpus — \
                     stale entry"
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// SEMANTIC checker-column authority, verdict-directed. For EVERY
    /// deep-pinned row — independent of the presentation-byte lists —
    /// the row's `checker` text is parsed into the typed
    /// checker-syntax form
    /// ([`u6_flow_expect_tests::checker_syntax`], one canonical
    /// projection: order-insensitive exact union sets, ordered
    /// intersections, exact object member sets, arity-exact function
    /// prints, reference names matched against the graph trio) and
    /// compared against the LIVE graph:
    ///
    /// * a [`Verdict::MatchesChecker`] row's live surface must EQUAL
    ///   its parsed checker — editing the checker column (or regressing
    ///   the surface) fails here;
    /// * every OTHER verdict (KnownOwed / Degraded) must NOT match —
    ///   the recorded divergence must be REAL in the tree, so silently
    ///   repairing the debt, or editing the checker column to the live
    ///   value, fails here and forces the deliberate re-pin.
    ///
    /// An unparseable deep-pinned checker is a FAILURE naming the gap —
    /// extend the parser deliberately; never exempt silently.
    #[test]
    fn deep_pinned_rows_semantic_equality_follows_their_verdict() {
        use super::u6_flow_expect_tests::{checker_syntax, render_node, with_live_flow_node};
        let mut failures = Vec::new();
        for row in CORPUS {
            if !matches!(row.expect, Expect::Node(_)) {
                continue;
            }
            let Flow::Result { function, .. } = row.flow else {
                continue; // the expect/boundary lane already fails this shape
            };
            let parsed = match checker_syntax::parse(row.checker) {
                Ok(parsed) => parsed,
                Err(err) => {
                    failures.push(format!(
                        "{}: checker text `{}` did not parse into the typed checker-syntax \
                         form ({err}) — a deep-pinned checker that cannot be compared is an \
                         unverified semantic claim; extend the parser deliberately",
                        row.id, row.checker
                    ));
                    continue;
                }
            };
            let (matches, rendered) = with_live_flow_node(
                row.aux,
                &format!("{}__sem", row.id),
                row.script,
                function,
                |dispatch, node| match node {
                    Some(node) => (
                        checker_syntax::matches_node(dispatch, node, &parsed, 0),
                        render_node(dispatch, node, 0),
                    ),
                    None => (false, "<no value>".to_owned()),
                },
            );
            match row.verdict {
                Verdict::MatchesChecker => {
                    if !matches {
                        failures.push(format!(
                            "{}: labelled MatchesChecker but the LIVE semantic surface does \
                             not equal the parsed checker `{}` — measured `{rendered}`. \
                             Either the surface regressed or the checker column was edited; \
                             re-measure against the pinned oracle before re-pinning.",
                            row.id, row.checker
                        ));
                    }
                }
                _ => {
                    if matches {
                        failures.push(format!(
                            "{}: labelled {:?} but the LIVE semantic surface EQUALS the \
                             parsed checker `{}` — the recorded divergence is GONE. This \
                             failure is the INTENDED signal: the debt looks repaired (or \
                             the checker column was edited to the live value); re-pin the \
                             row and close its ledger entries in the same change.",
                            row.id, row.verdict, row.checker
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// CONTROL — the checker columns of the deep rows the byte lists
    /// exempt (X88, N26) are DISCRIMINATING through the semantic
    /// projection: each live surface accepts exactly its recorded
    /// checker text and rejects every minimally-mutated neighbour. This
    /// is what binds an exempted row's pins to its `checker` text — the
    /// proven hatch is editing N26 checker text to name the wrong
    /// intersection arm with everything green.
    #[test]
    fn checker_column_mutations_are_rejected_semantically() {
        use super::u6_flow_expect_tests::{checker_syntax, with_live_flow_node};
        fn row(id: &str) -> &'static Row {
            CORPUS
                .iter()
                .find(|row| row.id == id)
                .unwrap_or_else(|| panic!("corpus row `{id}` exists"))
        }
        let x88 = row("X88_nested_label_inherits_enclosing_suffix_return");
        with_live_flow_node(
            x88.aux,
            "ctl_x88_checker",
            x88.script,
            "makeProps",
            |dispatch, node| {
                let node = node.expect("X88 produces a value");
                let accepts = |text: &str| {
                    let parsed = checker_syntax::parse(text)
                        .unwrap_or_else(|err| panic!("`{text}` must parse: {err}"));
                    checker_syntax::matches_node(dispatch, node, &parsed, 0)
                };
                assert!(
                    accepts(x88.checker),
                    "the recorded checker `{}` must match X88's live surface",
                    x88.checker
                );
                assert!(
                    accepts("\"b\" | \"a\""),
                    "union comparison is order-insensitive — the checker's own print order \
                     must not matter"
                );
                assert!(
                    !accepts("\"a\" | \"c\""),
                    "a WRONG constituent must be rejected"
                );
                assert!(
                    !accepts("\"a\""),
                    "a DROPPED constituent (subset) must be rejected"
                );
                assert!(
                    !accepts("\"a\" | \"b\" | \"c\""),
                    "an EXTRA constituent (superset) must be rejected"
                );
            },
        );
        let n26 = row("N26_structurally_possible_predicate_intersection_survives");
        with_live_flow_node(
            n26.aux,
            "ctl_n26_checker",
            n26.script,
            "makeProps",
            |dispatch, node| {
                let node = node.expect("N26 produces a value");
                let accepts = |text: &str| {
                    let parsed = checker_syntax::parse(text)
                        .unwrap_or_else(|err| panic!("`{text}` must parse: {err}"));
                    checker_syntax::matches_node(dispatch, node, &parsed, 0)
                };
                assert!(
                    accepts(n26.checker),
                    "the recorded checker `{}` must match N26's live surface",
                    n26.checker
                );
                assert!(
                    !accepts("{ v: string | (A & C); }"),
                    "a checker edited to name the WRONG intersection arm must be rejected — \
                     this is the proven exemption hatch"
                );
                assert!(
                    !accepts("{ v: string | (B & A); }"),
                    "REVERSED intersection arms must be rejected — intersections are ordered"
                );
                assert!(
                    !accepts("{ v: number | (A & B); }"),
                    "a wrong union constituent must be rejected"
                );
                assert!(
                    !accepts("{ w: string | (A & B); }"),
                    "a wrong member NAME must be rejected"
                );
                assert!(
                    !accepts("{ v: string; }"),
                    "a dropped constituent must be rejected"
                );
            },
        );
        // Function-print parameter/return and number-literal clauses,
        // exercised on CONTROL programs (no deep-pinned row carries a
        // parametered signature or number literal yet — these controls
        // are what keep those comparator clauses characterized).
        with_live_flow_node(
            "",
            "ctl_checker_fn",
            "function makeProps() { return (a: string, b: number) => \"a\" as const }",
            "makeProps",
            |dispatch, node| {
                let node = node.expect("the control signature produces a value");
                let accepts = |text: &str| {
                    let parsed = checker_syntax::parse(text)
                        .unwrap_or_else(|err| panic!("`{text}` must parse: {err}"));
                    checker_syntax::matches_node(dispatch, node, &parsed, 0)
                };
                assert!(
                    accepts("(a: string, b: number) => \"a\""),
                    "the live parametered signature must accept its own checker print"
                );
                assert!(
                    accepts("(x: string, y: number) => \"a\""),
                    "parameter NAMES are print artifacts and must not participate"
                );
                assert!(
                    !accepts("(a: number, b: number) => \"a\""),
                    "a wrong parameter TYPE must be rejected"
                );
                assert!(
                    !accepts("(a: number, b: string) => \"a\""),
                    "swapped parameter types must be rejected — parameters are ordered"
                );
                assert!(
                    !accepts("(a: string) => \"a\""),
                    "a wrong arity must be rejected"
                );
                assert!(
                    !accepts("(a: string, b: number) => \"b\""),
                    "a wrong return must be rejected"
                );
            },
        );
        with_live_flow_node(
            "",
            "ctl_checker_num",
            "function makeProps() { return 1 as const }",
            "makeProps",
            |dispatch, node| {
                let node = node.expect("the control literal produces a value");
                let accepts = |text: &str| {
                    let parsed = checker_syntax::parse(text)
                        .unwrap_or_else(|err| panic!("`{text}` must parse: {err}"));
                    checker_syntax::matches_node(dispatch, node, &parsed, 0)
                };
                assert!(
                    accepts("1"),
                    "the live number literal must accept its print"
                );
                assert!(!accepts("2"), "a different numeric value must be rejected");
                assert!(!accepts("\"1\""), "a string print must be rejected");
                assert!(!accepts("number"), "the widened primitive must be rejected");
            },
        );
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
        if dump {
            panic!(
                "U6_CORPUS_DUMP=1: measurements dumped above; corpus_svelte_twins EVALUATED \
                 NO PINS in this mode. A dump run is measurement, never evidence — re-run \
                 without U6_CORPUS_DUMP for a verdict."
            );
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

// Oracle — how the `checker` column is obtained

/// The pinned checker. CHECKER only, never `.d.ts`.
#[cfg(test)]
pub(crate) const TSGO_VERSION: &str = "7.0.0-dev.20260526.1";

/// One generated probe program for a row.
///
/// Two steps, deliberately. A one-step `const x: null = f(…)` reports NOTHING
/// when the contextual type feeds inference, and a raw call bound to `const`
/// reads UNWIDENED literals — both silently turn the probe into a no-op. A
/// `declare const` of the probe type followed by an assignment to `null` never
/// feeds inference back into the call, so the checker prints the type it
/// actually computed.
///
/// The `IsAny` half is the second trap: `any` IS assignable to `null`, so an
/// `any` row emits NOTHING from the shape probe and the FIRST diagnostic you
/// see is the `IsAny` one reporting `true`. `0 extends 1 & T` is the only
/// reliable `any` detector.
///
/// The program is derived from the ROW, so a refreshed `checker` column can
/// never be measured against a program that has drifted from the row.
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

#[cfg(test)]
mod oracle {
    use super::*;

    /// EVERY row records the checker's answer, and the probe that produced it
    /// is reproducible from the row itself.
    ///
    /// The checker is NOT invoked here. `tsgo` is GENERATION-ONLY in this
    /// codebase (`no_tsgo_runtime_driver_anywhere_in_default_build`): the
    /// resolver / query-time path must never spawn or contact it, and the sole
    /// legitimate driving site is the `oracle-gen`-gated generator. So the
    /// `checker` column is a RECORDED measurement, refreshed by the procedure
    /// below rather than by this suite.
    ///
    /// # Refreshing the `checker` column
    ///
    /// ```text
    /// U6_CORPUS_DUMP=1 cargo test -p verter_session --lib u6_flow_shape_corpus \
    ///     -- --nocapture --test-threads=1 2>&1 | grep '^PROBE '
    /// ```
    /// writes one `<row_id>.ts` per row (plus `<row_id>__aux.ts`) into a fresh
    /// temp directory and prints it. Then, from that directory:
    /// ```text
    /// ls *.ts | xargs <tsgo> --noEmit --strict --ignoreConfig --pretty false
    /// ```
    /// Each row yields `Type 'X' is not assignable to type 'null'.` twice: the
    /// first is the row's `checker`, the second is `true`/`false` for
    /// `checker_is_any`. An `any` row emits only ONE, reporting `true`.
    ///
    /// What THIS test enforces is that the column is never empty and never
    /// hand-waved: a row without a recorded answer records no ground truth.
    #[test]
    fn every_row_records_the_checkers_answer() {
        let mut missing = Vec::new();
        for row in CORPUS {
            if row.checker.is_empty() {
                missing.push(row.id);
            }
            // The probe must be derivable — a row whose probe type is empty
            // cannot be re-measured, so its `checker` could never be refreshed.
            assert!(
                !row.probe.is_empty(),
                "{}: a row must carry the probe type its `checker` was measured \
                 for, or the column can never be refreshed",
                row.id
            );
            let program = probe_program(row.probe, row.script);
            assert!(
                program.contains(row.script) && program.contains(row.probe),
                "{}: the probe program must be derived from the ROW, so a refreshed \
                 measurement can never be taken against a drifted program",
                row.id
            );
        }
        assert!(
            missing.is_empty(),
            "these rows record no checker answer (tsgo {TSGO_VERSION}), so they \
             pin no ground truth: {missing:?}"
        );
    }

    /// The probe's DISCRIMINATION, proven in both directions without invoking
    /// the checker.
    ///
    /// A probe that reported the same thing for every program would make every
    /// `checker` value meaningless. Two programs the checker types
    /// DIFFERENTLY must produce DIFFERENT probe programs, and the same program
    /// must produce the same probe.
    #[test]
    fn the_probe_discriminates_in_both_directions() {
        let a = probe_program(
            "ReturnType<typeof makeProps>",
            "function makeProps() { return { label: \"x\" } }",
        );
        let b = probe_program(
            "ReturnType<typeof makeProps>",
            "function makeProps() { return { label: 1 } }",
        );
        assert_ne!(
            a, b,
            "the NEGATIVE control: two programs the checker types differently must \
             produce different probe programs — a probe that collapses them measures \
             nothing"
        );
        let a_again = probe_program(
            "ReturnType<typeof makeProps>",
            "function makeProps() { return { label: \"x\" } }",
        );
        assert_eq!(
            a, a_again,
            "the POSITIVE control: the same row must produce the same probe, or a \
             refreshed measurement is not comparable to the recorded one"
        );
        // The two traps the probe exists to avoid, asserted structurally.
        assert!(
            a.contains("declare const __v:") && a.contains("export const __shape: null = __v;"),
            "the probe must be TWO steps: a one-step `const x: null = f(…)` reports \
             nothing when the contextual type feeds inference"
        );
        assert!(
            a.contains("type IsAny<T> = 0 extends 1 & T ? true : false;"),
            "the probe must carry the IsAny half: `any` is assignable to `null`, so \
             the shape probe alone cannot see an `any` row"
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
                Verdict::KnownOwed { owed_absent, note } => {
                    if note.is_empty() {
                        failures.push(format!("{}: KnownOwed with no note", row.id));
                    }
                    // The debt must be OBSERVABLE from this row, otherwise the
                    // label is decorative: either the lane refuses, or the TSX
                    // lane faults, or the row names the needles whose
                    // appearance means the debt was repaired.
                    // A narrowing row carries no framework column at all; its
                    // tripwire is the pinned MEMBER shape, which flips the
                    // moment the owning block starts narrowing correctly.
                    let pins_members = matches!(row.flow,
                        Flow::Result { members, .. } | Flow::Declared { members, .. }
                        if !members.is_empty());
                    // A recursive expectation is a full-depth pin of the
                    // CURRENT (wrong) value: the owner's fix flips it.
                    let pins_expect = matches!(row.expect, Expect::Node(_));
                    let observable = matches!(row.runtime, Runtime::Refused)
                        || matches!(row.tsx, Tsx::Faults(_))
                        || !owed_absent.is_empty()
                        || pins_members
                        || pins_expect;
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
        let mut owed: Vec<&str> = CORPUS
            .iter()
            .filter(|r| matches!(r.verdict, Verdict::KnownOwed { .. }))
            .map(|r| r.id)
            .collect();
        owed.sort_unstable();
        let mut pinned: Vec<&str> = OPEN_DEBTS.to_vec();
        pinned.sort_unstable();
        assert_eq!(
            owed.len(),
            OPEN_DEBTS.len(),
            "the open-debt ledger changed: pinned {pinned:?}, measured {owed:?} — if an owner \
             CLOSED a debt, update OPEN_DEBTS in the same change that re-pins the row"
        );
        assert_eq!(
            owed, pinned,
            "the open-debt ledger names a different SET of rows than the table labels KnownOwed"
        );
    }

    /// ANTI-RECURRENCE FLOOR (the §7 defect class): a row whose `checker`
    /// names a VALUE the root [`NodeShape`] vocabulary cannot distinguish
    /// must either carry a recursive [`Expect::Node`] pin or be NAMED in
    /// [`SHALLOW_PINNED_ROWS`] — exact set equality in BOTH directions,
    /// so deleting a deep pin is loud (the row appears unnamed) and a
    /// stale ledger entry is loud too. A deep-pinned row must also carry
    /// its [`Boundary::Audit`] companion: the pin pair rides together,
    /// and deleting either half fails here instead of silently skipping
    /// the row green.
    #[test]
    fn value_indistinct_rows_carry_deep_pins_or_are_named_shallow() {
        /// The [`NodeShape`] buckets the FLOOR covers: `Other` (all
        /// function values), `Union` (all constituent sets), `Literal`
        /// (all literal values) — the three buckets the repaired §7
        /// five-row defect actually rode. These are NOT the only
        /// conflating buckets: `Primitive` conflates `string` ↔
        /// `number` ↔ …, `ObjectSpreadProgram` conflates construction
        /// plans, and `OpaqueOther` conflates error kinds — but
        /// extending the floor to them was measured to sweep ~180 of
        /// the corpus's rows into the ledger (nearly every row),
        /// erasing the ledger's review signal, so those buckets are
        /// DELIBERATELY un-floored here and recorded as such: a
        /// `Primitive` member flip (e.g. N24's `{ v: string }` going
        /// `string` → `number`) is caught only by a deep pin or by the
        /// runtime `has` needles where the row carries them — NOT by
        /// this floor, and NOT by the checker column: checker
        /// cross-validation (byte and semantic) runs over DEEP-PINNED
        /// rows only, so on a shallow row the checker column is
        /// recorded documentation compared by nothing. Widening the
        /// floor is a separate ledger-governance change.
        /// `Object` roots stay out — their per-member shapes are
        /// asserted member-by-member, and a member in one of the three
        /// floored buckets is caught by the member scan.
        fn indistinct(shape: NodeShape) -> bool {
            matches!(
                shape,
                NodeShape::Other | NodeShape::Union | NodeShape::Literal
            )
        }
        let mut measured: Vec<&str> = Vec::new();
        let mut failures = Vec::new();
        for row in CORPUS {
            let needs = match row.flow {
                Flow::Result { node, members, .. } | Flow::Declared { node, members, .. } => {
                    indistinct(node) || members.iter().any(|(_, member)| indistinct(*member))
                }
                // A Skip row drives no flow lane; a NoValue row pins a
                // refusal, not a value — neither names a value to deepen.
                Flow::Skip | Flow::NoValue => false,
            };
            let deep_pinned = matches!(row.expect, Expect::Node(_));
            if deep_pinned && !matches!(row.boundary, Boundary::Audit { .. }) {
                failures.push(format!(
                    "{}: carries Expect::Node without Boundary::Audit — the deep-pin pair \
                     rides together; restore the boundary pin",
                    row.id
                ));
            }
            if matches!(row.boundary, Boundary::Audit { .. }) && !deep_pinned {
                failures.push(format!(
                    "{}: carries Boundary::Audit without Expect::Node — the deep-pin pair \
                     rides together; restore the expect pin",
                    row.id
                ));
            }
            if matches!(row.boundary, Boundary::AuditRefusal { .. }) {
                if !matches!(row.flow, Flow::NoValue) {
                    failures.push(format!(
                        "{}: Boundary::AuditRefusal requires Flow::NoValue — the refusal \
                         boundary models the same refusal the flow lane pins",
                        row.id
                    ));
                }
                if deep_pinned {
                    failures.push(format!(
                        "{}: Boundary::AuditRefusal cannot carry Expect::Node — a refusal \
                         has no result node",
                        row.id
                    ));
                }
            }
            if needs && !deep_pinned {
                measured.push(row.id);
            }
        }
        measured.sort_unstable();
        let mut named: Vec<&str> = SHALLOW_PINNED_ROWS.iter().map(|entry| entry.0).collect();
        named.sort_unstable();
        for (id, owner, reason) in SHALLOW_PINNED_ROWS {
            let Some(row) = CORPUS.iter().find(|row| row.id == *id) else {
                continue; // the set-equality below reports the stale entry
            };
            if row.owner != *owner {
                failures.push(format!(
                    "{id}: the shallow ledger names owning block {:?} but the row's owner \
                     column is {:?} — the ledger's owner must equal the row's",
                    owner, row.owner
                ));
            }
            if reason.trim().is_empty() {
                failures.push(format!(
                    "{id}: a shallow ledger entry must record WHY the row is still shallow"
                ));
            }
        }
        if measured != named {
            failures.push(format!(
                "the shallow-pinned ledger drifted.\nmeasured (value-indistinct rows WITHOUT \
                 an Expect::Node pin): {measured:?}\nnamed (SHALLOW_PINNED_ROWS): {named:?}\n\
                 If a deep pin was DELETED, restore it. If a NEW value-indistinct row landed \
                 shallow, deepen it or name it in SHALLOW_PINNED_ROWS deliberately. If a \
                 named row was deepened, remove it from the ledger in the same change."
            ));
        }
        // The BURN-DOWN-ONLY governance, ENFORCED (not prose): the
        // ledger may only shrink. Deepening a row removes its entry;
        // adding an entry (a new deliberately-shallow value-indistinct
        // row) requires LOWERING nothing but consciously raising this
        // ceiling in the same reviewed change — the same mechanism as
        // the `CORPUS.len() >= 87` floor, pointed the other way.
        assert!(
            SHALLOW_PINNED_ROWS.len() <= SHALLOW_PINNED_ROWS_CEILING,
            "the shallow-pinned ledger GREW ({} entries > ceiling {}) — the ledger is \
             burn-down-only; a new value-indistinct row lands DEEP by default. If a \
             deliberate, reviewed shallow exception is truly required, raise \
             SHALLOW_PINNED_ROWS_CEILING in the same change and record why.",
            SHALLOW_PINNED_ROWS.len(),
            SHALLOW_PINNED_ROWS_CEILING
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

#[cfg(test)]
mod programme_ledgers {
    use super::*;

    /// Every narrowing row has an owner and is pinned against today's
    /// substrate. A weakening that starts matching the checker fails
    /// until the row is reclassified.
    #[test]
    fn narrowing_rows_are_owned_by_a_narrow_block() {
        let mut seen_blocks = std::collections::BTreeSet::new();
        for row in CORPUS {
            let Demand::Narrowing(block) = row.demand else {
                continue;
            };
            seen_blocks.insert(block.id());
            assert!(
                matches!(row.runtime, Runtime::Skip)
                    && matches!(row.tsx, Tsx::Skip)
                    && matches!(row.svelte, Svelte::Skip),
                "{}: a narrowing row is PLAIN TypeScript semantics — it must carry no framework \
                 column. Framework emission is secondary evidence, never the subject.",
                row.id
            );
            assert!(
                matches!(row.flow, Flow::Result { members, .. } if !members.is_empty())
                    || matches!(row.expect, Expect::Node(_)),
                "{}: a narrowing row must pin at least one MEMBER shape or carry a recursive \
                 Expect::Node pin — the enclosing node's discriminant alone is the same \
                 whether or not the guard applied, so a row with neither measures nothing",
                row.id
            );
            match row.verdict {
                Verdict::KnownOwed { .. } => assert_eq!(
                    row.owner.id(),
                    block.id(),
                    "{}: a narrowing row's owner column must be its block",
                    row.id
                ),
                Verdict::MatchesChecker => {}
                other => panic!(
                    "{}: a narrowing row is either KnownOwed against its block or (as a \
                     deliberate CONTROL) MatchesChecker; got {other:?}",
                    row.id
                ),
            }
        }
        assert_eq!(
            seen_blocks,
            [
                "U6.NARROW_INSTANCEOF",
                "U6.NARROW_INVALIDATION",
                "U6.NARROW_LATTICE",
                "U6.NARROW_SUBSTITUTION",
                "U6.NARROW_TYPEOF",
            ]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
            "every narrowing block must have at least one seeded row — a block with no row \
             inherits no fence"
        );
    }

    /// The FRAMEWORK-ONLY worklist, in one query.
    ///
    /// These rows exist only as framework shapes (a macro payload spelling,
    /// `withDefaults`, `defineModel`, `defineSlots`, the runtime-form macros).
    /// Framework-specific POLICY for them is the project owner's post-merge
    /// pass; this list is that pass's input.
    #[test]
    fn the_framework_only_worklist_is_pinned() {
        let measured: Vec<&str> = CORPUS
            .iter()
            .filter(|r| matches!(r.subject, Subject::FrameworkOnly))
            .map(|r| r.id)
            .collect();
        assert_eq!(
            measured, FRAMEWORK_ONLY_WORKLIST,
            "the framework-only worklist changed. Adding a framework-only shape means adding \
             its id here too, so the owner's post-merge pass stays a single query."
        );
    }

    /// The corpus is a TYPESCRIPT SEMANTICS corpus first.
    ///
    /// A row whose identity is the semantic answer must assert on the semantic
    /// answer. A framework column may accompany it — a semantic answer that
    /// dies on the way to a consumer is still a defect — but it may never be
    /// the ONLY thing a `Subject::TypeScript` row asserts.
    #[test]
    fn every_typescript_row_asserts_the_semantic_answer() {
        let mut failures = Vec::new();
        for row in CORPUS {
            if matches!(row.subject, Subject::FrameworkOnly) {
                continue;
            }
            if matches!(row.flow, Flow::Skip) {
                failures.push(format!(
                    "{}: a TypeScript-subject row must assert the SEMANTIC answer (the \
                     flow-return graph node), not only a framework emission",
                    row.id
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "TypeScript-subject rows:\n{}",
            failures.join("\n")
        );
    }
}

/// The rows whose shape exists ONLY as a framework shape. The project owner's
/// post-merge framework pass owns their POLICY; the corpus only records what
/// the substrate does today.
#[cfg(test)]
const FRAMEWORK_ONLY_WORKLIST: &[&str] = &[
    "C05_withdefaults_intersection_clean",
    "C06_withdefaults_intersection_degraded",
    "F01_withdefaults",
    "F02_definemodel",
    "F03_defineslots",
    "F04_defineprops_runtime_spread",
    "F05_defineoptions_runtime_spread",
];

/// Value-indistinct rows (the `checker` names a value [`NodeShape`]
/// buckets away: function/`Other`, union constituents/`Union`,
/// literal value/`Literal`) that do not carry [`Expect::Node`].
///
/// Each entry is `(row id, owner, reason)`. Owner must equal the row's
/// `owner` column (guard-asserted).
///
/// Anti-recurrence floor: deleting a deep pin makes the row appear
/// unnamed and fails
/// `value_indistinct_rows_carry_deep_pins_or_are_named_shallow`. A new
/// value-indistinct row without a deep pin must be named here.
///
/// Burn-down only: remove an entry in the same change that deepens
/// the row. [`SHALLOW_PINNED_ROWS_CEILING`] caps the size, and
/// `value_indistinct_rows_carry_deep_pins_or_are_named_shallow`
/// fails any growth past it.
#[cfg(test)]
const SHALLOW_PINNED_ROWS: &[(&str, Owner, &str)] = &[
    (
        "C03_emits_intersection_clean",
        Owner::SharedTypeResolution,
        "member `evA` Other — a function value; deepening pins the signature (params + return)",
    ),
    (
        "C13_emits_heritage_clean",
        Owner::SharedTypeResolution,
        "member `evA` Other — a function value; deepening pins the signature (params + return)",
    ),
    (
        "CC02_annotated_return_literal_union",
        Owner::U6ContextualCore,
        "member `mode` Union — deepening pins the exact constituent set",
    ),
    (
        "CC03_as_const_plain_return",
        Owner::U6ContextualCore,
        "member `label` Literal + member `n` Literal — deepening pins the exact literal value",
    ),
    (
        "CC04_as_const_member",
        Owner::U6ContextualCore,
        "member `label` Literal — deepening pins the exact literal value",
    ),
    (
        "CC05_satisfies_literal_union",
        Owner::U6ContextualCore,
        "member `label` Literal + member `n` Literal — deepening pins the exact literal value",
    ),
    (
        "CC08_contextual_through_call_return",
        Owner::U6ContextualCore,
        "member `label` Union — deepening pins the exact constituent set",
    ),
    (
        "CC09_satisfies_widening_target",
        Owner::U6ContextualCore,
        "member `label` Literal + member `n` Literal — deepening pins the exact literal value",
    ),
    (
        "E05_scalar_flow_answer_keeps_tsx_surface",
        Owner::SharedCompilePipeline,
        "root Literal — deepening pins the exact literal value",
    ),
    (
        "F01_withdefaults",
        Owner::FrameworkOnly,
        "member `extra` Union — deepening pins the exact constituent set",
    ),
    (
        "F03_defineslots",
        Owner::FrameworkOnly,
        "member `default` Other — a function value; deepening pins the signature (params + return)",
    ),
    (
        "G06_emits_plain",
        Owner::U6FlowReturnSubstrate,
        "member `evA` Other + member `evB` Other — a function value; deepening pins the signature (params + return)",
    ),
    (
        "N18_logical_and_opaque_false_edge_keeps_union",
        Owner::U6NarrowTypeof,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "N21_guard_union_uses_final_alternative_overlay",
        Owner::U6NarrowTypeof,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "N22_double_negated_guard_union_uses_final_alternative_overlay",
        Owner::U6NarrowTypeof,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "N23_impossible_conjunction_drops_dead_disjunction_alternative",
        Owner::U6NarrowLattice,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "X10_destructured_default_conditional",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X14_accessor_pair",
        Owner::U6FlowReturnSubstrate,
        "member `g` Other — a function value; deepening pins the signature (params + return)",
    ),
    (
        "X21_satisfies_plain_return",
        Owner::U6ValueInference,
        "member `label` Literal + member `n` Literal — deepening pins the exact literal value",
    ),
    (
        "X23_switch_fallthrough_var",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X24_try_write_catch_read",
        Owner::U6ValueInference,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "X25_try_assertion_catch_scope",
        Owner::U6ValueInference,
        "member `caught` Union — deepening pins the exact constituent set",
    ),
    (
        "X27_finally_fallthrough_break_override",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X29_write_annotated_union_write_widens",
        Owner::U6ValueInference,
        "member `v` Literal — deepening pins the exact literal value",
    ),
    (
        "X30_switch_terminating_arm_write_fallthrough",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X31_switch_default_break_state",
        Owner::U6ValueInference,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "X32_switch_exhaustive_single_case",
        Owner::U6ValueInference,
        "member `v` Literal — deepening pins the exact literal value",
    ),
    (
        "X33_switch_case_narrows_discriminant",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X34_switch_exhaustive_union_no_implicit_undefined",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X37_labeled_conditional_break_write",
        Owner::U6ValueInference,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "X38_switch_conditional_break_write",
        Owner::U6ValueInference,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "X50_switch_break_exit_closes_crossed_scope",
        Owner::U6ValueInference,
        "member `n` Union — deepening pins the exact constituent set",
    ),
    (
        "X52_finally_entry_joins_pending_break",
        Owner::U6ValueInference,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "X54_switch_live_fallthrough_reaches_default",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X55_finally_entry_joins_pending_return",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X56_finally_return_preserves_try_return",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X57_if_arm_closes_lexical_shadow",
        Owner::U6ValueInference,
        "member `y` Union — deepening pins the exact constituent set",
    ),
    (
        "X61_finally_break_preserves_own_exit",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X63_hoisted_var_no_init_preserves_write",
        Owner::U6ValueInference,
        "root Literal — deepening pins the exact literal value",
    ),
    (
        "X66_hoisted_annotated_var_authority_serves_forward_read",
        Owner::U6ValueInference,
        "member `v` Union — deepening pins the exact constituent set",
    ),
    (
        "X67_destructured_parameter_authority_precedes_writes",
        Owner::U6ValueInference,
        "member `v` Literal — deepening pins the exact literal value",
    ),
    (
        "X68_finally_return_over_labelled_break_keeps_undefined",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X69_overlapping_object_union_assignment_selects_narrow_arm",
        Owner::U6ValueInference,
        "member `kind` Literal — deepening pins the exact literal value",
    ),
    (
        "X70_loop_callback_argument_is_not_invoked_closure",
        Owner::U6FlowReturnSubstrate,
        "root Literal — deepening pins the exact literal value",
    ),
    (
        "X71_loop_member_write_compares_full_selected_path",
        Owner::U6FlowReturnSubstrate,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X72_loop_unreachable_write_does_not_trigger_refusal",
        Owner::U6FlowReturnSubstrate,
        "root Literal — deepening pins the exact literal value",
    ),
    (
        "X75_fresh_object_assignment_selects_optional_member_arm",
        Owner::U6ValueInference,
        "root Other — a function value; deepening pins the signature (params + return)",
    ),
    (
        "X76_fresh_computed_object_assignment_selects_optional_member_arm",
        Owner::U6ValueInference,
        "root Other — a function value; deepening pins the signature (params + return)",
    ),
    (
        "X77_spread_object_assignment_preserves_declared_union",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X78_mutable_var_capture_uses_declared_authority",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X79_forward_let_capture_uses_declared_authority",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X80_wrapped_labelled_try_finally_keeps_undefined",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X81_while_false_body_write_is_inert",
        Owner::U6FlowReturnSubstrate,
        "root Literal — deepening pins the exact literal value",
    ),
    (
        "X84_required_property_assignment_preserves_optional_union_arm",
        Owner::U6ValueInference,
        "root Union — deepening pins the exact constituent set",
    ),
    (
        "X86_destructured_parameter_capture_retains_declared_union",
        Owner::U6ValueInference,
        "root Other — a function value; deepening pins the signature (params + return)",
    ),
    (
        "N29_switch_optional_chain_discriminant",
        Owner::U6NarrowLattice,
        "member Union carrying Opaque(Miss) — the recursive expectation vocabulary has no Miss variant, so the surface is unspellable as a deep pin; the member pin plus the KnownOwed note carry it",
    ),
    (
        "N32_optional_chain_property_discriminant",
        Owner::U6NarrowLattice,
        "member Union carrying Opaque(Miss) — no Miss variant in the recursive expectation vocabulary",
    ),
    (
        "N33_computed_property_discriminant",
        Owner::U6NarrowLattice,
        "member Union carrying Opaque(Miss) — no Miss variant in the recursive expectation vocabulary",
    ),
    (
        "N36_aliased_discriminant",
        Owner::U6NarrowLattice,
        "member Union carrying Opaque(Miss) — no Miss variant in the recursive expectation vocabulary",
    ),
    (
        "N37_destructured_local_discriminant",
        Owner::U6NarrowLattice,
        "member Union carrying Opaque(Miss) — no Miss variant in the recursive expectation vocabulary",
    ),
    (
        "N39_instanceof_imported_class",
        Owner::U6NarrowTypeof,
        "member Union carrying Opaque(Miss) beside DeclRef(Box) — no Miss variant in the recursive expectation vocabulary",
    ),
    (
        "N41_instanceof_member_expression_constructor",
        Owner::U6NarrowTypeof,
        "member Union carrying Opaque(Miss) beside DeclRef(Box) — no Miss variant in the recursive expectation vocabulary",
    ),
    (
        "N43_boolean_wrapped_guard",
        Owner::U6NarrowTypeof,
        "the member Union EQUALS the checker, so a deep pin would have to be MatchesChecker while the result is a typed ReturnOnly; the KnownOwed note and the member pin carry the divergence",
    ),
    (
        "N50_sequence_discriminant_test",
        Owner::U6NarrowLattice,
        "member Union carrying Opaque(Miss) — no Miss variant in the recursive expectation vocabulary",
    ),
    (
        "N61_unannotated_const_assertion_does_not_narrow",
        Owner::U6NarrowSubstitution,
        "member Union — the published value EQUALS the checker, so a recursive pin would assert the divergence this KnownOwed row records does not exist",
    ),
    (
        // `Array.isArray` is not applied as a predicate on either edge.
    "N74_array_isarray_true_arm",
        Owner::U6NarrowTypeof,
        "member Union carrying Array(number) — no Array variant in the recursive expectation vocabulary",
    ),
    (
        "N75_array_isarray_false_arm",
        Owner::U6NarrowTypeof,
        "member Union carrying Array(number) — no Array variant in the recursive expectation vocabulary",
    ),
    (
        "N81_equality_against_let_widened_target_does_not_narrow",
        Owner::U6NarrowTypeof,
        "member Union — the published value EQUALS the checker, so a recursive pin would assert a divergence this KnownOwed row does not have",
    ),
    (
        "N84_let_aliased_condition_does_not_narrow",
        Owner::U6NarrowTypeof,
        "member Union — the published value EQUALS the checker, so a recursive pin would assert a divergence this KnownOwed row does not have",
    ),
    (
        "X95_evolving_let_both_branches_join",
        Owner::U6ValueInference,
        "member Union — the published value EQUALS the checker (the const-asserted literals are preserved through the join), so a recursive pin would assert a divergence this KnownOwed row does not have; it stays parked on the ConditionalVarDefinition admission",
    ),
    (
        "X96_evolving_let_explicit_undefined_initializer",
        Owner::U6ValueInference,
        "member Union carrying Opaque(Miss) — no Miss variant in the recursive expectation vocabulary",
    ),
    (
        "N88_in_unknown_key_keeps_subject_as_typed_superset",
        Owner::U6NarrowLattice,
        "member Union — the recorded checker answer carries `Record<\"c\", unknown>` generic \
         syntax the checker-syntax comparer cannot parse yet, so a deep pin cannot be \
         cross-validated; deepen when the comparer grows generic-argument support",
    ),
];

/// Burn-down ceiling of [`SHALLOW_PINNED_ROWS`]. Lower freely as rows
/// deepen; raising it admits a new shallow exception and must record why.
/// Asserted by `value_indistinct_rows_carry_deep_pins_or_are_named_shallow`.
///
/// The remaining shallow entries are each structurally blocked rather than
/// shallow by convenience: a measured value the recursive vocabulary cannot
/// spell, an over-narrow CONTROL whose published value EQUALS the checker
/// while the result is a typed `ReturnOnly` (a recursive pin would have to
/// assert a divergence the row does not have, which
/// `deep_pinned_rows_semantic_equality_follows_their_verdict` correctly
/// rejects), or a recorded CHECKER text the deep-pin comparer cannot yet
/// parse. Each ledger entry records which class it is in.
#[cfg(test)]
const SHALLOW_PINNED_ROWS_CEILING: usize = 72;

/// The shapes this corpus landed with as OPEN debts — production disagrees
/// with the checker, or deletes a type-check surface the checker types.
///
/// This is the block's remaining debt in ONE place. Closing a debt means
/// re-pinning its row AND removing its id here, in the same change.
#[cfg(test)]
const OPEN_DEBTS: &[&str] = &[
    // ── TypeScript semantics: flow-return substrate ──────────────────────
    // An intersection / heritage arm whose flow return is WHOLLY unmodelled is
    // silently DROPPED instead of failing closed. This is the family the 15
    // existing `ReturnType<typeof …>` tests structurally could not reach:
    // none of them uses `&` or `extends`.
    "C04_emits_intersection_degraded",
    "C11_props_intersection_unmodelled_arm",
    "C12_heritage_unmodelled_clause",
    // ── Consumer reach: the TSX lane FAULTS ──────────────────────────────
    // The file loses its whole type-check surface for programs the checker
    // types without difficulty.
    "D10_callee_new_spread_only",
    "D11_callee_new_spread_key",
    "E01_spread_any",
    "E02_spread_index_signature",
    "E03_spread_array",
    // ── NARROWING ────────────────────────────────────────────────────
    // The narrowing blocks landed: every seeded narrowing row now matches
    // the checker except N09_narrow_then_write, whose remaining debt is
    // not the narrowing — `v.trim()` is a call to a string-intrinsic
    // method and the walk authority has no lib/intrinsic member surface
    // for a primitive base (`UnrepresentableCallee`, ReturnOnly).
    "N09_narrow_then_write",
    // The impossible-predicate STATEMENT spelling keeps the dead `x`
    // contributor (`v: A | B | "ok" | "no"` where the checker computes
    // `"no" | "ok"`), wrong-and-warm. Exposed by the recursive expect
    // pin — the root `v: Union` member pin could not see it.
    "N25_impossible_predicate_statement_keeps_dead_contributor",
    "N55_in_operator_nonliteral_key",
    // The checker narrows `"k" in x` for an undeclared key to
    // `(subject) & Record<key, unknown>` on the positive edge; that
    // intersection carrier is not mintable, so the subject publishes
    // unchanged behind the typed guard gap (superset, ReturnOnly).
    "N88_in_unknown_key_keeps_subject_as_typed_superset",
    // The positive `typeof x === "function"` edge over `object` narrows to
    // the checker's global `Function` surface; the flow environment has no
    // resolvable lib `Function`, so the arm stays `object` behind the
    // typed guard gap (superset, ReturnOnly).
    "N90_typeof_function_over_object_narrows_to_function",
    // ── CALL RESOLUTION — context-sensitive callback inference ──────────
    // A callback argument's un-annotated parameter is never contextually
    // typed: withheld from the first inference pass and never re-typed
    // under the fixed substitution, so a type parameter inferable ONLY
    // from the callback's return binds `unknown` where the checker
    // computes the contextual union.
    "CC06_contextual_arrow_param",
    // ── TypeScript semantics: adversarial axes (X family) ──────────────
    // A get/set pair surfaces as a duplicate member key: refused, TSX faults.
    "X14_accessor_pair",
    // Async return wrapping is unmodelled: the Promise is silently unwrapped
    // and the inner object is published as a props surface.
    "X18_async_return",
    // The macro lane correctly rejects a generator return, but the TSX lane
    // faults with the same code — the consumer-reach debt class.
    "X19_generator_yield",
    // A return-bearing loop remains outside the value-inference surface. The
    // NoValue refusal is honest until loop-owned break/return joining exists.
    "X82_loop_break_finally_return_awaits_return_bearing_loop_support",
    // A scalar literal is correctly rejected as a props macro type, but that
    // runtime diagnostic must not delete the file's IDE TSX surface.
    "E05_scalar_flow_answer_keeps_tsx_surface",
    // ── TypeScript semantics: mapped heritage (index signature, nested builtin) ──
    // The mapped route over an index-signature heritage interface drops the
    // index signature and publishes complete; the direct-alias route drops it
    // byte-identically — a pre-existing mapped source-member enumeration
    // defect whose reach the heritage work extended.
    "C14_mapped_heritage_drops_index_signature",
    // A nested builtin in the heritage clause's key domain is not reduced by
    // the one-hop fallback — a conservative zero-member publication where the
    // checker computes the closed surface.
    "C15_heritage_nested_builtin_open",
    // ── satisfies-contextual widening (value inference) ────────────────
    // The satisfies TARGET never contextually types the operand's members:
    // a fresh member literal keeps its `Literal` node where the checker's
    // contextual target widens it — a strict subtype of the truth, warm.
    // The gap and its repair are named on both rows' notes.
    "CC09_satisfies_widening_target",
    "X21_satisfies_plain_return",
    // ── NARROWING VOCABULARY — guard forms the flow lattice does not
    //    carry. Every row publishes an HONEST SUPERSET as a typed
    //    ReturnOnly (zero candidates, recomputed cold), never a
    //    silently narrowed answer.
    "N27_switch_true_guard_dispatch",
    "N28_switch_typeof_dispatch",
    "N29_switch_optional_chain_discriminant",
    "N30_switch_true_negated_guard_dispatch",
    "N32_optional_chain_property_discriminant",
    "N33_computed_property_discriminant",
    "N34_non_null_asserted_property_discriminant",
    "N35_const_aliased_condition",
    "N36_aliased_discriminant",
    "N37_destructured_local_discriminant",
    "N39_instanceof_imported_class",
    "N41_instanceof_member_expression_constructor",
    // The `instanceof` arm direction the graph cannot prove derived: a
    // subclass test over a base-typed subject is assignable in exactly
    // one direction, and structural assignability cannot separate a
    // genuine subclass from a same-shape underived constructor. The
    // subject publishes unnarrowed behind the typed guard gap.
    "N94_instanceof_subclass_test_over_base_gaps",
    "N42_comma_sequence_guard",
    "N43_boolean_wrapped_guard",
    "N44_typeof_over_unknown",
    "N46_typeof_over_any",
    "N47_correlated_tuple_discriminant",
    "N48_closure_narrows_captured_binding",
    "N50_sequence_discriminant_test",
    // Predicate / assertion CALL TARGETS the guard rail does not accept: an
    // arrow-expression binding, an object-literal method, a class method, an
    // annotated `const`, and a generic predicate instantiated at the call
    // site. Each publishes the unnarrowed union as a typed ReturnOnly. The
    // two `does_not_narrow` rows are the paired over-narrow CONTROLS: their
    // published value is already correct and only the admission is owed, so
    // a repair must leave their surfaces alone.
    "N56_arrow_predicate_annotated_binding",
    "N57_object_literal_method_predicate",
    "N59_generic_predicate_instantiated_at_call",
    "N60_class_method_assertion_narrows",
    "N61_unannotated_const_assertion_does_not_narrow",
    "N62_annotated_const_assertion_narrows",
    // Discriminant / `in` key SPELLINGS outside the decidable-guard set:
    // an enum member reference, a const-typed literal key, a numeric key,
    // and a `typeof`-narrowed callable. The optional-member row is the one
    // NARROWER-than-checker answer in this group; its note says so.
    "N65_enum_member_discriminant",
    "N69_in_operator_const_literal_key",
    "N70_in_operator_numeric_key",
    "N72_typeof_function_guard",
    "N74_array_isarray_true_arm",
    "N75_array_isarray_false_arm",
    "N76_loose_equality_null_removes_both",
    "N79_equality_against_const_literal_binding",
    "N80_equality_against_const_literal_target_narrows",
    "N81_equality_against_let_widened_target_does_not_narrow",
    "N84_let_aliased_condition_does_not_narrow",
    // Evolving `let` bindings, the `never`-default switch admission, the
    // `??` short circuit, index-signature reads, and a closure created
    // inside a narrowed arm.
    "X91_assert_never_default_arm_contributes_nothing",
    "X94_evolving_let_one_branch_keeps_undefined",
    "X95_evolving_let_both_branches_join",
    "X96_evolving_let_explicit_undefined_initializer",
    "X97_evolving_let_switch_without_default_keeps_undefined",
    "X101_optional_chain_nullish_coalesce",
    "X105_closure_captures_narrowed_binding_in_guarded_arm",
    "X108_record_index_read_has_no_undefined",
    "X109_optional_index_read_through_optional_chain",
    // ── SUBTYPE-REUNION: TypeScript's return-position reunion applies
    //    subtype reduction (a subtype arm is absorbed into its supertype
    //    peer); the canonical union keeps both constituents. Each row is
    //    EXTENSIONALLY equal to the checker — the extra arm is redundant,
    //    never wrong — so the surfaces stay clean and warm while parked
    //    with the value-inference owner. Subtype absorption must NOT be
    //    added to the canonical layer; the debt closes in the inference
    //    layer that owns reunion.
    "X26_switch_assertion_case_scope",
    "X36_labeled_break_drops_arm_assertion",
    "X39_try_catch_throw_point_join",
    "X45_switch_fallthrough_case_narrows_by_chain_tests",
    "X46_try_catch_template_throw_point",
    "X47_try_catch_sequence_throw_point",
    "X48_try_catch_if_guard_throw_point",
    "X49_try_catch_new_callee_throw_point",
    // ── INTERSECTION-REDUCTION residue, the same extensional-equality
    //    class: the checker reduces a literal intersected with the empty
    //    object to the bare literal; the canonical intersection keeps
    //    both constituents. Clean and warm; closes with the same
    //    value-inference reduction work.
    "N106_declared_intersection_keeps_literal",
    "N115_member_intersection_call_stays_pinned",
];

// Per-owner conformance — the merge go/no-go

/// Per-owner conformance, pinned.
///
/// `(owner, total, matching, parked)` — `matching` is the number of rows whose
/// computed answer EQUALS the checker's; `parked` is the number of
/// [`Verdict::KnownOwed`] rows. **Merging this branch back to `main` is gated
/// on every parked row being green**, so `parked == 0` across every owner is
/// the go/no-go and this table shows which owner is blocking.
///
/// Pinned rather than merely printed: a passing gate that quietly PRINTS a
/// worse number is not a gate. Any movement — an owner improving OR regressing
/// — fails [`corpus_conformance_by_owner`] with the full table in the failure
/// message, which is the signal to re-pin the rows and this ledger together.
#[cfg(test)]
const CONFORMANCE: &[(Owner, usize, usize, usize)] = &[
    (Owner::U2IndexedAccess, 3, 1, 2),
    (Owner::U2MappedTemplate, 4, 1, 2),
    (Owner::U6CallResolve, 16, 15, 1),
    // Eight switch-, try/catch- and reunion-family rows are parked as the
    // SUBTYPE-REUNION class: TypeScript's return-position reunion applies
    // subtype reduction and absorbs a subtype arm into its supertype; the
    // canonical union keeps both constituents. Extensionally equal, so
    // the rows stay clean and warm while parked with the value-inference
    // owner.
    (Owner::U6ValueInference, 93, 74, 16),
    (Owner::U6LoopClosure, 6, 1, 2),
    (Owner::U6ContextualCore, 8, 7, 1),
    (Owner::U6FlowReturnSubstrate, 63, 47, 3),
    (Owner::U6NarrowTypeof, 44, 24, 20),
    // The `instanceof` arm rule: derived-arm selection with nullish
    // stripping and the whole-subject intersection fallback are exact;
    // the assignable-but-unproven-derived arm direction (a subclass test
    // over a base-typed subject, a structural twin constructor) publishes
    // the unnarrowed subject behind the typed guard gap and is parked.
    (Owner::U6NarrowInstanceof, 4, 3, 1),
    // N25's MatchesChecker label predated the recursive expect pin; the
    // deep measurement showed the dead contributor SURVIVES (wrong-and-
    // warm), so the row is parked against its narrowing block.
    (Owner::U6NarrowLattice, 38, 24, 14),
    (Owner::U6NarrowSubstitution, 12, 6, 6),
    (Owner::U6NarrowInvalidation, 2, 1, 1),
    (Owner::SharedTypeResolution, 12, 7, 3),
    (Owner::SharedCompilePipeline, 8, 1, 7),
    (Owner::FrameworkOnly, 7, 5, 0),
];

#[cfg(test)]
mod conformance {
    use super::*;

    fn tally() -> Vec<(Owner, usize, usize, usize)> {
        Owner::ALL
            .iter()
            .map(|owner| {
                let rows: Vec<&Row> = CORPUS.iter().filter(|r| r.owner == *owner).collect();
                let matching = rows
                    .iter()
                    .filter(|r| matches!(r.verdict, Verdict::MatchesChecker))
                    .count();
                let parked = rows
                    .iter()
                    .filter(|r| matches!(r.verdict, Verdict::KnownOwed { .. }))
                    .count();
                (*owner, rows.len(), matching, parked)
            })
            .collect()
    }

    fn render(tally: &[(Owner, usize, usize, usize)]) -> String {
        let mut out = String::from(
            "\n╔═══ U6 SHAPE CORPUS — CONFORMANCE AGAINST tsgo 7.0.0-dev.20260526.1 ═══\n\
             ║ owner                                    rows  match   conf   PARKED\n\
             ╟───────────────────────────────────────────────────────────────────────\n",
        );
        let (mut t, mut m, mut p) = (0usize, 0usize, 0usize);
        for (owner, total, matching, parked) in tally {
            t += total;
            m += matching;
            p += parked;
            let conf = if *total == 0 {
                "   —  ".to_owned()
            } else {
                format!("{:5.1}%", 100.0 * *matching as f64 / *total as f64)
            };
            let note = if *total == 0 {
                "   ← NO ROWS SEEDED: this block has no fence"
            } else if *parked > 0 && !owner.has_convergence_owner() {
                "   ← NO OWNER ASSIGNED"
            } else {
                ""
            };
            out.push_str(&format!(
                "║ {:<40} {total:>4}  {matching:>5}  {conf}  {parked:>6}{note}\n",
                owner.id()
            ));
        }
        let conf = format!("{:5.1}%", 100.0 * m as f64 / t as f64);
        out.push_str(&format!(
            "╟───────────────────────────────────────────────────────────────────────\n\
             ║ {:<40} {t:>4}  {m:>5}  {conf}  {p:>6}\n\
             ╚═══ MERGE GATE: parked must reach 0 (currently {p}) ═══\n",
            "TOTAL"
        ));
        out
    }

    /// The per-owner conformance number, pinned and reported.
    #[test]
    fn corpus_conformance_by_owner() {
        let measured = tally();
        let table = render(&measured);
        // Visible with `--nocapture`, and written where a gate operator can
        // read it without re-running anything.
        println!("{table}");

        assert_eq!(
            measured, CONFORMANCE,
            "the per-owner conformance number MOVED.\n{table}\n\
             This is the merge go/no-go, so it is pinned rather than merely printed. If an owner \
             IMPROVED, re-pin the affected rows and this CONFORMANCE ledger in the same change. \
             If an owner REGRESSED, that is the finding."
        );
    }

    /// Rows parked with NO assigned owner at all, called out by name.
    ///
    /// These have nobody assigned to fix them and will otherwise be the last
    /// thing blocking the merge.
    #[test]
    fn unassigned_parked_rows_are_named() {
        let mut unassigned: Vec<&str> = CORPUS
            .iter()
            .filter(|r| {
                !r.owner.has_convergence_owner() && matches!(r.verdict, Verdict::KnownOwed { .. })
            })
            .map(|r| r.id)
            .collect();
        // Compared as a SET: the corpus is append-only, so row order tracks
        // when a shape was measured, not what it means.
        unassigned.sort_unstable();
        assert_eq!(
            unassigned, UNASSIGNED_PARKED_ROWS,
            "the set of parked rows owned by no scheduled U-block changed. Keep this list exact: \
             it is the only place these rows are visible as a scheduling gap rather than as \
             generic debt."
        );
    }
}

/// Parked rows that belong to NO `U*` block. Nobody is scheduled to fix these.
#[cfg(test)]
const UNASSIGNED_PARKED_ROWS: &[&str] = &[
    // SHARED.TYPE_RESOLUTION — the intersection / heritage surface reducer
    // drops an arm whose flow return is wholly unmodelled.
    "C04_emits_intersection_degraded",
    "C11_props_intersection_unmodelled_arm",
    "C12_heritage_unmodelled_clause",
    // SHARED.COMPILE_PIPELINE — the TSX (IDE) lane deletes the file's whole
    // type-check surface for programs the checker types without difficulty.
    "D10_callee_new_spread_only",
    "D11_callee_new_spread_key",
    "E01_spread_any",
    "E02_spread_index_signature",
    "E03_spread_array",
    "E05_scalar_flow_answer_keeps_tsx_surface",
    // The generator-return shape: the macro lane's rejection is correct, the
    // TSX lane fault is not.
    "X19_generator_yield",
];
