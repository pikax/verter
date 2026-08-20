#!/usr/bin/env node
// One-shot migration script for RULING 2 (rev11/rulings import). Not a permanent repo tool —
// safe to delete after the migration it performs is verified. Copies each scratchpad ruling
// verbatim into docs/arch/refactor/rev11/rulings/, replaces the machine-specific absolute path
// prefix with <MACHINE_ROOT>, and prepends a typed YAML frontmatter header. Body bytes are never
// hand-edited — only the mechanical prefix substitution and the prepended header touch the file.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const SRC_DIR =
  "<SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/rulings";
const DEST_DIR = join("<MACHINE_ROOT>/verter-wt-rulings", "docs/arch/refactor/rev11/rulings");
const MACHINE_ROOT_PATTERN = "<MACHINE_ROOT>/Documents/dev";
const MACHINE_ROOT_TOKEN = "<MACHINE_ROOT>";

// Manifest: one entry per migrated document. `supersedes`/`supersededBy` are per-claim, not
// per-document, per RULING 2's explicit instruction (the flow-file reconciliation is the worked
// example: it supersedes exactly two of D1 Fork 2's claims and retains the rest).
const MANIFEST = [
  {
    file: "ARCH-RULING-C1-FOUR-FORKS.md",
    id: "C1-FOUR-FORKS",
    type: "architecture-ruling",
    date: "2026-08-20",
    dateSource:
      "file-mtime (no in-document date; codex transcript, session 01a01cbf-a9a5-73b2-aced-080ee25e38c3)",
    binds: ["C1"],
    summary:
      "Codex architecture-falsification review of C1-CHARTER-DRAFT.md's four proposed positions (crate placement, WorkspaceRead dependency, non-blocking I/O guarantee, NeedInputs coverage scope); all four VIOLATE stated invariants and are replaced: extract into existing verter_semantic (not a new crate); move the six WorkspaceRead-taking entry points upward behind an owned RouteAnalysisInputs snapshot; extraction and I/O confinement are one coupled decision requiring a capability-limited observation interface; full-coverage-required for every non-flow ModuleResolverCore/TypeInfoCore operation reachable from a C2 projection attempt.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Raw Codex CLI transcript (~14.5k lines); substantive content is two bookend spans (prompt ~13-68, verdicts ~14363-14472) around large repeated dumps of CLAUDE.md and skill docs read as context. The closing verdict block appears twice back-to-back (14367-14416 and 14423-14472), byte-identical — a transcript-capture duplication artifact, not two rulings. Flags the draft as factually wrong that both modules are private (resolver_core is pub mod; only resolver_store is private).",
  },
  {
    file: "ARCH-ADDENDUM-C1-THREE-GAPS.md",
    id: "C1-THREE-GAPS-ADDENDUM",
    type: "architecture-ruling",
    date: "2026-08-20",
    dateSource:
      "file-mtime (in-document: 'Source: bounded architecture challenge... run against program/architecture-lock at 8c2189389', no calendar date stated)",
    binds: ["C1"],
    summary:
      "Addendum resolving three execution gaps in the C1 charter left open by C1-FOUR-FORKS: (1) cross-crate trait sealing is impossible as drafted — seal semantic-owned snapshots, no foreign implementations; (2) the proposed file/directory relocation does not close the dependency cut set as-is — enumerates the full move/stay/split disposition per module; (3) the exhaustive-impl 'AttemptOutcome full coverage' proof is invalid under stable Rust — replaced with one closed, non-overridable inherent gateway (TypeInfoCore::attempt over a closed NonFlowOperation enum).",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Explicitly states baseline preserved: the C1 charter and all four ARCH-RULING-C1-FOUR-FORKS rulings 'remain binding and were not reopened' — this is additive gap-filling, not a supersession. States no accepted ADR, DAG edge, or program outcome changes under any of the three gap resolutions.",
  },
  {
    file: "ARCH-RULING-D1-SIX-FORKS.md",
    id: "D1-SIX-FORKS",
    type: "architecture-ruling",
    date: "2026-08-20",
    dateSource: "file-mtime (no in-document date)",
    binds: ["D1"],
    summary:
      "Codex xhigh consult rules on all six open forks in the D1 (private sole-flow-solver foundation) charter draft: capability-matrix ratification timing gates D2 not D1; C1/D1 file-relocation boundary (Fork 2, see supersedes below); D1's obligation ledger must extend the existing ObligationRuntime, never a peer ledger; A6 effective-flow row enumerability; binding-slot identity mechanism; AMD-001's four artifacts verified delivered, discharging the PRIVATE_CHECKPOINT prerequisite. No fork requires a program/ADR/DAG amendment (Fork 4 calls for a capability-matrix revision, explicitly 'program AMD: no').",
    supersedes: [],
    supersededBy: [
      {
        ruling: "C1-D1-FLOW-FILE-RECONCILIATION",
        claim:
          "Fork 2's disposition 'C1 moves flow_return.rs, flow_return_callee.rs, and dispatch_txn.rs; NOT flow_slice_content.rs' — the 'NOT flow_slice_content.rs' claim is superseded (flow_slice_content.rs MOVES whole to verter_semantic), and the whole-file MOVE claims for flow_return.rs and dispatch_txn.rs are superseded (both instead SPLIT). Fork 2's whole-file MOVE for flow_return_callee.rs is RETAINED, as is Fork 3's 'extend the same ObligationRuntime; no peer ledger' ruling.",
      },
    ],
    contradicts: [],
    notes:
      "This document's own text frames Fork 2 purely as a correction of the unratified D1-CHARTER-DRAFT.md, and never itself asserts disagreement with C1.md or ARCH-RULING-C1-FOUR-FORKS.md — the conflict with C1's actual convergence map was caught externally, by a human, and is what ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md cites as the C1/D1 contradiction benchmark case. 12,360-line raw transcript; closing verdict block duplicated verbatim (~12299-12328 and ~12331-12360). Notes a sandbox limitation: a local test invocation was blocked by mkdtemp denial, so Fork 6 relies on landed source/commit evidence rather than a fresh green run.",
  },
  {
    file: "ARCH-RULING-C1-D1-FLOW-FILE-RECONCILIATION.md",
    id: "C1-D1-FLOW-FILE-RECONCILIATION",
    type: "architecture-ruling",
    date: "2026-08-20",
    dateSource:
      "file-mtime (no in-document date; tip 9275f0e40 on program/architecture-lock, codex session 01a0207a-d3b3-7771-823a-4d5e405af7fd)",
    binds: ["C1", "D1"],
    summary:
      "Reconciles the direct conflict between C1's revised charter and D1 Fork 2 over flow_return.rs / flow_return_callee.rs / dispatch_txn.rs / flow_slice_content.rs disposition: flow_slice_content.rs and flow_return_callee.rs MOVE whole to verter_semantic; flow_return.rs and dispatch_txn.rs SPLIT (semantic evaluator/value/transaction-state portions move, live capture/cache/admission/audit/flight-publication portions stay in verter_session). Finds no cross-block ObligationRuntime ownership problem. Changes no accepted ADR, DAG edge, or program outcome.",
    supersedes: [
      {
        ruling: "D1-SIX-FORKS",
        claim:
          "Fork 2's 'NOT flow_slice_content.rs' claim, and its whole-file 'MOVES flow_return.rs / dispatch_txn.rs' claims (both files instead SPLIT).",
      },
    ],
    supersededBy: [],
    contradicts: [],
    notes:
      "This is the worked example RULING 2 (the migration directive) names explicitly for per-claim (not per-document) supersession. Final per-file disposition table: flow_return.rs SPLITS; flow_return_callee.rs MOVES whole; dispatch_txn.rs SPLITS; flow_slice_content.rs MOVES whole. 20,276-line raw transcript; the disposition table + supersession sentence appear twice back-to-back (~20247-20253 and ~20270-20276), byte-identical.",
  },
  {
    file: "ARCH-RULING-C2-FIVE-FORKS.md",
    id: "C2-FIVE-FORKS",
    type: "architecture-ruling",
    date: "2026-08-20",
    dateSource: "file-mtime (no in-document date)",
    binds: ["C2"],
    summary:
      "Codex rules on all five open forks in the C2 charter draft: (A) delete legacy bridge types VueMacroSemanticInput/MacroRuntimeBundle/MacroTscBundle, retain only refactored C3 payload primitives; (B) add DAG edge B6->C2 — requires a formal DAG amendment; (C) own CompileTypeInfo in verter_compiler, composing (not implementing) C1's sealed semantic ObservationSnapshot; (D) five first-party construction modes grouped into three orchestration classes, resolving an apparent ADR-003/compile-transaction.md inconsistency the draft flagged (finds no real contradiction); (E) private nominal tokens + type-state with one mint path for anti-replay, not a cross-crate sealed trait.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Fork B is the ONLY fork of the five tagged 'DAG edge changed — formal amendment required' (the other four are explicitly 'DAG unchanged'); the required amendment (B6->C2) is not itself ratified by this document. Builds on (does not supersede) ARCH-RULING-C1-FOUR-FORKS.md and ARCH-ADDENDUM-C1-THREE-GAPS.md for CompileTypeInfo crate allocation and the composition-not-implementation rule. 16,107-line transcript, mostly reference-material dumps before the final verdict section.",
  },
  {
    file: "ARCH-RULING-CM1-FINDINGS-BC.md",
    id: "CM1-FINDINGS-BC",
    type: "architecture-ruling",
    date: "2026-08-20",
    dateSource:
      "in-document, inferred (responds to a directive stamped 'Status: RATIFIED, 2026-08-20'; the ruling's own issuance date is not separately stated)",
    binds: ["CM1", "BV1", "BS1", "C1", "BV2", "B5", "C2", "C3"],
    summary:
      "Creates block CM1 (Path A, {BV1, BS1} -> CM1 -> C1, sibling of the existing {BV1, BS1} -> BV2 -> B5 edge) to own structural repair of two beta.4-blocking component-meta regressions: Finding B (defineExpose entries never offered to macro-type expansion) and Finding C (runtime prop constructors are display-only, not semantic). Rules the benchmark's UnraisableSource error is a third, distinct, still-open defect — NOT the same defect as the reproduced Unknown(MissingOutput) silent degrade, contradicting the originating directive's hypothesis. Assigns a TSC/declaration-output fidelity gap and a runtime-framework-surface semantic-API gap to BV2, not CM1 or C3.",
    supersedes: [],
    supersededBy: [],
    contradicts: [
      {
        ruling: "BETA4-REGRESSION-INTAKE",
        claim:
          "The directive's hypothesis that Findings B/C and the benchmark's UnraisableSource symptom are the same request-view defect (framed as a Path A vs Path B decision). This ruling finds neither reproduced defect matches that error path and rules UnraisableSource a separate, still-unresolved defect.",
      },
    ],
    notes:
      "Despite the 'FINDINGS-BC' filename, this is one unified ruling placing both findings under one new block. 28,444-line transcript (largest in the corpus); the '## Decision' verdict section appears twice back-to-back (~28307-28374 and ~28377-28444), byte-identical.",
  },
  {
    file: "ARCH-RULING-BV2-FINDING-A-REPAIR-AND-PLACEMENT.md",
    id: "BV2-FINDING-A-REPAIR-AND-PLACEMENT",
    type: "architecture-ruling",
    date: "2026-08-20",
    dateSource: "file-mtime (no in-document date)",
    binds: ["BV2", "BV1", "BS1", "B5", "B6"],
    summary:
      "Rules on the release-blocking VDOM template-codegen panic (overwrite_segmented precondition violated, types.rs:712): repair by giving leave_template's root-prefix owner sole structural ownership of comment removal within its claimed header range (deferring/absorbing visit_comment's independent overwrite, no CodeTransform reordering/narrowing/widening); assigns this repair plus a newly-discovered sibling SSR comment-only collision to a new block BV2, inserted into the DAG as {BV1, BS1} -> BV2 -> B5 (the prior direct BV1/BS1 -> B5 edge is replaced). No accepted ADR or final program outcome changes.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "BV2 is a provisional/newly-minted block id ratified by this document, not a pre-existing charter (no BV2.md charter file existed prior to this ruling). 7,622-line transcript; the closing verdict appears duplicated once at the very end (transcript-rendering artifact).",
  },
  {
    file: "ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md",
    id: "CONCURRENCY-OPERATING-MODEL",
    type: "architecture-ruling",
    date: "2026-08-20",
    dateSource:
      "file-mtime (in-document: 'run against program/architecture-lock at 5b899200b', no calendar date stated)",
    binds: ["program-wide (ledger/concurrency operating model, not a single block)"],
    summary:
      "Two bounded read-only consults rule the operating model: allow up to five disjoint blocks in IMPLEMENTATION and targeted testing, but SERIALISE final certification (one full gate + one impact-bounded mandate re-attestation per landing) — because the gate cascade under concurrent certification is quadratic (N(N+1)/2) while implementation/review iteration dominates wall-clock. Recommends separating IN_PROGRESS into implementation vs certification states.",
    supersedes: [],
    supersededBy: [
      {
        ruling: "CONCURRENCY-CEILING-AND-ROSTER",
        claim:
          "This ruling's own implicit ~2-block concurrent-certification cap discussion is superseded by the maintainer's explicit ceiling of 5 concurrent claude-max blocks/trains (with grok-implementer trains beyond 5); this ruling's underlying quadratic-gate-cost analysis and 'serialise certification' recommendation are not themselves contradicted, only the numeric ceiling is superseded by direct maintainer ratification.",
      },
    ],
    contradicts: [],
    notes:
      "Lists prerequisites still outstanding at time of writing: a stack-window validator + composite cross-validation (AMD-001, tools did not yet exist), review-verdict-to-candidate binding (fixed in flight), and landing_equivalence_digest strengthening. Notes the practical bottleneck found independently: most of the next nine blocks lack charters, which parallelises with zero merge risk.",
  },
  {
    file: "ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md",
    id: "ORCHESTRATION-AUTHORITY-MODEL",
    type: "architecture-ruling",
    date: "2026-08-20",
    dateSource:
      "file-mtime (no in-document date; responds to MAINTAINER-DIRECTIVE-HARDEN-ORCHESTRATION dated 2026-08-20)",
    binds: [
      "program-wide (authority/ratification model, rulings custody, effective-state generator, mutation testing, block-checkpoint model)",
    ],
    summary:
      "The ruling this migration task itself executes RULING 2 of. Five rulings: (1) replace prose Status: lines with digest-bound ratified block-authorization records in a repository authority registry; (2) move every binding ruling into rev11/rulings/, bind via typed effect records, treat external rulings as nonbinding; (3) generate one fail-closed effective-state model with effect-level supersession and a named contradiction taxonomy (SINGLETON/POLARITY/HANDOFF/CARDINALITY/DAG_EFFECT/SUPERSESSION/SCOPE_OVERLAP/OBLIGATION/ORACLE/REFERENCE conflicts); (4) make every validator/oracle check a registered variant with a co-located mutation, uncovered checks fail the suite; (5) represent large-block checkpoints as digest-bound same-block private layers gating internal progress only, never program acceptance. Sequencing: mutation registry first, then authority/ruling migration, then effective-state generation, then checkpoint enforcement.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "This document's own text (§2, 'Ruling custody and binding') envisions curated decision documents in rulings/ with raw session transcripts preserved separately under evidence/rulings/ — this migration pass instead migrates the raw transcripts verbatim into rulings/ per the dispatching orchestrator's explicit brief, deferring the curated-document / evidence-transcript split to a later workstream (see migration report). 11,603-line transcript.",
  },
  {
    file: "B2-scope-and-concurrency-ruling-codex-1.md",
    id: "B2-B3-SCOPE-AND-CONCURRENCY",
    type: "architecture-ruling",
    date: "2026-08-16",
    dateSource: "file-mtime (no in-document date)",
    binds: ["B2", "B3", "B4"],
    summary:
      "Codex ruling: B2 and B3 SERIALIZE (B2 first, B3 rebases onto B2's accepted tree) — the offered range-level-disjointness proof fails because carrier_compiler.rs is jointly owned via one trait declaration. B2 scope directives: joint official-case rows need per-facet (not aggregate) disposition; CarrierCompiler::parse becomes fallible; no new SyntaxCompatibilityId (reuse B1's CompatibilityDomainId/CompatibilityEpoch pair); B3 owns option normalization, B2 owns canonical equivalence; the five version counters are disposable-cache invalidators, not compatibility domains (four of five deleted outright); encounter_order is the primary strict-diagnostic ordering key; mandatory parse-diagnostic spans; Svelte diagnostics routed into the canonical host channel; B2 must land structural type-state carrier-geometry confinement (current RegisteredProjectorSeal/witness validation is insufficient).",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Only the joint-case aggregate-exit amendment (B2.md E1) requires maintainer ratification; the remaining rulings follow existing authority. Superseded in part by MAINTAINER-RULING-AMD-009's Ruling 1 (B2.md:15-17 exit E1 correction is explicitly ratified there, adopting this ruling's directive).",
  },
  {
    file: "B3-scope-ruling-codex-1.md",
    id: "B3-SCOPE",
    type: "architecture-ruling",
    date: "2026-08-16",
    dateSource: "file-mtime (no in-document date)",
    binds: ["B3"],
    summary:
      "Primary ruling RESCOPE_REQUIRED: B3 must atomically convert every presently-reachable production route (internal one-shot, host per-file/virtual, host/NAPI compile_many, NAPI compile_with_audit, WASM compile/audit/virtual, existing project-aware staged compilation, bundler/unplugin) at its request-construction point — no route stays on its current option type until K2. Requires maintainer ratification to amend B3.md's predecessor/scope, product-inventory.md, and program.md's K2 scope. Additional rulings: 'exhaustive capability reachability' means constructor reachability + exact typed refusal, not emitted-product correctness; inline+SSR rejects at construction, inline+Vapor constructs but refuses pre-codegen; framework_extras moves to an ephemeral execution-input carrier excluded from request identity; CompileTargetTag is a public audit schema outside the Typeinfo protobuf contract; Svelte output liveness DEFERs to BS1 as debt row FC-SVELTE-001.",
    supersedes: [],
    supersededBy: [
      {
        ruling: "AMD-009",
        claim:
          "This ruling's RESCOPE_REQUIRED amendment demand (transferring atomic all-route migration from K2/later owners to B3) is the substance MAINTAINER-RULING-AMD-009 Ruling 1 ratifies at charters/B3.md:16-18 and AMD-005:129-130 (K2 retains only final typed-carrier representation and Any+Send+Sync removal, not the initial conversion).",
      },
    ],
    contradicts: [],
    notes:
      "Companion document to B2-scope-and-concurrency-ruling-codex-1.md and parallelism-ruling-codex.md, all from the same 2026-08-16 codex session batch.",
  },
  {
    file: "parallelism-ruling-codex.md",
    id: "BF3-PARALLELISM",
    type: "architecture-ruling",
    date: "2026-08-16",
    dateSource: "file-mtime (no in-document date)",
    binds: ["BF3", "J1"],
    summary:
      "Rules nothing may run and land concurrently with BF3 — only non-landing, read-only preparatory work (investigation, inventories, draft charters/evidence) is legal. J1 fails the safety bar against BF3 (same Svelte compiler pipeline: CSS plan / CodeTransform mappings / StyleSyntaxIr are shared, not disjoint). Corrects the ledger: BF3 is recorded READY, not IN_PROGRESS, with no context-packet digest bound — governance requires an accepted context packet before execution starts.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Companion document to B2-scope-and-concurrency-ruling-codex-1.md and B3-scope-ruling-codex-1.md.",
  },
  {
    file: "MAINTAINER-DIRECTIVE-HARDEN-ORCHESTRATION.md",
    id: "HARDEN-ORCHESTRATION",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["program-wide orchestration machinery"],
    summary:
      "RATIFIED, gates further block progress beyond BS1. Directs four workstreams before advancing: machine-enforced block authorization/ratification; one generated effective-state view from DAG+amendments/rulings+ledger with contradictions failing loudly; mutation tests for validator/oracle failure modes; internal review checkpoints for large blocks with final acceptance staying atomic. Tabulates seven same-day defects the orchestration machinery falsely certified as evidence for the diagnosis.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "This is the directive ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md answers, and RULING 2 of that answer is what this migration task itself executes.",
  },
  {
    file: "MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md",
    id: "CSS-CLEAN-CUTOVER",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["Track J (J1-J4)", "CSS/style pipeline architecture"],
    summary:
      "Updates the Track J plan: remove lightningcss completely, verter_css_syntax becomes the sole CSS-family syntax authority for CSS/SCSS/Sass/Less/Stylus, no legacy parser/printer path retained. Specifies Verter's CSS responsibility boundary (owns parsing/facts/Vue semantics/CodeTransform/source-maps; does not own SCSS-family lowering, normalization, minification, autoprefixing, arbitrary PostCSS/JS callbacks), the required style pipeline stages, generic multi-source-map composition (not a CSS-specific merge_source_maps), the preprocessor contract, and J1-J4 planning implications with required acceptance evidence.",
    supersedes: [
      {
        ruling: "NO-LIGHTNINGCSS",
        claim:
          "That document's final state ('ALL CSS WORK STOPS UNTIL THE J TRAIN' — the J1 charter draft PARKED, not ratified, not landed, and not to be advanced; no further CSS consults, drafts or amendments) is superseded: this directive updates the Track J plan now, on the finding that CSS costs zero schedule today so this is the cheapest moment to do the planning. The underlying architectural decision (lightningcss removal, verter_css_syntax as sole authority) is NOT superseded — it is carried forward and detailed further.",
      },
    ],
    supersededBy: [],
    contradicts: [],
    notes:
      "States explicitly it 'SUPERSEDES the CSS re-entry proposal's \"keep the suspension\" recommendation' and that the scope of this directive is to update the PLAN, not itself dispatch J1 implementation (charters ratified per-artifact under the batched-ratification protocol).",
  },
  {
    file: "MAINTAINER-RULING-NO-LIGHTNINGCSS.md",
    id: "NO-LIGHTNINGCSS",
    type: "maintainer-directive",
    date: "2026-08-17",
    dateSource: "stated",
    binds: ["Track J / J1", "BCSS0 (superseded within this document)", "CSS/style pipeline"],
    summary:
      "Binding, project-wide: lightningcss is not Verter's CSS engine and is to be removed; verter_css_syntax is the single CSS authority; a capability gap is a build instruction, not a reason to keep lightningcss. Recorded before its own supporting architecture consult returned, so it forecloses that consult's 'lightningcss must stay' arm. The remainder of the document is a same-day ratchet of sequencing decisions (recorded chronologically within this one file, each dated 2026-08-17): BCSS0 held pending scope ruling -> released to proceed on lightningcss as originally scoped (removal deferred to a later train, debt row CSS-ENGINE-001) -> corrected to name J1 (an existing in-program, dependency-eligible block) as the owner rather than a new out-of-program train -> BCSS0 attempts to re-point its source-map correction at the canonical style_planner route -> found infeasible (byte-identity conflict with BCSS0's own invariant) -> BCSS0's entire product (engine swap + standalone CSS source-map correction) transfers to J1, BCSS0 SUPERSEDED -> final directive: ALL CSS WORK STOPS UNTIL THE J TRAIN, the J1 charter draft PARKED (unratified), BCSS0 removed from B2/B3 predecessor lists entirely.",
    supersedes: [],
    supersededBy: [
      {
        ruling: "CSS-CLEAN-CUTOVER",
        claim:
          "The final in-document directive's 'ALL CSS WORK STOPS UNTIL THE J TRAIN / J1 charter draft PARKED, not to be advanced / no further CSS consults, drafts or amendments' state — superseded by the 2026-08-20 directive which resumes Track J planning immediately. The core architectural decision (lightningcss removal; verter_css_syntax sole authority) is retained and carried forward, not superseded.",
      },
    ],
    contradicts: [],
    notes:
      "Internally self-superseding chronology: read as a ratchet where each later dated entry in the same file supersedes the immediately preceding sequencing decision in that file, while the top-level architectural decision (lightningcss removal) never wavers. The document also records debt row CSS-ENGINE-001 (later folded into J1's CSS-AUTH-001) and an undispositioned style-path wrong-output violation (style_planner.rs:745,942; compile/mod.rs:608) deferred to J1 as CSS-REFUSE-001. States BCSS0 is formally SUPERSEDED and its branch block/bcss0 (tip 74a5a0291, 8 commits, nothing landed) retained only as reference — this requires a program-dag.toml amendment (BCSS0 removed from B2/B3 predecessor lists) that this document states is being authored for ratification, not itself landed by this document.",
  },
  {
    file: "MAINTAINER-DIRECTIVE-BETA4-REGRESSION-INTAKE.md",
    id: "BETA4-REGRESSION-INTAKE",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["program-wide (release boundary)", "BV2", "CM1"],
    summary:
      "RATIFIED program-level regression intake after an independent benchmark run (pikax/vue-benchmarks) found beta.4-vs-beta.3 regressions on Windows/Node/rustc release builds. Verifies and classifies Findings A (panic! escalation in template/code_gen/types.rs), B (UnraisableSource in meta_resolve/output.rs), and C (runtime prop constructor lowering) as correctness discoveries and beta.4 release blockers. Dispatches two bounded read-only root-cause investigations (Finding A; Findings B+C) under strict standing constraints (no panic swallowing, no invented types, no benchmark/fixture special-casing) — governance intake (DAG edges, charters) follows root cause rather than preceding it.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "This directive's Finding A investigation is what ARCH-RULING-BV2-FINDING-A-REPAIR-AND-PLACEMENT.md answers (creating block BV2); its Findings B/C investigation is what ARCH-RULING-CM1-FINDINGS-BC.md answers (creating block CM1, and finding the benchmark's UnraisableSource hypothesis in this directive does NOT match either reproduced defect — see CM1-FINDINGS-BC's CONTRADICTIONS field).",
  },
  {
    file: "MAINTAINER-INTENT-PARSER-CRATE.md",
    id: "PARSER-CRATE-OWNERSHIP-INTENT",
    type: "maintainer-directive",
    date: "2026-08-18",
    dateSource: "stated",
    binds: ["verter_parser crate ownership (cross-cutting, not a single block)"],
    summary:
      "verter_parser is Verter's parsing crate, not Vue's — Vue and Svelte SFC are both first-party carriers and both belong there; any future first-party SFC carrier lands there too. Settles the question of purpose only; does not settle module boundaries, internal foldering, program placement, or sequencing against B2/B3 — those are delegated to an open-ended codex architect consult. Notes the current split (svelte_reactivity.rs in verter_parser, Svelte tokenizer/template parsing in verter_compiler) is inconsistent, not merely untidy.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "States explicitly: if the delegated consult contradicts this intent, that is a genuine conflict between a maintainer design decision and an architecture finding, to be surfaced to the maintainer rather than letting either side silently win. No such consult document is present in this migrated corpus.",
  },
  {
    file: "MAINTAINER-RULING-AMD-005-BV1-BS1-AUTHORISED.md",
    id: "AMD-005-BV1-BS1-AUTHORISED",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["BV1", "BS1", "AMD-005"],
    summary:
      "Narrow ruling given after an orchestrator escalation: BV1 and BS1 are authorised; AMD-005 itself is NOT ratified wholesale (its DAG amendments, oracle/exclusion rules, capability and performance locks §4-§9 stay PROPOSED, no execution authority). BV1's landing stands (ACCEPTANCE_RECOMMENDED -> ACCEPTED); BS1 returns from LOCKED to dispatchable. Neither block's authority derives from AMD-005 any longer; their ledger rows cite this ruling directly with enabling_amendment = \"\".",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Records the orchestrator's dispatch error (BV1 was executed before its authority was established) rather than erasing it. Notes a durable fix in flight: enabling_amendment becomes a structured ledger field validated against the amendment's real Status: line.",
  },
  {
    file: "MAINTAINER-RULING-AMD-009.md",
    id: "AMD-009",
    type: "maintainer-directive",
    date: "2026-08-16",
    dateSource: "stated",
    binds: ["B2", "B3", "AMD-005", "AMD-010", "JS-1"],
    summary:
      "Four rulings recorded in one session. Ruling 1: AMD-009 ratified AS DRAFTED in the NARROW form (3 documents / 4 deltas) — B3.md predecessor correction, B3 bounded option-admission/route-conversion ownership, AMD-005:129-130 superseded so K2 owns only final typed-carrier representation, B2.md E1 corrected to require no blocked parse facet (not no blocked aggregate row). Ruling 2: B3's conversion boundary stops at the outermost Rust ingress, does not extend into unplugin/wasm JS/TS surfaces; every residual JS/TS silent-ignore needs a named later owner. Ruling 3: JS-1 resolved as ADOPT-NOW bug fix (Verter is strict by default). Ruling 4 (SUPERSEDES Ruling 3's disposition): JS-1 reassigned OUT OF PROGRAM SCOPE, maintainer-owned post-program.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Internal supersession: 'Ruling 4 — JS-1 scope ... SUPERSEDES Ruling 3's disposition' is explicit in the source document's own heading — Ruling 3's substance (strict-by-default, the index signature is a defect) stands unchanged, only JS-1's disposition changes from ADOPT-NOW to maintainer-owned post-program work. Also records a standing ruling (no separate amendment needed): B2/B3 serialize, per B2-scope-and-concurrency-ruling-codex-1.md's finding that carrier_compiler.rs is a third jointly-owned file. Sequencing note: AMD-009 must not be committed until BF3 lands (would destroy BF3's linear fast-forward landing) — see MAINTAINER-RULING-BF3-SECTION7.md for the later full-§7 ratification and AMD-010 renumbering of this document's Ruling 1/2 amendment.",
  },
  {
    file: "MAINTAINER-RULING-BF3-SECTION7.md",
    id: "BF3-SECTION7-RATIFICATION",
    type: "maintainer-directive",
    date: "2026-08-16",
    dateSource: "stated",
    binds: ["BF3", "AMD-009", "AMD-010", "BA0", "BS0", "BCSS0", "BRT0"],
    summary:
      "Cures a recording defect: the only recorded maintainer act (a narrow product ruling) had been over-read to license full AMD-009 §7 structural effect (four new blocks BA0/BS0/BCSS0/BRT0, a program-dag.toml amendment, five charter rewrites, a ledger write) that it did not actually authorize. Ruling 1 confirms the intended ratification WAS the full §7 — the structural reshape stands, cured by direct maintainer act rather than unwound — but requires fixing five verified in-delta test defects, re-reviewing charters changed after the previously-bound package identity, and rebinding before BF3 can be acceptance-recommended. Ruling 2: the audit amendment keeps identifier AMD-009; the separately-ratified B3/B2 option-conversion amendment is renumbered AMD-010 (substance unchanged, no re-ratification needed). Includes a verbatim maintainer ratification act for AMD-009 §7 in full.",
    supersedes: [
      {
        document:
          "evidence/BF3/maintainer-product-ruling-no-error-on-bad-output.md (in-tree, not part of this migration)",
        claim:
          "The over-claimed reading that this narrow product ruling alone licensed full §7 structural effect. The product ruling's own actual text remains valid for exactly what it says.",
      },
    ],
    supersededBy: [],
    contradicts: [],
    notes:
      "Explicitly withholds: BF3 is NOT accepted, B2/B3 are NOT unlocked, no production error-on-bad-output path is authorized, and BA0/BS0/BCSS0/BRT0 are created but not accepted. This is the document renumbering part of AMD-009 (this file's own MAINTAINER-RULING-AMD-009.md sibling) into AMD-010 — see that document's notes.",
  },
  {
    file: "MAINTAINER-ACT-AT2.md",
    id: "AT2-NAMED-ACT",
    type: "maintainer-directive",
    date: "2026-08-17",
    dateSource: "stated",
    binds: ["BF3", "BA0", "AT-2 finding row"],
    summary:
      "Explicit maintainer act (requested after a review seat correctly refused to infer authority from an unnamed general ruling): rejects AT-2's claim that a reachable batch entry publishes a product beside a genuine typed refusal; reclassifies AT-2 as a latent HostBacked construction hazard with reachability unproven; retains the DEFER to BA0; carries it as an #[ignore]d characterization test; drops the required-RED Svelte-refusal atomicity target. Authorizes exactly the bytes already in the tree (evidence/BF3/dispositions.md AT-2 row, charters/BA0.md lines 28 and 37); no production guard, typed refusal, withhold path, retraction, or removal ID.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Does NOT accept BF3, does NOT accept BA0, does NOT unlock B2/B3. Clarified by MAINTAINER-ACT-AT2-CLARIFICATION.md (same date) on two scope points the seat also declined to infer.",
  },
  {
    file: "MAINTAINER-ACT-AT2-CLARIFICATION.md",
    id: "AT2-ACT-CLARIFICATION",
    type: "maintainer-directive",
    date: "2026-08-17",
    dateSource: "stated",
    binds: ["BF3", "BA0", "AT-2 finding row"],
    summary:
      "Supplements MAINTAINER-ACT-AT2.md, issued after a review seat again correctly declined to infer coverage. Clarification 1: the original act covers all three hunks in BA0.md's required-RED Svelte-refusal obligation (findings-table row, Required procedure paragraph, Required exits paragraph), not just the two originally named locators. Clarification 2: reclassifying AT-2 removes it from BF3's exhaustion-exit obligation entirely (exhaustion demands evidence only for genuine failures; AT-2 is no longer classified as one) — the residual hazard is carried by the #[ignore]d test and BA0's retained ownership, and any future demonstrated reachability is a NEW finding with its own RED target.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Explicitly a scope clarification, not new authorization: 'Authorizes no byte beyond what is already landed; this act describes coverage, it does not request an edit.' Does not accept BF3/BA0, does not unlock B2/B3, authorizes no production guard/refusal/withhold/retraction/removal ID.",
  },
  {
    file: "MAINTAINER-RULING-BUGS-AND-TYPES.md",
    id: "BUGS-AND-TYPES",
    type: "maintainer-directive",
    date: "2026-08-17",
    dateSource: "stated",
    binds: [
      "program-wide (every remaining block, not only BF3)",
      "AT-2 (applied here as the prompting case)",
    ],
    summary:
      "General standing rule, given in response to an AT-2 disposition question but binding project-wide: no error path for a type problem (Verter compiles/builds and returns; only a genuine compilation error produces an error); a test-discovered issue is a bug fixed in owning production code, never wrapped in a guard/tracker/refusal/allowlist; types are WAIVED from that fix-now rule for the program's duration (maintainer fixes types personally post-program); interim handling is every bug captured as an added #[ignore]d test with the fix deferred to a named owner.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Applies its own rule to the prompting AT-2 case in the same document, arriving at the same disposition MAINTAINER-ACT-AT2.md separately records as a direct act. This is the general standing rule referenced by name ('the standing bugs-and-types rule' / 'the maintainer's standing bug handling rule') throughout later documents in this corpus, e.g. MAINTAINER-RULING-NO-LIGHTNINGCSS.md's CSS-REFUSE-001 debt row.",
  },
  {
    file: "MAINTAINER-RULING-CODEX-NEVER-ORCHESTRATES.md",
    id: "CODEX-NEVER-ORCHESTRATES",
    type: "maintainer-directive",
    date: "2026-08-18",
    dateSource: "stated",
    binds: ["program-wide dispatch discipline"],
    summary:
      "Codex is never an orchestrator — advisory only (review seat, architecture consult/ruling, scoping/premise verification), no write capability, never dispatches other agents, sequencing advice from codex remains advice the orchestrator decides on. Addendum, same day: codex is also not an implementer — supersedes an earlier 2026-08-05 reversal that had made codex the default implementer/fix agent; all implementation dispatches to claude-max.",
    supersedes: [
      {
        document: "un-migrated 2026-08-05 note (not part of this corpus)",
        claim: "That codex is the default implementer/fix agent.",
      },
    ],
    supersededBy: [
      {
        ruling: "DISPATCH-ROSTER",
        claim:
          "This document's 'implementers are claude-max' framing is refined (not reversed) — claude-max becomes the dispatch vehicle for implementers too, and a claude-max orchestrator may use Agent subagents for its own fan-out.",
      },
    ],
    contradicts: [],
    notes:
      "Unchanged per this document: codex remains default for architecture decisions, premise falsification, and review seats (codex + grok, never a Claude subagent as a review seat).",
  },
  {
    file: "MAINTAINER-RULING-COMMENT-CLEANUP-PASS.md",
    id: "COMMENT-CLEANUP-PASS",
    type: "maintainer-directive",
    date: "2026-08-19",
    dateSource: "stated",
    binds: ["program-wide, per-block landing process"],
    summary:
      "Every block gets a grok comment-cleanup pass, run after review mandates pass and before the squash, scoped to that block's diff only: remove AI watermark phrasing, restatement of the obvious, over-long explanations, and plan/phase archaeology; keep non-obvious invariant comments, safety/ordering/fail-closed rationale, rustdoc/JSDoc on public APIs, and any comment a test/guard asserts on. Comments only, zero code/behaviour change; re-run targeted tests + cargo fmt afterwards; the cleanup seat must not be the block's own reviewer.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes: "",
  },
  {
    file: "MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md",
    id: "CONCURRENCY-CEILING-AND-ROSTER",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["program-wide concurrency policy"],
    summary:
      "Concurrency authorised in principle, conditional on a mechanically testable property (git merge-tree --write-tree rehearsal, no merge-conflict risk). Ceiling: up to 5 concurrent blocks/trains on claude-max, superseding an earlier 2-block cap. Beyond 5, additional claude-max orchestrators may dispatch, but with grok as the implementer instead of claude-max. Notes the ratified validator still fails closed at one IN_PROGRESS block and needs a reviewed change plus the fence/rehearsal/review-binding machinery before the ceiling is actually usable; flags one live defect (unbound review verdicts) as already a false-green in serial mode, worse under concurrency; and poses an open design question on the restack cascade cost at N>2, with a candidate byte-identity-equivalence answer under evaluation.",
    supersedes: [
      {
        document: "the orchestrator's own P2 proposal (not part of this corpus)",
        claim: "A 2-block concurrent-train cap.",
      },
    ],
    supersededBy: [],
    contradicts: [],
    notes:
      "This ruling's numeric ceiling supersedes the earlier ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md's implicit low certification-concurrency recommendation (see that document's superseded_by field) — the underlying quadratic-gate-cost analysis in that document is not contradicted, only the resulting numeric policy is superseded by direct maintainer ratification.",
  },
  {
    file: "MAINTAINER-RULING-DISPATCH-ROSTER.md",
    id: "DISPATCH-ROSTER",
    type: "maintainer-directive",
    date: "2026-08-18",
    dateSource: "stated",
    binds: ["program-wide dispatch discipline"],
    summary:
      "claude-max is the dispatch vehicle for implementers too, not only managers/orchestrators — supersedes an older 'use claude, not claude-max' note. A claude-max orchestrator may use Agent subagents for its own implementation fan-out. Unchanged: review seats stay external CLIs (codex/grok, never a Claude subagent); long-running workers launch in the foreground of a run_in_background:true Bash call, never a trailing &/nohup/setsid; a -p process is one-shot and the only sanctioned wait is a blocking foreground loop.",
    supersedes: [
      {
        document:
          "an older, un-migrated 'use claude, not claude-max' dispatch note (not part of this corpus)",
        claim: "That implementers should be dispatched via claude, not claude-max.",
      },
    ],
    supersededBy: [],
    contradicts: [],
    notes: "",
  },
  {
    file: "MAINTAINER-RULING-GATE-SCOPE.md",
    id: "GATE-SCOPE",
    type: "maintainer-directive",
    date: "2026-08-17",
    dateSource: "stated",
    binds: ["program-wide gate discipline"],
    summary:
      "The full gate runs once, at landing readiness, immediately before squash/fast-forward — never mid-work, never after every fix round. A test-only change or a leaf-with-no-consumers production change does not warrant a full gate (targeted tests only); a production change with real reach does. The leaf claim must be verified by an actual call-site search, never merely asserted.",
    supersedes: [],
    supersededBy: [
      {
        ruling: "GATE-UNRESTRICTED",
        claim:
          "Not superseded in substance — that later ruling explicitly states it 'SHARPENS the existing one-gate-at-a-time discipline... rather than contradicting it' and restates that the landing gate is never skipped for a change with real reach. Listed here for cross-reference only; no claim in this document is actually overridden.",
      },
    ],
    contradicts: [],
    notes: "",
  },
  {
    file: "MAINTAINER-RULING-GATE-UNRESTRICTED.md",
    id: "GATE-UNRESTRICTED",
    type: "maintainer-directive",
    date: "2026-08-17",
    dateSource: "stated",
    binds: ["program-wide gate execution parameters"],
    summary:
      "Ruling 1: re-diagnoses the OOM reboots as caused by CONCURRENT gate runs, not single-gate parallelism — one gate at a time stays the real control (unrelaxed); within that one gate, use full host parallelism, dropping the earlier --build-jobs/--test-threads throttle; keep a hard --memory-limit RSS-kill watchdog (18GiB on a 24GiB host). Ruling 2: every regression found gets a test hardened structurally (privacy/type-state/sealed-trait first, then whole-artifact assertion, then a proven plant-red-green, only then an ordinary assertion) — never a name-keyed source scanner. Ruling 3: adopts remaining audit recommendations (consumer-falsification gate, capped ledger notes, fixing the gate's own false-greens). CORRECTION (same day): the leak hypothesis is falsified by root-cause investigation — the dominant memory consumer is the BUILD (concurrent rustc), not the guards; refines policy to --build-jobs at tool default (do not raise to 8), --test-threads raised to 8, --memory-limit 18GiB kept, one-gate-at-a-time kept.",
    supersedes: [
      {
        document: "an earlier memory-ceiling ruling (not part of this corpus)",
        claim: "The --build-jobs 2 --test-threads 2 throttle.",
      },
    ],
    supersededBy: [],
    contradicts: [],
    notes:
      "Records a residual, explicitly unresolved tension: an earlier program ruling claimed even CARGO_BUILD_JOBS=2 was insufficient because Surface 1 (nextest) drove memory critical, which is in tension with this document's build-dominates finding — the document states both cannot be fully right and defers to the watchdog as an empirical resolver rather than forcing a rule. This is a genuine open self-contradiction in the corpus's ancestry, flagged here per the migration brief rather than resolved.",
  },
  {
    file: "MAINTAINER-RULING-GREEN-BRANCH-AND-TRIAGE.md",
    id: "GREEN-BRANCH-AND-TRIAGE",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["program-wide gate-failure triage discipline"],
    summary:
      "program/architecture-lock is GREEN BY INVARIANT, not by hypothesis — a red working branch is a P0, never re-derived with a second full gate run to check whether it pre-existed on trunk. On a branch gate failure, triage in isolation (re-run the failing tests alone, several times): deterministic failure is REAL, intermittent is FLAKY; report to the maintainer; fix flaky tests ASAP. Abolishes the standing 'known pre-existing baseline / environmental' disposition category entirely — cites a real deterministic production bug (compose_generated_chunk aborting on an empty source map) that hid behind that category across four landing records.",
    supersedes: [
      {
        document:
          "the orchestrator's gate-range-mode line of work (not part of this corpus, described as 'withdrawn')",
        claim:
          "Detecting and running only the affected range of tests between two commits as the canonical gate.",
      },
    ],
    supersededBy: [],
    contradicts: [],
    notes:
      "Explicitly withdraws gate.mjs range mode as a maintainer suggestion (not landed); the affected-tests selector itself stands as an inner-loop tool, not wired into the canonical gate.",
  },
  {
    file: "MAINTAINER-RULING-LANDING-IS-ORCHESTRATOR-ONLY.md",
    id: "LANDING-IS-ORCHESTRATOR-ONLY",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["program-wide landing/orchestration protocol"],
    summary:
      "A block/train orchestrator never writes to program/architecture-lock (no fast-forward, no merge, no commit) — it stops and reports once its own checks pass. The program orchestrator runs its own independent checks against that branch, performs the squash, and lands; landing authority is not delegated. Issues found are fixed directly or by spawning another orchestrator, never self-certified by the original block orchestrator. The gate is one of the program orchestrator's checks and may run in parallel with the others (still one gate at a time on the machine). Includes standing brief text to paste into every dispatched block/train brief.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Cites two same-day incidents as motivation (BV1 fast-forwarded itself into trunk by its own manager) versus BS1's manager doing the right thing unprompted (wrote its proposed transition and stopped) — the latter becomes the rule.",
  },
  {
    file: "MAINTAINER-RULING-NO-BUILD-INVOKING-TESTS.md",
    id: "NO-BUILD-INVOKING-TESTS",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["Rust test suite composition"],
    summary:
      "RATIFIED: a test executes code, it does not spawn a compiler, build a CLI, or build a Rust project — Rust tests must be pure Rust exercising Verter code, JS tests pure JS exercising JS code. Removes trybuild-backed compile-fail fixtures (81 fixtures across 6 crates) that proved structural invariants (E0308 newtype non-interchangeability, E0451 private-field unconstructability, sealing/unreachability) via out-of-process cargo builds. States plainly that this removes the sole regression detector for those 81 invariants and delegates the replacement design (in-crate compile_error!/const-assertions, negative trait bounds, moving compile-fail out of the test surface, or accepting the loss with review-only enforcement) to the architecture seat as an open decision.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Triggered by the canonical gate failing at exactly the 360s budget on two trybuild fixtures that pass 3/3 in isolation (98s cold / 0.8s warm) because one trybuild invocation spawns cargo against verter_session's ~233-crate dependency closure. States the asymmetry that matters: loosening a structural restriction never breaks a normal build, so the compile-fail fixture was the sole regression detector in that direction.",
  },
  {
    file: "MAINTAINER-RULING-AUTO-ACCEPT.md",
    id: "AUTO-ACCEPT",
    type: "maintainer-directive",
    date: "2026-08-19",
    dateSource: "stated",
    binds: ["program-wide acceptance protocol"],
    summary:
      "Delegates the routine acceptance-recording act to the program orchestrator, conditional on ALL of: every required mandate PASS on this exact candidate per DAG class; verdicts issued by seats that actually looked at this candidate (no inherited/implicit-close verdicts); orchestrator-independently-verified identity/digests/commit hygiene/validator green; no maintainer-reserved item entangled (no amendment/rescope/gate recalibration/irreversible contract change riding along). Any BLOCKING/NOT_PROVEN mandate, anything needing ratification, an unevidenced 'pre-existing' failure claim, or known false/unverifiable evidence still comes to the maintainer.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Explicit: delegated is the paperwork act; NOT delegated is judgement about whether the bar was met — if in doubt, the block waits.",
  },
  {
    file: "MAINTAINER-RULING-BS1-COMPLETION-AUTHORITY.md",
    id: "BS1-COMPLETION-AUTHORITY",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["BS1"],
    summary:
      "Issued after an escalation that BS1's charter Required-exits rest on unratified AMD-005. BS1 remains IN_PROGRESS; its seven completed Svelte corrections are authorized/retained but do not alone establish completion. The earlier BV1/BS1 authorisation ruling did not ratify AMD-005's acceptance criteria. Requires a standalone, exact-byte, self-contained, digest-bound BS1 completion packet (executable FC-* definitions, exact BF3-guard removals, a row-by-row evidence matrix). Conformance/architecture verdicts may carry only where byte-equivalence proves unchanged subject matter; a fresh adversarial seat must independently attest the exact final candidate.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Extended (not superseded — 'Supersedes nothing') by MAINTAINER-RULING-BS1-COMPLETION-CORRECTION.md. This document's own 'Orchestrator execution state' section records the §5-first-half discharge (byte-identity across the f46de1b6a rebase) later re-verified against a different base in EVIDENCE-BS1-RESTACK-BYTE-IDENTITY.md.",
  },
  {
    file: "MAINTAINER-RULING-BS1-COMPLETION-CORRECTION.md",
    id: "BS1-COMPLETION-CORRECTION",
    type: "maintainer-directive",
    date: "2026-08-20",
    dateSource: "stated",
    binds: ["BS1"],
    summary:
      "Extends MAINTAINER-RULING-BS1-COMPLETION-AUTHORITY (states explicitly it supersedes nothing). BS1-COMPLETION-PACKET.md accepted as authoritative gap-analysis but NOT ratified as the completion contract as-is. Corrects the charter's BF3-removal premise to zero eligible removals (BS1 instead retires seven BS0-authored #[ignore] guards). FC-HYDRATION-001 and FC-PERF-001 ruled BLOCKED/UNPROVEN, explicitly not N/A. Requires a standalone gate correction (real offline Svelte-oracle prerequisite probe, fail-loud on missing/invalid cache, enable bf2-authoritative in the canonical archive, prove the 45 additional tests execute) landed and reviewed independently before further BS1 completion evidence, followed by a byte-identity-proven rebase, a compound-ownership scoping ruling for SVELTE-MODULE, and an independently-reviewed performance-lock bootstrap with no threshold derived from BS1's own candidate results.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Its required gate correction (§5+§10) is what lands as 9275f0e40 per EVIDENCE-BS1-RESTACK-BYTE-IDENTITY.md, which discharges this ruling's §6 (rebase + byte-identity proof) but explicitly states the adversarial verdict does NOT carry across the fix-round rewrite — a fresh adversarial pass is owed, which ATTESTATION-BS1-ADVERSARIAL-EXACT-CANDIDATE.md then provides.",
  },
  {
    file: "ATTESTATION-BS1-ADVERSARIAL-EXACT-CANDIDATE.md",
    id: "BS1-ADVERSARIAL-EXACT-CANDIDATE-ATTESTATION",
    type: "attestation",
    date: "unknown",
    dateSource:
      "file-mtime 2026-08-20 (no in-document date; binds to a candidate that predates the later restack recorded in EVIDENCE-BS1-RESTACK-BYTE-IDENTITY.md, so this attestation predates that document within the same day)",
    binds: ["BS1"],
    summary:
      "Discharges BS1-COMPLETION-AUTHORITY §5 second half: an unprimed, isolated claude-max subagent independently attests candidate 9786e756b (base f46de1b6a, evidence commit a48d92e82) PASS, via eight genuine plant->prove-RED->revert->prove-GREEN cycles against production code plus blast-radius probes against the real pinned svelte@5.56.8 oracle. Binds ONLY to 9786e756b — the document states explicitly that if BS1's remaining completion-packet work changes the candidate, this attestation does not carry, and the review-identity binding at 71fb82dec mechanically refuses a PASS on a stale reviewed SHA.",
    supersedes: [],
    supersededBy: [
      {
        ruling: "BS1-RESTACK-BYTE-IDENTITY",
        claim:
          "This attestation's binding to candidate 9786e756b does not carry to the restacked candidate 761651109 — that document states so explicitly ('The adversarial verdict does NOT carry by this proof... A fresh adversarial pass is owed at that point, not now'), and this attestation's own text anticipates exactly that condition.",
      },
    ],
    contradicts: [],
    notes:
      "Lists all eight plant/RED/revert/GREEN cycles verbatim with the specific production defect each proved (function-decl name mapping, shorthand attribute binding, destructure conflation, member/each-item binding, non-ASCII char-boundary panic, store-gated EACH_ITEM_IMMUTABLE).",
  },
  {
    file: "EVIDENCE-BS1-RESTACK-BYTE-IDENTITY.md",
    id: "BS1-RESTACK-BYTE-IDENTITY",
    type: "attestation",
    date: "unknown",
    dateSource: "file-mtime 2026-08-20 (no in-document date)",
    binds: ["BS1"],
    summary:
      "Discharges BS1-COMPLETION-CORRECTION item 6: proves the seven-fix Svelte code diff is byte-identical (68,598 bytes both) before and after restacking BS1 from base f46de1b6a onto the landed gate-correction commit 9275f0e40 (restacked candidate 761651109). Confirms the gate correction landed independently reviewed (first review BLOCKING on an unmitigated race, root-caused and fixed, delta review reproduced the race with the fix reverted and confirmed clean with it restored). States conformance/architecture verdicts carry across the restack by this proof; the adversarial verdict does NOT (bound to the pre-restack SHA only) — a fresh adversarial pass is owed.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Lists BS1's remaining outstanding items as of this document: corrected completion contract awaiting maintainer ratification; two stale conformance records needing regeneration from the pinned oracle; 14 UNPROVEN rows; FC-HYDRATION-001/FC-PERF-001 BLOCKED/UNPROVEN.",
  },
  {
    file: "DISPOSITION-B4-C1-SERIALIZE.md",
    id: "B4-C1-SERIALIZE",
    type: "disposition",
    date: "unknown",
    dateSource: "file-mtime 2026-08-19",
    binds: ["B4", "C1"],
    summary:
      "Unprimed codex concurrency consult verdict: B4 and C1 serialize (B4 first, then C1 rebased onto B4's accepted tip) — virtual_file_pipeline.rs is co-owned (C1's macro/type projection at ~:3000, B4's final map/publication assembly at ~:3281, both touch compile_entry ~:3071-3103), and the ratified stack-window policy fixes depth at 1 with sequential landing regardless. C1 is not blocked on B3 semantically; this is a tree-ownership constraint, not a dependency one.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes: "",
  },
  {
    file: "DISPOSITION-BS1-SERIALIZE.md",
    id: "BS1-SERIALIZE-BEHIND-BV1",
    type: "disposition",
    date: "unknown",
    dateSource: "file-mtime 2026-08-20",
    binds: ["BS1", "BV1"],
    summary:
      "Unprimed codex consult verdict: BS1 serializes behind BV1 — notably not for code-overlap reasons (the Vue/Svelte oracle domains and descriptor/registry tables are already disjoint), but because the live validator fails closed at one IN_PROGRESS block, AMD-005 permits BV1/BS1 overlap only with a reviewed relaxation that does not exist, the A6 stack policy stays depth-one/sequential, and the BF2 golden store has one exclusive-writer combined manifest. Flags blockers 1-2 as governance artifacts, not physics, and escalates relaxing them as a maintainer decision.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes: "",
  },
  {
    file: "DISPOSITION-TYPECHECK-POC-TO-H-TRAIN.md",
    id: "TYPECHECK-POC-TO-H-TRAIN",
    type: "disposition",
    date: "2026-08-18",
    dateSource: "stated",
    binds: ["H2", "H3 (future Track H blocks)"],
    summary:
      "Routes a typecheck-performance POC (branch origin/poc/api-tax-combined) to the H train as INPUT/REFERENCE, not an approved design — the H train owns the actual design decision. Verifies the mechanism (off-overlay host FS callbacks reach the Rust actor over the transport) but flags the claimed numbers as UNKNOWN/unmeasured. Names three things H must NOT copy from the POC: the illegitimate CLI-vs-API transport-split implementation (bypasses the mandatory ExternalTsProjectResolver->CarrierRegistry->EngineBackend->BoundProject path); the overlay-FS-skip micro-optimization (dead in the combined branch, process-global counters don't belong in a permanent API surface); the sibling-declaration mechanism (wrong companion identity, silently skips a real user file, leaks/races/overwrites unconditionally).",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Placement: H2 owns the single-owner backend cutover and benchmark evidence; H3 owns atomic companion publication. Explicitly not B2, not B3, no new block.",
  },
  {
    file: "MAINTAINER-RULING-PARALLEL-REVIEW-SEATS.md",
    id: "PARALLEL-REVIEW-SEATS",
    type: "maintainer-directive",
    date: "2026-08-18",
    dateSource: "stated",
    binds: ["program-wide review-seat protocol"],
    summary:
      "Review mandates run concurrently, not sequentially. Read-only seats (conformance, architecture) run in parallel on the shared candidate worktree (codex exec --sandbox read-only mutates nothing); the adversarial seat gets its own worktree cut from the exact candidate commit, since it plants and reverts mutations. Fix cycles fan out the same way: dispatch all seats for a round at once, fix once against the union of findings. Same-day amendment: the adversarial mandate is reassigned from an external CLI to a Claude Agent subagent in its own worktree, because a read-only codex seat structurally cannot perform the required plant/RED/revert/GREEN cycle.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Unchanged throughout: codex+grok only for conformance/architecture seats, never a Claude subagent there; prompts neutral/unprimed; grok keeps default-to-BLOCK; round cap 3; no seat grades its own work.",
  },
  {
    file: "MAINTAINER-RULING-REVIEW-BUDGET.md",
    id: "REVIEW-BUDGET-BY-ARTIFACT-CLASS",
    type: "maintainer-directive",
    date: "2026-08-17",
    dateSource: "stated",
    binds: ["program-wide review protocol"],
    summary:
      "Review rigour is reallocated by artifact class, backed by a process audit (8,581 review-report lines / 239 findings across four doc-only campaigns produced zero production defects, while review mandates pointed at running code produced all 16 found production defects). Production code (crates/, packages/, scripts/) keeps unchanged full rigour. Evidence/landing records/context packets get NO review rounds — authored once accurately, checked by orchestrator fact-verification not reviewer prose preference. Charters/amendments/specs get one cheap grok soundness pass (model 4.6, Extra High), escalating to codex only on a real flagged contradiction. Grok is explicitly encouraged liberally up front for pre-implementation scoping and premise verification.",
    supersedes: [],
    supersededBy: [],
    contradicts: [],
    notes:
      "Explicit: correctness standards on code (TDD, no stubs, proven-applied mutation plants, honest UNPROVEN) are untouched — the objection is to review volume on prose, not rigour on code.",
  },
];

if (MANIFEST.length !== 42) {
  throw new Error(`manifest has ${MANIFEST.length} entries, expected 42`);
}

function yamlString(s) {
  return JSON.stringify(s);
}
function yamlStringArray(arr) {
  return `[${arr.map(yamlString).join(", ")}]`;
}
function yamlClaimArray(arr) {
  if (arr.length === 0) return " []";
  return (
    "\n" +
    arr
      .map((c) => {
        const key = c.ruling !== undefined ? "ruling" : "document";
        return `  - ${key}: ${yamlString(c.ruling ?? c.document)}\n    claim: ${yamlString(c.claim)}`;
      })
      .join("\n")
  );
}

mkdirSync(DEST_DIR, { recursive: true });

let totalReplacements = 0;
const perFileReplacements = [];

for (const entry of MANIFEST) {
  const srcPath = join(SRC_DIR, entry.file);
  let body = readFileSync(srcPath, "utf8");

  // Mechanical machine-root substitution only — no other content rewriting.
  const matches = body.split(MACHINE_ROOT_PATTERN).length - 1;
  if (matches > 0) {
    body = body.split(MACHINE_ROOT_PATTERN).join(MACHINE_ROOT_TOKEN);
    totalReplacements += matches;
    perFileReplacements.push([entry.file, matches]);
  }

  const frontmatter = [
    "---",
    `ruling_id: ${yamlString(entry.id)}`,
    `type: ${yamlString(entry.type)}`,
    `date: ${yamlString(entry.date)}`,
    `date_source: ${yamlString(entry.dateSource)}`,
    `binds: ${yamlStringArray(entry.binds)}`,
    `source_file: ${yamlString(entry.file)}`,
    `summary: ${yamlString(entry.summary)}`,
    `supersedes:${yamlClaimArray(entry.supersedes)}`,
    `superseded_by:${yamlClaimArray(entry.supersededBy)}`,
    `contradicts:${yamlClaimArray(entry.contradicts)}`,
    `notes: ${yamlString(entry.notes)}`,
    "---",
    "",
    "",
  ].join("\n");

  writeFileSync(join(DEST_DIR, entry.file), frontmatter + body);
}

console.log(`Migrated ${MANIFEST.length} rulings to ${DEST_DIR}`);
console.log(`Total <MACHINE_ROOT> substitutions: ${totalReplacements}`);
for (const [file, n] of perFileReplacements) {
  console.log(`  ${file}: ${n}`);
}

// --- INDEX.md ---

const byType = new Map();
for (const e of MANIFEST) {
  if (!byType.has(e.type)) byType.set(e.type, []);
  byType.get(e.type).push(e);
}
const typeOrder = ["maintainer-directive", "architecture-ruling", "attestation", "disposition"];
const typeLabel = {
  "maintainer-directive": "Maintainer directives",
  "architecture-ruling": "Architecture rulings",
  attestation: "Attestations",
  disposition: "Dispositions",
};

function indexRow(e) {
  const supersededBy = e.supersededBy.length
    ? e.supersededBy.map((c) => c.ruling ?? c.document).join(", ")
    : "—";
  const binds = e.binds.join(", ");
  return `| [\`${e.id}\`](${e.file}) | ${e.type} | ${e.date} | ${binds} | ${supersededBy} |`;
}

let index = "";
index += "# Rulings index\n\n";
index += "One row per ruling document migrated from the session scratchpad under RULING 2 of\n";
index +=
  "[`ORCHESTRATION-AUTHORITY-MODEL`](ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md). Each document carries a\n";
index +=
  "typed YAML frontmatter header (`ruling_id`, `type`, `date`, `date_source`, `binds`, `source_file`,\n";
index +=
  "`summary`, `supersedes`, `superseded_by`, `contradicts`, `notes`) prepended to the verbatim original\n";
index +=
  "text — body content was not rewritten, only the frontmatter and a mechanical `<MACHINE_ROOT>` path\n";
index +=
  "substitution were applied. `supersedes`/`superseded_by` are per-CLAIM, not per-document: a ruling can\n";
index +=
  "supersede one claim of another while the rest of that document remains binding — see each document's\n";
index += "own frontmatter for the exact claim text.\n\n";
index +=
  "**Not yet built by this migration:** the effective-state generator and authority registry described in\n";
index +=
  "RULING 1/3 of `ORCHESTRATION-AUTHORITY-MODEL` — this index is hand-curated, not a generated fail-closed\n";
index +=
  "model. Do not treat `superseded_by = —` as proof a ruling is uncontested; it means no OTHER migrated\n";
index +=
  "ruling's own text names it as superseded. Ledger `digest` binding is a separate step owned by the\n";
index += "program orchestrator (RULING 1), not performed here.\n\n";

for (const type of typeOrder) {
  const rows = byType.get(type) ?? [];
  if (rows.length === 0) continue;
  index += `## ${typeLabel[type]} (${rows.length})\n\n`;
  index += "| ID | Type | Date | Binds | Superseded by |\n";
  index += "|---|---|---|---|---|\n";
  for (const e of rows) index += indexRow(e) + "\n";
  index += "\n";
}

writeFileSync(join(DEST_DIR, "INDEX.md"), index);
console.log(`Wrote INDEX.md (${MANIFEST.length} rows across ${byType.size} types)`);
