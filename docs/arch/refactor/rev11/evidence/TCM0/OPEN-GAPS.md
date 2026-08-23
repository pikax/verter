# TCM0 — Open gaps tracked explicitly (2026-08-23 integration pass)

This file exists because a three-way independent review (conformance/architecture/adversarial, all
`gpt-5.6-sol` at `xhigh`, run against commit `84a5018fe`) of the 2026-08-23 TCM-plan integration returned
FAIL on all three legs, and several of the findings are genuine, verified gaps in TCM0's own evidence that
predate this integration and are not this integration block's work to close. Per this program's own rule
("do not leave a gap unassigned, and do not use 'blocked' as a disposition"), each is named here with an
explicit owner and gate rather than left ambiguous. Gaps this integration COULD and DID fix are fixed in
place (`cache-lifecycle-contracts.md`'s ABI/derived-key correction, `feature-ownership-ledger.md`'s
taxonomy/#5a/#2-vs-#6 clarifications) and are not repeated here.

**Re-validation command, recorded verbatim (2026-08-23).** Prior commit messages narrated
`node scripts/validate-program-state.mjs --mode live` as the re-validation command. That bare
form is not runnable: it exits 2 with `--dag, --state, and --mode are all required` (confirmed
fresh against this file's own tree). The command that actually runs and was actually used to
produce the "3 pre-existing, unrelated violations" claim is:

```
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml \
  --mode live \
  --authority docs/arch/architecture-lock/ledger/authority-registry.toml
```

which exits `1` and prints exactly:

```
VIOLATION: state block BV2 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block B5 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block CM1 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
FAIL: 3 violation(s) in docs/arch/architecture-lock/ledger/program-state.toml against docs/arch/refactor/rev11/program-dag.toml (mode live)
```

confirming the narrated claim (same 3 pre-existing, unrelated BV2/B5/CM1 violations) — but the
narration must cite the runnable form, not the bare one, going forward.

**Owner.** `block/ledger-subordinate-to-code` — an active, in-flight block already working these exact
three rows (its own history includes "test(gate): exercise BV2's context-packet grandfather" and prior
BV2/B5/CM1 ratification commits). Not this integration block, and not a new block invented here: TCM0's
own evidence only needs to disclose the pre-existing violations accurately, not resolve a grandfather
exemption for rows this integration did not create.

**Gate.** `block/ledger-subordinate-to-code`'s own landing. Until it lands, `validate-program-state.mjs
--mode live` continues to report exactly these 3 violations for any tree that includes this integration's
changes — that is expected, not a regression this integration introduced or must fix.

## G-LEDGER-SCOPE — feature-ownership ledger covers 44 trait methods, not the steering's full capability list

**Finding.** The steering's charter item 3 lists capabilities beyond the `TypeProvider` trait's 44
methods — auto-imports, implementation, rename preparation, formatting, call hierarchy, code lens,
folding, selection ranges, document symbols, component surface resolution, template expression typing,
background semantic analysis (`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §"Complete
feature-ownership ledger"). `feature-ownership-ledger.md` explicitly scopes itself to the trait's 44
methods (its own header: "The trait defines 44 distinct methods... This ledger covers all 44"). Whether
each named steering capability already IS one of the 44 (e.g. "auto-imports" may already be served by
`get_completions`/`resolve_completion`; "formatting" may not exist as a distinct capability in this
codebase at all) has not been verified row-by-row.

**Owner.** TCM0's own remaining closure work — NOT this integration block (a contract/charter integration
pass cannot itself perform a fresh capability inventory without re-doing TCM0's investigation), and NOT
TCM1-TCM4 (their charters correctly consume the ratified ledger as-is; re-deriving ownership rows is
explicitly out of their owned scope, per each charter's "Owned-scope boundary" section).

**Gate.** TCM0's own acceptance. TCM0's charter (`charters/TCM0.md`) already states the acceptance bar:
"TCM0 cannot be accepted with... an unclassified `TypeProvider` method." If the steering-named
capabilities beyond the 44 are genuinely distinct (not already covered under an existing row's current
description), TCM0 needs a follow-up evidence pass adding rows for them before TCM0 leaves LOCKED — a
gap in TCM0's OWN acceptance readiness, not a defect this integration introduced or can close by editing
prose.

## G-TOPOLOGY — no comparative topology benchmark numbers exist yet

**Finding.** `topology-benchmark-plan.md` states plainly it is "plan, not results" — no comparative
numbers across the projection-plane or semantic-plane topology candidates have been produced.
`performance-baselines.md` similarly locks only a small set of hard requirements and one single-topology
reference point (34ms/1037ms/0ms), explicitly excluding comparative numbers "since they do not exist yet."

**Owner.** TCM0 (the plan's own author) — the plan names WHO runs these benchmarks (its own harness
description), not this integration. The rewritten TCM2/TCM3 charters correctly cite the single-topology
reference point as an interim floor (Material Bounds sections) and do not claim it is a full comparative
lock — this is stated explicitly in each charter's Material Bounds intro (corrected 2026-08-23, see
below).

**Gate.** TCM0's own acceptance requires the topology selection ("Select the non-dominated topology based
on evidence, not implementation convenience" — steering charter item 7). Until real numbers exist, TCM0
cannot claim a topology was SELECTED on evidence, only that a harness and candidate list exist.

## G-PERF-NUMBERS — the full locked equivalent-work baseline table is not populated

**Finding.** Beyond the hard requirements and single reference point in `performance-baselines.md`, the
steering's full metric list (§"Performance and memory acceptance" — edit-to-hover, edit-to-completion,
edit-to-definition, build, incremental build, watch, declaration emit, etc.) has no locked numeric
threshold. TCM3/TCM4's rewritten charters cite this file as authority for their Material Bounds sections;
that citation is accurate for what IS locked (the hard requirements) but should not be read as claiming
the full numeric table exists.

**Owner.** TCM0, per the same charter item 10 ordering rule ("Do not choose acceptance thresholds after
viewing implementation results" — thresholds must be locked BEFORE TCM1-TCM4 produce results to compare
against, which is exactly why this is TCM0's work, not TCM2/TCM3/TCM4's own).

**Gate.** TCM0's own acceptance, same as G-TOPOLOGY.

## G-CONFORMANCE-FIXTURES-TCM2 — projection-plane conformance fixtures not yet produced

**Finding.** `evidence/TCM0/` contains one test specification (the acyclic-invariant deadlock/reentrancy
test) against the steering's much larger "Required conformance coverage" list. The projection-plane slice
of that list — mapper purity, single-input projection, signed-offset bounds, and the Vue/Svelte/script-mode
fixture matrix as it applies to the content-mapper surface — is TCM2's own implementation work, not a
TCM0 pre-deliverable: TCM2's charter cites "Required conformance coverage" by reference in its own
Numbered Exit Criteria section.

**Owner.** TCM2 — the block that implements the projection-plane surface these fixtures exercise.

**Gate.** TCM2's own Numbered Exit Criteria (`charters/TCM2.md`), already wired to cite this coverage
list. No additional gate needed.

## G-CONFORMANCE-FIXTURES-TCM3 — semantic-plane conformance fixtures not yet produced

**Finding.** Same steering coverage list as G-CONFORMANCE-FIXTURES-TCM2; the semantic-plane slice —
snapshot correctness, cancellation, configured/inferred project and trust-state coverage as it applies to
semantic-capability dispatch — is TCM3's own implementation work. TCM3's charter cites "Required
conformance coverage" by reference in its own Numbered Exit Criteria section.

**Owner.** TCM3 — the block that implements the semantic-plane surface these fixtures exercise.

**Gate.** TCM3's own Numbered Exit Criteria (`charters/TCM3.md`), already wired to cite this coverage
list. No additional gate needed.

## G-CONFORMANCE-FIXTURES-TCM4 — activation/deletion conformance fixtures not yet produced

**Finding.** Same steering coverage list as G-CONFORMANCE-FIXTURES-TCM2/TCM3; the activation/deletion
slice — attestation, trust, JSONC safety, missing/malformed/duplicate mappers, multi-installation
monorepos, project references — is TCM4's own implementation work. TCM4's charter cites "Required
conformance coverage" by reference in its own Numbered Exit Criteria section.

**Owner.** TCM4 — the block that implements the activation/deletion surface these fixtures exercise.

**Gate.** TCM4's own Numbered Exit Criteria (`charters/TCM4.md`), already wired to cite this coverage
list. No additional gate needed.

## G-TCM0-ACCEPTANCE-ROWS-25-26 — the rows #25-26 ruling gates TCM0's own acceptance

**Finding.** `feature-ownership-ledger.md` rows #25-26 remain `CANDIDATE — governance ruling required`.
TCM0's charter forbids accepting the block with "an intentional capability removal lacking explicit
governance approval," and rows #25-26 remain exactly that until the ruling lands.

**Owner.** TCM0 itself. Naming TCM3 as preparer here (an earlier draft of this row did) is wrong: TCM3's
own `predecessors` in `program-dag.toml` are `["TCM0", "TCM1"]` — TCM3 cannot be dispatched, let alone
prepare anything, until TCM0 is ACCEPTED, so gating TCM0's acceptance on TCM3 work is an unsatisfiable
cycle. The decision packet this ruling needs is already fully assembled by TCM0's OWN evidence —
`feature-ownership-ledger.md`'s "Correction, 2026-08-23" section names the two legal outcomes and the
reasoning record — so no further block-level execution work is needed before the maintainer can rule; the
ruling can be requested directly off TCM0's own evidence, with no dependency on TCM3 starting first.

**Gate.** The maintainer ruling (approve deletion of `register_carrier_member(s)`/
`activate_carrier_member(s)` per the content mapper's own wire-identity fields, or retain them under
`VerterWithTypeSemanticOracle`). One gate; TCM0 leaves LOCKED only once it is decided. This is the SAME
ruling `charters/TCM3.md`'s exit criterion 5 (TCM3-EC-G1) requires — closing it here, before TCM3 is even
dispatched, means TCM3-EC-G1 is satisfied by citing the already-recorded ruling rather than re-requesting
it.

## G-TCM4-DELETION-ROWS-25-26 — the same ruling gates TCM4's deletion of the rows #25-26 code

**Finding.** `deletion-closure.md` names rows #25-26's code as a deletion candidate gated behind
TCM3-EC-G1. TCM4 may not delete `register_carrier_member(s)`/`activate_carrier_member(s)` until that
ruling resolves in favor of deletion — this is a distinct downstream consequence of the same ruling from
G-TCM0-ACCEPTANCE-ROWS-25-26 (TCM0's acceptance) and tracked as its own row so each has exactly one gate.
Unlike G-TCM0-ACCEPTANCE-ROWS-25-26, naming TCM3 here is NOT circular: `program-dag.toml` already puts
TCM3 strictly between TCM0's acceptance and TCM4's dispatch (`TCM3.predecessors = ["TCM0","TCM1"]`,
`TCM4.predecessors = ["TCM0","TCM1","TCM2","TCM3"]`), so by the time TCM3 exists to act, the ruling this
row needs has already been recorded via G-TCM0-ACCEPTANCE-ROWS-25-26's own gate closing first.

**Owner.** TCM3 — its own named exit criterion (TCM3-EC-G1, `charters/TCM3.md` exit criterion 5) is
satisfied by citing the ruling G-TCM0-ACCEPTANCE-ROWS-25-26 already obtained, not by re-deriving it.

**Gate.** The maintainer ruling (same ruling as above, cited by ID in `authority-registry.toml` per
TCM3-EC-G1's own evidence requirement). If it resolves against deletion, this row closes as "retained"
rather than "deleted" — either outcome is a closed row.

## G-STRING-SURFACE-CITATIONS — PARTIALLY RESOLVED 2026-08-23, count intentionally NOT closed (was: mapping-products-string-surface.md names two structs that do not exist)

**Original finding.** Round-2 review (against `da31a892d`) found that `mapping-products-string-surface.md`'s
central-finding struct list (naming "at least nine distinct struct definitions in `verter_compiler`")
actually lists eleven field citations, and two of the named structs — `SvelteClientOutput` and
`SvelteIdeProjector` — do not exist anywhere in the repository. `CssProcessResult` likewise has no
`struct CssProcessResult` definition in the tree.

**First correction attempt, and why it also failed.** A mechanical re-derivation (`grep -rn "pub
source_map" --include="*.rs" crates/verter_compiler/src/`, each hit resolved to its containing struct)
found 24 fields and was committed as a claimed CLOSED, trustworthy count. **A follow-up architecture
review proved that claim wrong too**: one struct attribution was still incorrect
(`vue_module.rs:383`'s field belongs to `ComposedFragments`, not `MainFragmentTag`), and the grep pattern
structurally cannot see enum variant fields, differently-named fields (`template_map_json`,
`source_projection_map`, `runtime_source_map`), or non-`pub` fields — 8 more confirmed by direct source
read. This is the exact failure mode round-2's own report had already warned about ("risks introducing a
second error under review pressure").

**Current state.** The three nonexistent struct names ARE corrected to their real ones
(`SvelteIdeProjector`→`SvelteIdeProjection`, `SvelteClientOutput`→`ClientModule`,
`CssProcessResult`→`ProcessStyleResult<'a>`, plus `GeneratedChunk`→`GeneratedChunkOutput`), and the wrong
`vue_module.rs:383` attribution is fixed (`ComposedFragments`, not `MainFragmentTag`). The count itself is
NOT claimed closed: `mapping-products-string-surface.md` now states "at least 36" total fields found
carrying this convention (32+ in `verter_compiler`, 4 in `verter_protocol`, of which 1 —
`FfiBlockOverrideEntry.source_map` — is caller-supplied inbound data with no `CodeTransform` producer
relationship and is out of TCM1's migration scope) and explicitly disclaims exhaustiveness — a manual/grep
method has now undercounted this surface twice, which is itself evidence a third manual attempt is not
the right tool. `TCM1.md` is updated to match: it cites "at least 35 in scope, not closed" rather than a
specific total, and its exit criterion 1 is reworded to a SCOPED, COMPILER-ENFORCED completeness proof
(delete `CodeTransform`'s
string-returning producer methods; every caller that expected a string fails to compile until migrated) —
not a name/text scanner, which this program's own structural-enforcement rule forbids landing.

**Owner.** TCM0's own remaining closure work, SPECIFICALLY the still-open sub-question: is a truly
exhaustive STARTING count required before TCM1 may be dispatched, or is TCM1's own exit-criterion-1
deletion-based discovery (delete the old producer, let the compiler find every break) sufficient without
one? This candidate does not decide that question — it is recorded here as a genuine open item, not
silently resolved either way. A one-time type-aware tool pass (e.g. a `syn`-based scan) may still help
PLAN the migration, but is not itself the landed completeness proof and would not be kept as an ongoing
guard.

**Scope is wider than `verter_compiler`/`verter_protocol` alone.** A round-4 adversarial review found the
same string-encoded map-data convention on carriers this file's inventory does not cover at all,
INCLUDING, FOR EXAMPLE (this is a sample, not a closed sub-list — `CachedVirtualFile.source_map` at
`verter_session/src/types.rs:2894` is another instance the same review pass did not name):
`crates/verter_session/src/types.rs` (`VirtualFileResponse.source_map: Option<Arc<str>>` at line 2042,
`CachedTsx.source_map: Option<Arc<str>>` at line 2903, `IdeResponse.source_map: Option<Arc<str>>` at
line 2914) and `crates/verter_lsp/src/protocol_types.rs` (two DTO fields near line 330/339). This is
consistent with, not a contradiction of, this row's own "not claimed exhaustive" position — it is further
evidence a manual pass cannot close this count, not a new blocker, since neither `mapping-products-
string-surface.md` nor `TCM1.md` claims coverage limited to two crates was ever complete.

**Gate.** Not closed. Struct-name and line-attribution accuracy for the `verter_compiler`/
`verter_protocol` subset are fixed; the count is honestly open-ended — now confirmed to extend into
`verter_session` and `verter_lsp` as well — pending either (a) a structural tool-based PLANNING pass
across every crate touching generated-code source maps (a migration aid, not a landed guard), or (b) an
explicit TCM0 acceptance ruling that TCM1's own migration-time deletion-based discovery (its exit
criterion 1: delete `CodeTransform`'s string-returning producer methods, let the compiler find every
break) is sufficient without a pre-closed inventory.

## G-SEMANTIC-API-CERTIFICATION — TCM0's charter item 2 requires bulk symbol/type/reference/completion/diagnostic probes; only an inventory exists

**Finding.** `charters/TCM0.md` item 2 literally requires: "Probe session initialisation, snapshot
acquisition/update/disposal, project and source-file lookup, `Program` and `TypeChecker` operations, bulk
symbol/type/reference queries, completions, diagnostics, cancellation, and failure behaviour." Against
this list, `package-lock-and-semantic-api.md` executes real, live probes for session initialisation
(§4a), disposal (§4b), the stale-`Program`-after-dispose defect (§4c), and cancellation absence (§4e,
verified by exhaustive grep of the type definitions, a legitimate proof method for an ABSENCE claim).
`§4.0`'s "full session-API method table," by contrast, is explicitly an INVENTORY — "Read in full" from
`APIMethodInfo`'s type declaration, not an executed probe — for bulk symbol/type/reference queries,
completions, diagnostics, and general failure behaviour beyond the two named defect classes. No probe
script, transcript, or measurement for those method classes is cited anywhere in this file (the four
named probe scripts at `Reproduction` §288-290 cover only init-timing and stale-snapshot). This is
distinct from, and larger than, the two already-disclosed gaps (`API.fromLSPConnection` attach-hang,
exact wire method-name spelling) — those two are named and delegated explicitly in the file's own text;
this one (bulk-method live correctness) is not named as a gap anywhere in TCM0's evidence, which is why
round-2 review flagged it as an undisclosed shortfall rather than a disclosed, gated delegation.

**Reconciliation with the production-certification ruling — recorded here, NOT by editing the ruling.**
`MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md` is a maintainer-RATIFIED, digest-bound document;
this integration pass does not edit it (an earlier draft of this candidate did add a clarifying paragraph
to it and re-pinned its digest — reverted, since rebinding an active ratified ruling's digest without a
fresh ratification act is itself a governance violation, flagged by round-3 architecture review). The
ruling's own text, unedited, certifies the CANDIDATE PACKAGE — identity and version selection — for
production activation; nothing in it claims TCM0's charter item 2 probe requirement is satisfied, and its
own "What this does NOT reopen or waive" section already names two gaps it knows about. This entry names
a THIRD, broader gap (bulk-method live correctness) the ruling text does not mention either way. Reading
"certified for production activation" as "TCM0's Semantic API certification requirement is satisfied"
would be a misreading of the ruling — package-identity certification and charter-item-2 probe closure are
two different questions — but that reconciliation lives here, in TCM0's own evidence tracking, not as an
addition to the ratified ruling itself.

**Owner.** TCM0's own remaining closure work — same reasoning as `G-LEDGER-SCOPE`/`G-TOPOLOGY` above: a
docs-integration pass cannot itself execute live probes against a real `tsc --lsp`/native-binary session
without redoing TCM0's own investigation.

**Gate.** TCM0's own acceptance — but see the maintainer decision this entry surfaces rather than
resolves: whether TCM0 must run these probes itself before acceptance (the charter's literal assignment),
or whether a ratified amendment moves the bulk-correctness probes to TCM2/TCM3 alongside the two
already-delegated gaps (the same reallocation pattern the ruling already applied to wire-spelling and
attach-hang). This is not TCM0's, TCM1's, or this integration's decision to make unilaterally — recorded
as an open maintainer decision, not silently resolved either way.

## G-PROJECTION-MASK-TOTALITY — the class×relation×region×owner×capability mask policy is not a total function

**Finding.** `projection-class-contract.md`'s terminal policy (its "Terminal policy" section) states the
mask is the AND of five factors but does not supply, for every combination of those five axes, an
explicit computed mask — `AuthoredTransformed`'s own class-baseline mask names some features as always-
included and others as conditionally excluded, leaving several of the 20 `SpanMapFeature` bits genuinely
undecided for that class (round-2 architecture/conformance/adversarial reviews all cite the same lines,
56-63, as the location of the gap). Separately, `feature-ownership-ledger.md`'s reconciliation note
explicitly defers per-row `projection_class` assignment for the `TokenCompletion` grouping to TCM1/TCM2
("a genuine, named TCM1/TCM2 task, not this integration's"). Neither of these is a fabrication risk to
close by writing an exhaustive table now — the five-axis space includes region/owner combinations that do
not exist until TCM1-TCM3 produce real carriers to classify — but the contract as written is not yet the
"implementation-deterministic" total policy TCM2's terminal-mask emission (its owned-scope item 10) needs
to consume without making new, unreviewed policy decisions of its own.

**Owner.** TCM0's own charter item 5 acceptance bar ("Ratify the minimal class set and the terminal
policy") — same reasoning as `G-LEDGER-SCOPE`: this is TCM0's own evidence-completeness work, not
something a docs-integration pass can close without either fabricating axis combinations that do not yet
have real carriers to classify, or silently narrowing reviewer scrutiny by asserting totality that is not
there.

**Gate.** TCM0's own acceptance. TCM0 needs either (a) a genuinely total mask table/closed algorithm
covering every legal class×relation×region×owner×capability combination that can exist given TCM0's own
evidence (the classes, relations, and owner rows are already closed sets — only the per-combination mask
values are incomplete), or (b) an explicit, honest statement that full totality is intentionally deferred
to TCM2's implementation with a named exit-criterion proving TCM2 closes every combination it actually
encounters — not the current state, where the contract reads as terminal but is not.

## G-DELETION-CLOSURE-ITEMS-17-18 — two deletion-closure items cannot be enumerated by name yet

**Finding.** The steering states plainly "Do not defer this inventory to TCM4"
(`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §"Deletion closure"), and `TCM0.md:92` repeats it —
TCM0 must name every mechanism TCM4 deletes, not merely describe a category. `deletion-closure.md` names
17 of the 19 steering items by mechanism, but items 17 (old APIs/DTOs whose only owner was the deleted
route) and 18 (historical content-mapper codecs) have nothing to name today: no TCM1-TCM3 implementation
exists yet to produce the DTOs or codecs in question. An earlier draft of `deletion-closure.md` treated
this as a settled exception (a stated discovery algorithm run at TCM4 execution time), which round-2
review correctly flagged as unassigned — unlike ledger rows #25-26 (which have a named gate, TCM3-EC-G1,
and require a maintainer ruling), items 17-18 had no owner, no maintainer decision, and no resolution
gate, and `TCM4.md`'s own owned-scope item 9 compounded this by simultaneously claiming TCM4 "does not
re-derive the deletion list at execution time" while requiring exactly that re-derivation for these two
items.

**Owner.** TCM0's own remaining closure work — same reasoning as `G-LEDGER-SCOPE`/`G-TOPOLOGY` above: a
docs-integration pass cannot itself produce TCM1-TCM3's DTOs/codecs to name them, and fabricating a fixed
list before those blocks exist would be worse than an honest gap.

**Gate.** TCM0's own acceptance must decide, before TCM0 leaves LOCKED, which of two legal resolutions
applies: (a) items 17-18 stay genuinely unenumerable until TCM1-TCM3 exist, and TCM0's acceptance record
explicitly ratifies a per-type execution-time discovery method as the closure mechanism for exactly these
two items (the same reallocation pattern already used for the wire-spelling and attach-hang gaps); or (b)
TCM0 is held in LOCKED until TCM1-TCM3 produce enough of the DTO/codec surface that items 17-18 can be
enumerated by name before TCM4 dispatch. Until one of these is ratified, `deletion-closure.md` rows 17-18
and `TCM4.md` owned-scope item 9 (plus the required-outcomes bullet and exit criterion 5 that name the
same gap) record the gap as OPEN, not as a settled algorithm-only disposition.

## G-DIAGNOSTIC-CONVERGENCE — compiler-diagnostic LSP vs CLI duplication has no valid owner or resolution gate

**Finding.** `diagnostic-ownership-matrix.md` records the two independent compiler-diagnostic
implementations (LSP live `TypeProvider::get_diagnostics` vs `verter-tsc` CLI `api_check.rs`) as a
required convergence — two engines answering "get TS diagnostics" is the Shared Optimized Codebase
failure mode — and assigned it vaguely to "TCM1/TCM2". That assignment cannot stand: `charters/TCM1.md`
forbids semantic-API clients ("No content-mapper process, no TypeScript JSON-RPC types in compiler
core, no TypeScript semantic-API client — those are TCM2/TCM3"), and `charters/TCM2.md` explicitly
assigns semantic-session ownership to TCM3 ("No `TypeScriptApiSessionState` or `VerterSemanticClientState`
implementation — those are TCM3's local owners"). `deletion-closure.md`'s `verter-tsc` CLI compiler-
diagnostic row remains conditional on that unresolved convergence. TCM0 records the required
convergence; it does not execute it (no production code in this block). The matrix's own "new owner"
column already splits the two paths (`TypeScriptLspDirect` for LSP, `VerterWithTypeSemanticOracle` for
CLI) while its dedup column says they must converge onto one — those two sentences cannot both be a
settled design.

**Owner.** TCM0's own remaining closure work — same reasoning as `G-DELETION-CLOSURE-ITEMS-17-18`: a
docs-integration pass cannot itself pick the later-block owner without asserting a resolution the
charters have not ratified. TCM1 is ruled out by its own owned-scope boundary; TCM2 is ruled out by
its own assignment of semantic-session to TCM3. TCM3 is the candidate implied by those two charter
boundaries, but is NOT named as the owner here: naming it would close the assignment this row exists
to keep open.

**Gate.** TCM0's own acceptance. TCM0's acceptance record must name (a) which later block owns the
convergence work, and (b) what "converge onto one" means (CLI folds into the LSP's `TypeScriptLspDirect`
path, both fold into the oracle-session path, or a third option TCM0's evidence supports). Until that
record exists, `diagnostic-ownership-matrix.md`'s open item and `deletion-closure.md`'s CLI-diagnostic
row stay OPEN, not assigned to TCM1/TCM2.

## G-TEMPLATE-SRC-PROJECT-CONTEXT-CONTRACT — `<template src>`'s content-mapper model lacks its required positive proof

**Finding.** The steering permits an external unit to be content-mapped only under one of four named
models, and only model 2 ("independently content-mapped under a proven project/context contract",
`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §11) fits `<template src>`.
`external-source-decision-table.md` row 7 asserts `<template src>` is content-mapped because "it needs the
same template→TSX transform as an inline template", but that sentence only establishes the transform KIND,
not the required project/context contract. Diagnostic ownership IS already proven
(`diagnostic-ownership-matrix.md`'s external-unit row: an external file's diagnostics attribute to its own
URI, unchanged, correct today). What is missing: a positive proof that (1) the mapper's `transform()` input
for the external file is genuinely that file's own content, distinct from the referencing SFC's; (2) which
TypeScript project owns the external file for content-mapping purposes; (3) which configuration
(`tsconfig`) identity applies to it. `TCM2.md`'s exit criterion 5 only specifies a NEGATIVE test (a
cross-source-unit range is rejected at construction) — that proves foreign-origin spans do not leak into
the wrong output, not that a project/context contract for the legitimate `<template src>` case exists.

**Owner.** TCM2 — the block that implements the projection-plane mapper surface `<template src>` content
runs through; a docs-integration pass cannot itself decide project/config identity for a not-yet-built
mapper without doing TCM2's own design work.

**Gate.** TCM2's own Numbered Exit Criteria (`charters/TCM2.md`). TCM2 may not claim `<template src>`
resolved under model 2 until it adds a positive fixture proving the three missing elements above,
alongside the existing negative rejection test in exit criterion 5.

## Two findings that are NOT gaps — pre-existing, deliberate TCM0 delegations, reconfirmed

Two review findings characterized existing TCM0-evidence text as "wrong layer" (a later block owning work
the steering assigned to TCM0). Both are RECONFIRMED as deliberate, already-recorded TCM0 decisions, not
defects this integration introduced or should silently reverse:

- **TCM2 closing the exact content-mapper wire method-name spelling.** `package-lock-and-semantic-api.md`
  §5 already records this as an explicit open verification gap TCM0 could not close from a stripped
  binary via static `strings` extraction, naming it "recorded here as an open gap for TCM2, not glossed
  over" — TCM0's own text, written before this integration, deliberately delegates this probe to TCM2.
- **TCM3 running the `API.fromLSPConnection` session-attach hang probe.** `tcm1-tcm4-charter-refinements.md`'s
  TCM3 section (also pre-existing TCM0 evidence) explicitly states TCM0 "did NOT probe
  `API.fromLSPConnection`... TCM3's charter should name this as a required probe... not assume it
  inherits TCM0's certification by association" — again a deliberate TCM0 delegation, not an oversight.

Both are correct as rewritten in TCM2.md/TCM3.md's owned scope and exit criteria.
