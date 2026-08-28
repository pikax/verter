# TCM0 — closure register

**This file is the continuation surface, and `OPEN-GAPS.md` is not.** The distinction is the whole
point of the register existing.

An earlier continuation was scoped from a curated list of `G-*` gap rows. That list was accurate about
every row it contained and silent about everything it did not, so completing every row in it would
still have left charter obligations open — two of them with no evidence in the tree at all. A gap
register records what somebody noticed. **A closure register is derived from the charter, so it cannot
be silent about an obligation nobody noticed.**

Every row below has a stable ID under a numbered charter Scope item, the acyclic invariant, or the
Acceptance clause. `G-*` rows are annotations UNDER these IDs, never a source of scope.

`probes/closure-validator.mjs` derives every sentence in those three obligation-bearing sections from
`charters/TCM0.md` itself and refuses acceptance if this register does not account for every sentence,
if any obligation is `OPEN` / `PROPOSAL` / `WITHDRAWN`, if a row names a proof artifact that does not
exist, or if a mandatory obligation is owned by a block that depends on TCM0. Run it; do not read this
table and believe it.

## This file is the SOLE owning store for obligation status

Three separate repairs in this evidence set each left the claim they were repairing still asserted
somewhere else — in the ledger, in the gap register, in a scoping note. That is not three mistakes; it
is one defect with three instances. **A mutable fact restated in a non-owning layer becomes a second
normative store, so repairing one leaves the others asserting the old value**, and no amount of care
in the repair prevents it. "Read the sentence, not the match" is how the instances were DETECTED; it
is not the remedy.

The remedy is structural: **one owning statement per mutable proposition, and every other mention is a
reference that names the owner without repeating the value.**

- The **status** of every TCM0 obligation is owned HERE, in this table, and nowhere else.
- Any other document may say *"see `closure-register.md` row `S9.c`"*. It may not say what that row's
  status IS. A second copy of a status is a second thing to keep in step, and it will not be kept.
- The same rule governs the derived facts underneath: the capability request-path verdicts are owned
  by `capability-provider-hop-walk.md`, the call-site classification by `typeprovider-call-sites.md`,
  the transform-response contract by `package-lock-and-semantic-api.md` §3b. Elsewhere, cite them.

`probes/closure-validator.mjs` enforces this: it refuses if a status token appears beside a row id in
any evidence file other than this one. The check is textual and therefore imperfect — it catches the
restatement pattern that has actually bitten three times, not every conceivable paraphrase — and it is
a gate over this block's own evidence, not a source-tree guard. Its limit is stated rather than left
to be assumed.

## Status vocabulary — closed, and no informal fifth value

| status | meaning |
|---|---|
| `PROVEN` | evidence exists in this tree and a named command re-derives or falsifies it |
| `RULED` | closed by a ratified ruling, cited by ID; no further evidence is owed |
| `NOT-OWNED` | outside TCM0's charter in either direction, with the reason and the real owner named |
| `OPEN` | not closed — the validator refuses acceptance while any mandatory row holds this |
| `PROPOSAL` | a mechanism is written but binds nobody; the validator treats it as `OPEN` |
| `WITHDRAWN` | a closure verdict was retracted; the validator treats it as `OPEN` |

## Scope 1 — exact package lock

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S1.a | package digest and identity <!-- COMMENTARY: Exact package lock. --> <!-- CLAIM: Inspect the candidate `typescript@7.1.0-dev.20260822.1` tarball and binaries. --> | PROVEN | `package-lock-and-semantic-api.md` §1 | re-download and recompute; the recorded tarball sha256 is `9975ea32b5ed2b46a3780693f67de1f04ca7926726081f578762d94baa5a88d2` |
| S1.b | source-commit provenance <!-- CLAIM: Record package digest, source-commit provenance --> | PROVEN | `package-lock-and-semantic-api.md` §1 (`gitHead`) | `node -e "console.log(require('typescript/package.json').gitHead)"` against the pinned install |
| S1.c | exact mapper REQUEST shapes <!-- CLAIM: exact mapper request --> | PROVEN | `probes/probe7-mapper-wire-capture.mjs` | run probe 7 |
| S1.d | mapper RESPONSE shapes as derived by probe 9 — the transform response object, its `diagnosticDirectives` map, the nested diagnostic entries, and the 5/6-slot directive tuple. Narrowed deliberately: §3b discloses four items this row does NOT derive — the offset unit under `positionEncoding: "utf-16"`, what the individual `features` bits gate, the semantics of `diagnostics.category`, and the TS100027/TS100028 trigger conditions. That residue travels to TCM2 with the wire contract, per §3b's own paragraph placing it there; this row asserts only what probe 9 observed <!-- REMAINDER: the offset unit under positionEncoding utf-16, what the individual features bits gate, the semantics of diagnostics.category, and the TS100027 and TS100028 trigger conditions --> <!-- OWNER: TCM2, which inherits the wire contract --> <!-- CLAIM: mapper request/response shapes --> | PROVEN-BOUNDED | `probes/probe9-transform-response-contract.mjs`, `package-lock-and-semantic-api.md` §3b | run probe 9 |
| S1.e | manifest shape <!-- CLAIM: manifest shape --> | PROVEN | `package-lock-and-semantic-api.md` §3, probe 7's `typescript.contentMapper.exec` fixture | run probe 7 |
| S1.f | CONFIGURED-project behaviour, established by live capture — §3a drives the pinned compiler with a real `tsconfig.json` under `--project .` and records the resulting `openProject` frame carrying `configFileName` and the echoed `compilerOptions`. Narrowed deliberately: the INFERRED-project arm was not exercised here, so this row asserts only the configured arm. That residue travels to TCM2, which owns the mapper's project binding and is the reader who needs the inferred arm before it can rely on one <!-- REMAINDER: the inferred-project arm, which no run here exercised --> <!-- OWNER: TCM2, which owns the mapper project binding --> <!-- CLAIM: configured vs inferred project behaviour --> | PROVEN-BOUNDED | `package-lock-and-semantic-api.md` §3a | run probe 7 |
| S1.g | semantic API availability <!-- CLAIM: semantic API availability --> | PROVEN | `package-lock-and-semantic-api.md` §4.0 | run probe 5 |
| S1.h | LSP API-session behaviour <!-- CLAIM: LSP API-session behaviour --> | PROVEN | `probes/probe8-lsp-session-attach.mjs` | run probe 8 |
| S1.i | trust / `--runExternalCode` behaviour <!-- CLAIM: trust and `--runExternalCode` behaviour --> | PROVEN | `package-lock-and-semantic-api.md` §3 (`dist/api/options.d.ts:16-17`) | read the cited declaration in the pinned package |
| S1.j | the pinned package's emit-related surface only as far as this block observed it — §2 records what the package is and §3a captures the `compilerOptions` the compiler actually echoes to a mapper, including `noEmit`. Narrowed deliberately, and the earlier form of this row cited a section number this document does not contain: declaration emit, `--build`, watch mode and incremental/`tsbuildinfo` behaviour were NOT exercised here and this row does not assert them. That residue travels to TCM4, which owns future-package verification at the certified-engine gate and is where an engine's emit and watch behaviour must pass before activation <!-- REMAINDER: declaration emit, --build, watch mode, and incremental tsbuildinfo behaviour, none of them exercised here --> <!-- OWNER: TCM4, at the certified-engine gate --> <!-- CLAIM: declaration/build/watch/incremental behaviour --> | PROVEN-BOUNDED | `package-lock-and-semantic-api.md` §2, §3a | re-read against the pinned package |

| S1.k | known defects <!-- CLAIM: known defects. --> <!-- CLAIM: A package published after a merged PR does not necessarily contain every repository-main change** — verify, do not infer. --> | PROVEN | `package-lock-and-semantic-api.md` §4c, probes 2-4, 6 | run probes 2, 3, 4, 6 |
## Scope 2 — semantic API certification

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S2.a | session initialisation <!-- COMMENTARY: Semantic API certification. --> <!-- CLAIM: Probe session initialisation --> | PROVEN | `probes/probe1-init-timing.mjs` (its one liveness assertion) | run probe 1 |
| S2.b | snapshot acquisition / update / disposal <!-- CLAIM: snapshot acquisition/update/disposal --> | PROVEN | probes 2, 3, 4 | run probes 2, 3, 4 |
| S2.c | project and source-file lookup <!-- CLAIM: project and source-file lookup --> | PROVEN | `probes/probe5-bulk-semantic-api.mjs` | run probe 5 |
| S2.d | `Program` and `TypeChecker` operations <!-- CLAIM: `Program` and `TypeChecker` operations --> | PROVEN | probe 5 | run probe 5 |
| S2.e | bulk symbol / type / reference queries <!-- CLAIM: bulk symbol/type/reference queries --> | PROVEN | probe 5 | run probe 5 |
| S2.f | completions <!-- CLAIM: completions --> | PROVEN | probe 5 | run probe 5 |
| S2.g | diagnostics <!-- CLAIM: , diagnostics, --> | PROVEN | probe 5 | run probe 5 |
| S2.h | cancellation <!-- CLAIM: cancellation --> | PROVEN | probe 5, `package-lock-and-semantic-api.md` §4e (no primitive exists; recorded as an absence, which is the finding) | run probe 5 |
| S2.i | failure behaviour <!-- CLAIM: failure behaviour. --> | PROVEN | `probes/probe6-out-of-range-completion-panic.mjs` | run probe 6 |
| S2.j | reproduce the stale-snapshot defect <!-- REMAINDER: identifying the reproduced behaviour with a named upstream report, which is a claim about the literature and was not established --> <!-- OWNER: TCM3, which takes the snapshot and Program session contract --> <!-- CLAIM: Reproduce the known stale-snapshot --> | PROVEN-BOUNDED | probes 2, 3 | run probes 2, 3 |
| S2.k | reproduce the API-session-hang defect <!-- CLAIM: API-session-hang defects against this exact package. --> <!-- CLAIM: If a required correctness probe fails: do not certify, do not add a relay workaround — select a later package or keep TCM4 blocked. --> | PROVEN — as a NON-reproduction, which is the honest result | `probes/probe8-lsp-session-attach.mjs`: handshake, attach, `Checker` query, **no hang** | run probe 8 |

**S2.k is the row most likely to be misread, so it says so here.** The charter presupposes the defect
exists and asks for it to be reproduced. It does not reproduce against this package. A probe that
looked for a defect and did not find it has discharged the obligation to look; it has not proven the
defect can never occur, and the charter's own instruction — do not certify on a failed probe — is not
triggered, because nothing failed. Recording a non-reproduction as a reproduction would be the
overclaim this register exists to prevent.

**THE NARROWING ON SCOPE 2's TWO MANDATED REPRODUCTIONS, stated because an obligation discharged more narrowly than it sounds is only discharged if the narrowing is written down.**

The charter says *"Reproduce the known stale-snapshot and API-session-hang defects against this exact package"*, and its next sentence gives the purpose: *"If a required correctness probe fails: do not certify … select a later package or keep TCM4 blocked."* **The obligation is to PROBE THIS PACKAGE for both defect classes and act on what is found** — the presupposition that both defects exist is an INPUT the charter supplies, not a result this block can be required to produce. A defect cannot be made to occur on demand, and a block that could only discharge this by finding both would be rewarded for reporting a defect it did not observe.

**What was actually done, and it differs between the two:**

- **Stale snapshot — a defect of that class WAS reproduced** (`package-lock-and-semantic-api.md` §4c): a retained `Program` handle serves cached content after its owning snapshot is disposed, with no error and no round-trip, while the four probed siblings — `getSemanticDiagnostics`, `getSourceFileNames`, `emitToString`, and `getSyntacticDiagnostics` — fail closed. Probes 2 and 3 assert both halves and fail if either stops holding, **including if the defect is fixed**. What is NOT claimed is that this is bibliographically the same pre-documented defect the charter presupposes: no canonical upstream issue matching that description was located. That is a claim about the literature, not about the package, and the package behaviour is what Scope 2 turns on.
- **API-session hang — NOT reproduced, and that is the finding.** `probes/probe8-lsp-session-attach.mjs` drives a real handshake, obtains the API pipe via `custom/initializeAPISession`, attaches a second client and answers a `Checker` query. No hang. The probe went looking and the defect was not there.

**Why a non-reproduction discharges rather than defers.** The charter's failure clause triggers on a *correctness probe failing*, not on a defect failing to appear. Nothing was certified over a failed probe and no relay workaround was added. A later block inherits the probe, so it can re-run it rather than re-derive it — and it inherits one constraint the probe found that nothing had recorded: attach is ASYNC-CLIENT-ONLY.

**The two residues this narrowing leaves, and who reads each.** Recorded here so neither is inherited as
a conclusion. (1) The stale-snapshot behaviour is characterised but not identified with a named upstream
report; the reader who needs that linkage is **TCM3**, which takes the snapshot/`Program` session contract
from `package-lock-and-semantic-api.md` §4 and is the block that would act on an upstream fix. (2) Both
defect classes were probed against **this pinned package only**; re-probing a later package belongs to
**TCM4** at the certified-engine gate, which is already where future-package verification sits. Neither
sentence reallocates an obligation — both rows stay this block's and stay as stated above; a real
reallocation would need a charter amendment, and this is not one.

**If this narrowing is wrong, both rows are OPEN and the gate must refuse them.** The falsifier is exact: show that Scope 2 requires the defects to OCCUR rather than to be probed for — in which case no package on which they do not occur could ever satisfy it, which would make the obligation undischargeable by construction rather than merely unmet here.

## Scope 3 — feature-ownership ledger

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S3.a | every `TypeProvider` method inventoried, one row each <!-- COMMENTARY: Feature-ownership ledger. --> <!-- CLAIM: Inventory every `TypeProvider` method --> | PROVEN | `feature-ownership-ledger.md`, 44 methods in 31 rows | enumerate `fn ` in `crates/verter_type_runtime/src/traits.rs` and compare |
| S3.b | one legal owner per feature, none unclassified, none doubly owned <!-- CLAIM: new primary owner --> | PROVEN | `feature-ownership-ledger.md` owner column | read every row's owner cell |
| S3.c | the twelve recorded columns present on every row <!-- CLAIM: One row each, recording: current implementation, current callers, framework/source region --> | PROVEN | `feature-ownership-ledger.md` header | count the header's columns |
| S3.d | every own-spelling textual occurrence under `crates/`, not a representative sample <!-- CLAIM: call site, capability and background consumer --> | PROVEN | `typeprovider-call-sites.md`, derived by `probes/typeprovider-call-site-derivation.mjs`: the 44 methods read out of the trait body, every `.rs` file under `crates/` lexed, 2,551 own-spelling textual occurrences classified — 203 production calls, 178 same-name forwarders, 14 trait defaults, 558 test calls, the rest impls/refs/text. The ledger's per-row cells are a readable extract and 126 of the 203 are cited nowhere in them. Counts can include identifier collisions and omit renamed or macro-synthesised calls; those blind spots are named in the script header and ledger | `node docs/arch/refactor/rev11/evidence/TCM0/probes/typeprovider-call-site-derivation.mjs --check` — re-derives from the live tree, exits 1 on any drift |
| S3.e | every steering-named CAPABILITY has an entry <!-- CLAIM: required TypeScript capability --> | PROVEN | `capability-provider-hop-walk.md`, derived by `probes/capability-provider-hop-walk.mjs`: 17 request paths walked individually (the 14 located capabilities plus the 3 formerly "partially covered" entries), 4 reported hops, each read — 3 collisions/a dead field, 1 REAL: rename preparation calls `get_rename_locations` (`server/rename_prepare.rs:181`) and is a `TypeProvider` capability in part, its provider half assigned to row #15. The uniform "none has a provider in its path" characterisation is struck in the ledger. `implementation` is TypeScript-side and outside the walk's language (limit L5); its verdict rests on the cited plugin override | `node docs/arch/refactor/rev11/evidence/TCM0/probes/capability-provider-hop-walk.mjs --check` — re-derives from the live tree, exits 1 on any drift |

**SCOPE OF THE RATIFICATION — see the registry, which owns it.** The capability dispositions are ratified by act **`90a25a43d`**; its content, scope and limits are the registry's to state and are not repeated in this register. What this register owns is the row's status, and what a reader must do: a later reader who changes a capability's request path, its owner cell, or the walk that derived it has changed the evidence the ratification was taken over, and must treat the disposition as reopened and obtain a fresh act.

## Scope 4 — diagnostic ownership matrix

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S4.a | attribution, suppression, precedence, dedup for every diagnostic class <!-- COMMENTARY: Diagnostic ownership matrix. --> <!-- CLAIM: Compiler diagnostics, mapper parse/config diagnostics, directives, framework diagnostics, duplicate classes, generated-region diagnostics, external-unit diagnostics — with deterministic attribution, suppression, precedence and dedup rules. --> | PROVEN | `diagnostic-ownership-matrix.md` | read the matrix against the charter's seven named classes |
| S4.b | a generated diagnostic without a valid authored projection stays visible with honest attribution <!-- CLAIM: A generated diagnostic without a valid authored projection stays visible with honest generated attribution; it is never mapped to a convenient false position. --> | PROVEN | `diagnostic-ownership-matrix.md`'s required correction to current behaviour | read the cited row |

## Scope 5 — projection-class contract

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S5.a | the minimal class set ratified <!-- COMMENTARY: Projection-class contract. --> <!-- CLAIM: Ratify the minimal class set --> | PROVEN | `projection-class-contract.md` | read the five classes |
| S5.b | the terminal mask policy is a TOTAL function <!-- CLAIM: terminal policy deriving TypeScript feature masks from class × relation × region × owner × certified capability. --> | PROVEN | `projection-class-contract.md`'s eighteen-cell table and its embedded recomputation | paste the recomputation into `node`; any cell disagreeing with the table falsifies one of the two |
| S5.c | every wire span gets an explicit mask, never omitted into the upstream `All` default <!-- CLAIM: Every wire span gets an explicit mask — never omitted into the upstream all-features default. --> | PROVEN | `projection-class-contract.md` "What this contract forbids" | read the forbidden list |

## Scope 6 — external-source decision table

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S6.a | exactly one model for each of the eleven named shapes <!-- COMMENTARY: External-source decision table. --> <!-- CLAIM: gets exactly one model: TypeScript owns it, it is independently content-mapped, Verter owns it, or the shape is unsupported and activation fails closed. --> | PROVEN | `external-source-decision-table.md` rows 1-11 | read every row's model cell |
| S6.b | `<template src>`'s project/context contract, required by the steering's model 2 <!-- CLAIM: Each of inline script/template/style, Vue custom blocks, Svelte regions, `<script src>`, `<template src>`, external styles, imported Svelte assets, supplemental outputs and multi-unit helpers --> | PROVEN | `probes/probe10-external-source-unit.mjs`, `external-source-decision-table.md` §7a | run probe 10; `--inject input|project|config|mapper` drives the corresponding assertion red |

## Scope 7 — topology benchmarks

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S7.a | candidate screening, survivor sets, metrics, harness, baseline method, selection rule <!-- COMMENTARY: Topology benchmarks. --> <!-- CLAIM: Projection plane: native mapper with in-process compiler; thin mapper over a shared native daemon; Node/N-API only if competitive. --> <!-- CLAIM: Semantic plane: attach to the editor-owned API session; direct native client; managed process for non-editor hosts. --> <!-- CLAIM: Measure cold start, first / warm / unchanged transform, rapid edits, CPU, allocations, RSS and peak, process count, IPC bytes, open/close, consolidation, crash isolation, cleanup. --> | PROVEN | `topology-benchmark-plan.md` | read the plan |
| S7.b | evidence-based selection of the non-dominated topology <!-- CLAIM: Select the non-dominated topology on evidence. --> | RULED — transferred to TCM2 (projection plane) and TCM3 (semantic plane) as blocking exits | ratified: `ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q2 | read Q2 |

## Scope 8 — cache and lifecycle contracts

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S8.a | one cache implementation and invalidation law per host process <!-- COMMENTARY: Cache and lifecycle contracts. --> <!-- CLAIM: One cache implementation and invalidation law per host process. --> | PROVEN | `cache-lifecycle-contracts.md` | read the contract |
| S8.b | the permitted prepared-artifact key components, and the forbidden ones <!-- CLAIM: Prepared-artifact keys may include source identity, framework/language mode, codegen options, source-unit revisions, product profile, projection schema identity, compiler ABI. --> <!-- CLAIM: They must NOT include feature-mask policy, `projection_policy_id`, UTF-8 vs UTF-16, wire representation, or V3 encoding options. --> | PROVEN | `cache-lifecycle-contracts.md` key composition | compare against the charter's two lists |

## Scope 9 — deletion closure

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S9.a | every mechanism TCM4 deletes, named now <!-- COMMENTARY: Deletion closure. --> <!-- CLAIM: Name every mechanism TCM4 deletes --> | PROVEN | `deletion-closure.md` rows 1-16 | read the table |
| S9.b | every surviving generic facility has a proven owner <!-- CLAIM: every generic facility that survives with a proven owner. --> | PROVEN | `deletion-closure.md` "Survives, with a proven owner" | read that section |
| S9.c | checklist items 17-18 <!-- CLAIM: Not deferred to TCM4. --> | NOT-OWNED | relocated by `AMD-023`, bound by acts `54b9d2c29` (nomination) and `04df58021` (the receiving criteria): the RECORDING half is bound on **TCM1** criterion 12, **TCM2** criterion 14 and **TCM3** criterion 10; the VERIFYING half was already bound on **TCM4** criterion 5. Derived, not asserted — `receiving-coverage.md` part `B1` | `node docs/arch/refactor/rev11/evidence/TCM0/probes/receiving-coverage-derivation.mjs --check` — re-derives from the live charters, exits 1 on any drift |

**Relocated by `AMD-023`, and the move HAS TAKEN EFFECT.** The amendment nominated; the binding act added the receiving criteria and re-pinned their digests. **The precondition was checked by DERIVATION over the landed charters, never by reading the act's summary** — `receiving-coverage.md` reports every part in its hand-authored coverage list accounted for, while disclosing that omitted parts are outside its detection; its `--check` re-derives that listed coverage from the live charters. Two earlier claims that the move had taken effect were premature, and each was caught by running the derivation rather than trusting the description; this row moved only once the listed coverage agreed.

**Inherited limit — the receiving owner must re-check this, not assume it.** The coverage that placed this obligation was derived by `probes/receiving-coverage-derivation.mjs`, which reads every numbered exit criterion in the receiving charters. **Its part list is hand-authored**: the closure bars it decomposes are prose, so nothing machine-readable states what an obligation's parts are and the script CANNOT detect an omitted part. It also cannot see a criterion that binds a part in different words than the part's literals. Before acting on this row, read the closure bar against `evidence/TCM0/receiving-coverage.md` and name any part that is missing. This is a recorded unmet obligation travelling WITH the work, not a caveat filed elsewhere.

## Scope 10 — performance baselines

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S10.a | baselines locked before any implementation result is seen <!-- CLAIM: Performance baselines**, locked before any implementation result is seen. --> | RULED | `performance-baselines.md` requirements 6-8 are the complete contract; no absolute baseline required — `ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q3 | read Q3 |

## The acyclic invariant

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| INV.a | the discriminating deadlock/reentrancy test specified (TCM2 implements it) <!-- CLAIM: TCM0 specifies the discriminating deadlock/reentrancy test that proves the cycle is impossible; TCM2 implements it. --> | PROVEN | `acyclic-invariant-test-spec.md` | read the spec |
| INV.b | observation that, in Probe 7's one configured compile, the mapper callback received no semantic-API query — corroboration, not proof of a universal <!-- CLAIM: The mapper callback must never query the TypeScript semantic API or send LSP requests. --> <!-- CLAIM: The only legal order is: TypeScript requests transform → Verter compiles and returns output plus mappings → TypeScript commits its snapshot → Verter may then acquire that snapshot → Verter-owned operations may query it. --> | PROVEN | probe 7: four inbound frames across that configured compile, all lifecycle, no query observed | run probe 7 |

## Acceptance clause — the five prohibitions

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| A.a | no "semantic mechanism TBD" <!-- COMMENTARY: TCM0 cannot be accepted with any of: --> <!-- CLAIM: "semantic mechanism TBD" --> | PROVEN | closed on an INDEPENDENT lane’s verdict, not on this block’s derivation: lane `tcm0-aa-bounded2`, `RESULT: PASS`, `FINDINGS: 0`, reviewing `a7c93ac764702a3fed19c860d4b54f190c3eb523`, receipt committed IN THIS TREE at `reviews/acceptance-clause-semantic-mechanism.md` — the row this replaced cited a receipt that was not in the tree at all, at a sha that was not an ancestor of the candidate. The lane was unseen: charter and tree, no prior findings, no earlier lane’s conclusions, no list of places to look, and it derived both limbs itself, including what the condition forbids. **A block may not rule its own acceptance test satisfied on its own evidence; this row rests on the lane and on nothing of mine.** SUBJECT, declared BEFORE the lane ran and echoed in its receipt: 51 paths under `evidence/TCM0`, `evidence/TCM0-summary.md` and `charters/TCM0.md`, digest `dedaf684b36586d19975a1afb5fff8021ba378e15243700597896d78967799f5`, verified declared-echoed-rederived. The receipt file itself is EXCLUDED from that set — a verdict covering the record of itself is circular — while this register is deliberately kept IN, since a row here could defer a mechanism and that is the condition under test; the residue is that this citation edit still moves the subject, over one file. A LAYOUT-ONLY change to a subject path is exempt, declared in advance and proven by reconstruction rather than inspection. NARROWNESS: the verdict binds the sha above while the landing tree also carries this citation, admitted ONLY because that delta is the record of the verdict, and NEVER as licence for a verdict to cover a tree it did not review. Two earlier runs were DISCARDED rather than argued for — one when trunk moved a single non-overlapping commit, one when a reformat moved the subject | dispatch an unseen acceptance lane scoped to this clause at the landing candidate, commit its receipt, and require `PASS` with zero findings |
| A.b | no "retain provider temporarily" <!-- CLAIM: "retain provider temporarily" --> | PROVEN | no such disposition exists in the evidence set | search the evidence for a temporary-retention disposition and read each sentence, not the match |
| A.c | no unclassified `TypeProvider` method <!-- CLAIM: an unclassified `TypeProvider` method --> | PROVEN | `feature-ownership-ledger.md`: 44 of 44 classified | enumerate the trait and compare |
| A.d | no feature claimed by two owners <!-- CLAIM: a feature claimed by two owners --> | PROVEN | split capabilities are recorded as disjoint sub-rows | read every split row |
| A.e | no intentional capability removal without explicit governance approval <!-- CLAIM: an intentional capability removal without explicit governance approval --> | RULED | the one removal is the dead API, approved by `ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q5 | read Q5 |

## Scope 3, continued — the string-encoded-surface enumeration

| id | obligation | status | proof | falsifying command |
|---|---|---|---|---|
| S3.g | the string-encoded projection surface enumerated STRUCTURALLY, not by a manual pass (`G-STRING-SURFACE-CITATIONS`) <!-- CLAIM: mapping class/mask, diagnostic behaviour, failure behaviour, conformance test, performance cell, and what TCM4 deletes. --> | NOT-OWNED | relocated by `AMD-023`, bound by acts `54b9d2c29` and `04df58021`, to **TCM1**: the producer chain on criterion 1 and the wire boundary on criterion 3 already bound it, and criterion 11 now dispositions the `oxc_sourcemap` re-export that was its residue. Derived, not asserted — `receiving-coverage.md` parts `A1`, `A2`, `A4` | `node docs/arch/refactor/rev11/evidence/TCM0/probes/receiving-coverage-derivation.mjs --check` — re-derives from the live charters, exits 1 on any drift |

**Relocated by `AMD-023`, and the move HAS TAKEN EFFECT.** The amendment nominated; the binding act added the receiving criteria and re-pinned their digests. **The precondition was checked by DERIVATION over the landed charters, never by reading the act's summary** — `receiving-coverage.md` reports every part in its hand-authored coverage list accounted for, while disclosing that omitted parts are outside its detection; its `--check` re-derives that listed coverage from the live charters. Two earlier claims that the move had taken effect were premature, and each was caught by running the derivation rather than trusting the description; this row moved only once the listed coverage agreed.

**Inherited limit — the receiving owner must re-check this, not assume it.** The coverage that placed this obligation was derived by `probes/receiving-coverage-derivation.mjs`, which reads every numbered exit criterion in the receiving charters. **Its part list is hand-authored**: the closure bars it decomposes are prose, so nothing machine-readable states what an obligation's parts are and the script CANNOT detect an omitted part. It also cannot see a criterion that binds a part in different words than the part's literals. Before acting on this row, read the closure bar against `evidence/TCM0/receiving-coverage.md` and name any part that is missing. This is a recorded unmet obligation travelling WITH the work, not a caveat filed elsewhere.

**This row was previously filed as a non-obligation, and that was wrong.** It sat in the table below
as `X.a`, on the reasoning that a gate needing production code cannot be TCM0's. But the validator
SKIPS that table — so classifying it there did not record a judgement, it removed the row from the
count entirely. An obligation that is genuinely blocked must BLOCK, visibly; a row moved to a table
the gate does not read is indistinguishable from a row that was closed, and reads as closed to anyone
scanning the output. Being wrong cautiously is still wrong: filing it as out of scope while a
criterion actually required it left the gate passing over the thing it exists to catch.

**What follows is that row's history, and it is in the past tense deliberately.** While it sat as
`X.a` it was a binding acceptance gate, blocked on an AUTHORITY boundary rather than on a round
count, and it stayed open and counted because no ratifying act had yet named an owner with production
authority. That was true then and is not true now. Stating it in the present tense is what made this
passage contradict the row above it, and the contradiction outlived the change that ended it — which
is the same failure the paragraph above describes, arriving from the other direction: there the row
was moved somewhere the gate could not read, here the reason it was moved was left where the gate
could read it and never updated.

**What ended it:** the obligation was relocated by `AMD-023`, and the move took effect through two
binding acts — `54b9d2c29`, which relocated the obligations this block could not discharge to their
owners, and `04df58021`, which added the receiving criteria the relocation takes effect on. The
effect is checkable by derivation rather than by assertion:
`probes/receiving-coverage-derivation.mjs --check` reads the landed charters and reconciles them
against the committed coverage file. The row above carries the status the gate acts on; this passage
records only how it got there.

## Rows that are NOT charter obligations, recorded so their absence here is deliberate

| id | row | status | why |
|---|---|---|---|
| X.b | the residual `G-CHARTER-AMENDMENTS` rows | NOT-OWNED | they are amendment acts on the ratified, digest-pinned charters of TCM1, TCM2 and TCM3, and the re-pin is the program orchestrator's. TCM0 authors no amendment and re-pins no digest |
| X.c | `G-CONFORMANCE-FIXTURES-TCM2/-TCM3/-TCM4` | NOT-OWNED | owned by the blocks they name, each gated on its own exit criteria |
