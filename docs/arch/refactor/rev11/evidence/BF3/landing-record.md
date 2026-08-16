# BF3 landing record

Base: `b2d7b428b` (`program/architecture-lock` tip after the ratified audit amendment landed).
Candidate: `23452fd6c`, tree `ace9e6d77` — one squashed, test-only commit. Its evidence commit
`88d7ae1c0` and this record follow it on the program branch, as BV0's did.

Everything below is drawn from the committed artifacts in this directory, from the commits
themselves, or from verification the track orchestrator ran directly. Where an artifact does not
exist to support a claim, the gap is stated rather than narrated around — see
[Gaps in this record](#gaps-in-this-record).

## Arc

1. **A scope conflict, resolved by consult before any implementation.** The charter as ratified
   mandated a PRODUCTION mechanism for a known-wrong-but-successful cell: detect it before
   publication from typed data, return typed non-success, publish no partial product, and retract
   the whole capability cell when the broken subset is not safely distinguishable — each bound to
   a `BF3-RET-*` record with a removal ID. That collides head-on with the standing project rule
   that a wrong output is a BUG, not an error path. An open-ended, option-free architecture
   consult (prompt: [`scope-consult-prompt.md`](scope-consult-prompt.md); verbatim ruling:
   [`scope-consult-ruling.md`](scope-consult-ruling.md)) ruled the retraction procedure
   *"procedurally understandable but architecturally wrong"*, found no principled Svelte
   exception, and reshaped the block into **conformance exhaustion plus correction dispatch with
   ZERO production mechanism** — safety coming from refusing to advance the program (B2/B3 stay
   locked until the corrections land), not from teaching the shipped compiler to recognise its own
   defects. The deviation was recorded at [`scope-memo.md`](scope-memo.md) before work began, and
   the half that changes normative program text was escalated rather than executed.
   The consult also carved out the one legitimate production refusal: Svelte's existing
   `ServerGenerate` arm, which decides a real capability boundary from the typed request before
   any emitter work. The block tests and records it and adds nothing to it.

2. **Probe first, dispose second.** Implementation ran as five phases in one resumed `claude` CLI
   session in a dedicated worktree, each dispatched with an explicit zero-production-change
   ceiling. The exact dispatch bytes for every phase are in
   [`context-packet.md`](context-packet.md).
   - **A** — a Svelte counterpart to the existing Vue official-conformance gate: a digest-verified
     seed-matrix loader over the same committed `goldens/manifest.json`, the six `svelte@5.56.8`
     client cells driven through the genuine shipped compile route into
     `bin/check-candidate.mjs --authoritative`, plus mutation-discrimination plants per axis
     family and the six server cells' typed refusal characterised as it stands.
   - **B** — the remaining reachable-success product and route inventory: host virtual-file
     products, compile/analyze entry points, batch, NAPI, WASM, and the bundler plugin, each
     enumerated from source with its route identity proven separately from its semantic cell.
   - **C** — see (3).
   - **D/E** — closing the exhaustion gaps an independent adjudication refused to accept as
     `UNPROVEN` (see (4)).

3. **The re-investigation that overturned the block's own headline.** An independent skeptical
   re-investigation of phase A confirmed three findings and overturned four. Its largest
   correction: because `dev` has no public or default spelling anywhere — verified independently
   across NAPI, the protocol/WASM input, the host `CompileProfile`, and unplugin — the `dev0` and
   `dev1` goldens issue the **identical** production request, so the six committed client records
   collapse to **three distinct reachable requests**, and phase A's headline "6/6 client cells
   fail" was materially overbroad. It also found the diagnostics plant had no application proof,
   that the mapping plant only proved a blatant content-integrity violation rather than the
   anchor/segment-coverage machinery the `legacy-slots` finding actually rests on, and that the
   `legacy-slots` map measurement was an unverified claim rather than a settled number. All four
   were corrected in phase C.
   Both review seats then read the charter's "the exact six client cells" as a quantifier and read
   the three-request collapse as failing it. The fix satisfied both readings rather than picking
   one: **all six client cells (and all six server cells) are individually driven and recorded**,
   with reachability demoted from a filter to an attribution label — the recorded reason a `dev1`
   cell's divergence is not attributed to the compiler. The refused `props-events` request, which
   never enters the comparator at all, is recorded as an explicit cell outcome so no cell in the
   inventory is silent.

4. **An architecture adjudication that refused `UNPROVEN` as exhaustion.** Phase B recorded the
   TypeScript-observable family, Svelte `compile_many`, and the NAPI/WASM transports as
   `UNPROVEN`. An independent adjudication (prompt:
   [`adjudication-prompt.md`](adjudication-prompt.md); verbatim ruling:
   [`adjudication-ruling.md`](adjudication-ruling.md)) ruled that two of those three reasons were
   simply wrong, and set the standard an honest `UNPROVEN` must meet:

   > An honest `UNPROVEN` record identifies the exact claim, the missing discriminating
   > observation, why existing evidence cannot decide it, its owner, and a falsifiable closure
   > condition — and it blocks acceptance. A gap dressed as `UNPROVEN` uses nondiscriminating
   > equality such as `any == any`, calls a batch or transport route "the same" without executing
   > its boundary, or lets explanatory prose count as an actual result.

   The `any == any` diagnosis was exact: the TypeScript observation host built its program solely
   from the supplied artifact map, so with no resolvable framework declarations every surface
   compared equal at `any` and the family proved nothing. Phases D and E provisioned the real
   observation domains — the pinned framework closures under the harness's `.oracle-installs`, and
   a second domain over the workspace's own built declarations — made module-resolution failure
   REFUSE the authoritative observation instead of degrading silently, and added planted controls
   proving a correct and an empty prop surface observe differently. The same ruling supplied the
   defect ownership decomposition this block dispatches to (`BS0` / `BA0` / `BCSS0`, with `BRT0`
   added for the route/transport rows), and the governance shape: an audit may close with its
   defects uncorrected **only** if a ratified amendment first makes every resulting correction
   block a mandatory B2/B3 predecessor.

5. **Three review rounds, four fix passes.** Review seats were external CLIs only — Codex Sol at
   high reasoning effort for the conformance, architecture and targeted-delta mandates, Grok 4.6
   at extra-high effort with an explicit default-to-BLOCK posture for the adversarial mandate.
   Every fix brief is reproduced verbatim in [`context-packet.md`](context-packet.md), and each
   quotes the finding it answers, so the findings survive even though the seats' own reports do
   not (see [Gaps in this record](#gaps-in-this-record)). The highest-value catches:
   - **A charter prohibition was actually being violated.** Both round-1 seats independently found
     the block inferring meaning from generated output — `split_once("$.each(")` to read the
     `{#each}` flags argument, and output substrings (`svelte/internal/client`, `_sfc_main`,
     `?vue&type=`) to classify batch carrier identity. Both were replaced with structural
     evidence: the emitted module is parsed with `oxc_parser` and the call's argument read from
     the AST, and carrier identity comes from the host's own `file_language` adapter row.
   - **A test whose name promised an axis its body ignored.** The adversarial seat forced
     `source_map: true` in production against
     `the_options_taking_audited_compile_entry_honours_its_explicit_options` and the test **stayed
     green** — it asserted only `canonical_id`. Fixing it triggered a re-audit of every landed test
     against the question "does this assert the thing its name claims?".
   - **A completeness check that was a union, not a partition.** Dropping `ensureIdeCompiled` from
     the executed-spellings list kept the transport completeness test green, because the spelling
     also appeared in the out-of-scope list. Two lists that may both contain the same name cannot
     prove completeness; the classification became a partition where membership in two classes is
     itself a failure.
   - **A fail-closed gate with a known bypass.** The TypeScript observation domain refused an
     unresolvable `import()` type node, an import declaration and a module augmentation, but a
     `require("svelte")` call slipped through and yielded `any`; a later round found
     `/// <reference path="…" />` did too. The gate now enumerates reference channels through the
     compiler's own scanner rather than a hand-written form list.
   - **The false-green runtime axis — the most dangerous finding in the block.** The Svelte client
     runtime smoke mounted compiled modules through a bare `svelte/internal/client` specifier
     whose resolution depended on where the scratch directory happened to sit; one directory
     further up sits a **different Svelte version**. On one tree it bound the pinned runtime and
     passed; on another it bound the other copy and died inside the official runtime. A test that
     passes while measuring the wrong runtime is worse than no test: the executor now terminates
     that walk at its first step, reports the runtime it bound, and the test pins it to the derived
     pinned version, so a mount measured against anything else FAILS rather than deciding nothing.
     This is recorded in [`test-invocations.md`](test-invocations.md) under "The same hazard in
     the other direction".
   - **A reported pass that was red.** The implementer's fix-round-2 report claimed both emitting
     client requests "mount and render byte-identically to their goldens". The track orchestrator
     ran the named test directly and it FAILED — the pinned OFFICIAL control did not mount, so the
     axis decided nothing. An independent reviewer found the same thing. The final fix brief made
     the process failure, not the test, the first thing to explain.
   - **Vacuous green runs.** Three test-filter invocations matched ZERO tests and exited 0
     (libtest has no alternation syntax; two suites are feature-gated). The disclosure existed only
     in a temporary file until the last fix pass landed it in the tree; it is now
     [`test-invocations.md`](test-invocations.md), whose first rule is *read `running N tests`,
     never the exit code*.
   - **Two bundler findings were re-measured and one was withdrawn.** `BND-1` had called the
     Vue-pinned legacy factory directly on a `.svelte` id; executing the two public pinned entries
     showed each applies its documented include contract, and the finding is REJECTED. `BND-2` was
     split: the public Vite virtual-script products do carry their requested maps and are green,
     while `VerterVue.rollup()`'s inline product drops the host-published map and remains a defect.

6. **Amendment ratification.** The half of the consult's direction that changes normative program
   text was packaged as AMD-009 and ratified by the designated maintainer through a binding
   product ruling ([`maintainer-product-ruling-no-error-on-bad-output.md`](maintainer-product-ruling-no-error-on-bad-output.md)),
   bound to package commit `9e457ca78`; the record is
   [`amd009-ratification-packet.md`](amd009-ratification-packet.md). It supersedes the retraction
   procedure and creates `BA0`, `BS0`, `BCSS0` and `BRT0` as mandatory B2/B3 predecessors. It
   explicitly does **not** accept this block and does **not** unlock B2 or B3.

7. **Gate fixes found by the track orchestrator's own canonical run.** Five architecture guards
   failed on the final tree — none of them a review finding, all of them real:
   `lib_rs_stays_under_line_ceiling` (six `#[cfg(test)] mod` declarations pushed
   `crates/verter_session/src/lib.rs` from 856 to 862 lines), the three `std::fs` guards
   (`no_std_fs_in_semantic_session_paths`, `no_std_fs_outside_native_fs_or_allow_list`,
   `vfs_boundary_is_authoritative`), and `no_direct_oxc_parser_calls_outside_scheduler_path` — the
   last one triggered precisely by the structural reader that had replaced the forbidden
   string-scan. The ceiling was not raised: the test modules moved under
   `crates/verter_session/src/framework/`. The guard entries follow the existing sanctioned
   precedent (`crates/verter_workspace/tool-output-allowlist.toml`, whose `bf2_seed_matrix.rs`
   entry does the same thing for the same reason), each naming an exact file with its own
   rationale; no guard was weakened and no pattern broadened.
   A sixth failure, `tracked_files_contain_no_machine_specific_path_markers`, was **not** this
   block's: the marker was a literal home-directory path in the program ledger's own prose,
   present at this block's base and on the program tip, in a file this block is forbidden to edit.
   It was reported upward and fixed separately at `81bcaf263`.

## What this block did NOT do

No production behaviour change of any kind — no guard, typed refusal, publication gate,
withholding path, retraction table, runtime tracker, or known-divergence list, and no compiler
correction. `git diff b2d7b428b 23452fd6c --stat` is 30 files, +10231/−24, and every changed
`crates/` path is a `#[cfg(test)]` module, a committed test fixture, an architecture-guard
allowlist entry, or the `Cargo.toml` feature declarations that gate the new suites out of the
default hermetic run.

## Dispositions

Thirteen rows, in [`dispositions.md`](dispositions.md): **10 DEFER** and **3 REJECTED as a
defect**. Every DEFER's resolution gate is its owner block's acceptance, no later than plan close,
and before any downstream dispatch; every DEFER names an acceptance identifier and a gating test.

| owner | rows |
|---|---|
| `BS0` | `SV-1` `{#each}` flags set `EACH_ITEM_REACTIVE` where official does not (21 vs 20) · `SV-2` instance-script `$props()` refused where official accepts · `SV-3` client map omits authored script-declaration provenance · `SV-4` untyped `$props()` destructure publishes an empty props surface |
| `BA0` | `AT-1` a combined IDE-requesting compile publishes the TSX product after a runtime refusal · `AT-2` per-entry batch atomicity |
| `BCSS0` | `CSS-1` the standalone CSS route accepts and ignores `sourcemap: true` |
| `BRT0` | `RT-1` the batch route compiles `.svelte` as Vue and drops its refusals · `TR-1` NAPI returns null where WASM throws for a missing product · `BND-2` the Rollup/non-Vite inline transform drops the host-published map |
| rejected | `RA-1` parse-derived `list_virtual_files` naming · `RA-2` unreachable `has_runtime_surface` arm · `BND-1` the documented entry-specific bundler include contract |

Each genuine defect carries a pair: an `#[ignore]`d conformance target asserting the correct
official behaviour (authored from the oracle's own output, and the correction's acceptance gate),
and a green characterization pinning the exact current divergence so a silent regression or a
partial fix is caught in either direction. `AT-2` is the one row whose driven evidence does not
reproduce it — the note in [`dispositions.md`](dispositions.md) records why (the failure class
that would exercise a genuine *Svelte* refusal inside a batch is unreachable while `RT-1` stands),
and the row's class and owner are left exactly as ratified rather than quietly re-classed.

## Verification (run directly by the track orchestrator on the landed tree)

Run in a dedicated worktree at `81bcaf263` after `pnpm install --frozen-lockfile` and
`pnpm build:ts` (both rc=0), on the toolchain the repository pins — `rustc 1.97.1 (8bab26f4f)`,
`clippy 0.1.97 (8bab26f4f6)`. Every figure below was observed directly, not taken from a worker's
report.

- `node scripts/gate.mjs --build-jobs 4 --test-threads 4 --memory-limit 12GiB` — **VERDICT: PASS
  (all three surfaces green)**, exit 0.
  - Surface 1 (nextest, process isolation): **24383 run, 24383 passed**, 0 failed, 0 timed out,
    0 tolerated, 584 skipped.
  - Surface 2 (in-process `verter_session` libtests from the same archive): 3 suites clean,
    0 tolerated failures.
  - Surface 3 (shipped `no-debug-assertions` cfg, `verter_session` + `verter_scheduler`):
    **8617 run, 8617 passed**, 566 skipped.
  - Freshness-tooling preflight reported `already-present — tolerance DISABLED`, so the
    proto/TS byte-pin ran genuinely and a freshness failure would have been a hard failure.
- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` — clean.
- `cargo check --workspace --release` — clean.
- Every suite in [`test-invocations.md`](test-invocations.md), at its recorded invocation, matched
  its recorded expectation exactly: Svelte official-conformance `running 19` → 16 passed/3 ignored;
  PublicApi-TSC-declaration `running 8` → 7/1; IDE-TSX `running 3` → 3/0; product-route inventory
  `running 22` → 20/2; batch route `running 7` → 6/1; transport equivalence `running 11` → 10/1.
- All **eight** `#[ignore]`d conformance targets were run individually with `--ignored` and each
  reported `running 1 test` and **FAILED at its own named assertion**, not at setup, a missing file
  or a compile error: `each_flags_…match_the_official_compiler` (`…gate.rs:857`),
  `a_runes_props_read_in_the_instance_script_compiles…` (`:903`),
  `the_client_source_map_covers_every_required_authored_anchor` (`:965`),
  `an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript`
  (`public_api_typescript_observation.rs:523`),
  `the_standalone_css_route_publishes_valid_requested_maps…`
  (`framework_product_surface_tests.rs:1262`),
  `a_svelte_batch_matches_the_single_file_route_item_for_item`
  (`svelte_batch_route_tests.rs:622`),
  `a_refused_combined_request_publishes_no_product_at_all`
  (`framework_product_surface_tests.rs:1458`), and
  `the_bundler_rollup_inline_transform_preserves_requested_source_maps`
  (`transport_route_equivalence_tests.rs:1494`).
- `packages/framework-conformance-harness` full `npx vitest --run`: **640/640 across 30 files.**
  (This run was warm; see the timeout-fragility finding below, which reproduces only on a cold
  first run of the new executor spec.)

This record itself adds one Markdown file under `docs/`. The gate above ran on the tree without it;
after committing it, the three guards a new tracked document can affect —
`tracked_files_contain_no_machine_specific_path_markers`, `tracked_paths_are_portable`, and
`every_critical_rule_in_docs_has_registered_guard` — were re-run directly against the final tree
and pass.

## Re-attestation, and why this block is NOT acceptance-recommended

The block used its three review rounds. Per this program's round cap, closing verdicts came from a
bounded per-mandate re-attestation against the final candidate, scoped to the delta
`a1ef593d1 → 81bcaf263` (35 files, +3842/−214) — the part of the work that never received a clean
review across all three mandates, including the entire gate-fix range. Seats were external CLIs
only: Codex Sol at high effort for conformance and architecture, Grok 4.6 at extra-high effort with
an explicit default-to-BLOCK posture for adversarial. Each answered two questions about that delta
only: did each change do what it claims, and does any change introduce a defect. Findings outside
the delta were recorded, not actioned.

**All three returned `BLOCKING`.** The re-attestation did not close the block; it opened new
findings, and the largest of them is a governance defect that no track-level actor may settle.

### The governance blocker (both Codex seats, independently; confirmed by consult)

The only recorded designated-maintainer act is
[`maintainer-product-ruling-no-error-on-bad-output.md`](maintainer-product-ruling-no-error-on-bad-output.md).
Its "Ratification effect" section says, verbatim, *"This ruling ratifies the AMD-009 §1 and §2
no-retraction direction"*, and *"the live program ledger is unchanged by this evidence record."*
[`amd009-ratification-packet.md`](amd009-ratification-packet.md) nevertheless records the package as
RATIFIED with the full effect of AMD-009 §7 — reshaping this charter, creating `BA0`/`BS0`/`BCSS0`/
`BRT0` as mandatory B2/B3 predecessors, and superseding AMD-006 §4/§8.1 and AMD-005 §5/§12 — while
stating on its own face that the accept line "is not represented as a verbatim maintainer chat
response". Two live documents in the same tree still say the amendment is UNRATIFIED
([`scope-memo.md`](scope-memo.md); `../framework-conformance/bf3-safety-retraction-scope.md`).
Separately, AMD-009 requires fresh reviewed identities and explicit acceptance for any changed byte
of the package, yet five charters changed by +43/−30 after the bound identity `9e457ca78` — including
substantive scope changes to `BRT0.md` and `BCSS0.md` — with no later acceptance recorded.

An independent governance consult was dispatched on exactly these verified facts. Its ruling: the
maintainer ratified only the §§1-2 no-retraction direction; the packet's full-§7 claim is an
overstatement that must be corrected; the charter, DAG and ledger changes do not stand as ratified,
as execution authority, or as evidence of ratification respectively; the post-binding charter edits
were permissible only as edits to an unratified proposal; and no track-level orchestrator may make
the binding correction. Its ordered remedy is program-orchestrator and maintainer work, not track
work: correct the evidence state so full ratification is not represented as authoritative, produce
one internally consistent exact package, have it reviewed, obtain an explicit maintainer act on
§7, then reconcile DAG and ledger, then re-attest the resulting candidate.

The audit's own technical work is not what fails here. What fails is the authority the reshaped
charter, the four correction-owner blocks, and the DAG edges rest on.

### Findings inside the delta, verified and not fixed

Recorded rather than actioned: the block needs a fresh package, a fresh maintainer act and a fresh
re-attestation regardless, so closing these against the current candidate would be work landed
against a tree that must be re-reviewed anyway. Each is reproducible from the reports.

- **`BS0.md` is factually wrong about the tree.** Its SV-4 row says there is no `#[ignore]`d
  correct-behaviour target "because the projector defines the correct surface", and its required
  exits say "the three ignored correct-behavior targets". [`dispositions.md`](dispositions.md) names
  a fourth, and all three seats ran it: it executes one test and fails for its stated reason
  (TypeScript observes `{}` rather than required `label` plus optional `disabled`). Correcting the
  charter is itself a post-binding charter edit, so it is part of the package rebind above.
- **Bundler public-spelling coverage regressed inside the delta.** At `a1ef593d1` the probe drove
  `unpluginFactory`; at `81bcaf263` it drives only `VerterVue` and `VerterSvelte`. The built
  artifact exports `Verter`, `VerterVue`, `VerterSvelte`, `unpluginFactory` and `default`, so three
  public/default spellings are now neither executed nor partitioned — against an exit that says
  every public/default route. Verified directly:
  `git show a1ef593d1:packages/unplugin/scripts/probe-bundler-route.mjs` names `unpluginFactory`;
  the current file does not.
- **The Rollup acceptance target is non-discriminating against a lying boolean.** The adversarial
  seat forced the probe's `publicTransformHasMap` to `true` while the real `map` stayed `null`, and
  `the_bundler_rollup_inline_transform_preserves_requested_source_maps` went GREEN. It asserts the
  probe's boolean rather than independently observing the map object.
- **The gate-fix rehoming introduced a silent-empty path.** Commenting out the three `mod`
  declarations in `crates/verter_session/src/framework/mod.rs` left all three of this block's own
  documented invocations reporting `running 0 tests` / `test result: ok`, exit 0 — the exact vacuity
  hazard [`test-invocations.md`](test-invocations.md) exists to warn about, now reachable by
  deleting the modules. Nothing asserts a non-zero executed count.
- **The new client-executor spec is timeout-fragile.** Its first cold run at this tree failed with
  `Test timed out in 5000ms` (5392ms elapsed); warm re-runs pass in ~1.2-1.5s. The child
  `spawnSync` timeout is 30s but the `it(...)` has no `testTimeout`.
- **The JS executor spec does not pin the runtime version.** Forcing `resolveBoundRuntime` to
  return `9.9.9` left the vitest spec green; only the Rust smoke
  (`every_emitting_client_request_mounts_and_renders_what_the_golden_renders`) caught it. The pinned
  runtime is genuinely bound and genuinely asserted — but by one of the two suites, not both.
- **Partial-coverage `NOT_PROVEN` items**, recorded as such, not as passes: no committed plant
  drives a wrong-render candidate through the same candidate-vs-golden runtime comparison the other
  six axis families use; the widened guard allowlists were read rather than plant-tested; and the
  route-inventory JSON still admits several bundler lanes as "read-verified citations" rather than
  driven results.

### One finding adjudicated INVALID

The architecture seat read the reachability-classification test
(`svelte_official_conformance_gate.rs:188-205`, which correctly asserts three reachable client
*requests*) and concluded the six-client-cell quantifier was unmet. It is met:
`every_committed_client_cell_is_driven_and_reaches_its_recorded_outcome` asserts `cells.len() == 6`
at `:264-266` and drives each, and `:518` asserts the six committed server cells. The conformance
and adversarial seats both verified this independently and recorded it as PASS.

## Gaps in this record

- **The three review rounds' own reports were never committed.** Unlike BF2 and BV0A, this block
  has no `evidence/BF3/reviews/` directory. The findings survive only because each fix brief
  quotes the finding it answers, and those briefs are reproduced verbatim in
  [`context-packet.md`](context-packet.md). Per-seat round verdicts, per-finding severities, and
  anything a seat raised that no fix brief quoted are therefore not recoverable from the tree.
- **The implementation session's own phase reports** (`/tmp/bf3-phase*-report.md`,
  `/tmp/bf3-fix*-report.md`) were temporary and are gone. Their load-bearing content was landed
  deliberately — the per-cell facts as `crates/verter_session/src/svelte_conformance_cell_record.json`
  held against the live suite by `the_committed_cell_record_matches_what_the_suite_observes`, the
  dispositions as [`dispositions.md`](dispositions.md), and the vacuous-invocation disclosure as
  [`test-invocations.md`](test-invocations.md) — but the reports themselves are not evidence and
  nothing in this record rests on them.
- **The bounded sub-delta reviews recorded in
  [`amd009-ratification-packet.md`](amd009-ratification-packet.md)** (`4b2bf8d94..a1ef593d1`
  conformance PASS; `a1ef593d1..885961a76` adversarial PASS; `885961a76..b6aa54699` architecture
  PASS) are recorded there as a summary only; their reports are likewise not in the tree. That
  packet itself states the later fixes did not receive a full clean 3/3 re-review — which is
  exactly why the re-attestation delta above starts at `a1ef593d1` rather than later.

## Status at the close of this landing

This record lands so the block has a durable evidence file; it is **not** an acceptance
recommendation. The candidate's mechanical verification is fully green and its technical evidence
is the strongest this program has produced for a Svelte/product-route surface — the shipped-path
Svelte oracle exists and discriminates, every committed cell is driven and recorded, the
TypeScript-observable family is observed under real pinned declaration closures with a fail-closed
resolution gate, and every genuine defect carries a named owner, an acceptance identifier and a
target that fails today for its stated reason.

What is not established is the authority the reshaped charter, the four correction-owner blocks and
the amended DAG rest on, plus five verified in-delta test defects. Both must be settled before this
block can be recommended for acceptance, and the first of them is maintainer-reserved.
