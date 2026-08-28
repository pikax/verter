# Reviews of the exhaustion-closure delta

Two EXTERNAL CLI seats, run SEQUENTIALLY in one worktree — concurrent seats planting
mutations in a shared tree cross-contaminate each other's results. No seat authored any of
the text or code it reviewed, and each was required to author its OWN plants rather than
re-run the ones the delta records.

- **Codex** `gpt-5.6-sol`, reasoning effort `high` — two rounds: the whole delta, then the
  fix delta its round-1 findings produced.
- **Grok** 4.6, reasoning effort Extra High, with an explicit default-to-BLOCK posture, on
  the delta as fixed.

Both prompts were neutral ("is this correct?"), required a verdict on charter procedure
item 4 with citations, and stated that `NOT-EVIDENCED` is a legitimate answer.

## Outcome

| | claim 1 — the recompile write | claim 2 — the AT-2 amendment | item 4 |
|---|---|---|---|
| Codex (round 2) | PASS | **BLOCKING on authority**, substance resolved | EVIDENCED |
| Grok | PASS | PASS | EVIDENCED |
| Codex (confirm round, after the naming act) | — | PASS, Finding 2 DISCHARGED | item 6 EVIDENCED |

**Both seats agree** the recompile WRITE is attributed (each proved it with its own plants
separating the CALL from the WRITE), that item 4 is EVIDENCED, and that AT-2's ORIGINAL
claim is not a demonstrated defect.

**They split on one thing: whether the AT-2 amendment is authorized.** The maintainer's act
is a general standing ruling that does not name `AT-2`; applying it to that row is the
program orchestrator's reading. Grok reads that as sufficient; Codex does not, and asks for
an explicit maintainer act naming `AT-2` or a revert of the `AT-2` row and the matching
`BA0` obligation lines — under which item 6 returns to `NOT-EVIDENCED`. That objection was
a governance decision above track level, it was **correct**, and it is now **RESOLVED** by
the first of the two remedies it named: the maintainer issued an act that names `AT-2`. See
[Resolution of the authority objection](#resolution-of-the-authority-objection) at the end
of this file. The reports below are unedited; the resolution is recorded beneath them, not
folded into them.

Round 1 found four defects, all real: the AT-2 artifact was mislabelled and did not
discriminate what its name claimed; the "`buildStart` reaches `getVirtualFile` at exactly
ONE place" claim was false (the Svelte branch's compiled-style read is a second in-hook
site); and the record contradicted its own evidence index on item 6. Three were resolved at
HEAD; the fourth is the authority objection above, resolved afterwards by the maintainer
act recorded at the end of this file.

All reports are reproduced verbatim below.

---

## Codex — round 1 (the whole delta)

```
VERDICT: BLOCKING

PER-CLAIM  
claim 1: PASS — ran transport suite: 22 tests, 21 passed/1 ignored; shipped probe passed; exact-return plant went RED. The tested Vue-only fixture attributes the child call and cache write, although the “only reachable call” prose is false globally.  
claim 2: BLOCKING — batch suite passed 10/2 ignored and the ignored target passed once, but it also passed after replacing the production diagnostic filter with `Vec::new()`; the cited act does not explicitly authorize the AT-2 amendment.

PROCEDURE ITEM 4: EVIDENCED — product/route 22/2, PublicApi/TSC/declaration 7/1, IDE 3/0, batch 10/2, and transport 21/1 all ran. Bundler write attribution is established by the marked return being served after observation is disarmed, with the parent unchanged and cross-file-off still publishing both products: `packages/unplugin/scripts/probe-bundler-route.mjs:591-670`, `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:3502-3591`, `docs/arch/refactor/rev11/evidence/BF3/test-invocations.md:146-153`.

EXIT CRITERIA

- Procedure 1 — UNCHANGED-BY-DELTA — authoritative Svelte suite ran 20 → 17 passed/3 ignored; `docs/arch/refactor/rev11/evidence/BF3/test-invocations.md:146-150`.
- Procedure 2 — UNCHANGED-BY-DELTA — six client/server cells remain individually driven; `docs/arch/refactor/rev11/evidence/BF3/landing-record.md:59-65`.
- Procedure 3 — UNCHANGED-BY-DELTA — committed runtime-axis plant evidence remains at `docs/arch/refactor/rev11/evidence/BF3/landing-record.md:578-605`.
- Procedure 4 — SATISFIED-BY-DELTA — `crates/verter_session/src/framework/framework_product_surface_inventory.json:2-8`; `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:3502-3591`.
- Procedure 5 — UNCHANGED-BY-DELTA — classifications remain enumerated at `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:21-33`.
- Procedure 6 — NOT-EVIDENCED — the new artifact does not discriminate independent error-list reading; `crates/verter_session/src/framework/svelte_batch_route_tests.rs:1227-1305`, production property at `crates/verter_session/src/host_compile.rs:743-760`.
- Procedure 7 — UNCHANGED-BY-DELTA — owners and acceptance IDs remain listed at `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:23-31`.

- Required exits sentence 1 — NOT-EVIDENCED — retained AT-2 reachability remains explicitly UNKNOWN/open; `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:108-116`.
- Required exits sentence 2 — NOT-EVIDENCED — the same row is explicitly “reachability unproven”; `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:29`.
- Required exits sentence 3 — NOT-EVIDENCED — the amendment relies on an orchestrator interpretation and a non-discriminating artifact; `docs/arch/refactor/rev11/evidence/BF3/maintainer-standing-ruling-bugs-and-types.md:38-50`.
- Required exits sentence 4 — NOT-EVIDENCED — reachable classes are tested, but the successful-response construction’s error-list behavior is not; `docs/arch/refactor/rev11/evidence/BF3/test-invocations.md:658-665`.
- Required exits sentence 5 — SATISFIED-BY-DELTA — route parity and mutation control discriminate the recompile write; `docs/arch/refactor/rev11/evidence/BF3/test-invocations.md:522-553`.
- Required exits sentence 6 — NOT-EVIDENCED — the mandatory atomicity/exhaustion remainder is still UNKNOWN; `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:108-116`.
- Required exits sentence 7 — UNCHANGED-BY-DELTA — AMD-009 and correction-block predecessor state is recorded at `docs/arch/refactor/rev11/charters/BA0.md:3-8`.
- Required exits sentence 8 — UNCHANGED-BY-DELTA — B2/B3 remain locked; `docs/arch/refactor/rev11/evidence/BF3/landing-record.md:848-850`.

PLANTS YOU AUTHORED

- P-AT2 | replaced `host_compile.rs:745-751` diagnostic filtering with hardcoded `Vec::new()` | marker `BF3_REVIEW_PLANT_AT2_HARDCODE_EMPTY`: HEAD 0, working tree 1 | ignored test stayed GREEN, proving non-discrimination | yes; SHA restored `13f2cd52…`
- P-WRITE | cached `recompiled.code.slice(0, -1)` instead of the exact return | marker `BF3_REVIEW_PLANT_RECOMPILE_TRUNCATE`: HEAD 0, working tree 1; rebuilt unplugin and regenerated freshness | targeted test RED at exact child equality | yes; source/freshness SHAs restored, rebuilt probe `loaded:true`, `fresh:true`

FINDINGS

- BF3-1, P1, `crates/verter_session/src/framework/svelte_batch_route_tests.rs:1227` — the ignored test is mislabelled/non-discriminating. Hardcoding `errors` empty leaves it green, so it does not establish “filtered, not hardcoded” or independent error-list reading. Extract/test the successful-response conversion with a synthetic response carrying product plus error-severity diagnostics.
- BF3-2, P1, `docs/arch/refactor/rev11/evidence/BF3/maintainer-standing-ruling-bugs-and-types.md:40` — the verbatim act does not name AT-2; the document admits the amendment is the program orchestrator’s interpretation. This conflicts with the preserved ruling that no track actor may amend a ratified row (`at2-deviation-memo.md:96-101`). Obtain an explicit maintainer AT-2 amendment or revert the row/BA0 changes.
- BF3-3, P2, `docs/arch/refactor/rev11/evidence/BF3/test-invocations.md:505` — “exactly ONE” reachable `getVirtualFile` site is false. `buildStart` calls `readCompiledStyleArtifacts` at `packages/unplugin/src/index.ts:780-785`, which calls `getVirtualFile` at `:56-79`. An independent `Verter.vite` Svelte-style drive observed that call with cross-file optimization off. Narrow the claim to this Vue-only fixture and its exact child `rawId`.
- BF3-4, P2, `docs/arch/refactor/rev11/evidence/BF3/landing-record.md:843` — “No charter exit criterion is now NOT-EVIDENCED” contradicts the same evidence index, which still says item 6 remains `NOT-EVIDENCED` at `test-invocations.md:658-665`. Reconcile after resolving BF3-1/BF3-2.

OVERCLAIMS

- “`buildStart` reaches `host.getVirtualFile` at exactly ONE place” — false for the general hook call graph; it is only true for the probe’s Vue-only fixture.
- “its empty error list is filtered, not hardcoded” — the test proves neither; the hardcoded-empty plant passed.
- “the act … already asked for, word for word” — the quoted maintainer act contains no AT-2-specific amendment.
- “No charter exit criterion is now recorded `NOT-EVIDENCED`” — contradicted by the retained UNKNOWN reachability record and `test-invocations.md:658-665`.

All touched files were restored byte-identically; final worktree diff is empty and `cargo fmt --all --check` passes. The full memory-heavy gate was not run, as requested.
```

---

## Grok — the delta as fixed

```
VERDICT: LAND

PER-CLAIM
claim 1: PASS — `getVirtualFile` during `buildStart` is the recompile call on this fixture; the substitution equality is the WRITE, not just the CALL. Ran `transport_route_equivalence` (22/21/1), the named attribution test, the probe, and plants R1/R2.
claim 2: PASS — original AT-2 claim is not demonstrated; the rename is honest; authority is a recorded reading of a general ruling given for this row, not a second named act. Ran `svelte_batch_route` (12/10/2), the ignored test (PASS), plants C2-hardcode (stays green) and C2-inject (RED).

PROCEDURE ITEM 4: EVIDENCED — last open residue was the recompile WRITE. Inventory no longer has `PARTIALLY DRIVEN`/`UNPROVEN` (`framework_product_surface_inventory.json:8`). I ran the full transport suite (NAPI/WASM/bundler, including `the_bundler_cross_file_recompile_write_is_attributed_to_the_recompile_call` at `transport_route_equivalence_tests.rs:3491`), the probe's three recompile lanes (`probe-bundler-route.mjs:657-672`), and plants that separate CALL from WRITE. PublicApi/TSC/host product-surface suites were not re-executed in this review (unchanged by the delta).

EXIT CRITERIA
1. UNCHANGED-BY-DELTA — Svelte shipped-path gate still lives at `svelte_official_conformance_gate.rs`; invocation `test-invocations.md:148`. Not re-run here.
2. UNCHANGED-BY-DELTA — six client cells still driven by `every_committed_client_cell_is_driven_and_reaches_its_recorded_outcome` (`svelte_official_conformance_gate.rs:264`). Not re-run here.
3. UNCHANGED-BY-DELTA — axis plants still at `the_gate_detects_a_planted_defect_on_every_applicable_axis_family` (`:1017`) and `the_runtime_comparison_detects_a_planted_wrong_render` (`:1285`). Not re-run here.
4. SATISFIED-BY-DELTA — recompile WRITE now attributed (`transport_route_equivalence_tests.rs:3491-3599`, `probe-bundler-route.mjs:551-672`, `inventory.json:8`). Other item-4 surfaces (PublicApi/TSC, host inventory) are prior work, not re-run.
5. SATISFIED-BY-DELTA — AT-2 reclassified from demonstrated atomicity violation to latent construction hazard (`dispositions.md:29`, `:87-89`).
6. SATISFIED-BY-DELTA — under the amended row AT-2 is not a genuine defect, so no defect-regression is owed; reachable-class table remains `a_genuinely_failing_batch_entry_publishes_no_partial_product` (`svelte_batch_route_tests.rs:1046`, green in the run above).
7. SATISFIED-BY-DELTA — AT-2 still DEFER to BA0 with acceptance id unchanged (`dispositions.md:29`); BA0 obligation lines rewritten (`BA0.md:28`, `:37-50`). No production guard/refusal added (delta is test/evidence/charter only).
exit "The full retained inventory has actual results." SATISFIED-BY-DELTA — `inventory.json:8` now `DRIVEN` including write attribution.
exit "`UNPROVEN` records an open proof gap and cannot count as exhaustion." UNCHANGED-BY-DELTA — constraint; this delta stops using UNPROVEN/metrics-build as the write's closure (`inventory.json:8`, `landing-record.md:623-644`).
exit "Every genuine failure has exact … regression, owner, and acceptance/test ID; no guard or removal ID exists." UNCHANGED-BY-DELTA for the other rows (`dispositions.md:23-33`); SATISFIED-BY-DELTA for AT-2 no longer counted as a genuine failure (`dispositions.md:29`).
exit "`FC-ATOMIC-001` remains non-vacuous … a refusal publishes none." UNCHANGED-BY-DELTA — green table + warning-only control (`svelte_batch_route_tests.rs:1046`, `:1088`); I ran both, both pass.
exit "Route-parity tests, harness mutation controls, and correction-owner regressions replace cold-path and guard tests." UNCHANGED-BY-DELTA — no new production path; route-parity still the transport suite I ran.
exit "If no genuine failure exists, only the per-failure clauses are vacuous; inventory, oracle, route, atomicity, and mutation-control exits remain mandatory." UNCHANGED-BY-DELTA — those exits still have tests; AT-2's per-failure clause is the one made vacuous (`BA0.md:37-40`).
exit "BF3 may close as an audit only after AMD-009 is ratified and BA0, BS0, BCSS0, and BRT0 exist …" UNCHANGED-BY-DELTA — `BF3.md:58-59`; this delta does not accept BF3.
exit "B2 and B3 stay locked until BV0, BF3, BA0, BS0, BCSS0, and BRT0 are all accepted." UNCHANGED-BY-DELTA — `BF3.md:59-60`; `landing-record.md:869-874` still withholds acceptance.

PLANTS YOU AUTHORED
R1 | `index.ts:809` write `"/* REVIEW-PLANT-R1-EMPTY-WRITE */"` instead of `recompiled.code` (call kept) | HEAD 0 / WT 1, SHA `c41a9d05`→`56a07140` | attribution test RED at `:3581` (served plant string, not host+marker); readings 1–2 still reached | restored, rebuild, freshness byte-equal to committed (`de42571d` / `5735c184`)
R2 | extra `getVirtualFile({rawId: filename})` in the Vue precompile branch (`index.ts:791`) | HEAD 0 / WT 1, SHA `c41a9d05`→`c22a50f3` | RED at `:3517` — observed `[Child, Parent, Child]` not `[Child]` | restored, rebuild, freshness reproduced; post-restore attribution test ok
C2-hardcode | site 3 `errors = Vec::new()` (`host_compile.rs:745`) | HEAD 0 / WT 1, SHA `b624595f`→`be78c5f3` | ignored test STAYS GREEN | restored SHA `b624595f`
C2-inject | site 3 `errors.push("REVIEW-PLANT-C2-INJECT-ERROR")` | HEAD 0 / WT 1, SHA `b624595f`→`1f734d44` | ignored test RED at `:1304` `assert_publishes_cleanly`; green warning-only control also RED at `:1106` | restored SHA `b624595f`

All four plants restored. `git status --porcelain` empty. No leftover `REVIEW-PLANT-` markers. Freshness JSON matches `HEAD` after the unplugin rebuilds.

FINDINGS
F1, P2, `dispositions.md:43-44` — "decide this row directly" overstates a general ruling that does not name AT-2 (`maintainer-standing-ruling-bugs-and-types.md:40-42`). Same file later admits this (`dispositions.md:54-65`). Not blocking: the factual amendment is right, and the objection is already recorded. Fix: lead with the provenance paragraph, or get an act that names AT-2.
F2, P3, `svelte_batch_route_tests.rs:1234-1236` — `#[ignore]` on a test that PASSES today. Consult said an ignored target is legitimate only if it fails on current behavior (`at2-disposition-ruling.md:67`). This is a named BA0 artifact, not a stub, and not the usual failing correct-behavior target. The HostBacked warning-only control already asserts the same precondition (`:1184-1192`).

OVERCLAIMS
- `dispositions.md:43-44` "decide this row directly" — the verbatim act is general and does not name AT-2. What it actually authorizes is "add ignored tests for bugs found"; the clause-for-clause AT-2 amendment is the orchestrator's reading (`maintainer-standing-ruling-bugs-and-types.md:38-52`).
- Commit 2 named the test `the_host_backed_success_construction_reads_its_product_and_its_errors_independently`. That name was an overclaim (C2-hardcode stays green). HEAD renamed it (`:1236`) and the comment now says the independent-read half is unproven (`:1219-1226`). Judge HEAD, not the intermediate name.
- Claim 1's "exactly one place in `buildStart`" was also an overclaim in commit 1; HEAD corrected it to two sites (`index.ts:68` Svelte style, `:803` recompile). Load at `:668` is not on the `buildStart` graph. Vue-only fixture + bare-canonical equality + flag-off control make the remaining site the recompile call.

Notes
- Specified range is three WIP commits, not two: `35028ee8b`, `789baf987`, `234409080`.
- JS `getVirtualFile` sites reachable from `buildStart`: `:68` (Svelte `readCompiledStyleArtifacts` via `:785`) and `:803`. `computeCrossFileOptimizations` is a separate native method (`verter_napi/src/lib.rs:2038`) and does not go through the JS prototype. Negative control (`crossFileOptimize: false`) observed `[]` while both modules still published.
- Marked substitution establishes the WRITE: R1 kept the call and changed only the cache value; reading 3 went red, readings 1–2 did not. Probe on HEAD: flag-on one child read (`codeLength` 360); flag-off zero reads; substitute child `398` = 360 + marker, parent unmarked.
- The ignored test is a genuine precondition test, not a stub. `#[ignore]` + PASS today is honest as "not required-RED", and a category mismatch with this program's failing-ignored convention. C2-inject shows `assert_publishes_cleanly` is live; C2-hardcode shows it does not prove site 3 reads its error list.
```
```

---

## Codex — round 2 (the fix delta)

```
VERDICT: BLOCKING

FINDING 1: RESOLVED — renamed claim is limited to the reachability precondition (`svelte_batch_route_tests.rs:1198-1226`). My injected-error plant failed at `:1293`; hardcoding `errors = Vec::new()` stayed green, matching the explicit disclaimer.

FINDING 2: UNRESOLVED — STILL BLOCKING. Recording the objection is not authority. The maintainer act does not name AT-2 (`maintainer-standing-ruling-bugs-and-types.md:40-60`), while the controlling consult permits only a maintainer amendment (`at2-disposition-ruling.md:80-92`). Neither requested remedy occurred.

FINDING 3: RESOLVED — HEAD correctly records two paths: style read at `packages/unplugin/src/index.ts:56-68,785` and recompile at `:803`. The assertion distinguishes the bare child ID (`transport_route_equivalence_tests.rs:3457-3466,3517-3521`). Transport suite: 21 passed, 1 ignored.

FINDING 4: RESOLVED — `test-invocations.md:672-683` now aligns with the landing record’s stated position and explicitly references the authority objection; `landing-record.md:859-867` records the dependency.

PROCEDURE ITEM 4: EVIDENCED — `charters/BF3.md:27-30`; affected transport suite passed 21/22 with the pre-existing ignored Rollup-map target.

PROCEDURE ITEM 6: NOT-EVIDENCED — `charters/BF3.md:34`; without an authorized AT-2 amendment, the ratified row remains governing.

NEW FINDINGS  
None. No new technical defect or weakened assertion. The item-6 authority overclaim remains Finding 2, not a new finding.

PLANTS YOU AUTHORED

`AT2-OK-ERROR` | injected an Error into successful `VirtualFileResponse` at `virtual_file_pipeline.rs:1121` | marker HEAD=0, worktree=2; SHA changed | RED at test line 1293 | yes; original SHA restored

`AT2-HARDCODE` | replaced HostBacked success error filtering with `Vec::new()` at `host_compile.rs:745` | marker HEAD=0, worktree=1; SHA changed | GREEN, as the narrowed claim predicts | yes; original SHA restored

All markers are zero, both files are byte-identical to HEAD, and the worktree is clean. Targeted batch suite: 10 passed, 2 ignored; restored ignored target: 1 passed.
```

---

## Resolution of the authority objection

**The seat was right, and it was answered rather than argued down.** Codex's Finding 2, held
across both rounds and terminal in round 2 —

> FINDING 2: UNRESOLVED — STILL BLOCKING. Recording the objection is not authority. The
> maintainer act does not name AT-2 (`maintainer-standing-ruling-bugs-and-types.md:40-60`),
> while the controlling consult permits only a maintainer amendment
> (`at2-disposition-ruling.md:80-92`). Neither requested remedy occurred.

— named two acceptable remedies: an explicit maintainer act naming `AT-2`, or a revert of the
`AT-2` row and the matching `BA0` obligation lines. **The first remedy was taken.** The
maintainer was asked directly and issued an act that names the row:

> Reject AT-2's claim that a reachable batch entry publishes a product beside a genuine typed
> refusal; reclassify AT-2 as a latent HostBacked construction hazard with reachability unproven;
> retain the DEFER to BA0; carry it as an `#[ignore]`d characterization test; and drop the
> required-RED Svelte-refusal atomicity target.

The act's own text records why the objection was correct: *"A review seat blocked that inference:
the general ruling never NAMES AT-2, and a general act does not authorize a change to a specific
ratified findings row. The seat was correct — acting on an unnamed authority is the same governance
defect that blocked this block once already."* The act is reproduced in full, with its scope,
evidence and stated effect, at
[`maintainer-act-at2-amendment.md`](maintainer-act-at2-amendment.md).

Three properties of the remedy matter for reading this file:

1. **It names the row.** The precise defect the seat identified — a general act applied to a
   specific ratified row by a track-level actor — is cured, not restated.
2. **It authorizes exactly the bytes already present.** The act bounds itself to the `AT-2` row in
   [`dispositions.md`](dispositions.md) and `charters/BA0.md` lines 28 and 37, and both are
   byte-unchanged by the commit that records the act. No further edit was taken under it.
3. **It does not accept anything.** BF3 is not accepted, `BA0` is not accepted, B2/B3 stay locked,
   and no production guard, refusal, withhold path, retraction or removal ID is authorized.

Consequence for the closing round's verdicts: Codex's `PROCEDURE ITEM 6: NOT-EVIDENCED` was
conditioned in its own words on *"without an authorized AT-2 amendment, the ratified row remains
governing"*. The amendment is now authorized by a maintainer act naming the row, so that condition
no longer holds. Every other verdict in both reports stands exactly as issued — including Codex's
round-1 PASS on claim 1 and EVIDENCED on item 4, and Grok's F2 category note on the `#[ignore]`d
characterization, which the act independently settles by directing that the hazard be carried as an
`#[ignore]`d characterization rather than a required-RED target.

**What this section does NOT claim.** It does not convert either seat's verdict into an
architecture-mandate PASS. Both seats reviewed the exhaustion-closure DELTA, not the block. The
full architecture mandate over the block as a whole is recorded separately at
[`architecture-mandate-review.md`](architecture-mandate-review.md).

---

## Confirm round — the conformance mandate, re-issued on the discharged objection

The remedy above changes the premise of the blocking seat's finding, so the finding was put BACK to
an external seat rather than declared discharged by the actor who took the remedy. Recording an
objection is not authority, and neither is satisfying one — this block's whole history is that
distinction, and applying it to ourselves is the only consistent reading.

A fresh `codex exec` process (`gpt-5.6-sol`, effort `high`) was given a NARROW confirm: is the
blocking finding discharged as the finding itself stated it, is charter procedure item 6 evidenced on
the tree as it now stands, and does the delta since that round weaken any conformance claim. The
prompt quoted the blocking finding verbatim, required the seat to enumerate the genuine-defect rows
ITSELF from [`dispositions.md`](dispositions.md) and answer per row with the test item it checked,
stated that `BLOCKING` and `NOT-EVIDENCED` are legitimate verdicts, and told it that the separate
architecture mandate returned `BLOCKING` while explicitly NOT binding its own verdict in either
direction.

**`CONFORMANCE VERDICT: PASS`. Finding 2 DISCHARGED. Procedure item 6 EVIDENCED. No findings.**

The seat did not take item 6 on the record's word. It ran each of the nine genuine rows' targets
alone with `--ignored`, confirmed each selected exactly one test, and reported the assertion line and
the observed failure for every one — SV-1 at emitted flag 21 versus official 20, SV-2 at the typed
advanced-rune refusal, SV-3 at the enumerated missing authored anchors, SV-4 at TypeScript observing
`[]` instead of `disabled,label`, RT-1 at Vue bytes with an absent refusal, AT-1 at the refused
combined request publishing IDE output, CSS-1 at requested passthrough CSS with no map, TR-1 at the
new parity assertion, BND-2 at a null public map. It separately confirmed the three rejected rows
create no item-6 obligation.

On the new TR-1 target it went past the test to the production conversions, citing NAPI mapping
`MissingVirtualNode` to `None` and WASM mapping the same host error through `host_err` — the two
sides of the divergence the target now gates — and confirmed the adjacent characterization still
pins today's shapes and reran green beside it.

On the one point where the remedy is incomplete it was precise rather than accommodating: it noted
that the act's `BA0` locator enumerates lines 28 and 37 and not the same operative consequence
restated at `BA0.md` lines 59-62, and held that this narrower architecture-governance issue does not
undo the as-stated remedy, because Finding 2's first alternative required a maintainer act naming the
amended row and did not require an exhaustive locator. That is a conformance reading of a conformance
finding; the governance question it points at remains open and is recorded in
[`architecture-mandate-review.md`](architecture-mandate-review.md) and the landing record.

### Verbatim report

````
CONFORMANCE VERDICT: PASS

Q1 FINDING 2 — DISCHARGED — The first expressly permitted remedy occurred in `c864febd1`: the maintainer's verbatim act names `AT-2` and orders rejection of the old reachable-defect claim, reclassification as a latent/unproven hazard, retention of DEFER to BA0, an `#[ignore]`d characterization, and removal of the required-RED Svelte-refusal target (`docs/arch/refactor/rev11/evidence/BF3/maintainer-act-at2-amendment.md:25-50`); it states the authority objection is discharged (`docs/arch/refactor/rev11/evidence/BF3/maintainer-act-at2-amendment.md:65-70`) and the current row contains exactly that amendment (`docs/arch/refactor/rev11/evidence/BF3/dispositions.md:29`). No revert occurred. This satisfies Finding 2 as stated: its first alternative required an explicit maintainer act naming the amended row. The act record separately notes that its BA0 locator enumerates lines 28 and 37 but not the same operative consequence restated at BA0 lines 59-62 (`docs/arch/refactor/rev11/evidence/BF3/maintainer-act-at2-amendment.md:98-118`); that narrower architecture-governance issue does not undo the as-stated remedy, whose first alternative did not require a revert or an exhaustive BA0 locator once the maintainer named and amended AT-2.

Q2 PROCEDURE ITEM 6 — EVIDENCED — The criterion is non-vacuous: nine rows are genuine defects. Each cited target has a substantive body and was run alone with `--ignored`, selected exactly one test, and failed at the named defect: SV-1 `each_flags_for_a_keyed_runes_each_match_the_official_compiler` (`crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:852-869`) failed at `crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:857` on emitted flag 21 versus official 20; SV-2 `a_runes_props_read_in_the_instance_script_compiles_to_a_runtime_module` (`crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:888-909`) failed at `crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:903` on the typed advanced-rune refusal; SV-3 `the_client_source_map_covers_every_required_authored_anchor` (`crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:929-969`) failed at `crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:965` on the enumerated missing authored anchors; SV-4 `an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript` (`crates/verter_session/src/compile/map_equality_tests/public_api_typescript_observation.rs:503-537`) failed at `crates/verter_session/src/compile/map_equality_tests/public_api_typescript_observation.rs:523` because TypeScript observed `[]`, not `disabled,label`; RT-1 `a_svelte_batch_matches_the_single_file_route_item_for_item` (`crates/verter_session/src/framework/svelte_batch_route_tests.rs:596-660`) failed at `crates/verter_session/src/framework/svelte_batch_route_tests.rs:656` with Vue bytes, absent refusal, and a partial product; AT-1 `a_refused_combined_request_publishes_no_product_at_all` (`crates/verter_session/src/framework/framework_product_surface_tests.rs:1445-1493`) failed at `crates/verter_session/src/framework/framework_product_surface_tests.rs:1482` because the refused combined request published IDE output; CSS-1 `the_standalone_css_route_publishes_valid_requested_maps_for_passthrough_and_transformed_css` (`crates/verter_session/src/framework/framework_product_surface_tests.rs:1255-1301`) failed at `crates/verter_session/src/framework/framework_product_surface_tests.rs:1285` because requested passthrough CSS had no map; TR-1 `the_transports_report_a_missing_node_the_same_way` (`crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1327-1395`) failed at `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1378` on NAPI `missing` versus WASM `error`; BND-2 `the_bundler_rollup_inline_transform_preserves_requested_source_maps` (`crates/verter_session/src/framework/transport_route_equivalence_tests.rs:2670-2808`) failed at `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:2796` with `hostHasMap=true`, inline product true, and public map null. These are precisely the genuine rows in `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:23-31` and `docs/arch/refactor/rev11/evidence/BF3/dispositions.md:256-259`; AT-2 is latent/unproven (`docs/arch/refactor/rev11/evidence/BF3/dispositions.md:29`), RA-1/RA-2 are rejected (`docs/arch/refactor/rev11/evidence/BF3/dispositions.md:32-33`), and BND-1 is rejected (`docs/arch/refactor/rev11/evidence/BF3/dispositions.md:258`), so none creates another item-6 obligation.

Q3 DELTA — PASS — `9104e0be7..HEAD` changes no production implementation: it adds the ignored TR-1 test plus evidence documents. The new test is not a stub: it drives both built transports, proves the in-process subject is `HostOutcome::Missing` (`crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1341-1358`), rejects a published/code-bearing answer on either transport (`crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1360-1375`), then asserts parity (`crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1377-1381`) and preserves typed-error classification if that design wins (`crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1383-1394`). Its isolated run failed today exactly at parity with NAPI `{"outcome":"missing"}` and WASM `{"outcome":"error","message":"HostError::MissingVirtualNode: /probe/Server.svelte"}`; the code confirms NAPI maps `MissingVirtualNode` to `None` (`crates/verter_napi/src/lib.rs:1683-1710`) while WASM maps the same host error through `host_err` (`crates/verter_wasm/src/lib.rs:382-398`). It neither weakens nor duplicates the adjacent characterization: the existing test still pins today's two exact shapes and no-product result (`crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1253-1309`) and independently reran green, whereas the new target demands the corrected, shape-neutral equality and is ignored until correction. Evidence-only edits record the new authority/test; they do not retract any previously upheld conformance evidence.

FINDINGS — none
PLANTS YOU AUTHORED — none
git status --porcelain: <empty>
````

---

## The locator point this seat named, answered by maintainer act

The confirm seat above passed on the conformance question and was nonetheless precise about where
the remedy was incomplete, in its own words:

> The act record separately notes that its BA0 locator enumerates lines 28 and 37 but not the same
> operative consequence restated at BA0 lines 59-62 …; that narrower architecture-governance issue
> does not undo the as-stated remedy, whose first alternative did not require a revert or an
> exhaustive BA0 locator once the maintainer named and amended AT-2.

That reading held two things apart correctly: the conformance finding it was asked about was
discharged, AND the governance question the locator points at was still open. The architecture seat
met that same question head-on, ruled the escalation CORRECT, and stated the only two ways out —
*"only the maintainer can confirm coverage or direct reversion."*

**The maintainer confirmed coverage.** A clarification act rules that the naming act covers all
three `BA0.md` hunks: the charter states the same required-RED Svelte-refusal obligation in the
findings-table row, the Required procedure paragraph, and the Required-exits paragraph, so dropping
that obligation — which the naming act authorizes — necessarily edits every location stating it. The
third hunk introduces no instruction the act does not already reach and grants `BA0` no scope, and
reverting it would leave the charter self-contradictory. The same act separately rules that
reclassifying `AT-2` removes it from BF3's exhaustion obligation, closing the second item the
architecture seat held open.

The act is recorded in full at
[`maintainer-act-at2-scope-clarification.md`](maintainer-act-at2-scope-clarification.md); its effect
on the architecture seat's two open points is recorded beneath that seat's own report in
[`architecture-mandate-review.md`](architecture-mandate-review.md). No byte is changed by it — it
describes coverage of bytes already landed. Nothing in either seat's verdict above is altered: this
seat's `PASS` stands as issued, and its note stands as issued, now with the answer beneath it.
