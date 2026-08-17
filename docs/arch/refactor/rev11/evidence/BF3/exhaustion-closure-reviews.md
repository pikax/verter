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

**Both seats agree** the recompile WRITE is attributed (each proved it with its own plants
separating the CALL from the WRITE), that item 4 is EVIDENCED, and that AT-2's ORIGINAL
claim is not a demonstrated defect.

**They split on one thing: whether the AT-2 amendment is authorized.** The maintainer's act
is a general standing ruling that does not name `AT-2`; applying it to that row is the
program orchestrator's reading. Grok reads that as sufficient; Codex does not, and asks for
an explicit maintainer act naming `AT-2` or a revert of the `AT-2` row and the matching
`BA0` obligation lines — under which item 6 returns to `NOT-EVIDENCED`. That objection is
unresolved, recorded here and in [`dispositions.md`](dispositions.md),
[`maintainer-standing-ruling-bugs-and-types.md`](maintainer-standing-ruling-bugs-and-types.md)
and the landing record, and it is a governance decision above track level.

Round 1 found four defects, all real: the AT-2 artifact was mislabelled and did not
discriminate what its name claimed; the "`buildStart` reaches `getVirtualFile` at exactly
ONE place" claim was false (the Svelte branch's compiled-style read is a second in-hook
site); and the record contradicted its own evidence index on item 6. Three are resolved at
HEAD; the fourth is the authority objection above.

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
