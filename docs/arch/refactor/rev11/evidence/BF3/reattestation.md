# Bounded re-attestation of the cure delta

The block used its three review rounds before the cure. Closing verdicts therefore come from a
bounded per-mandate re-attestation against the CURE DELTA only — each seat answering two questions
about that delta and no others: did each change do what it claims, and does any change introduce a
defect. Findings outside the delta are recorded, never actioned.

Seats are EXTERNAL CLIs only. No seat graded its own work, and no seat authored any of the text or
code it reviewed.

- **Conformance** — Codex `gpt-5.6-sol`, reasoning effort `high`.
- **Architecture** — Codex `gpt-5.6-sol`, reasoning effort `high`.
- **Adversarial** — Grok 4.6, reasoning effort Extra High, explicit default-to-BLOCK posture, and
  required to author its OWN plants rather than re-run the implementer's.

Every prompt required a per-claim verdict AND an enumeration of every numbered item of the charter's
"Owned scope and required procedure" and every sentence of its "Required exits", each marked
`SATISFIED-BY-DELTA` / `UNCHANGED-BY-DELTA` / `NOT-EVIDENCED` with a citation; an uncited criterion
is `NOT-EVIDENCED` and BLOCKING by default. Seats ran SEQUENTIALLY in one worktree, because
concurrent seats planting mutations in a shared tree produce cross-contaminated results.

## What the rounds did

Four rounds ran, each scoped to the delta the previous round produced. **Every round found real
defects in the checks the cure itself introduced, and every one of them was a check that would have
passed while proving nothing.** That is the value this record carries forward: the fix rounds were
not cosmetic.

| round | scope | outcome |
|---|---|---|
| 1 | the whole cure delta | all three BLOCKING — the map oracle validated envelope shape rather than preservation; the census died with an adjacent edit; the recorded package identity was stale; the ratification wording read as a byte-level blessing |
| 2 | the round-1 fix delta | all three BLOCKING — the export partition was satisfied by a spelling that never ran; the census trusted a path string the suite itself owned; the artifact helper accepted a map that maps nothing |
| 3 | the round-2 fix delta | all three BLOCKING — map identity omitted `sourcesContent` VALUES; the export "kind" was a probe opinion; the census accepted any sibling path |
| 4 | the round-3 fix delta | BLOCKING — the census row was bound to a witness that another module could also declare; an expectation was still read from the probe; and, most importantly, **the residue the implementer had written down was FALSE** |

The last of those is the one worth naming. The implementer recorded a limit claiming that no
out-of-process observation could separate `VerterVue` from `unpluginFactory` and that only in-process
driving could witness a run. The adversarial seat measured both halves and disproved both — the
spellings separate on `typeof`, on `.vite` callability and on the plugin-key flattening that only the
`createUnplugin` wrapper carries, and an apply-counting `Proxy` at the entry's import attributes an
invocation out of process. The write-up was corrected to state the residue accurately and narrowly:
what remains open is INVOCATION ATTRIBUTION, and its named closure is a non-zero apply count per
driven export.

## What remains open, recorded and not closed

- **Invocation attribution** on the bundler probe: nothing requires a driven export to have been
  APPLIED, so a record can print an export's true readings while sourcing its drive results from a
  sibling. The readings themselves are cross-checked against a test-owned re-read of the same
  fingerprinted entry, and a hidden or mistyped export IS caught.
- **The witness decoy**: a census row is bound to "a test of this path exists", not to the module
  that declared the constant. Closing it needs the row bound to the declaring module's own identity
  rather than to a free `&str`.
- **All four `mod` declarations removed at once** remains undecidable from inside the same test
  binary — the general execution-attestation problem, not something an in-binary check can settle.
- **The pre-existing exhaustion gaps** the seats' exit-criteria enumeration surfaced (procedure 3,
  procedure 4, procedure 6 and three "Required exits" sentences). These were NOT part of this cure's
  scope and are unchanged by it; they are the reason this block is not acceptance-recommended. See
  the landing record.

All reports are reproduced verbatim below, including the ones whose findings were adjudicated as
stated limits rather than fixed.

---

## Round 1 — conformance

```
VERDICT: BLOCKING

PER-CLAIM
D1: PASS — BS0.md:32,36-47 now names four ignored targets; the SV-4 invocation printed `running 1 test` and failed at public_api_typescript_observation.rs:523 because TypeScript saw `[]` instead of `["disabled", "label"]`.
D2: PASS — the clean probe enumerated exactly Verter/VerterSvelte/VerterVue/default/unpluginFactory; transport_route_equivalence_tests.rs:1487-1556 executes four and proves default===VerterVue, `running 15 tests` passed 14/ignored 1, and a proven-new Verter overlap plant failed the partition before exact-hash restore.
D3: BLOCKING — the true-boolean/null-map plant still failed at transport_route_equivalence_tests.rs:1883, but a proven-new `{version:3,mappings:"A",sources:["/reattest-forged.vue"]}` field made the same ignored target PASS while the real transform map stayed null; the target checks envelope shape, not preservation/parity.
D4: PASS — the three recorded suite runs printed 23→21/2, 8→7/1, and 15→14/1; disabling each mod declaration separately, with unique/new/hash-change proof, produced `running 1 test` then the census's named observed-zero failure, and each exact-hash restore returned green.
D5: PASS — svelte-client-executor.spec.mjs:11,22-24,150,177 binds both assertions to SVELTE_DOMAIN.packageVersion with 30s child < 60s parent; clean run passed 2/2, a proven-new 9.9.9 plant failed both against 5.56.8, and exact-hash restore passed 2/2.
G1: BLOCKING — the seven-file digest reproduces as 6b83efe82c93411df78fc768c63043e8f91ed1b54cb754b769120c70a6a4f2a6, but AMD-009:229-253 requires reviewed identities and explicit acceptance for changed bytes while amd009-ratification-packet.md:20-25 admits the act predates the cured content, names no identity, and did not inspect those bytes.

EXIT CRITERIA
procedure 1: UNCHANGED-BY-DELTA — landing-record.md:36-40 identifies the shipped-route authoritative Svelte gate; its recorded command freshly printed `running 19 tests` / 16 passed / 3 ignored.
procedure 2: UNCHANGED-BY-DELTA — every_committed_client_cell_is_driven_and_reaches_its_recorded_outcome asserts six and drives each (svelte_official_conformance_gate.rs:264-490); every_committed_server_cell_is_refused_by_the_shipped_route asserts six typed ServerGenerate outcomes (:516-555); both ran in the 19-test command against the 5.56.8 matrix.
procedure 3: NOT-EVIDENCED — landing-record.md:333-335 records that no committed plant drives a wrong-render candidate through the candidate-vs-golden runtime comparison; the all-axis requirement therefore still lacks a discriminator.
procedure 4: NOT-EVIDENCED — the delta closes exported-spelling partitioning, but framework_product_surface_inventory.json:2 still says bundler style/load/recompile/CSS-scoping and runtime-render batch lanes are read-verified citations, not driven results.
procedure 5: UNCHANGED-BY-DELTA — dispositions.md:21-33 and 89-92 give product/route/artifact classes for every recorded mismatch, including the re-measured BND rows.
procedure 6: NOT-EVIDENCED — dispositions.md:35-51 admits AT-2 is not reproduced and its test drives a different batch canonical-conflict failure, so it is not an independently discriminating regression for the ratified AT-2 claim.
procedure 7: UNCHANGED-BY-DELTA — dispositions.md:21-32,89-92 names each genuine row's immediate owner, acceptance ID, and test; landing-record.md:170-199 records no production guard/refusal/retraction/removal substitution.
exit "full retained inventory has actual results": NOT-EVIDENCED — framework_product_surface_inventory.json:2 expressly retains undriven bundler lanes as read-verified citations.
exit "UNPROVEN cannot count as exhaustion": NOT-EVIDENCED — landing-record.md:333-337 still records runtime-mutation, allowlist-plant, and route-inventory items as partial-coverage NOT_PROVEN.
exit "every genuine failure has ... acceptance/test ID; no guard or removal ID": NOT-EVIDENCED — dispositions.md:35-51 says the AT-2 gating test does not reproduce the ratified failure class, so its cited test does not cover the criterion's independently discriminating-regression words.
exit "FC-ATOMIC-001 remains non-vacuous": UNCHANGED-BY-DELTA — a_refused_combined_request_publishes_no_product_at_all first proves a typed refusal then forbids the IDE product and freshly ran 1/fails at that publication; a_genuinely_failing_batch_entry_publishes_no_partial_product requires errors plus no code/map/lang and ran green in the 8-test batch suite.
exit "route-parity/mutation-control/correction-owner regressions": NOT-EVIDENCED — landing-record.md:333-337 records a missing runtime-comparison mutation and undriven route lanes; additionally the D3 forged-map plant passes the BRT0 correction target.
exit "BF3 may close only after ratification and the four predecessors exist": NOT-EVIDENCED — program-dag.toml:81-127 contains BA0/BS0/BCSS0/BRT0 and both predecessor edges, but the rebound bytes lack the identity-bound explicit acceptance required by AMD-009:229-253.
exit "B2/B3 stay locked": UNCHANGED-BY-DELTA — program-state.toml:447-458 keeps B2 LOCKED (and the following B3 row remains LOCKED); program-dag.toml:118-127 retains all six predecessors.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)
1. G1-UNBOUND-RATIFICATION, P1, docs/arch/refactor/rev11/evidence/BF3/amd009-ratification-packet.md:20, the act is explicit §7 text authority but expressly predates and does not bind/inspect the rebound package, contrary to AMD-009:229-253 and the ordered cure; obtain a post-review maintainer act explicitly accepting the exact rebound digest/identities, then rebind again if any byte changes.
2. D3-FORGED-MAP-PASSES, P2, crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1270, assert_source_map_artifact accepts any v3 envelope with non-empty mappings/sources: a zero-authored-position unrelated map made the Rollup preservation target green while the production map remained null; carry the host map artifact into the comparison and assert normalized artifact parity (including mapped authored segments/source identity), with this plant RED.
3. VALIDATOR-INVOCATION-MISRECORDED, P3, docs/arch/refactor/rev11/evidence/BF3/landing-record.md:517, the displayed `node scripts/validate-program-state.mjs` invocation prints usage and exits 2 because --dag/--state/--mode are mandatory, not the recorded 62-block OK; record the two full live/template commands (which I ran and which each validated 62 blocks).

OUT-OF-DELTA (recorded, not actioned)
landing-record.md:333-335 — no wrong-render plant traverses the runtime candidate-vs-golden comparison.
crates/verter_session/src/framework/framework_product_surface_inventory.json:2 — multiple bundler lanes remain read-verified rather than driven.
dispositions.md:35-51 — AT-2's cited test measures a different batch-conflict failure and does not reproduce AT-2.
```

---

## Round 1 — architecture

```
VERDICT: BLOCKING

PER-CLAIM
D1: PASS — `BS0.md:29-52` now names four ignored targets; I ran SV-4 and got `running 1 test`, failing at `public_api_typescript_observation.rs:523` because `[] != ["disabled", "label"]`.
D2: PASS — `transport_route_equivalence_tests.rs:1503-1556` enumerates the built ESM namespace, rejects BOTH/NEITHER/stale membership, and witnesses every executed case; both new tests passed, and an overlap plant made the partition test fail on `Verter`.
D3: BLOCKING — the test now reads `publicTransformMap`, and a lying-boolean plant stayed RED, but `assert_source_map_artifact` (`:1270-1290`) accepts any v3/non-empty envelope and never compares it with the host map; substituting `{version:3,mappings:"A",sources:["/forged.vue"]}` made the ignored target pass.
D4: BLOCKING — each suite-only removal correctly failed its census, but the census is registered beside the suites in `framework/mod.rs:33-44`; removing the product-suite and census registrations together produced `running 0 tests` / `ok` / exit 0.
D5: PASS — `svelte-client-executor.spec.mjs:11,22-24,127-180` derives the expected version from `SVELTE_DOMAIN.packageVersion`, asserts both returned runtime versions, and sets 30s child < 60s parent; I ran 2/2 green.
G1: BLOCKING — the digest reproduces, but `amd009-ratification-packet.md:20-23` admits the act predates and does not bind the rebound bytes, contrary to AMD-009 §7's exact-identity/explicit-acceptance rule (`AMD-009...md:229-253`) and the charters' “Ratified content identity” claims.

EXIT CRITERIA
procedure 1: UNCHANGED-BY-DELTA — `landing-record.md:36-40,222-225` records the digest-verified shipped-path authoritative gate and `test-invocations.md:41` records `running 19 tests`.
procedure 2: UNCHANGED-BY-DELTA — `landing-record.md:59-65,344-346` cites the assertions that all six client cells and all six server cells are individually driven; `test-invocations.md:41` records the non-zero suite.
procedure 3: NOT-EVIDENCED — `landing-record.md:333-335` expressly says no committed plant drives a wrong-render candidate through the candidate-vs-golden runtime comparison.
procedure 4: NOT-EVIDENCED — D2 closes the five-export spelling gap (`transport_route_equivalence_tests.rs:1503-1671`), but `landing-record.md:336-337` still records several bundler lanes as read-verified citations rather than driven results.
procedure 5: UNCHANGED-BY-DELTA — `dispositions.md:21-33,89-92` supplies the product/route/harness classes before owner assignment, with AT-2's reachability qualification at `:35-51`.
procedure 6: NOT-EVIDENCED — the changed Rollup regression asserts only a producer-controlled map shape, not preservation/parity with the matching host artifact (`transport_route_equivalence_tests.rs:1270-1290,1855-1889`).
procedure 7: UNCHANGED-BY-DELTA — `dispositions.md:21-33,89-92` names each correction owner and acceptance/test ID; `landing-record.md:170-177` records no production guard/refusal/removal mechanism.
exit "full retained inventory has actual results": NOT-EVIDENCED — `landing-record.md:333-337` retains NOT_PROVEN runtime-discrimination and read-verified bundler-lane gaps.
exit "UNPROVEN cannot count as exhaustion": NOT-EVIDENCED — those open proof gaps remain expressly recorded at `landing-record.md:333-337`.
exit "every genuine failure has ... acceptance/test ID; no guard or removal ID": NOT-EVIDENCED — IDs/owners exist in `dispositions.md:21-33,89-92`, but D3's changed acceptance regression does not prove map preservation.
exit "FC-ATOMIC-001 remains non-vacuous": UNCHANGED-BY-DELTA — my `framework_product_surface` run executed 23 tests; the refusal target asserts no JS/CSS/map and the combined-request target asserts no IDE publication (`landing-record.md:226-240`).
exit "route-parity/mutation-control/correction-owner regressions": NOT-EVIDENCED — D3 remains non-discriminating against a substituted structurally non-empty but unrelated/semantically empty map, and `landing-record.md:333-335` retains the runtime mutation gap.
exit "BF3 may close only after ratification and the four predecessors exist": NOT-EVIDENCED — the four rows exist (`program-dag.toml:82-103`), but the post-cure bytes lack the explicit identity-bound maintainer acceptance required by AMD-009 §7 (`:229-253`).
exit "B2/B3 stay locked": UNCHANGED-BY-DELTA — `landing-record.md:527-530` records both locked; I ran the live/template validator with the declared DAG/state paths and both validated 62 non-zero rows.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)
1. G1-UNBOUND-BYTES, P1, `docs/arch/refactor/rev11/evidence/BF3/amd009-ratification-packet.md:20-23`, the maintainer act ratifies §7 text but explicitly predates/blesses no rebound tree, while AMD-009 §7 requires reviewed identities plus explicit acceptance and the charters claim their digest is ratified; obtain a maintainer act naming the rebound digest/blob manifest or exact commit/tree after review, then bind status lines to that act (or remove the byte-authority claims).
2. D4-CENSUS-SELF-VACUITY, P1, `crates/verter_session/src/framework/mod.rs:33-44`, the census has the same hand-written registration owner as the suites and disappears with an adjacent edit; this violates `CLAUDE.md` Verification Must Prove Execution because the compiled universe self-attests its completeness; make the canonical driver independently derive the source inventory, parity-check it against libtest discovery, require non-zero execution, and exercise removal of the census itself through that entry point.
3. D3-SHAPE-NOT-PRESERVATION, P2, `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1270-1290`, the new oracle validates only public-map envelope shape and discards the matching host map's bytes, so an unrelated map or `mappings: "A"` can satisfy a test named “preserves”; normalize and compare the public artifact with the host artifact (or assert required authored anchors/segments), with substituted and zero-mapped artifacts as RED controls.

OUT-OF-DELTA (recorded, not actioned)
- `landing-record.md:333-337` retains a missing wrong-render runtime plant, unplanted widened allowlists, and read-verified bundler lanes.
- `dispositions.md:35-51` retains AT-2 as DEFER although the driven evidence does not reproduce the claimed violation while RT-1 stands.
```

---

## Round 1 — adversarial

```
VERDICT: BLOCKING

PER-CLAIM
D1: PASS — BS0.md:34-54 names the fourth ignored target and “all four”; `public_api_typescript_observation.rs:505-536` is `#[ignore]`d and asserts required `label` + optional `disabled`. Delta is that text. Did not re-run the ignored test.
D2: BLOCKING — forged `rawVueFactory` as a clone of `vuePublicEntry` (`ADVERSARIAL_D2_FORGED_RAW_FACTORY`; `unpluginFactory` never called; marker absent from HEAD). Probe `outcome=transformed`. `every_exported_bundler_spelling_is_executed_or_classified_out_of_scope` and `the_bundler_raw_and_auto_entries_apply_their_documented_carrier_contracts` both `ok` (`/tmp/d2-partition.txt`, `/tmp/d2-raw-auto.txt`). Partition is satisfied while a spelling is never executed. Restored.
D3: BLOCKING — planted `{version:3, mappings:"A", sources:["ADVERSARIAL_D3_EMPTY_MAP"]}` on Vue `loadedScriptMap` and Rollup `publicTransformMap` (unique, not in HEAD). `the_bundler_virtual_script_loads_publish_requested_source_maps` stayed `ok` (`/tmp/d3-vite-green.txt`). `assert_source_map_artifact` accepts a map that maps nothing. Ignored Rollup target still RED, but at identity (`/tmp/d3-rollup-ignored.txt: left mappings "A"`), not at the advertised artifact helper. Characterization caught Vue (`/tmp/d3-characterization.txt`). Restored.
D4: BLOCKING — emptied all 23 `#[test]` in `framework_product_surface_tests.rs` via `#[cfg(any())]` and set `CENSUS_MODULE_PATH="framework::"` (`ADVERSARIAL_D4_*`, not in HEAD). Documented filter ran `running 1 test` / `the_framework_product_surface_suite_is_present_and_non_vacuous ... ok` / exit 0 (`/tmp/d4-empty-wide.txt`). Census can pass while the suite is empty. Honest-path empty-only plant DID fail at floor 23 (`/tmp/d4-empty-only.txt`). Restored both.
D5: PASS — planted `resolveBoundRuntime` → `"0.0.0-ADVERSARIAL-D5"` (unique, not 9.9.9, not in HEAD). Both `it(...)` failed `expected '0.0.0-ADVERSARIAL-D5' to be '5.56.8'` at spec:150 and :177 (`/tmp/d5-forged-version.txt`). Unplanted control 2/2 pass (`/tmp/d5-unplanted.txt`). `PARENT_BUDGET_MS=60000` > `CHILD_DEADLINE_MS=30000` (spec:22-24). Pin-shaped hardcoded `"5.56.8"` (no json read) is green, but oracle `package.json` is already `5.56.8` — not a discriminating hide.
G1: BLOCKING — packet identity is not the current package. Claimed BA0/BCSS0/BF3/BRT0/BS0 blobs `c7095a7f4`/`1b2fdbbf4`/`6c0189170`/`58f8ffcbd`/`472a0b88a` + digest `6b83efe82c93…`; live `git hash-object` is `c1bc7e1b2`/`76f5c8e74`/`589cfdb7a`/`66ff6f5d8`/`be3b9d863` + digest `0bdef4b095cf…`. Drift is `179911f75` (status-line rewrite after bind). Packet:116 cites `reattestation.md` — file does not exist. §7 changed-byte clause is not discharged for HEAD bytes.

EXIT CRITERIA
procedure 1: UNCHANGED-BY-DELTA — `test-invocations.md:60` Svelte official-conformance invocation; module still present (`svelte_official_conformance_gate.rs`). Not re-run here.
procedure 2: UNCHANGED-BY-DELTA — `every_committed_client_cell_is_driven_and_reaches_its_recorded_outcome` at `svelte_official_conformance_gate.rs:264`; landing-record.md:341-345.
procedure 3: UNCHANGED-BY-DELTA — original plants recorded at `test-invocations.md:95-114`. This seat’s new plants are D2–D5, not the original axis set.
procedure 4: NOT-EVIDENCED — bundler “every exported spelling executed or partitioned” is satisfied by a non-executed `unpluginFactory` (`/tmp/d2-partition.txt`).
procedure 5: UNCHANGED-BY-DELTA — `dispositions.md:19-32` still the ratified class/owner table; delta only adds the empty-map observation at `:113-128`.
procedure 6: UNCHANGED-BY-DELTA — each DEFER row in `dispositions.md:23-31` still names an ignored target + characterization; SV-4 and BND-2 targets exist (`public_api_typescript_observation.rs:505`, `transport_route_equivalence_tests.rs:1906`).
procedure 7: SATISFIED-BY-DELTA — `BS0.md:34-49` now records SV-4’s acceptance/test ID; `dispositions.md` has no guard/removal IDs.
exit "full retained inventory has actual results": NOT-EVIDENCED — D2 plant shows a public bundler spelling can be unexecuted while the completeness test is green.
exit "UNPROVEN cannot count as exhaustion": UNCHANGED-BY-DELTA — adjudication quoted at landing-record.md:72-78.
exit "every genuine failure has ... acceptance/test ID; no guard or removal ID": UNCHANGED-BY-DELTA — `dispositions.md:22-31`; D1 only reconciled the SV-4 charter sentence.
exit "FC-ATOMIC-001 remains non-vacuous": UNCHANGED-BY-DELTA — `a_refused_combined_request_publishes_no_product_at_all` still `#[ignore]`d at `framework_product_surface_tests.rs:1442-1444`; AT-1 still a real RED target.
exit "route-parity/mutation-control/correction-owner regressions": NOT-EVIDENCED — D2 partition and D4 census mutation controls did not catch the plants this seat applied (`/tmp/d2-partition.txt`, `/tmp/d4-empty-wide.txt`).
exit "BF3 may close only after ratification and the four predecessors exist": SATISFIED-BY-DELTA — `program-dag.toml:82-127` has BA0/BS0/BCSS0/BRT0 as B2/B3 predecessors; verbatim act at `maintainer-ruling-section7-ratification.md:66-69`. Identity of the bound bytes is G1, not “exist”.
exit "B2/B3 stay locked": SATISFIED-BY-DELTA — live `program-state.toml` B2/B3 `status=LOCKED`, `maintainer_decision=PENDING`; act `:83-84` withholds unlock. BF3 remains `BLOCKED`/`PENDING`.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)
1. D2-PARTITION-UNEXECUTED, P1, `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1617-1633` — executed class is `outcome == "transformed"` on a named case key, not a proof that the spelling’s factory ran. Cloning `vuePublicEntry` into `rawVueFactory` keeps the partition and the raw/auto contract green. Bind each executed spelling to `publicFactory` (and object identity) of that spelling, or drive the factory inside the assertion.
2. D4-CENSUS-SELF-PATH, P1, `crates/verter_session/src/framework/suite_census.rs:121-124` + `framework_product_surface_tests.rs:52` — census counts `starts_with(CENSUS_MODULE_PATH)` and the suite owns that string. Emptying the suite and widening the path to `"framework::"` makes the documented invocation `running 1 test` / ok / exit 0. Do not trust a suite-owned prefix; discover the module path from the compiled test names / `mod` item, or fail if the path is not a unique suite prefix.
3. G1-STALE-IDENTITY, P1, `docs/arch/refactor/rev11/evidence/BF3/amd009-ratification-packet.md:73-83` — recorded blobs/digest do not match HEAD charters (broken by `179911f75`). Recompute the manifest over the current seven files and treat the new bytes as a changed-byte event (fresh identity + explicit acceptance), or revert the status-line rewrite.
4. G1-FALSE-REATTEST-CITE, P1, `docs/arch/refactor/rev11/evidence/BF3/amd009-ratification-packet.md:116` — cites `reattestation.md`; that file is absent. Delete the cite, or commit the reviews. Do not claim “no changed byte is unreviewed” from a missing document.
5. D3-ENVELOPE-MAPS-NOTHING, P2, `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1312-1325` and `:1784-1787` — `assert_source_map_artifact` (and the green Vite Vue acceptance) pass on `mappings: "A"`. The ignored Rollup target is saved only by the later identity compare. Require `mapped_segments >= 1` in the artifact helper, or stop calling the Vue Vite envelope an acceptance.
6. LANDING-COUNT-DRIFT, P2, `docs/arch/refactor/rev11/evidence/BF3/landing-record.md:507-508` vs `test-invocations.md:63-66` — record claims invocations “matched exactly” at product `running 23`, batch `8`, transport `15`; index says `24`/`9`/`16`. This seat’s `--list` shows 23 product-suite tests + 1 census = 24. Update the record or stop claiming a match.
7. LANDING-STALE-LINE, P3, `landing-record.md:514` — cites Rollup failure at `transport_route_equivalence_tests.rs:1883`; that line is now the empty-map characterization. The ignored target is `:1906` / artifact assert `:2027`.

OUT-OF-DELTA (recorded, not actioned)
- `@verter/unplugin` package.json also exports `./vue`, `./sveltejs`, `./vite`, `./rollup`, `./webpack`, `./esbuild` — not in the `dist/index.mjs` named-export partition.
- AT-2 still has no ignored correct-behavior target (`BA0.md:28`; `dispositions.md:35-51`).
- All-four-`mod` deletion still compiles and is admitted as undecidable (`test-invocations.md:36-40`).
- Live ledger BF3 notes still describe the pre-cure blockers (`program-state.toml:360`); packet says this track does not write that file.
- Pre-cure partial-coverage `NOT_PROVEN` items remain as recorded (`landing-record.md:333-337`).
```

---

## Round 2 — conformance

```
VERDICT: BLOCKING

FIX A: BLOCKING — the unrelated mapped-map plant failed parity and the empty Vue map failed the mapped-map oracle, but corrupted `sourcesContent` made the Rollup target PASS, and a relabelled/kind-forged `VerterVue` clone made both export guards PASS without calling `unpluginFactory`.
FIX B: PASS — each single `mod` removal produced E0432/E0433; empty-with-honest-path failed at 0<23, empty-plus-widen failed the census-prefix check, and restored controls ran 24/9/16 as recorded.
GOVERNANCE WORDING: BLOCKING — both full validator commands validated 62 blocks, but the claimed package digest is `6b83efe8…` while the current seven files reproduce `0bdef4b0…`; the act still names no rebound identity, so the changed-byte clause is not discharged.

DOES THE FIX DELTA INTRODUCE A DEFECT?
`SourceMapIdentity` drops `sourcesContent` values (and `names`), permitting a corrupted preserved-map artifact.
The export discriminator trusts relabellable producer fields, so an omitted raw factory can still be represented by a clone.
The governance wording changed all five charter blobs without updating the package identity it calls exact.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)
1. G1-STALE-UNBOUND-RATIFICATION, P1, docs/arch/refactor/rev11/evidence/BF3/amd009-ratification-packet.md:73, the manifest/digest does not identify the current charter bytes and the pre-byte act cannot satisfy §7's explicit changed-byte acceptance; recompute the final manifest, review it, and obtain an identity-bound maintainer acceptance.
2. D2-RELABELLED-KIND-CLONE-PASSES, P1, crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1824, the guards trust the probe's `exportName`/`kind` fields: cloning `VerterVue`, relabelling both fields, and planting a throw if `unpluginFactory` was called left both guards green; add an independent per-export sentinel/read-and-call witness that fails when the named export is not actually driven.
3. D3-MAP-CONTENT-OMITTED, P2, crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1401, parity compares sources, mappings, and only `sourcesContent` presence; replacing the embedded authored sources with corruption passed the ignored preservation target; compare the load-bearing `sourcesContent` values and `names` (or the full normalized map artifact).

OUT-OF-DELTA (recorded, not actioned)
NONE
```

---

## Round 2 — architecture

```
VERDICT: BLOCKING

FIX A: BLOCKING — `published_map()` panics on every non-map host outcome, but my unique Svelte `sourcesContent`-forgery plant still passed the green host-parity target.
FIX B: PASS — all four single-`mod` removals produced compiler errors, the widened parent path failed before counting, and the proven all-four residual correctly remains gate-integrity work rather than a forbidden scanner.
GOVERNANCE WORDING: BLOCKING — the charter qualifier is honest, but the packet's claimed `6b83…` identity is not the current `0bdef…` package and §7 is called discharged despite an act that binds no identity.

DOES THE FIX DELTA INTRODUCE A DEFECT?
`SourceMapIdentity` ignores `sourcesContent` values and other load-bearing map fields, so a changed public artifact can equal the host identity.
The status-line rewrite invalidated the package manifest while the packet continued to claim exact-byte recording and changed-byte discharge.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)
1. G1-STALE-AND-UNBOUND-BYTES, P1, docs/arch/refactor/rev11/evidence/BF3/amd009-ratification-packet.md:73, the recorded digest/blob rows are stale (`6b83…` recorded versus `0bdef…` reproduced), and lines 104-124 call §7's explicit-acceptance clause discharged although the act predates the cure and names no identity; recompute the final manifest and obtain explicit acceptance of that identity, or state that the clause remains unsatisfied.
2. D3-PARTIAL-MAP-IDENTITY, P2, crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1401, parity compares only `sources`, `sourcesContent` presence, and `mappings`; replacing the actual Svelte `sourcesContent` bytes with `CONFIRM2_FORGED_SOURCES_CONTENT` left the named green parity test PASS; compare the fully parsed host/public map artifacts, or retain every load-bearing field and value in the normalized identity.

OUT-OF-DELTA (recorded, not actioned)
NONE
```

---

## Round 2 — adversarial

```
# Confirm2 adversarial — fix delta `e827faeed..HEAD` (`ccacdc2d8`)

Default BLOCK. New plants only (`ADVERSARIAL_C2_*`, absent from HEAD). Each plant proven present/unique/new before the run; every file hash-restored; post-restore green: bundler `running 5` → 4/1, product `running 24` → 22/2.

Uncommitted `landing-record.md` +2 gate counts was not part of the planted surface and was left untouched.

## Plants (new)

| id | plant | applied proof | result |
|---|---|---|---|
| C2-SC-FORGE | Svelte Vite `loadedScriptMap.sourcesContent = ["ADVERSARIAL_C2_SC_FORGE"]`; sources/mappings unchanged | HEAD 0; probe JSON contained marker; sources=`Plug.svelte` mappings=`A` | Vite parity **GREEN** (`/tmp/confirm2-sc-vite.txt`) |
| C2-MAPPED-VUE | Vue `loadedScriptMap` → `{version:3,mappings:"AAAA",sources:["/ADVERSARIAL_C2_MAPPED_VUE.vue"]}` | HEAD 0; probe emitted that object | Vite acceptance **GREEN**; characterization RED at Vue floor (`/tmp/confirm2-mapped-vue-*.txt`) |
| C2-VUE-SOURCES | Vue sources retargeted to `/ADVERSARIAL_C2_VUE_SOURCES.vue`; mappings length kept 84 | HEAD 0; probe sources forged, mappings_len=84 | Vite acceptance **GREEN** and characterization **GREEN** (`/tmp/confirm2-vue-sources-*.txt`) |
| C2-ROLLUP-MAPPED | `vueRollupEntry.publicTransformMap` stuffed with mapped envelope; `publicTransformHasMap` still false | HEAD 0; probe showed stuffed artifact + hasMap=false | Ignored Rollup **RED at identity** (`/tmp/confirm2-rollup-mapped.txt`) — not green |
| C2-KIND-FORGE | skip `driveExport("unpluginFactory")`; clone VerterVue with `exportName`+`kind:"raw-factory"` | HEAD 0; valueType stayed `object` | partition **GREEN** and distinction **GREEN** (`/tmp/confirm2-kind-forge-*.txt`) |
| C2-SAMEKIND | relabel VerterVue as VerterSvelte (kind kept) | HEAD 0; svelte accept bits became Vue's | partition GREEN; distinction **RED** on `.vue` include (`/tmp/confirm2-samekind-*.txt`) |
| C2-SIDEWAYS | empty all 23 product `#[test]` via `#[cfg(not(all()))]`; `CENSUS_MODULE_PATH="verter_session::lib_tests"` | HEAD 0; 24 marker hits; hash ≠ orig | documented filter `running 1 test` / census **ok** / exit 0 (`/tmp/confirm2-sideways.txt`) |
| C2-EMPTY-ONLY | same emptying; path left as `module_path!()` | HEAD 0; 23 markers | census **RED** at floor 0 < 23 (`/tmp/confirm2-empty-only.txt`) |
| C2-NOMOD-* | comment out each of the four `mod` lines | HEAD 0 each; restored after each | all four **compile errors** (E0432 / E0433) (`/tmp/confirm2-nomod-*.txt`) |
| C2-PUBMAP | `host_node` always `source_map: None` | HEAD 0 | Vite parity **panics** via `published_map` (`/tmp/confirm2-pubmap.txt`) |

## Package identity (personally hashed)

```
claimed: 6b83efe82c93411df78fc768c63043e8f91ed1b54cb754b769120c70a6a4f2a6
live:    0bdef4b095cf6fac264a133507c4a835e4cb98a86e0f1587383c725e1c9066b8
```

Charter blobs claimed `c7095a7f4`/`1b2fdbbf4`/`6c0189170`/`58f8ffcbd`/`472a0b88a`; live `c1bc7e1b2`/`76f5c8e74`/`589cfdb7a`/`66ff6f5d8`/`be3b9d863`. AMD-009 + `program-dag.toml` match. Drift is `179911f75` status-line rewrite recorded under the pre-rewrite identity.

`reattestation.md` exists (G1-FALSE-REATTEST-CITE closed as a cite).

## Judgements

- Parity is a real sources+mappings+presence oracle on Svelte Vite and Rollup Vue. A mapped unrelated envelope no longer greens Rollup. `sourcesContent` **values** and `file`/`names` are dropped. Vue Vite has no host counterpart; `assert_mapped_source_map_artifact` is the whole acceptance and is satisfied by any mapped envelope, including a sources-retargeted real mapping.
- `published_map()` panics with the host outcome; it does not return `None`.
- Mutual registration: each single-`mod` removal is a compile error. Emptying a suite while keeping `module_path!()` fails the floor. Emptying **and** retargeting the suite-owned `&str` to a disjoint large module greens the documented invocation — prefix validation does not bind the path to the suite.
- Derived export cases: a same-kind clone fails on accept bits. VerterVue and `unpluginFactory` collide on `(accepts_vue, accepts_svelte)`. A clone that also patches `kind` (and need not even patch `valueType`) keeps both completeness and distinction green. Kind is still a probe opinion.
- Svelte envelope-only + host parity is honest: the product and the host both map nothing; characterization + SV-3 own that. It does not leave a maps-nothing *envelope* as the Vue/Rollup acceptance.
- Governance status lines no longer claim byte-level blessing. The packet still says the manifest “is computed over the exact post-cure bytes” and gives a reproduction script that does **not** reproduce on this tree. Limit is accurate about the act; identity claim is not.

## Restore

All six hashed files byte-identical to pre-plant snapshots. Post-restore green as above.
```

---

## Round 3 — conformance

```
VERDICT: BLOCKING

FIX A: PASS — one-byte `sourcesContent[0]` plants made the green Vite parity and ignored Rollup target each run 1 test and fail (rc 101); restored Vite control passed, and parsed `Value` equality excludes no non-null member.
FIX B: BLOCKING — I cloned `VerterVue`, forged `exportName`, all evidence, and `pluginKeys` to the raw-factory shape, rigged real `unpluginFactory` driving to throw, and both named guards still passed (rc 0).
FIX C: BLOCKING — I emptied `svelte_batch_route_tests` and retargeted its census constant to direct sibling `framework::script_facts`; the documented filter ran only the census test and passed (1/1, rc 0).
FIX D: PASS — retargeting both Vue `sources` entries to `/elsewhere/Unrelated.vue` made the named green target run 1 test and fail (rc 101); restored control passed.
PACKAGE IDENTITY: PASS — computed `0bdef4b095cf6fac264a133507c4a835e4cb98a86e0f1587383c725e1c9066b8`, exactly matching the packet.

DOES THE FIX DELTA INTRODUCE A DEFECT?
FIX B leaves the claimed export-to-observation tie forgeable because both “independent” observations are fields controlled by the same probe.
FIX C leaves a new same-parent sideways retarget: another direct sibling with enough tests clears the floor for an empty suite.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)
1. C3-CONF-1, P1, crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1702, `pluginKeys` is not independent of `evidence`; both come from the same forgeable JSON record, and a fully forged clone passed both guards without driving `unpluginFactory`; bind execution evidence independently of the probe record (the stated in-process observation boundary) and add this exact full-forgery RED mutation.
2. C3-CONF-2, P1, crates/verter_session/src/framework/suite_census.rs:92, direct-sibling validation proves only location class, not that a census path names its owning suite; an empty batch suite retargeted to `framework::script_facts` passed; bind every census row to its exact compiler-derived suite identity, then prove empty-suite retargets to both disjoint and same-parent modules fail.

OUT-OF-DELTA (recorded, not actioned)
NONE
```

---

## Round 3 — architecture

```
VERDICT: BLOCKING

FIX A: PASS — forged one byte in both Svelte `sourcesContent[0]` records; the green Vite target failed at line 2121 and the ignored Rollup target at line 2281, then the restored Vite control passed.
FIX B: BLOCKING — cloned `VerterVue` into `unpluginFactory`, forged its name, callable evidence, and `pluginKeys`, and armed the real factory path to throw; both claimed guards still passed.
FIX C: BLOCKING — disabled all eight batch-suite tests and retargeted its path to direct sibling `framework::script_facts`; the documented filter ran only the census test and passed by counting that sibling's 17 tests.
FIX D: PASS — retargeted both Vue `sources` entries to `/elsewhere/Unrelated.vue`; the green target failed at line 2084, then passed after exact restoration.
PACKAGE IDENTITY: PASS — computed `0bdef4b095cf6fac264a133507c4a835e4cb98a86e0f1587383c725e1c9066b8`, matching the packet and all seven recorded blob OIDs.

DOES THE FIX DELTA INTRODUCE A DEFECT?
FIX B treats a second probe-authored JSON field as independent evidence, so a probe-forged case can satisfy both observations.
FIX C accepts any direct sibling rather than binding each census row to its actual suite, leaving a green sideways-retarget bypass.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)
1. ARCH-FIX-B-1, P2, crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1703, `pluginKeys` is emitted by the same mutable probe as `evidence`; forging it to omit `configResolved` made a cloned wrapped case satisfy both guards while the real raw factory was never driven. Derive the second observation outside the probe record under test and bind it directly to `module_[exportName]` (for example, a test-owned fixed/in-process observer).
2. ARCH-FIX-C-1, P2, crates/verter_session/src/framework/suite_census.rs:91, direct-sibling validation does not prove the path is this suite's path; `svelte_batch_route` can count uncensused sibling `script_facts` and clear floor 8 while empty. Derive the module identity from an imported suite marker's compiler type path, or compare against a census-owned exact expected sibling identity before counting.

OUT-OF-DELTA (recorded, not actioned)
NONE
```

---

## Round 3 — adversarial

```
VERDICT: BLOCKING

FIX A: PASS — new plants `CONFIRM3_A_FILE` (forge Svelte `file` only) and `CONFIRM3_A_EXTRA` (add `x_confirm3`; all specified fields intact) failed Vite parity and the ignored Rollup target at whole-artifact `assert_eq!`; dump included `version`/`file`/`names`/`sources`/`sourcesContent` values/`mappings`; both plants absent from HEAD, unique in WT+probe JSON, restored, green Vite re-ran ok.
FIX B: PASS — new `CONFIRM3_B_CLAIMED` (skip-drive `unpluginFactory`, clone VerterVue, evidence `{function, length:99, name:CONFIRM3_B_EVIDENCE}`, pluginKeys left, throw not hit) failed both guards on flattened `configResolved`; `CONFIRM3_B_FLOOR` (same plus drop `configResolved`+`handleHotUpdate` so keys == live raw factory) greened both; restored, greens ok.
FIX C: PASS — new disjoint plant empty product + `CENSUS_MODULE_PATH="verter_session::compile"` (77 tests, not `lib_tests`) listed 1 test and failed sibling check; emptying-alone and single-`mod` compile errors were not re-planted (unchanged this delta); restored, product 24 / batch 9 greens.
FIX D: BLOCKING — new `CONFIRM3_D_ONE` (second Vue `sources` only) failed names-only as claimed; new `CONFIRM3_D_ID` (both `sources` + probe `id` → `/elsewhere/CONFIRM3_D_ID.vue`, mappings/`sourcesContent`/`scriptRequest` left as `/probe/Plug.vue?vue&type=script&lang.js`) greened Vite acceptance and the characterization; restored, green ok.
PACKAGE IDENTITY: PASS — reproduction command digest `0bdef4b095cf6fac264a133507c4a835e4cb98a86e0f1587383c725e1c9066b8`; all seven blob OIDs match the packet.

DOES THE FIX DELTA INTRODUCE A DEFECT?
FIX D ties Vue `sources` to probe-controlled `vue["id"]`, so a retargeted map still passes when `id` is rewritten to match.
FIX C does not introduce a new pass; a same-parent retarget of the emptied batch suite at `framework::registry` (10 ≥ floor 8) still greens (residual).
FIX B's double-forge pass is the floor they stated, not a new defect.

FINDINGS (each: id, severity P1/P2/P3, file:line, what is wrong, what would fix it)
1. D-PROBE-ID-IS-NOT-THE-REQUEST, P1, crates/verter_session/src/framework/transport_route_equivalence_tests.rs:2087, `assert_source_map_names_only` compares `sources` to `vue["id"]`; forging both fields greens a map whose `scriptRequest` is still `/probe/Plug.vue?...`. Pass the literal requested carrier `"/probe/Plug.vue"` (or `scriptRequest`'s path before `?`), not a probe label.
2. C-BATCH-SIBLING-RETARGET, P2, crates/verter_session/src/framework/suite_census.rs:91, sibling binding closed disjoint retargets; emptying `svelte_batch_route_tests` and pointing its constant at `verter_session::framework::registry` still counts 10 tests and clears floor 8. Bind the counted prefix to this suite, or raise/shape the floor so no other `framework::` sibling can satisfy it.

OUT-OF-DELTA (recorded, not actioned)
AMD-009 §7 act still names no digest; this delta only refreshed the recorded numbers.
landing-record Surface 1/3 counts `24387`/`8621` were not re-executed (workspace gate forbidden).
Vue Vite still has no host map counterpart; names-only is the whole WHAT-acceptance.
```

---

## Round 4 — adversarial (terminal)

```
VERDICT: BLOCKING

FIX A: PASS — new plants. `CONFIRM4_A_SAMENAME` emptied `svelte_batch_route_tests` and pointed `CENSUS_WITNESS_PATH` at the product suite's existing `this_suite_is_registered_with_the_census` (only same-named tests on HEAD). Listing: 0 batch tests; product witness still present. Census RED at pairwise (`framework::framework_product_surface_tests::` vs itself). `CONFIRM4_A_WITNESS_ONLY` left the constant honest and cfg'd out every batch test except the witness (listing: that one test). Census RED at floor `1 < 8`. Both markers absent from HEAD, unique in WT, hash-restored, post-restore census green. Extra plant `CONFIRM4_A_DECOY` (not required to pass the claim): added the same witness name at `framework::registry` module root and retargeted the batch constant there — census GREEN (11 tests under `framework::registry::` ≥ 8) and batch `covers()` GREEN. Residual, not the one-site retarget they closed.

FIX B: PASS — new `CONFIRM4_B_FORGE` rewrote `vuePublicEntry.id` and `loadedScriptMap.sources` together to `/forged/CONFIRM4_B_FORGE.vue` (HEAD 0; live probe JSON had both). Maps test RED at `assert_bundler_case_carriers` (`Some("/forged/CONFIRM4_B_FORGE.vue")` vs `Some("/probe/Plug.vue")`); include-contract RED the same way. Host-route + characterization still GREEN (they never used case `id` as the subject). Restored; maps+include green. Extra `CONFIRM4_B_NULL_OPP`: `oppositeId=null` on both public cases — include and maps GREEN. Residual pin skip, not the claimed sources+id hole.

FIX C (as claimed, INCLUDING its stated limit): BLOCKING — hide/type plants fail as claimed; the included limit is false.
- `CONFIRM4_C_HIDE` dropped `unpluginFactory` from `exports`+`exportCases`. Both guards RED: probe enum ≠ observer `Object.keys` (observer still saw `unpluginFactory`).
- `CONFIRM4_C_TYPE` set `VerterSvelte.evidence.valueType="function"`. Both guards RED: probe `"function"` vs observer `"object"`.
- `CONFIRM4_C_FULLFORGE` skipped `driveExport("unpluginFactory")`, kept true factory evidence+pluginKeys, cloned `VerterVue.carriers`. Both guards GREEN (claimed residue confirmed).
- All three absent from HEAD, unique, hash-restored; partition green after each restore.
The hide/type closures are real. The stated reason they cannot close the full-forgery is not.

IS THE STATED LIMIT TRUE? no — they conflate "true readings agree with a re-read" (tautology) with "the two spellings cannot be separated" (false). Out-of-process, `VerterVue` and `unpluginFactory` separate on: `typeof` (object vs function), `.vite` callability (function vs undefined), and pluginKeys flattening (`configResolved`/`handleHotUpdate` only on the `createUnplugin` wrapper). Measured in `/tmp/confirm4-adv/C-limit.txt`. A `Proxy` on the factory *export binding* counted 0 applies from `VerterVue.vite({})` and 1 from calling the wrapped factory — so invocation of that spelling is observable in Node without in-process Rust driving (module hook / `--import` wrapping `dist/index.mjs` during the probe). A wholesale VerterVue-case clone would already fail this delta's observer + `assert_evidence_matches_the_driven_plugin`. The remaining hole is only a probe that prints the factory's *true* evidence/pluginKeys/carriers without applying that export. That is not "the spellings behave identically," and it is not closable-only-in-process.

NEW DEFECTS INTRODUCED:
- `transport_route_equivalence_tests.rs:1679-1687` + `test-invocations.md:207-218` — false limit / internally inconsistent "clone including pluginKeys" story (VerterVue pluginKeys are flattened; factory's are not). Dishonest residue write-up, not a test regression.
- No previously-passing assertion was weakened. Map tie and include/maps pins are stricter. Census sibling/pairwise/floor kept.

FINDINGS:
1. C-LIMIT-FALSE, P1, `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1679` and `docs/arch/refactor/rev11/evidence/BF3/test-invocations.md:207` — claims no out-of-process shape/behavioural observation can separate `VerterVue` from `unpluginFactory` and that only in-process driving can witness a run. Both halves are false (see limit experiment). Fix: state the residue as invocation-attribution of already-true readings; or instrument the probe import and require a non-zero apply count per driven export.
2. A-DECOY-WITNESS, P2, `crates/verter_session/src/framework/suite_census.rs:251` — row is bound to "a test of this path exists," not to the suite that exported the constant. Planting `this_suite_is_registered_with_the_census` on `framework::registry` + retargeting the batch constant greens the batch census. Fix: require `witness_module(constant)` to equal the declaring suite's `module_path!()`, or stop accepting a free `&str`.
3. B-NULL-OPPOSITE, P3, `crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1479` — `oppositeId` null skips the opposite pin; include+maps still green. Fix: require `oppositeId` present and equal to the test constant.

OUT-OF-DELTA:
- Host-route and characterization still ignore Vue case `id`/`sources` (pre-existing; not this delta's acceptance).
- `record["defaultIsVerterVue"]` is still probe-only; observer does not re-check `default === VerterVue`.
- Observer collects `rollupIsCallable` and never compares it.
- Confirm3 package-identity / AMD-009 act-without-digest not re-opened.
```
