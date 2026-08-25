# TCM0 — Open gaps tracked explicitly

This file is TCM0's gap register. Each row is either CLOSED, with the evidence that closed it, or
OPEN, with a named owner and a resolution gate. No row is left unassigned, and "blocked" is not a
disposition.

**Status, 2026-08-24 — rewritten from the ratified rulings.** Every disposition below is derived
from `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`. There are
**16 rows**: **6 CLOSED** and **10 OPEN**. There is no PARTIALLY CLOSED tier.

- **CLOSED by the ruling (5):** `G-TOPOLOGY` (Q2), `G-PERF-NUMBERS` (Q3),
  `G-TCM0-ACCEPTANCE-ROWS-25-26` (Q4), `G-TCM4-DELETION-ROWS-25-26` (Q4),
  `G-DIAGNOSTIC-CONVERGENCE` (Q6).
- **CLOSED on external evidence (1):** `G-LEDGER-VALIDATOR-VIOLATIONS` — closed by another block's
  landing, an externally verifiable fact, not by TCM0 self-certification.
- **OPEN, owned by the successor block (6):** `G-LEDGER-SCOPE`, `G-SEMANTIC-API-CERTIFICATION`,
  `G-PROJECTION-MASK-TOTALITY`, `G-STRING-SURFACE-CITATIONS`, `G-DELETION-CLOSURE-ITEMS-17-18`,
  `G-CHARTER-AMENDMENTS` (hybrid — its `TCM0.md` row is DISCHARGED by Q2; its two
  `G-TOPOLOGY`-sourced `TCM2.md`/`TCM3.md` rows are standing pre-dispatch mandates owned by the
  program orchestrator; its three withdrawn-closure rows transfer to the successor).
- **OPEN, owners outside TCM0 (4):** `G-CONFORMANCE-FIXTURES-TCM2`, `G-CONFORMANCE-FIXTURES-TCM3`,
  `G-CONFORMANCE-FIXTURES-TCM4`, `G-TEMPLATE-SRC-PROJECT-CONTEXT-CONTRACT`.

Every one of the sixteen is a section heading in this file, and every section heading is named
above.

**Why five rows this branch had marked CLOSED are now OPEN.** They were closed by TCM0's own closure
pass — a self-certification. Q1 returns the round-3 candidate as wrongly scoped, lands its accurate
work as a **NON-ACCEPTANCE evidence package**, and hands the incomplete contract remainder to a
**successor
block with fresh verification**. A closure that needs fresh verification is not a closure.

Every source-backed finding those five produced is retained in place as evidence; only the
"therefore CLOSED" verdict is withdrawn. `successor-block-scope.md` scopes what the successor must
show.

**On the ledger.** `docs/arch/architecture-lock/ledger/program-state.toml` is the program
orchestrator's to write, and Q8 reverts this branch's edit to it. Nothing in this file corrects that
ledger or should be read as having done so.

The closure pass established several things from the package and the source that the prior evidence
had wrong. They are listed here so a reader does not have to reconstruct them from the rows:

- Four probe scripts cited as reproduction evidence **were never committed anywhere in the
repository**.
  They now exist, are executable, and their transcript is committed (`probes/`).
- **The content-mapper wire protocol was captured live**, closing a gap previously delegated onward
as
  unobtainable: the method names are `initialize` / `openProject` / `transform` / `closeProject`.
- `CodeTransform` is **not** the single point of origin for mapping-product strings. It is one of
eight
  producers, and `TCM1.md`'s completeness proof as written would enumerate seven call sites in one crate.
- The ownership ledger's rows #25-26 rest on a premise that is **factually inverted**. That finding
is what
  Q4 followed: both rows are RETAINED under `VerterWithTypeSemanticOracle`, removable only after TCM3
  supplies and tests equivalent semantics.
- The diagnostic matrix's claim that the `verter-tsc` CLI *"keeps its own oracle-session call"* is
untrue
  of the tree; both paths already share `VerterHost`, the resolver/cache substrate, and the tsgo `--api`
  client crate.
- The candidate package **has no project-wide references primitive**, refuses completion lists
needing
  auto-imports, and delivers diagnostics in a wire shape that is not the classic TypeScript one.

**What this pass itself got wrong, and review caught.** Recorded here rather than quietly amended,
because three of the four were the same defect classes this pass was convened to close:

1. **Five of six probes contained no assertions** and exited 0 regardless of the package's
behaviour, while
   three documents described them as reproducing their findings. Two of probe 5's non-asserting checks
   guarded constraints this evidence declares BINDING on TCM2/TCM3. All are now assertions, and the two
   binding guards are proven to discriminate by planting the reversal — see `probes/README.md`.
2. **Three timing figures traced to no committed run** and contradicted the committed transcript,
and one
   of them had been hardened into a locked acceptance bar. All three are withdrawn; see `G-PERF-NUMBERS`.
3. **The acyclic invariant cited the wrong protocol's method table.** Corrected to a positive fact
in the
   shipped binary.
4. **Two exhaustiveness counts were wrong** (`chain_source_map`'s eighth production caller; "19
custom
   methods" is 18), in a file whose thesis is that prior counting attempts undercounted.

## Re-validation commands, re-run 2026-08-23

The bare form remains non-runnable — it exits 2 with `--dag, --state, and --mode are all required`,
re-confirmed against this tree. The runnable form is:

```
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml \
  --mode live
```

**Correction to the previously recorded result.** This file previously recorded that command (with
an explicit `--authority` flag) exiting `1` with exactly three violations — `BV2`, `B5` and `CM1`
each `ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256`. **Those
three no longer reproduce.** Re-run fresh against this tree, both with and without `--authority`,
the command exits `1` with exactly one violation:

```
VIOLATION: block CM1 (landing_order 3) does not land cleanly onto the cumulative result of every
prior
block in the fixed landing order — replaying its base_sha ... delta via git merge-tree ... reports
real
content conflicts (contracts/stacked-prs.md, MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md)
FAIL: 1 violation(s) ... (mode live)
```

`node scripts/effective-state.mjs` exits `0` (69 blocks, 66 rulings, 0 findings, no contradictions).

The three `context_packet_digest` violations were owned by `block/ledger-subordinate-to-code` and
closed by its landing; that row is therefore **CLOSED**. The one remaining violation is a CM1
landing-order conflict owned elsewhere and is not TCM0's. Any *additional* violation on a tree
containing TCM0's changes is TCM0's to answer for.

---

# CLOSED BY THE RULING

Five rows, each closed by a numbered question of
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`. The ruling decided
them; this register records the decision rather than re-deriving it.

## G-TOPOLOGY — CLOSED by ruling (Q2)

**Disposition.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q2 **RATIFIES the transfer**: TCM0 owns candidate screening, the survivor sets, the metrics, the
harness, the baseline method and the selection rule; **TCM2 owns evidence-based projection-plane
topology selection and TCM3 owns evidence-based semantic-plane topology selection, each as a
blocking exit of its own block.** Two consequences follow directly. Comparative topology numbers are
**not** a TCM0 acceptance precondition. And the transfer is **not** a pending charter amendment
awaiting maintainer ratification — the ruling is the ratification act. Every passage below that
calls the reallocation pending, unratified, a silent scope transfer, or a TCM0-acceptance blocker is
superseded by this disposition; the evidence and reasoning it rests on are retained.

**The question.** No comparative numbers exist across the topology candidates, yet TCM0's acceptance
requires selecting the non-dominated topology on evidence (charter Scope item 7,
`charters/TCM0.md:79`).

**Why the requirement as written cannot be met.** Four of the six candidate topologies do not exist,
and building them is TCM2's and TCM3's owned scope. `program-dag.toml` gives both blocks
`predecessors = ["TCM0", "TCM1"]`, so neither may be dispatched until TCM0 is ACCEPTED. Requiring
their output before TCM0 leaves LOCKED is an unsatisfiable cycle — the same shape already identified
and rejected for the rows #25-26 gate.

**Reallocating the selection is a SCOPE CHANGE, and it is routed as one.** Scope item 7 assigns both
the benchmarking and "Select the non-dominated topology on evidence" to TCM0; "on evidence" governs
the BASIS of the selection, not its owner or timing. The disproved charter assumption above
justifies REQUESTING an amendment — it does not itself AUTHORIZE one: governance §10 requires a
disproved assumption to produce a recommended amendment for maintainer decision, not a silent
reinterpretation. TCM0 accordingly recommended a reallocation rather than reinterpreting the charter
silently: TCM0's Scope item 7 narrows to candidate screening plus locking the survivor sets,
metrics, harness, non-dominance rule and baseline method; comparative selection among the unbuilt
survivors becomes a blocking numbered exit criterion of TCM2 (projection plane) and TCM3 (semantic
plane). **That recommendation was ratified as a ruling rather than as a charter amendment** (Q2), so
the selection obligation sits with TCM2 and TCM3 now, by ruling. Applying the wording to `TCM2.md`
and `TCM3.md` remains a charter-amendment act for the program orchestrator (proposal text:
`tcm1-tcm4-charter-refinements.md`); it is not a precondition of TCM0's acceptance and no longer
blocks anything here.

**What TCM0 does decide from the evidence it holds** (detail: `topology-benchmark-plan.md` →
addendum):

- **Node/N-API is struck from the projection-plane candidate list — a conditional-candidate
disposition,
  not a measured Pareto result.** The steering admits this candidate only conditionally ("Node/N-API
  topology only if it remains competitive after initial evidence",
  `MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md:865`), and nothing requires that initial screen to be a
  locked comparative number. The initial evidence is structural: it is the native path plus a Node runtime
  and an N-API crossing per `transform()`, for no capability the native path lacks. This disposition does
  not claim to discharge Scope item 7's selection clause.
- **The three semantic-plane candidates share one engine.** `package-lock-and-semantic-api.md` §2
  establishes the candidate ships no `tsserver.js`/`typescript.js`/`services.js` — compiler, checker and
  language service are all inside the Go binary. There is no lighter in-process option for any candidate
  to be lighter than; they differ only in process topology and session lifetime.
- **Selection among survivors IS a blocking exit** of the block that first builds enough to measure
—
  TCM2 for the projection plane, TCM3 for the semantic plane — ratified by Q2, governed by `performance-baselines.md`'s requirements 6-8 (a baseline captured before
  that block's own implementation exists, a comparison workload named in advance, every timing claim a
  distribution with raw samples), so the selection is still judged against numbers locked before results
  are visible.

## G-PERF-NUMBERS — CLOSED by ruling (Q3)

**Disposition.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q3: **requirements 6-8 of `performance-baselines.md` ARE the complete TCM0 Scope-10 performance
contract, and no dedicated-machine absolute baseline is required.** Independently owned correctness
and lifecycle gates remain applicable. This row is therefore CLOSED outright; it is no longer
"PARTIALLY CLOSED", and there is no open numeric half. Every passage below saying the numeric half
stays open, that an honest bar needs a quiet or dedicated machine TCM0 lacks, or that the row closes
only "on adequate hardware", is superseded — the contract is complete as it stands. The withdrawal
of the specific absolute figures, and the measurement instability that motivated it, are retained as
findings.

**The question.** The steering's full metric list has no locked numeric threshold, and charter item
10 requires thresholds locked before any implementation result is seen.

**CLOSED half — the threshold FORM, which needs no number.** Every metric on that list
(edit-to-hover, edit-to-completion, edit-to-definition, build, incremental build, watch, declaration
emit) is an *equivalent-work* comparison, and a no-regression-against-a-pre-captured-baseline
threshold is fully specified today and cannot be gamed later. `performance-baselines.md`
requirements 6, 7 and 8 lock it: the implementing block must capture the current path's timing as
its FIRST act, before any implementation of its own exists, commit that capture, name its workload
in advance, and report distributions over N>=10 iterations with raw samples. That satisfies charter
item 10's actual rule.

**OPEN half — every absolute figure is WITHDRAWN.** An earlier revision of this row claimed a
measured warm figure of 2 ms "replaces the defect-derived one" and hardened it into a restatement of
hard requirement 1. All three legs rejected that, and all three were right:

- The figures (32 ms / 1333 ms / 2 ms) came from a run that was **never committed**. The committed
  transcript of the same probe in the same tree recorded 7 ms / 324 ms / 0 ms.
- The warm figure it claimed to improve on was itself 0 ms in that transcript, so the stated
improvement
  did not exist.
- The bar is not reproducible. Ten-iteration characterisation on this host gives
double-digit-or-larger
  fastest-to-slowest spreads for construction, cold and warm within a single run, and the exact multiple
  drifts with every re-run: `probes/transcript.md` has recorded 11x/6x/30x, then 54x/6x/2x, then 5x/7x/10x
  across three different committed versions — see `performance-baselines.md`'s addendum for the current
  figures rather than a number pinned here, which would go stale on the next regeneration. The certified
  candidate fails the "single-digit-millisecond" bar derived from itself in a substantial fraction of runs.

`probes/probe1-init-timing.mjs` now runs N iterations and reports min/median/max plus every raw
sample, and asserts only that the cold path completes (no hang) — which is the charter's actual
item-2 question and needs no wall-clock bound.

**TCM0 could not have derived an absolute threshold honestly from the hardware available to it.** An
absolute threshold needs a quiet, dedicated machine and a representative fixture; TCM0 had a
contended developer workstation and a 3-file fixture, and deriving a bar from those runs and calling
it locked is precisely the failure this program has already rejected once. That reasoning stands as
a finding.

**Resolution.** Of the two outcomes this row named — (a) an absolute bar derived on a dedicated
machine, or (b) an explicit ratification that the equivalent-work form is the whole contract — **Q3
rules (b)**. Requirements 6-8 are the complete Scope-10 performance contract and no
dedicated-machine absolute baseline is required. Nothing here is deferred to "adequate hardware".

---

## G-TCM0-ACCEPTANCE-ROWS-25-26 — CLOSED by ruling (Q4): both rows RETAINED

**Disposition.** The ruling this row was waiting on exists.
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q4: **retain both
rows under `VerterWithTypeSemanticOracle`** — row 25 preserves local content/position conversion and
carrier-to-project routing; row 26 preserves oracle working-set activation. TCM4 may remove the
tsserver-specific methods **only after TCM3 supplies and tests equivalent semantics**. The rows no
longer carry `CANDIDATE — governance ruling required` and no longer gate TCM0's acceptance. The
inverted-premise correction below remains a true recorded finding — it is the finding the ruling
followed — but its conclusion that "the ruling must be re-requested on a corrected packet" is
superseded: the ruling was taken and decided.

**Finding, as it stood before the ruling.** `feature-ownership-ledger.md` rows #25-26 carried
`CANDIDATE — governance ruling required`. TCM0's charter forbids acceptance with "an intentional
capability removal lacking explicit governance approval", so a ruling was needed before either row
could be dispositioned.

**What changed, and it is material.** The deletion rationale in the packet rests on the premise that
these methods are a **tsgo-lane relay artifact** whose function the content mapper's own wire
identity fields subsume. That premise is **factually inverted**, verified from source:

- Neither base tsgo engine overrides these methods — `crates/verter_type_runtime/src/tsgo/ipc.rs`
and
  `.../tsgo/owned.rs` contain no `fn register_carrier_member`, `fn register_carrier_metadata`,
  `fn activate_carrier_member` or `fn activate_carrier_members`.
- The only substantive implementation is **tsserver-family**:
  `crates/verter_type_runtime/src/tsserver/ipc.rs:3126`, `:3348`, `:3222`, `:3271`.
- The one tsgo-side override, `crates/verter_lsp/src/tsgo/composite.rs:1283`/`:1298`/`:1313`, is a
**pure
  delegation** — its entire body forwards to `self.managed`, the managed tsserver-family provider.
- The trait's own doc says so: *"`TsserverTypeProvider` overrides it to hydrate its `contents` cache
and
  carrier→project map"* (`crates/verter_type_runtime/src/traits.rs:349-351`).

The engine that actually implements these methods hydrates a **content cache and a carrier→project
map**. The mapper's `virtualFileName` / `canonicalSourceFileName` / `supplementalSourceFileNames`
fields are identity strings; they do not obviously subsume a content cache. Outcome (a) — approve
deletion — is therefore no longer supported by the reasoning as written.

**Outcome.** Of the two legal outcomes — approve deletion, or retain under
`VerterWithTypeSemanticOracle` — Q4 rules **RETAIN**, on exactly the corrected reading above: the
tsserver-family carrier-registration path and its `contents` cache are real function, not a tsgo
relay artifact the mapper's identity strings subsume. Row 25 is retained for local content/position
conversion and carrier-to-project routing; row 26 for oracle working-set activation. **Gate for any
later removal:** TCM3 must first supply and test equivalent semantics — see
`G-TCM4-DELETION-ROWS-25-26`.

## G-TCM4-DELETION-ROWS-25-26 — CLOSED by ruling (Q4), in the RETAINED direction

**Disposition.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q4 settles this row in the retained direction: both rows stay under `VerterWithTypeSemanticOracle`,
and
**TCM4 may remove the tsserver-specific methods only after TCM3 supplies and tests equivalent
semantics.** That condition — TCM3-supplied, TCM3-tested equivalents — is the whole of the deletion
gate, and it replaces "cite the ruling by ID" as the thing that has to be true before TCM4 may act.

Strictly downstream of the row above. **Owner:** TCM3, then TCM4. **Gate:** TCM3 supplying and
testing semantics equivalent to what rows 25 and 26 preserve today (row 25: local content/position
conversion and carrier-to-project routing; row 26: oracle working-set activation). Until that exists
and is tested, TCM4 may not remove the tsserver-specific methods. TCM3-EC-G1's "cite the ruling"
formulation is satisfied by citing Q4; what it does NOT do is authorize deletion on its own, because
Q4's authorization is conditional on the equivalence work.

## G-DIAGNOSTIC-CONVERGENCE — CLOSED by ruling (Q6): TCM3 already owns it

**Disposition.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q6: **TCM3 already owns convergence** through its `TypeSemanticOracle` and
`VerterWithTypeSemanticOracle` diagnostic contract, and **no new block is authorized.** The ruling
also states the interim consequence, which this register discloses rather than repairs: **until TCM3
lands, severity taxonomy, canonical positioning and unpositionable-diagnostic behaviour REMAIN
DIVERGENT across the CLI and oracle paths.** Every passage below saying the convergence has no valid
later-block owner, is open by design, needs a program-level scoping decision, or needs "its own
block or folding into an existing DX-parity effort", is superseded. The source-level findings — that
the CLI does not keep a separate session, that the real duplication is the two wire→DTO→position
mappers, and that the `--api`-vs-`--lsp` parity gap is its precondition — are retained; they are
inputs TCM3 inherits.

**Finding, sharpened.** A source-level investigation of both compiler-diagnostic paths (detail:
`diagnostic-ownership-matrix.md` → closure) found that this matrix's own premise is wrong and that
the duplication is not where it was recorded:

- The CLI does **not** keep a separate session. `crates/verter_session/src/types.rs:1189-1191`
states
  in-source that `HostConfig::batch_typecheck()` *"Keeps the SAME shared `VerterHost` / resolver / cache
  substrate as [`lsp_interactive`]"*. Both paths also share `verter_tsgo_api`, `toolchain::discovery`,
  `verter_span::Utf16LineIndex`, and the IDE-companion codegen product.
- The real duplication is a **wire→DTO→position band that exists twice**:
  `crates/verter_tsc/src/api_check.rs:490 map_one` and
  `crates/verter_type_runtime/src/tsgo/owned.rs:367 map_api_diagnostic` both map the same
  `verter_tsgo_api::proto::types::Diagnostic`, with divergent outputs — two severities vs four, and line/col vs
  bytes. **Corrected 2026-08-23:** an earlier revision of this row also attributed "no
  tags/related-information vs both" and "hard error vs silent drop" to that pair. Neither holds AT that
  pair: `map_api_diagnostic` hardcodes `tags: Vec::new(), related_information: Vec::new()`
  (`owned.rs:397-398`), so neither named function populates them — the real sites are `tsgo/ipc.rs:1496`
  and `tsserver/ipc.rs:1364`, on the `--lsp` path — and the silent drop happens later, at
  `merge/diagnostics.rs:96`, not in either mapper. The matrix itself cites these correctly; only this
  row's compression was loose. The consequence matters: converging the two mappers does NOT by itself
  give the CLI tags or related-information.
- `TypeScriptLspDirect` and `VerterWithTypeSemanticOracle` have **zero occurrences under
`crates/`**. They
  are target vocabulary; no reading of current code can be justified by citing them.

**"Converge onto one" now has a checkable meaning:** delete one of those two functions so a single
mapper produces a single DTO carrying the richer contract, with the CLI projecting to line/col at
its reporter boundary rather than at the wire boundary, and the two unpositionable-diagnostic
policies reconciled in the same change.

**Why TCM0 did not name the owner, and why that reasoning does not survive.** TCM0's argument was
that the blocker is stated in-source at `crates/verter_type_runtime/src/tsgo/owned.rs:503-508`:
promoting the `--api` checker to the sole user-facing diagnostics surface *"requires closing its
per-carrier program parity with the `--lsp` program (the `vue`/JSX/tag/suggestion gaps) and is a
full-DX-contract concern, not this provider's job."* That is a
**`--api`-vs-`--lsp` parity gap that exists today and would exist unchanged if the TCM program were
cancelled tomorrow.** It is not created by, blocked on, or resolved by anything TCM1-TCM4 do. TCM1
and TCM2 are ruled out by their own charters' owned-scope boundaries; TCM3 and TCM4 are ruled out by
this evidence, because assigning it to either would import an unrelated DX-parity workstream into a
TCM block on no evidence — the exact outcome this row exists to prevent.

**Owner.** TCM3, by Q6 — the convergence falls inside its `TypeSemanticOracle` /
`VerterWithTypeSemanticOracle` diagnostic contract. The parity gap above is a real input TCM3
inherits, not a reason to leave the convergence unowned, and no new block is authorized to take it.
**Gate.** TCM3's own landing. **Until then:** severity taxonomy, canonical positioning and
unpositionable-diagnostic behaviour are divergent across the CLI and oracle paths — disclosed, not
repaired here. The packet TCM3 inherits is assembled in `diagnostic-ownership-matrix.md`'s closure
section.

---

# CLOSED ON EXTERNAL EVIDENCE

One row, closed by a fact outside this block that anyone can re-derive.

## G-LEDGER-VALIDATOR-VIOLATIONS (was: the header's BV2/B5/CM1 row) — CLOSED

**This closure survives the ruling, and it is the only one that does.** It was closed by another
block's landing — an externally verifiable fact anyone can re-derive by running the validator — not
by TCM0 certifying its own work. Q1's withdrawal of TCM0's self-certified closures does not reach
it.

The three `context_packet_digest` violations this file recorded as expected output no longer
reproduce; `block/ledger-subordinate-to-code` landed and closed them. See "Re-validation commands"
above for the fresh result. Recorded as closed rather than deleted, so a reader comparing against
the previous revision can see the claim was retired on evidence rather than quietly dropped.

---

# OPEN — owned by the successor block

Six rows. Five of them were marked CLOSED by this branch's own closure pass; that self-certification
does not stand. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q1
returns the round-3 candidate as wrongly scoped, lands the accurate work as a NON-ACCEPTANCE
evidence package, and hands the incomplete contract remainder to a **successor block with fresh
verification** — and a closure that needs fresh verification is not a closure. Every source-backed
finding the closure pass produced is retained below; only the "therefore CLOSED" verdict is
withdrawn. The sixth row (`G-CHARTER-AMENDMENTS`) is a hybrid: its `TCM0.md` row is discharged by
Q2, and its remaining rows transfer with the closures they derive from. Scope:
`successor-block-scope.md`.

## G-LEDGER-SCOPE — OPEN (successor block)

**WITHDRAWN as a closure, 2026-08-24 — this row is OPEN.** The findings below stand as evidence and
are not retracted. What is withdrawn is the "therefore CLOSED" verdict, which was TCM0's own
self-certification. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 returns this block's round-3 candidate as wrongly scoped, lands its accurate portion as a
NON-ACCEPTANCE evidence package, and hands the incomplete contract remainder to a **successor block
with fresh verification**. A closure that needs fresh verification is not a closure.

This row's own text is the clearest case: it discloses that TCM0 "does not ratify" the capability
characterisation for **14 capabilities it did not individually analyse**. A row with a self-declared
unratified residue is not closed.

**Owner.** The successor block (`successor-block-scope.md`). **Gate.** Fresh, independently
checkable verification — the successor's closure claim must be verifiable by someone who did not
produce it.

**The question.** The steering's charter item 3 names capabilities beyond the `TypeProvider` trait's
44 methods; `feature-ownership-ledger.md` scopes itself to those 44. Nobody had checked row-by-row
whether each named capability already IS one of the 44.

**What the check found** (full detail: `feature-ownership-ledger.md` → "Closure, 2026-08-23"):

- **The 44 is correct.** Re-enumerated directly from
`crates/verter_type_runtime/src/traits.rs:130-512`:
  44 methods, no `cfg`-gated ones, no capability-carrying supertrait, no `TypeProviderExt`.
- **14 of the steering's 32 named capabilities are served by real production code behind no
  `TypeProvider` method** — formatting, call hierarchy, code lens, folding, selection ranges, document
  symbols, rename preparation, component surface resolution, template expression typing, props, events,
  slots and snippets, directives, framework macros. Each is located to a `file:line` in the ledger's
  closure table. Two more are partially covered (the Verter-native halves of auto-imports and background
  semantic analysis), and the "cache methods" clause has no trait method at all.
- **`implementation` (goto-implementation) has no `TypeProvider` method or `verter_lsp` dispatch
handler,
  but IS served** — `capabilities.rs` sets no `implementation_provider` and the LSP dispatch table has no
  handler, both true, but a repo-wide search across the same seven spellings returns **11 hits, not one**:
  a JSON-RPC routing allowlist entry at `crates/verter_tsgo_api/src/egress.rs:494` (pass-through, not a
  capability), four test-only hits, and six production hits at
  `packages/typescript-plugin/src/index.ts:3095-3109` — a genuine Verter-owned carrier-routing override of
  `getImplementationAtPosition`. Corrected 2026-08-24 (round-2 review); full breakdown and search command
  in `feature-ownership-ledger.md` §3.
- **Six per-row citations in the ledger are factually wrong**, corrected in place. One is
load-bearing and
  is carried as its own consequence under `G-TCM0-ACCEPTANCE-ROWS-25-26` below.

**Why this closes.** The steering scopes the inventory to *"every method, call site, capability, and
background consumer **of the current `TypeProvider`**"* (`:275`), and its acceptance line reads
*"every
existing `TypeProvider` capability has a complete row"* (`:370`). The 14 are not `TypeProvider`
capabilities — none has a trait method and none has a provider in its request path — so the correct
entry against that bar is a located verdict, not an ownership row. All 32 checklist items are now
answered: 17 by an existing row, 14 by a "not a `TypeProvider` capability" verdict with its real
location, and one (`implementation`) by the same located verdict at a different layer — the
typescript-plugin carrier-routing override, not a `TypeProvider` method and not a proven absence
(ledger §3).

**Residue, disclosed not hidden — and it is why this row is open.** The 14 are uniformly
Verter-served today and uniformly unaffected by TCM1-TCM4 (no TypeScript engine is in any of their
request paths, so becoming a content mapper changes none of them). TCM0 records that
characterisation as a finding and does **not** ratify it as an ownership assignment for 14 rows it
did not individually analyse.

## G-SEMANTIC-API-CERTIFICATION — OPEN (successor block)

**WITHDRAWN as a closure, 2026-08-24 — this row is OPEN.** The findings below stand as evidence and
are not retracted. What is withdrawn is the "therefore CLOSED" verdict, which was TCM0's own
self-certification. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 returns this block's round-3 candidate as wrongly scoped, lands its accurate portion as a
NON-ACCEPTANCE evidence package, and hands the incomplete contract remainder to a **successor block
with fresh verification**. A closure that needs fresh verification is not a closure.

Q1 admits the probes and the transcript as evidence — they are committed, executable, and assert
discriminating properties — but **no ruling decides** whether the block that is accepted must itself
run charter item 2's bulk probes, or whether an amendment reallocates them. That undecided question
is precisely a contract remainder, and it goes to the successor.

**Owner.** The successor block (`successor-block-scope.md`). **Gate.** Fresh, independently
checkable verification — the successor's closure claim must be verifiable by someone who did not
produce it.

**The question.** Charter item 2 requires live probes of project/source-file lookup, `Program` and
`TypeChecker` operations, bulk symbol/type/reference queries, completions, diagnostics, cancellation
and failure behaviour. `package-lock-and-semantic-api.md` §4.0 supplied an INVENTORY read from a
type declaration for most of that list, and no probe script, transcript or measurement was cited
anywhere.

**What closed it.** The probes were written, executed against the pinned candidate, and committed:
`probes/probe5-bulk-semantic-api.mjs` (50+ checks across every clause of item 2, each asserting a
discriminating property; exits 0) and `probes/probe6-out-of-range-completion-panic.mjs`. Results are
in `package-lock-and-semantic-api.md` §6 and `probes/transcript.md`.

**The wire-spelling gap is closed too, and it was not supposed to be closable here.** §3 had
recorded the exact content-mapper wire method names as unobtainable from a stripped binary and
delegated them to TCM2. That was right about static extraction and wrong as a limit:
`probes/probe7-mapper-wire-capture.mjs` runs the pinned native `tsc --runExternalCode` against a
real `contentMappers` config with a stub mapper and captures every frame. The methods are
**`initialize`, `openProject`, `transform`, `closeProject`**, with their params shapes, the
`{package}@{version}:{n}` handle format, the configuration keys, and a 5-second `initialize` timeout
— all recorded in §3a and all asserted by the probe. The residual is narrowed to the `transform`
RESPONSE body layout, which stays with TCM2.

**A prior citation defect closed at the same time.** §5's Reproduction block named four probe
scripts — `probe1-init-timing.mjs`, `probe2-stale-snapshot.mjs`,
`probe3-stale-sourcefile-confirm.mjs`, `probe4-filechanges-correct.mjs` — that **were never
committed anywhere in the repository**. Every claim resting on them cited files that did not exist.
All four are now re-created from the behaviours §4a-4c record, re-executed, and committed; each
reproduces its recorded finding, including the §4c post-dispose asymmetry exactly (`getSourceFile`
returns the identical object in 0 ms while all four sibling `Program` methods throw `snapshot 1 not
found`). `probes/README.md` states plainly that these are re-creations and that the transcript is of
the re-run, not the original.

**Package identity independently re-verified**, not carried forward: sha1, sha256, 476 files and
`gitHead` all recomputed from a fresh download and all matching §1. The probe harness refuses to run
against any version other than the pin.

**Five new constraints fell out**, each binding on TCM2/TCM3 (§6.2): the diagnostic wire shape is
`{fileName?, pos, end, code, category, text, …}`, not `start`/`length`/`messageText`; there is no
project-wide references primitive and the failure mode is a silent empty result;
`getCompletionsAtPosition` rejects any completion list needing auto-imports; an out-of-range
completion position causes a recovered Go panic surfaced with a stack trace; and out-of-range
`Checker` positions degrade to the file's module symbol rather than failing. The last two mean
**positions must be clamped Verter-side; callee validation cannot be relied on.**

**Not touched by these bulk probes — but closed by their own:** the bulk probes all drive the
in-process spawn path, so they do not themselves exercise the content-mapper wire spelling or the
`API.fromLSPConnection` attach path. Both were closed by dedicated probes
(`probe7-mapper-wire-capture.mjs`, `probe8-lsp-session-attach.mjs` — see "Two former delegations,
now CLOSED" at the end of this file). What remains delegated is the `transform` RESPONSE body layout
(TCM2); the attach path leaves TCM3 a constraint rather than a probe: the topology is
ASYNC-CLIENT-ONLY.

## G-PROJECTION-MASK-TOTALITY — OPEN (successor block)

**WITHDRAWN as a closure, 2026-08-24 — this row is OPEN.** The findings below stand as evidence and
are not retracted. What is withdrawn is the "therefore CLOSED" verdict, which was TCM0's own
self-certification. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 returns this block's round-3 candidate as wrongly scoped, lands its accurate portion as a
NON-ACCEPTANCE evidence package, and hands the incomplete contract remainder to a **successor block
with fresh verification**. A closure that needs fresh verification is not a closure.

Totality is a claim over all fifteen `class × relation` cells and all twenty feature bits; it is
established only by someone re-deriving the table, which is what "independently checkable" means
here.

**Owner.** The successor block (`successor-block-scope.md`). **Gate.** Fresh, independently
checkable verification — the successor's closure claim must be verifiable by someone who did not
produce it.

**The question.** The terminal policy named five factors but wrote two class baselines as prose
conditionals, leaving several of the 20 `SpanMapFeature` bits undecided — a contract that reads as
terminal but is not.

**What closed it** (detail: `projection-class-contract.md` → closure). The policy is now a total
function: five factors, each a total map from a closed domain to a 20-bit constant, composing by
AND, with an explicit computed value for all fifteen `class × relation` cells. Every conditional was
either relocated to the factor that actually decides it or given a fail-closed default with a closed
widening set:

- `AuthoredTransformed`'s "excludes `Rename`/`CodeActions` when…" conditions were conditions on
transform
  reversibility, which is what `relation` encodes — relocated to the relation factor, leaving the baseline
  unconditional `All`.
- The relation factor is **derived from the shipped source, not invented**: upstream documents
  `SpanMap.isExact` as *"a precise, edit-safe projection through one verbatim segment"*, and its `mapPoint`
  / `mapRange` both yield `Exact` only for `Verbatim`, making `Alias` and `Atom` indistinguishable. So the
  five edit-producing bits are legal only on `ExactCopy`.
- `SynthesizedHelper`'s "`None` otherwise" is resolved fail-closed: baseline `None`, widened to
  `Hover|Definition` only for members of a closed `DocumentedAmbientSymbol` registry.
- `REGION` is total via a mandatory `All` default plus a closed exception table, **currently empty**
— every
  content-mapped region is a fully IDE-supported surface, so no exception is justified and none is invented.
- `OWNER_WIRE_ELIGIBLE` = 13535, computed from the ownership ledger. It clears ten of the twenty
bits and
  resolves the prose's hardest case outright: **`Rename` is cleared globally**, because ledger row #15
  already assigns rename to the oracle on the grounds that a `TypeScriptLspDirect` answer would miss the
  template-side occurrences.

**Not closed, and explicitly excluded:** per-row `projection_class` ASSIGNMENT for the
`TokenCompletion` grouping remains TCM1/TCM2's named task. The mask function is total over the class
axis; choosing a span's class is a different question and this closure makes no claim about it.

## G-STRING-SURFACE-CITATIONS — OPEN (successor block)

**WITHDRAWN as a closure, 2026-08-24 — this row is OPEN.** The findings below stand as evidence and
are not retracted. What is withdrawn is the "therefore CLOSED" verdict, which was TCM0's own
self-certification. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 returns this block's round-3 candidate as wrongly scoped, lands its accurate portion as a
NON-ACCEPTANCE evidence package, and hands the incomplete contract remainder to a **successor block
with fresh verification**. A closure that needs fresh verification is not a closure.

This row's subject is prior undercounting, and the closure pass undercounted inside it —
`chain_source_map`'s eighth production caller was missed and corrected mid-pass, and the inventory
is explicitly "not claimed exhaustive" after two manual passes each found the prior one incomplete.
Its charter-amendment consequence for `TCM1.md` transfers with it (`G-CHARTER-AMENDMENTS`).

**Owner.** The successor block (`successor-block-scope.md`). **Gate.** Fresh, independently
checkable verification — the successor's closure claim must be verifiable by someone who did not
produce it.

**The open sub-question.** Is an exhaustive starting count required before TCM1 may be dispatched,
or is `TCM1.md`'s exit-criterion-1 deletion-based discovery sufficient without one?

**The answer is neither option as posed** (detail: `mapping-products-string-surface.md` → closure).
The pre-count is not required — but the deletion does not replace it either, because
**`CodeTransform` is not the chokepoint the criterion assumes.**

Verified from source: the two STRING-RETURNING producers `generate_map_json` and
`generate_map_json_with_preamble` have **zero production call sites outside
`crates/verter_compiler`** and exactly **seven** inside it. (**Corrected 2026-08-23:** an earlier
revision extended that "exactly seven" to `chain_source_map` too and added "every other caller is a
test". Wrong on both counts — `chain_source_map` has an eighth production caller at
`crates/verter_compiler/src/assembly/vue_module.rs:174`, inside `pub(crate) fn rewrite_script`,
which mints map JSON the deletion also does not reach. Getting an exhaustiveness count wrong in the
row whose whole subject is prior undercounting is recorded, not quietly amended.) Deleting both
string producers therefore yields seven compile errors in one crate. Every map-carrying field in
`verter_session`, `verter_lsp`, `verter_protocol`, `verter_napi`, `verter_wasm`, `verter_ffi` and
`verter_dx_baseline` compiles unchanged, because eight independent producers mint map JSON without
touching `CodeTransform` — including `build_tsc_source_map`
(`crates/verter_compiler/src/tsc/script.rs:7042`), a `pub` parallel API called cross-crate from
`crates/verter_session/src/framework/api_projectors/svelte.rs:1056`, whose output is described
in-source as
*"the exact JSON shape the carrier store publishes and the editor plugin consumes"*. `lib.rs:41`'s
`pub use oxc_sourcemap;` is a documented escape hatch that lets any consumer mint one directly.

**This is a fourth category, not one of the three `TCM1.md` already excludes.** Its criterion
carefully names read-time projections, the FFI wire boundary, and externally-supplied inbound fields
as things the deletion does not prove. The eight producers are none of those — they are
Verter-produced projection data minted through a parallel path the criterion does not model.

**The sound instrument is a type change, not a deletion:** a value newtype over the encoded map with
a private inner field and no `From<String>`, retyping the map-carrying fields. The retype IS the
exhaustive enumeration, it is structural rather than name-keyed, and it needs no starting count. Two
details decide whether it seals: no such value newtype exists today (`verter_identity`'s
`EncodedSourceMapId` and siblings are *identity* newtypes over `Canonical` and explicitly disclaim
map construction — a name trap), and `pub use oxc_sourcemap;` must be reconsidered in the same
change.

**Consequence:** `TCM1.md`'s owned-scope item 1 and exit criterion 1 need amending before TCM1 is
dispatched. Carried as `G-CHARTER-AMENDMENTS` below. This pass does **not** edit `TCM1.md` and does
not re-pin its digest.

## G-DELETION-CLOSURE-ITEMS-17-18 — OPEN (successor block)

**WITHDRAWN as a closure, 2026-08-24 — this row is OPEN.** The findings below stand as evidence and
are not retracted. What is withdrawn is the "therefore CLOSED" verdict, which was TCM0's own
self-certification. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 returns this block's round-3 candidate as wrongly scoped, lands its accurate portion as a
NON-ACCEPTANCE evidence package, and hands the incomplete contract remainder to a **successor block
with fresh verification**. A closure that needs fresh verification is not a closure.

The accumulation-at-creation mechanism is a proposal that has to be adopted into three ratified
charters before it can bind anyone; until it is, items 17-18 have a method and no obligation. Its
charter-amendment consequences for `TCM1.md`/`TCM2.md`/`TCM3.md` transfer with it
(`G-CHARTER-AMENDMENTS`).

**Owner.** The successor block (`successor-block-scope.md`). **Gate.** Fresh, independently
checkable verification — the successor's closure claim must be verifiable by someone who did not
produce it.

**The question.** Which of two resolutions applies — (a) execution-time discovery ratified as the
closure mechanism, or (b) TCM0 held in LOCKED until TCM1-TCM3 make items 17-18 enumerable?

**(b) is unsatisfiable.** `TCM1.predecessors = ["TCM0"]`, `TCM2.predecessors = ["TCM0","TCM1"]`,
`TCM3.predecessors = ["TCM0","TCM1"]` — none may be dispatched until TCM0 is ACCEPTED, so requiring
their output first is the same cycle rejected for the rows #25-26 gate and for `G-TOPOLOGY`. Struck.

**(a) is ratified in a strengthened form** (detail: `deletion-closure.md` → closure). Verbatim (a)
would re-adopt the execution-time discovery round-2 review already rejected as unassigned. Instead,
items 17-18 close by **accumulation at creation**: each of TCM1/TCM2/TCM3 records every DTO or API
type it introduces or orphans whose sole producer/consumer pair lies inside the deleted set,
appended to `deletion-closure.md` as each block lands; TCM4's exit criterion 5 then *verifies* that
named list rather than discovering one. Item 18's correct end state is an **empty list established
by a negative check** — TCM2 proves it ships exactly one codec — with any interim codec it did carry
entering item 17's list at that moment.

This preserves the steering's *"Do not defer this inventory to TCM4"* rule (TCM4 still receives
names, never a search) and resolves `TCM4.md`'s internal contradiction rather than preserving it.
TCM4 needs no amendment: its required-outcomes item 3, owned-scope item 9 and exit criterion 5
already defer to whichever resolution TCM0 ratifies. TCM1/TCM2/TCM3 each need one added exit
criterion — carried as `G-CHARTER-AMENDMENTS` below.

## G-CHARTER-AMENDMENTS — OPEN (successor block), and partly DISCHARGED

**Disposition, split three ways.**

**DISCHARGED — the `TCM0.md` Scope-item-7 row (topology-selection reallocation).**
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q2 ratifies the
transfer by ruling, so no charter amendment is needed to effect it and nothing about it gates TCM0's
acceptance. That discharge covers the `TCM0.md` row and nothing else. The two `TCM2.md`/`TCM3.md`
rows Q2 generates are a SEPARATE disposition, immediately below — they are not discharged, and they
are not housekeeping.

**PENDING ADOPTION — the two `G-TOPOLOGY`-sourced rows (`TCM2.md` owned-scope item 16 / exit
criterion 14, and `TCM3.md` owned-scope item 9 / exit criterion 10).** These do NOT transfer to the
successor, because they rest on a SETTLED ruling rather than on a withdrawn closure: Q2 is ratified,
and nobody has to re-establish the finding under it. Nothing about the DECISION is pending — the
ruling is its own ratification act, as `G-TOPOLOGY` states. What is still owed is the charter TEXT
that makes the decision bind, which is exactly what this register already states for the
accumulation criterion in `G-DELETION-CLOSURE-ITEMS-17-18` above — an added exit criterion "has to
be adopted into three ratified charters before it can bind anyone." Until that adoption act happens,
Q2's selection obligation exists as a ruling but is written into no block's exit criteria, and a
reader of those charters is told nothing about it.

**Owner.** The **program orchestrator**, as the authority that amends a charter and re-pins its
digest in `docs/arch/architecture-lock/ledger/authority-registry.toml` (with the maintainer's
ratification act). **Gate.** Adoption BEFORE TCM2 and TCM3 are dispatched. These are **standing
pre-dispatch mandates**, not optional wording cleanup: they are the acts that convert Q2's ratified
transfer into the blocking projection-plane exit of TCM2 and the blocking semantic-plane exit of
TCM3. If either block is dispatched with its charter unamended, the block it binds has no exit
criterion requiring the topology selection Q2 assigned to it, and a ratified transfer becomes
unenforceable by omission — the same failure mode recorded at the end of this section for the
accumulation criterion.

**TRANSFERRED — the three rows that derive from WITHDRAWN closures.** `TCM1.md`'s owned-scope item 1
/ exit criterion 1 replacement (`G-STRING-SURFACE-CITATIONS`), the added accumulation exit criterion
for `TCM1.md`/`TCM2.md`/`TCM3.md`, and `TCM2.md`'s single-codec negative check (both
`G-DELETION-CLOSURE-ITEMS-17-18`). Those two closures are withdrawn (Q1), and an amendment derived
from a withdrawn closure cannot stand on its own, so these three rows move to the successor block
with the closures that generate them.

**Owner.** The successor block (`successor-block-scope.md`). **Gate.** Fresh, independently
checkable verification — the successor's closure claim must be verifiable by someone who did not
produce it. The successor must re-establish the underlying findings on independently checkable
grounds before any charter amendment is proposed from them. This owner and this gate govern the
three transferred rows only; the two `G-TOPOLOGY`-sourced rows above carry their own.

Three passes above established facts that invalidate parts of four ratified, digest-pinned charters —
one of them (`G-TOPOLOGY`) since settled by ruling, the other two now withdrawn to the successor.
This pass deliberately did **not** edit those charters and did **not** re-pin their digests — rebinding a
ratified document's digest without a fresh ratification act is itself a governance violation, the
same restraint already exercised for `MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md`.

| Charter | Amendment required | Source |
|---|---|---|
| ~~`TCM0.md` (`TCM0-CHARTER`, sha256 `2ea41dd8…`)~~ **DISCHARGED by Q2** — the transfer is ratified by ruling, so no amendment is required to effect it and it gates nothing. TCM0 owns candidate screening, survivor sets, metrics, harness, non-dominance rule and baseline method; comparative selection is a blocking TCM2 projection-plane exit and a blocking TCM3 semantic-plane exit. | — | `G-TOPOLOGY` |
| `TCM1.md` (`TCM1-CHARTER`, sha256 `2886c796…`) | Owned-scope item 1's "Single point of origin: `CodeTransform`" premise is false, and exit criterion 1's deletion proof covers seven call sites in one crate. Replace with the value-newtype retype. | `G-STRING-SURFACE-CITATIONS` |
| `TCM1.md`, `TCM2.md`, `TCM3.md` | One added exit criterion each: record every DTO/API type the block introduces or orphans whose sole producer/consumer pair lies inside the deleted set. For TCM1 this folds into the amendment above — one amendment act, not two. | `G-DELETION-CLOSURE-ITEMS-17-18` |
| `TCM2.md` (`TCM2-CHARTER`, sha256 `3cae6cef…`) | One added exit criterion: prove exactly one content-mapper codec ships, with no interim versioned codec in the landed tree. | `G-DELETION-CLOSURE-ITEMS-17-18` |
| `TCM2.md` (`TCM2-CHARTER`, sha256 `3cae6cef…`) | Add owned-scope item 16 and numbered exit criterion 14: select among the surviving projection-plane candidates using `topology-benchmark-plan.md`; capture and commit the current-path baseline as the block's first act under `performance-baselines.md` requirements 6-8; report the complete comparison; and, if multiple candidates remain non-dominated, apply and record a stated secondary criterion per the benchmark plan's selection rule. Proposal text: `tcm1-tcm4-charter-refinements.md`. | `G-TOPOLOGY` |
| `TCM3.md` (`TCM3-CHARTER`, sha256 `78efb323…`) | Add owned-scope item 9 and numbered exit criterion 10: the equivalent requirement for the surviving semantic-plane candidates — select using `topology-benchmark-plan.md`; capture and commit the current-path baseline as the block's first act under `performance-baselines.md` requirements 6-8; report the complete comparison; record a stated secondary criterion if multiple candidates remain non-dominated. Proposal text: `tcm1-tcm4-charter-refinements.md`. | `G-TOPOLOGY` |
| `TCM4.md` | **None identified.** Its existing deferral wording consumes whichever items-17-18 resolution is eventually accepted; since that resolution is withdrawn to the successor, this row is a prediction rather than a settled finding. | — |

**Who acts.** Charter re-ratification and digest re-pinning in
`docs/arch/architecture-lock/ledger/authority-registry.toml` are the program orchestrator's and
maintainer's acts, never this block's. **No `TCM0-CHARTER` amendment is required before TCM0
acceptance** — Q2 discharged that row. Of the rows that remain, the split is:

- The three rows sourced to `G-STRING-SURFACE-CITATIONS` and `G-DELETION-CLOSURE-ITEMS-17-18` are
  **proposals** resting on findings the successor must first re-establish; they are not standing
  mandates this register holds open against those blocks' dispatch.
- The two rows sourced to `G-TOPOLOGY` (`TCM2.md` item 16 / exit 14, `TCM3.md` item 9 / exit 10)
  **ARE standing mandates**, held open by this register against TCM2's and TCM3's dispatch. Their
  finding is settled by Q2 and needs no re-establishment; only the adoption act is outstanding, and
  it is the program orchestrator's. Adopting them is what makes topology selection a blocking exit of
  those blocks at all.

**This gate has no mechanical rail, and that is a real weakness rather than an oversight.** Nothing
in `validate-program-state.mjs` or the authority registry fails if TCM1 is dispatched with its
charter unamended. The concrete risk is specific: if TCM1-TCM3 are dispatched without the item-17
accumulation criterion, no block is ever required to write the list, and TCM4's exit criterion 5
then "verifies" a list nobody produced — closing `G-DELETION-CLOSURE-ITEMS-17-18` by omission rather
than by evidence. A read-only block can name the obligation and its owner but cannot install the
rail, so the exposure is recorded here explicitly for the authority that can — and it carries to the
successor along with the rows that generate it. No row here blocks TCM0's acceptance any more: the
one that did (`TCM0.md` Scope item 7) was discharged by Q2, and TCM0 is not being accepted by this
package in any case.

---

# OPEN — owners outside TCM0

Four rows, untouched by any ruling and unchanged by this pass.

## G-CONFORMANCE-FIXTURES-TCM2 — OPEN

The projection-plane slice of the steering's "Required conformance coverage" list — mapper purity,
single-input projection, signed-offset bounds, and the Vue/Svelte/script-mode fixture matrix as it
applies to the content-mapper surface — is TCM2's own implementation work, not a TCM0
pre-deliverable.
**Owner:** TCM2. **Gate:** TCM2's own Numbered Exit Criteria, already wired to cite this coverage
list. Unchanged by this pass.

## G-CONFORMANCE-FIXTURES-TCM3 — OPEN

The semantic-plane slice — snapshot correctness, cancellation, configured/inferred project and
trust-state coverage as it applies to semantic-capability dispatch. **Owner:** TCM3. **Gate:**
TCM3's own Numbered Exit Criteria. Unchanged by this pass, except that §6.2's five new constraints
(no project-wide references primitive, auto-import completion rejection, position clamping) are now
binding inputs to those fixtures.

## G-CONFORMANCE-FIXTURES-TCM4 — OPEN

The activation/deletion slice — attestation, trust, JSONC safety, missing/malformed/duplicate
mappers, multi-installation monorepos, project references. **Owner:** TCM4. **Gate:** TCM4's own
Numbered Exit Criteria. Unchanged by this pass.

## G-TEMPLATE-SRC-PROJECT-CONTEXT-CONTRACT — OPEN

`external-source-decision-table.md` row 7 asserts `<template src>` is content-mapped because it
needs the same template→TSX transform as an inline template, which establishes the transform KIND
but not the project/context contract the steering's model 2 requires. Missing: a positive proof that
the mapper's `transform()` input for the external file is that file's own content, which TypeScript
project owns it for content-mapping purposes, and which `tsconfig` identity applies. Diagnostic
ownership IS already proven. `TCM2.md`'s exit criterion 5 supplies only a NEGATIVE test (a
cross-source-unit range is rejected at construction). **Owner:** TCM2. **Gate:** TCM2's own Numbered
Exit Criteria — a positive fixture proving the three missing elements alongside the existing
rejection test. Unchanged by this pass.

---

# Two former delegations, now CLOSED by probing rather than by re-delegating

Both of these were recorded as deliberate TCM0 decisions to delegate onward, and a review leg
challenged both as shortfalls against the steering's literal assignment to TCM0. The challenge was
right, and the resolution was to do the work rather than defend the delegation:

- **The exact content-mapper wire method-name spelling — CLOSED.** §3 had recorded it as
unobtainable from
  a stripped binary via static `strings` extraction and delegated it to TCM2. It IS obtainable, by running
  the compiler: `probes/probe7-mapper-wire-capture.mjs` captures every frame TypeScript sends to a real
  configured mapper. The methods are `initialize` / `openProject` / `transform` / `closeProject`, with
  their params shapes, the `{package}@{version}:{n}` handle format, the configuration keys, and a 5-second
  `initialize` timeout (§3a). Residual, narrowed and still TCM2's: the `transform` RESPONSE body layout.
- **The `API.fromLSPConnection` session-attach probe — CLOSED.** §4a had recorded it as out of probe
budget
  and delegated it to TCM3. `probes/probe8-lsp-session-attach.mjs` drives a real LSP handshake, obtains the
  API pipe, attaches, and answers a semantic query over it. No hang. It also produced a finding stronger
  than the gap it closed: **attach is ASYNC-CLIENT-ONLY** — the sync client refuses socket connections
  (`dist/api/sync/client.js:11`) — which constrains TCM3's topology choice in a way nothing had recorded.

**A ratified ruling now recites these as open, and TCM0 does not edit it.**
`rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md` clause 2 states that "the two open
verification gaps stay open", gated to TCM2 (wire spelling) and TCM3 (attach probe). On the facts,
both are now closed by TCM0's own probes. That ruling is a maintainer-ratified, digest-pinned
document (`authority-registry.toml`), so this evidence pass **does not edit it and does not re-pin
its digest** — the same restraint applied to the TCM1/TCM2/TCM3 charters, and the same reason:
rebinding a ratified document's digest without a fresh ratification act is itself a governance
violation.

`decisions/ADR-021-typescript-content-mapper-dual-plane.md` carries the same two carry-forward items
in its own text, and Q1/Q8 exclude every `ADR-021` change from this package, so that document
likewise stands at its ratified text and does not record probes 7 and 8.

The situation is therefore recorded, not resolved: **both documents are superseded on the facts and
need a fresh ratification act to say so.** Until that act exists, a reader consulting the ruling
will be told two gaps are open that the evidence shows are closed. That contradiction is disclosed
here rather than hidden, and it is a governance item for the maintainer, not something TCM0 may
settle by editing the artifact that disagrees with it. It does not reopen either gap — probes 7 and
8 are the evidence, and evidence is not undone by a stale recital — but it does mean the ruling and
this register disagree until the maintainer acts.

**Nothing downstream is loosened by the closures.** TCM2 still owns the `transform` RESPONSE body
layout (the narrowed residual of the wire gap) and TCM3 still owns every semantic-plane obligation
its charter names; what changed is that neither has to re-derive a fact TCM0 has now established,
and TCM3 inherits a new hard constraint (attach is async-client-only) it would otherwise have
discovered late.

**The lesson, recorded because it generalises.** Both delegations were honest about what had NOT
been done, and both were wrong about what COULD be done. "Unobtainable from static analysis" is not
"unobtainable". A recorded gap should name the method that would close it, and the next pass should
try that method before inheriting the delegation — which is exactly what
`package-lock-and-semantic-api.md` §3's own text suggested ("a live protocol capture … tracing
stdio") and nobody had attempted.
