# TCM0 — Current TypeScript contract and dual-plane architecture lock

Read-only investigation and architecture lock, per `charters/TCM0.md` and
`amendments/DISC-2026-08-22-TYPESCRIPT-CONTENT-MAPPERS-amendment.md`. TCM0 changes no production route,
ships no code, certifies no package by assumption, and deletes nothing — every finding below is against
bytes actually inspected or a probe actually run, not against documentation alone, per the charter's own
warning that a published package does not necessarily contain every repository-main change.

**This is a decision-record block. It changes no file under `crates/`, `packages/`, or `scripts/`.**
Every architectural decision it locks is executed by a later, separately-authorized block (TCM1-TCM4,
each requiring its own digest-bound authority-registry record before dispatch).

## THIS IS A NON-ACCEPTANCE EVIDENCE PACKAGE

`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 reviewed this block's round-3 candidate and **returned it as wrongly scoped**. What lands here is the
accurate portion of that work — the source-backed inventories, the executable probes, and their
transcript — as **evidence only**:

- **TCM0 is NOT accepted by this package.** Nothing here closes TCM0's acceptance gates, and nothing here
  claims the contract lock is complete.
- **The incomplete contract remainder is a SUCCESSOR BLOCK with fresh verification**, explicitly not a
  round 4 of TCM0. Its scope — the five surviving gates and the residual charter-amendment rows, with what
  each must show an independent verifier — is written up in
  [`TCM0/successor-block-scope.md`](TCM0/successor-block-scope.md). Authorizing, chartering, digest-binding
  and dispatching that block are the program orchestrator's and the maintainer's acts.
- **Three exclusions are applied**, per the same ruling: the candidate's `program-state.toml` hunk is
  reverted (Q8 — the program orchestrator, as sole ledger writer, records the corrected notes on trunk);
  the candidate's `ADR-021` changes are reverted (so `ADR-021` stands at its ratified text and does NOT
  carry this block's probe findings); and every summary/gap/disposition passage claiming the superseded
  Q2-Q7 state is rewritten from the rulings.
- **What the ruling settled** and this package therefore does not re-litigate: the topology-selection
  transfer to TCM2/TCM3 (Q2, ratified); the performance contract (Q3 — requirements 6-8 are complete, no
  absolute baseline required); ledger rows 25 and 26 (Q4, retained); the dead `get_diagnostics_background`
  surface (Q5, ruled for deletion by a later code-bearing slice — not performed here); diagnostic-mapper
  convergence ownership (Q6 — TCM3 already owns it, no new block, divergence disclosed until TCM3 lands);
  and transcript staleness (Q7, acceptable). See `TCM0/OPEN-GAPS.md` for the row-by-row disposition.

## The evidence

| artifact | what it resolves |
|---|---|
| [`decisions/ADR-021-typescript-content-mapper-dual-plane.md`](../decisions/ADR-021-typescript-content-mapper-dual-plane.md) | the ratified architecture decision this investigation locks |
| [`TCM0/package-lock-and-semantic-api.md`](TCM0/package-lock-and-semantic-api.md) | charter items 1-2 — exact candidate package identity/provenance, the content-mapper protocol confirmed present in the actual downloaded bytes, and semantic-API certification: session init/disposal/stale-Program/cancellation-absence plus the full bulk symbol/type/reference/completion/diagnostic surface are all live-probed against the pinned candidate, with executable probes and their transcript committed under `evidence/TCM0/probes/`. The probes and transcript land as EVIDENCE; `G-SEMANTIC-API-CERTIFICATION` stays OPEN with the successor block as owner, since no ruling decides whether the accepted block must itself run charter item 2's bulk probes. §6 records five new constraints binding on TCM2/TCM3 |
| [`TCM0/mapping-products-string-surface.md`](TCM0/mapping-products-string-surface.md) | a best-effort (explicitly not claimed exhaustive — two manual passes each found the prior one incomplete) inventory of the string-encoded projection surface `source_projection_map()` represents, a correction to the amendment's own citation, and the acceptance bar this hands to TCM1 |
| [`TCM0/feature-ownership-ledger.md`](TCM0/feature-ownership-ledger.md) | charter item 3 — all 44 `TypeProvider` METHODS (31 ledger rows; 8 priority-tier variants folded into their base method's row), one owner each from the four legal owners, zero left unclassified. Rows #25-26 are RETAINED under `VerterWithTypeSemanticOracle` (`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q4), and row #31's `DisabledByExplicitApprovedContract` label is REJECTED with the row ruled for deletion by a later code-bearing slice (Q5 — not performed by this read-only block). Method coverage is NOT capability coverage: the closure section records 14 steering-named capabilities that are not `TypeProvider` capabilities at all (located to `file:line`), goto-implementation re-verdicted as served by the typescript-plugin carrier-routing override rather than by any `TypeProvider` method (corrected 2026-08-24; previously recited as proven absent), and six wrong per-row citations corrected — one of which inverted the premise the rows #25-26 deletion argument rested on, and is the finding Q4's retention followed |
| [`TCM0/diagnostic-ownership-matrix.md`](TCM0/diagnostic-ownership-matrix.md) | charter item 4 — attribution/suppression/precedence/dedup for every diagnostic class, including one required correction to current behavior (generated-region diagnostics must surface with honest attribution, not be silently dropped as today's code does) |
| [`TCM0/projection-class-contract.md`](TCM0/projection-class-contract.md) | charter item 5 — the minimal class set and the terminal mask-derivation policy, built directly on the upstream `SpanMapFeature`/`SpanMapKind`/`SpanMapFidelity` wire primitives confirmed present in the candidate |
| [`TCM0/external-source-decision-table.md`](TCM0/external-source-decision-table.md) | charter item 6 — one model each (TS-owned / content-mapped / Verter-owned / unsupported) for all 11 named external-source shapes |
| [`TCM0/topology-benchmark-plan.md`](TCM0/topology-benchmark-plan.md) | charter item 7 — the plan and harness for both planes' topology candidates; explicitly no comparative numbers produced yet. Candidate screening, survivor sets, metrics, harness, baseline method and selection rule are TCM0's and are decided on structural evidence; evidence-based comparative selection is TCM2's (projection plane) and TCM3's (semantic plane) as a **blocking exit of each block**, RATIFIED by `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q2 — not a pending charter amendment, and not a TCM0 acceptance precondition |
| [`TCM0/cache-lifecycle-contracts.md`](TCM0/cache-lifecycle-contracts.md) | charter item 8 — one cache/invalidation law per concern, built on the candidate's own confirmed ref-counted snapshot cache rather than a second parallel scheme |
| [`TCM0/deletion-closure.md`](TCM0/deletion-closure.md) | charter item 9 — six concrete deleted mechanisms and their survivors with proven owners. Ledger rows #25-26 are RETAINED (Q4), removable only after TCM3 supplies and tests equivalent semantics; `get_diagnostics_background` and row 31 are ruled for deletion (Q5) by a later code-bearing slice. Items 17-18's accumulation-at-creation mechanism lands as a proposal, its closure verdict withdrawn to the successor block |
| [`TCM0/performance-baselines.md`](TCM0/performance-baselines.md) | charter item 10 — thresholds locked from evidence gathered in this investigation, explicitly excluding any number an implementation this program hasn't built could only supply. Requirements 6-8 ARE the complete Scope-10 performance contract and **no dedicated-machine absolute baseline is required** (`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q3); independently owned correctness and lifecycle gates remain applicable |
| [`TCM0/acyclic-invariant-test-spec.md`](TCM0/acyclic-invariant-test-spec.md) | the discriminating deadlock/reentrancy test specification this charter requires TCM0 to write and TCM2 to implement |
| [`TCM0/tcm1-tcm4-charter-refinements.md`](TCM0/tcm1-tcm4-charter-refinements.md) | recorded refinements for a future amendment to apply to the still-LOCKED TCM1-TCM4 charters — those charter files are not edited by this block |

## What the investigation found that the amendment/discovery text got wrong

The ratified amendment and discovery documents both cite `checker.rs:411` as calling
`PositionMapper::from_json(… .unwrap_or(""))`. **Verified false as written**: that exact call exists only
in a test file (`kebab_tag_mapping_full_columns.rs:65`); `checker.rs:411` instead base64-encodes the
string directly into an inline `sourceMappingURL` comment for `tsc`/`tsgo` to parse independently. The
amendment's underlying thesis — the surface is string-encoded and must become typed — is unaffected;
only the specific citation needed correcting, and the true extent of the string-encoded surface turned
out to be considerably WIDER than the two cited lines — at least 36 struct/enum-variant fields across
`verter_compiler`, `verter_protocol`, `verter_session`, and `verter_lsp`, and explicitly NOT claimed
exhaustive after two manual passes each found the prior one incomplete
(`TCM0/mapping-products-string-surface.md`).

## What the investigation found that no document had stated at all

- **The npm `typescript` package no longer contains a compiler.** `typescript@7.x` ships only a thin
  JS/TS API client; the actual compiler/checker/language service is a separate native Go binary resolved
  through `optionalDependencies`. Every topology candidate in charter item 7 spawns or attaches to the
  SAME native binary — the "light JS engine vs heavy native engine" choice this investigation might have
  expected to evaluate does not exist.
- **The content-mapper protocol genuinely exists in the exact candidate bytes**, not merely in the
  upstream PR text — confirmed by disassembling the downloaded native binary and finding its
  `internal/contentmapper` Go symbol table (`OpenProjectParams`, `TransformParams`, `CloseProjectParams`,
  `InitializeResult`, `handshake`, etc.), matching the four-step `Initialize`→`OpenProject`→`Transform`→
  `CloseProject` lifecycle the upstream design describes. The exact literal wire method-name spelling
  could not be isolated from a stripped binary via static `strings` extraction — initially recorded as an
  open gap for TCM2, since CLOSED by live capture: `TCM0/probes/probe7-mapper-wire-capture.mjs` records
  every frame (`initialize`/`openProject`/`transform`/`closeProject`, §3a); the residual with TCM2 is the
  `transform` RESPONSE body layout only.
- **A genuine, reproduced defect, live-probed against the exact candidate**: a `Program` handle obtained
  from a `Snapshot` continues to silently serve cached, stale content after that `Snapshot` is disposed —
  with zero error and zero server round-trip — while every sibling `Program` method fails closed
  correctly (`"snapshot N not found"`) in the identical post-dispose state. Root cause located in the
  shipped client source (`SourceFileCache`'s ref-counting deliberately skips release for a still-latest
  disposed snapshot). This becomes a required TCM3 design constraint, not an open question.
- **The session-attach topology candidate (`API.fromLSPConnection`) was initially NOT probed for a hang**
  — recorded honestly as an untested gap rather than either a false certification or a false alarm — and
  was since probed by `TCM0/probes/probe8-lsp-session-attach.mjs`: a real LSP handshake, attach over the
  API pipe, and a `Checker` query answered with **no hang**, plus a constraint nothing had recorded — the
  attach topology is ASYNC-CLIENT-ONLY (`package-lock-and-semantic-api.md` §4a-attach).

## Decisions taken out of TCM0's hands, and where they landed

None of TCM0's own findings require ratification to RECORD (that is exactly what this investigation is
for). Two items were carried here as explicitly NOT TCM0's to decide. Both have since been decided by
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`:

| id | decision | outcome | artifact |
|---|---|---|---|
| TCM0-G1 | Whether `register_carrier_member`/`register_carrier_metadata`/`activate_carrier_member(s)` are deleted or retained. An earlier packet argued their function is fully subsumed by the content mapper's own identity fields; TCM0 found that premise factually inverted — the implementation is tsserver-family and hydrates a content cache, not a tsgo relay artifact | **RETAINED** (Q4), both rows under `VerterWithTypeSemanticOracle`: row 25 preserves local content/position conversion and carrier-to-project routing, row 26 preserves oracle working-set activation. TCM4 may remove the tsserver-specific methods **only after TCM3 supplies and tests equivalent semantics** | `feature-ownership-ledger.md` rows #25-26, `deletion-closure.md` |
| TCM0-G2 | Whether the topology-selection reallocation needed a ratified `TCM0.md` Scope-item-7 amendment before TCM0 acceptance | **DISCHARGED** (Q2): the transfer is ratified by ruling — TCM0 owns candidate screening, survivor sets, metrics, harness, baseline method and selection rule; TCM2 and TCM3 own evidence-based projection- and semantic-topology selection as blocking exits of their own blocks. No charter amendment gates TCM0 acceptance for it. The TCM1/TCM2/TCM3 amendment rows that derive from the withdrawn `G-STRING-SURFACE-CITATIONS` and `G-DELETION-CLOSURE-ITEMS-17-18` closures transfer to the successor block | `TCM0/OPEN-GAPS.md` §`G-CHARTER-AMENDMENTS`, `TCM0/tcm1-tcm4-charter-refinements.md` |

Charter amendment, digest re-pinning and `authority-registry.toml` edits remain the program
orchestrator's and the maintainer's acts; this block performs none of them.

## Claims this block does not make

- ~~**No claim that the exact JSON-RPC method-name spelling for the content-mapper protocol matches the
  literal strings `initialize`/`openProject`/`transform`/`closeProject`.**~~ WITHDRAWN as a non-claim: a
  byte-exact wire trace WAS subsequently produced (`TCM0/probes/probe7-mapper-wire-capture.mjs`), so the
  block now does claim the spelling. The surviving non-claim is narrower: no claim about the `transform`
  RESPONSE body layout, which stays with TCM2.
- ~~**No claim that the `API.fromLSPConnection` session-attach path is free of the "API-session hang"
  defect class the charter names.**~~ WITHDRAWN as a non-claim: the attach path WAS subsequently
  live-probed (`TCM0/probes/probe8-lsp-session-attach.mjs` — handshake, attach, `Checker` query, no
  hang). The surviving non-claims are narrower: no claim beyond the probed query surface, and the path is
  ASYNC-CLIENT-ONLY with a bind race requiring bounded retry (`package-lock-and-semantic-api.md`
  §4a-attach).
- **No claim that the reproduced stale-`Program` defect (§4c of `package-lock-and-semantic-api.md`) is
  THE SAME pre-documented defect the charter presupposes exists.** No canonical upstream issue matching
  that exact description was located during this investigation (a WebSearch pass found related but
  non-identical issues). What is claimed is narrower and fully evidenced: this specific behavior was
  reproduced, its root cause located in source, and its consequence recorded as a design constraint —
  independent of whether it is the same defect some other, unlocated report already describes.
- **No claim that the feature-ownership ledger's owner assignments are the ONLY architecturally valid
  split.** Several rows split across two owners with a stated discriminant (e.g. plain-TS vs.
  Verter-authored code actions); a later block may find a different split correct, but every row is
  classified — none is left as "TBD," satisfying the acceptance bar without claiming finality on judgment
  calls that are legitimately TCM1-TCM3's to refine.
- **No claim of comparative topology performance — and none is owed.** `topology-benchmark-plan.md` is a
  plan; the only numbers in this investigation are single-topology reference points, explicitly marked as
  such. Comparative topology numbers are not a TCM0 deliverable: `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
  Q2 ratifies TCM0's ownership of candidate screening, survivor sets, metrics, harness, baseline method
  and selection rule, and assigns evidence-based projection- and semantic-topology selection to TCM2 and
  TCM3 as blocking exits of their own blocks.
- **No claim that TCM0 is accepted, or that its acceptance gates are closed.** See the non-acceptance
  banner above: this is an evidence package, and the incomplete contract remainder is a successor block
  with fresh verification (`TCM0/successor-block-scope.md`).

## Verification

Per the charter, TCM0 runs no gate, no cargo test suite, no build slot — this is an investigation block.
Every factual claim is re-derivable from the commands recorded beside it in
`TCM0/package-lock-and-semantic-api.md`'s Reproduction section and the individual `grep`/`Read` citations
threaded through each evidence file. The load-bearing ones:

| claim | command/method |
|---|---|
| candidate package identity, provenance, dist-tag | `curl https://registry.npmjs.org/typescript/7.1.0-dev.20260822.1`; `curl https://registry.npmjs.org/typescript` for the full dist-tags table |
| tarball integrity | `shasum -a 1`/`shasum -a 256` on the downloaded tarball, compared against the registry's `dist.shasum`/`dist.integrity` |
| content-mapper protocol presence in the exact candidate | `strings -a` on the downloaded `@typescript/typescript-darwin-arm64@7.1.0-dev.20260822.1` native binary, grepped for `internal/contentmapper.*` |
| `checker.rs:411` citation correction | `grep -n "PositionMapper" crates/verter_tsc/src/checker.rs` (no hits) vs. direct `Read` of the actual base64/`sourceMappingURL` code at that line |
| stale-`Program`-after-dispose defect | four Node probe scripts run live against `npm install typescript@7.1.0-dev.20260822.1` in a scratch directory on this host, root-caused against the shipped `dist/api/sync/api.js` source |
| `TypeProvider` trait exhaustiveness (44 methods, 31 ledger rows) | `crates/verter_type_runtime/src/traits.rs:130-512`, cross-checked by grep for every method name across `crates/*/src` excluding test files; independently re-verified by a review pass that direct-enumerated every `fn` in the trait body |
| diagnostic/external-source current-state claims | file:line citations embedded directly in `diagnostic-ownership-matrix.md` and `external-source-decision-table.md`, independently re-`grep`/`Read`-able |
| authority-registry digest binding | `python3 -c "import hashlib; print(hashlib.sha256(open('charters/TCM0.md','rb').read()).hexdigest())"` matched byte-for-byte against `authority-registry.toml`'s recorded `sha256` for `TCM0-CHARTER` and the amendment document, before any work began |

Guards whose scan surface includes this content: `tracked_paths_are_portable` (path-shape enforcement,
generic); `every_critical_rule_in_docs_has_registered_guard` reads `CLAUDE.md`/`.claude/skills/*/
SKILL.md` only, so it does not see this directory — named because it is the guard a reader would assume
covers it, per the same honest-accounting convention A5 established.
`no_phase_archaeology_in_production_code` scans `crates/*/src/**` only and does not see this directory
either; TCM0 touches no production source, so the program-vocabulary prohibition on source is honoured
by not applying here at all.
