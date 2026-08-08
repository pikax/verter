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
//! adding a shape is one [`Row`] literal, and every lane (`checker`, the flow
//! graph node and its members, runtime bytes, TSX bytes, the Svelte twin) is
//! driven from that one literal by the shared drivers below. If adding a shape
//! requires editing a driver, the driver is wrong.
//!
//! # What a row carries
//!
//! PRIMARY — the semantic answer, and who owns it:
//!
//! * `script` — the authored program, spliced verbatim into every lane.
//! * `checker` — what tsgo `7.0.0-dev.20260526.1`
//!   (`--noEmit --strict --ignoreConfig`, CHECKER only, never `.d.ts`) prints
//!   for the row's `probe`. Verified live by
//!   [`oracle::corpus_checker_column_matches_tsgo`] whenever the pinned binary
//!   is resolvable, so it is never a guess.
//! * `flow` — the flow-return GRAPH NODE, its per-MEMBER node shapes, the
//!   typed `degradation`, and the `slot_candidate_count`.
//! * `owner` — the block the row is attributed to ([`Owner`]). Drives the
//!   per-owner conformance number, which is the merge go/no-go.
//! * `subject` — [`Subject::TypeScript`] or [`Subject::FrameworkOnly`].
//! * `demand` — [`Demand::MacroSurface`] or [`Demand::Narrowing`].
//! * `verdict` — [`Verdict`], the row's relationship to the checker.
//!
//! SECONDARY — did the answer reach a consumer intact? Optional; `Skip` by
//! default:
//!
//! * `runtime` — the EMITTED option value (`props: {…}` / `emits: […]`),
//!   BRACKET-MATCHED out of the rendered `CompileTarget::BUNDLER` module.
//! * `tsx` — the `CompileTarget::IDE | TEMPLATE_DATA` lane outcome, reached
//!   through `ensure_ide_compiled` + `get_ide`.
//! * `svelte` — the `.svelte` twin's `FrameworkSurfaceKind::Props` member set.
//!   NOTE: Svelte props are served by `resolve_framework_surface_with_audit`,
//!   NOT by `ComponentMetaAnalysis.props`; a harness driving the latter reports
//!   every Svelte row empty and proves nothing.
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
//!    `SemanticNodeData` directly, at the row's node AND at each of its
//!    members.
//!
//! # CONTRIBUTOR NOTE — read this before you build your own fixtures
//!
//! **This is a TypeScript semantics corpus.** A row's identity is the semantic
//! answer the substrate computes for a plain `.ts` program, measured against
//! the checker: the flow-return GRAPH NODE, its MEMBER shapes, the typed
//! `degradation`, the `slot_candidate_count`. Vue and Svelte emission are an
//! OPTIONAL SECONDARY column meaning "the semantic answer reached a consumer
//! intact" — real evidence (several defects here were caught only there), but
//! never the subject. Most rows you add will carry no framework column at all.
//!
//! **If you measured a shape, add it here and COMMIT it on your branch.**
//! Passing or failing. A reviewer who measured a shape in a scratch worktree,
//! found a defect, reported it in prose, and did not land the row has thrown
//! the measurement away — the next agent rebuilds it, slightly differently,
//! and the surface never grows. A failing row is worth more than a paragraph:
//! the fix agent's target is then exact and its red-first evidence already
//! exists. Landed coverage grows monotonically; measurement corpora do not,
//! unless you append.
//!
//! # Adding a row — the whole procedure
//!
//! 1. **Write the row.** One [`Row`] literal appended to [`CORPUS`], with
//!    `..Row::BLANK` filling everything else. `BLANK` defaults every FRAMEWORK
//!    lane to `Skip`, so a plain `.ts` semantic row is the SHORT literal:
//!    ```text
//!    Row { id: "N13_…", script: "…", checker: "{ label: string; }",
//!          flow: Flow::Result { function: "makeProps", node: NodeShape::Object,
//!                               members: &[("label", NodeShape::Union)],
//!                               degradation: Degr::None, candidates: 1 },
//!          verdict: Verdict::KnownOwed { … }, ..Row::BLANK },
//!    ```
//!    Add a framework column only when you want to assert that the answer
//!    survived the trip to a consumer.
//! 2. **Measure it, do not guess it.**
//!    ```text
//!    U6_CORPUS_DUMP=1 cargo test -p verter_session --lib u6_flow_shape_corpus \\
//!        -- --nocapture --test-threads=1 2>&1 | grep <your_row_id>
//!    ```
//!    Every lane prints its MEASURED value. Transcribe them into the row.
//! 3. **Record the checker's answer.** `checker` is what tsgo prints for
//!    `probe`; leave it empty and the row is rejected. Do not hand-write it —
//!    run the suite, and [`oracle::corpus_checker_column_matches_tsgo`]
//!    regenerates the probe from your own `script` + `probe` and byte-compares
//!    it against the pinned binary. If the row is `any`, set `checker_is_any`:
//!    `any` is assignable to `null`, so the shape probe alone cannot see it.
//! 4. **Pin the MEMBER shapes, not just the enclosing node.** For anything
//!    that depends on a computed member type — every narrowing row — the
//!    enclosing node is `Object` whether or not the guard applied. `members`
//!    is where the answer actually lives.
//! 5. **Pick the verdict.** [`Verdict`] is the row's relationship to the
//!    CHECKER, and [`verdict_consistency`] enforces each claim:
//!    * [`Verdict::MatchesChecker`] — the computed answer equals the
//!      checker's. No erased member, no refusal.
//!    * [`Verdict::Degraded`] — the member SET is right and some member TYPE
//!      is erased. An honest weaker answer.
//!    * [`Verdict::FailsClosed`] — production REFUSES, and refusing is the
//!      DESIGNED answer because the root's key set is genuinely unknowable.
//!    * [`Verdict::KnownOwed`] — production DISAGREES with the checker and the
//!      divergence is a debt. Name the `owner`; for a framework row put in
//!      `owed_absent` the needles that would APPEAR if the debt were repaired;
//!      for a semantic row the pinned `members` are the tripwire. Append the
//!      row's id to [`OPEN_DEBTS`]. This makes the row fail in BOTH directions
//!      — if the shape degrades further, AND the moment an owner fixes it —
//!      so a repair is visible instead of silent.
//! 6. **Set `demand` / `subject` when they apply.** A narrowing row sets
//!    [`Demand::Narrowing`] with its owning `U6.NARROW_*` block. A shape that
//!    exists only as a framework shape sets [`Subject::FrameworkOnly`] and
//!    joins `FRAMEWORK_ONLY_WORKLIST`.
//! 7. **Run the suite and commit the row.** Thirteen tests, about five seconds
//!    once the crate is built. No other crate and no other suite is involved.

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
    U6ContextualCore,
    U6FlowReturnSubstrate,
    U6NarrowTypeof,
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
        Owner::U6ContextualCore,
        Owner::U6FlowReturnSubstrate,
        Owner::U6NarrowTypeof,
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
            Self::U6ContextualCore => "U6.CONTEXTUAL_CORE",
            Self::U6FlowReturnSubstrate => "U6.FLOW_RETURN_SUBSTRATE",
            Self::U6NarrowTypeof => "U6.NARROW_TYPEOF",
            Self::U6NarrowLattice => "U6.NARROW_LATTICE",
            Self::U6NarrowSubstitution => "U6.NARROW_SUBSTITUTION",
            Self::U6NarrowInvalidation => "U6.NARROW_INVALIDATION",
            Self::SharedTypeResolution => "SHARED.TYPE_RESOLUTION  (no U-block)",
            Self::SharedCompilePipeline => "SHARED.COMPILE_PIPELINE (no U-block)",
            Self::FrameworkOnly => "FRAMEWORK_ONLY          (owner post-merge)",
        }
    }

    /// Whether the owner is a scheduled `U*` block. `false` means nobody is
    /// assigned, which is exactly the class that silently blocks a merge.
    pub(crate) const fn is_scheduled_block(self) -> bool {
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

/// WHAT a row demands — the row's SUBJECT, named rather than implied.
///
/// The corpus is the fence for the whole `U6` programme, not only for
/// `U6.FLOW_RETURN_SUBSTRATE`. Narrowing (`U6.NARROW_TYPEOF`,
/// `U6.NARROW_LATTICE`, `U6.NARROW_SUBSTITUTION`, `U6.NARROW_INVALIDATION`) is
/// the class where a weakening passes every test that was not written for it:
/// a branch join that silently widens, a `typeof` guard that stops applying
/// across a call boundary, a narrowed type that stays warm after its
/// predicate's dependency changes. None of those announce themselves, so the
/// fence has to exist BEFORE the work starts.
///
/// # Where a genuinely new subject attaches
///
/// A narrowing row whose evidence is a RETURNED MEMBER's type needs no new
/// machinery: the narrowed type is observable in the emitted option value
/// (`label: { type: String` vs an erased `type: null`), so it rides the
/// existing drivers and is only LABELLED here. That is
/// [`Demand::Narrowing`] — a new variant, not a new harness.
///
/// A subject the current drivers genuinely cannot express is
/// **type-at-an-arbitrary-position under a predicate** (the hover-shaped
/// demand: "what is `v` on line N, inside this guard"). Building it
/// speculatively would be an abstraction with no consumer, so it is NOT built.
/// The seam is exact and small, and is recorded here so the narrowing blocks
/// do not have to rediscover it:
///
/// 1. one new `Demand` variant carrying the position and the expected type;
/// 2. one new `drive_*` function beside [`drive_runtime`] / [`drive_flow`] /
///    [`drive_svelte`], demanding that position through the shared
///    `ProjectSemanticDispatch` (never a second resolver);
/// 3. one new lane test in `corpus_suite` that dispatches on the variant.
///
/// Nothing in [`Row`], [`Verdict`], [`report`], the oracle, or the debt ledger
/// changes for that, which is the property this enum exists to preserve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Demand {
    /// The published macro surface: the emitted runtime option value, the TSX
    /// projection, the flow-return graph node, and the `.svelte` twin.
    MacroSurface,
    /// A NARROWING shape, owned by one of the `U6.NARROW_*` blocks. Driven
    /// through exactly the same lanes — the narrowed type is evidence that
    /// surfaces in the published member's type — and labelled so the
    /// population is countable, filterable, and attributable.
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
            // The MEMBER shapes are the row's primary semantic evidence for
            // anything that depends on a computed member type. Only a closed
            // `Object` surface has named members; a spread PROGRAM is a
            // construction plan and reports none.
            let members: Vec<(String, NodeShape)> = match data.as_deref() {
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
            };
            MeasuredFlow::Result {
                node: node_shape(data.as_deref()),
                members,
                degradation: degr_of(result.degradation()),
                candidates,
            }
        }
        _ => MeasuredFlow::NoValue,
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
    NoValue,
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

include!("u6_flow_shape_corpus_rows_tests.rs");

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
                    if node != *got_node {
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
                    // The PRIMARY semantic assertion: each named member's own
                    // graph-node shape. A narrowing that stopped applying is
                    // visible ONLY here — the enclosing node is `Object`
                    // whether or not the guard was honoured.
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
                (Flow::NoValue, MeasuredFlow::NoValue) => {}
                (expected, got) => failures.push(report(
                    row,
                    &format!("flow lane of `{function}`"),
                    &format!("{expected:?}"),
                    &format!("{got:?}"),
                    "the flow lane's OUTCOME class changed (value \u{2194} no-value)",
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
// The ORACLE — how the `checker` column is obtained
// ─────────────────────────────────────────────────────────────────────────

/// The pinned checker. CHECKER only, never `.d.ts`.
#[cfg(test)]
const TSGO_VERSION: &str = "7.0.0-dev.20260526.1";

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
                    let pins_members = matches!(row.flow, Flow::Result { members, .. }
                        if !members.is_empty());
                    let observable = matches!(row.runtime, Runtime::Refused)
                        || matches!(row.tsx, Tsx::Faults(_))
                        || !owed_absent.is_empty()
                        || pins_members;
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
}

#[cfg(test)]
mod programme_ledgers {
    use super::*;

    /// Every narrowing row is OWNED by a `U6.NARROW_*` block, and every one of
    /// them is pinned against today's substrate until that block lands.
    ///
    /// The narrowing blocks are the class where a weakening passes every test
    /// that was not written for it. Seeding the rows BEFORE the work starts is
    /// what gives those blocks a fence on day one: each row fails the moment
    /// its shape starts behaving correctly, which forces a deliberate
    /// reclassification instead of a silent pass.
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
                matches!(row.flow, Flow::Result { members, .. } if !members.is_empty()),
                "{}: a narrowing row must pin at least one MEMBER shape — the enclosing node is \
                 `Object` whether or not the guard applied, so a row without a member \
                 assertion measures nothing",
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
    // `N07_branch_join_widens` was always absent here: it is the
    // over-narrow control and agreed with the checker from day one.
    "N09_narrow_then_write",
    // ── CONTEXTUAL — seeded before the owning block exists ──────────────
    // A declared return annotation's member union is collapsed to a lone
    // Primitive where the checker keeps `"a" | "b"`. Fails today by design;
    // the row flips the moment the annotation's union reaches the member.
    "CC02_annotated_return_literal_union",
    // ── TypeScript semantics: adversarial axes (X family) ──────────────
    // A whole-binding write to an annotated literal-union binding binds the
    // RHS widened where the checker assignment-reduces to the declared
    // constituent — pre-existing in the write application, surfaced by the
    // switch case-entry measurement.
    "X29_write_annotated_union_write_widens",
    // A get/set pair surfaces as a duplicate member key: refused, TSX faults.
    "X14_accessor_pair",
    // Async return wrapping is unmodelled: the Promise is silently unwrapped
    // and the inner object is published as a props surface.
    "X18_async_return",
    // The macro lane correctly rejects a generator return, but the TSX lane
    // faults with the same code — the consumer-reach debt class.
    "X19_generator_yield",
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
];

// ─────────────────────────────────────────────────────────────────────────
// PER-OWNER CONFORMANCE — the merge go/no-go
// ─────────────────────────────────────────────────────────────────────────

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
    (Owner::U2IndexedAccess, 1, 1, 0),
    (Owner::U2MappedTemplate, 4, 1, 2),
    (Owner::U6CallResolve, 4, 4, 0),
    (Owner::U6ValueInference, 26, 23, 1),
    (Owner::U6ContextualCore, 9, 8, 1),
    (Owner::U6FlowReturnSubstrate, 44, 36, 2),
    (Owner::U6NarrowTypeof, 9, 9, 0),
    (Owner::U6NarrowLattice, 3, 3, 0),
    (Owner::U6NarrowSubstitution, 2, 2, 0),
    (Owner::U6NarrowInvalidation, 2, 1, 1),
    (Owner::SharedTypeResolution, 9, 4, 3),
    (Owner::SharedCompilePipeline, 7, 1, 6),
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
            } else if *parked > 0 && !owner.is_scheduled_block() {
                "   ← NO U-BLOCK ASSIGNED"
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

    /// Rows owned by NO scheduled `U*` block, called out by name.
    ///
    /// These have nobody assigned to fix them and will otherwise be the last
    /// thing blocking the merge.
    #[test]
    fn unassigned_parked_rows_are_named() {
        let mut unassigned: Vec<&str> = CORPUS
            .iter()
            .filter(|r| {
                !r.owner.is_scheduled_block() && matches!(r.verdict, Verdict::KnownOwed { .. })
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
    // The generator-return shape: the macro lane's rejection is correct, the
    // TSX lane fault is not.
    "X19_generator_yield",
];
