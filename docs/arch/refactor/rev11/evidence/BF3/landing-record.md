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

## The first closing re-attestation, and why it did not close the block

> **Everything in this section is history.** It records the state at the first attempt to
> close the block. All five in-delta defects below, and the governance blocker, were
> subsequently cured — see [The cure](#the-cure) at the end of this record.

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

### Findings inside the delta, verified and not fixed AT THAT TIME

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


## The cure

The block was recorded BLOCKED on two things: an authority defect no track-level actor could
settle, and five verified in-delta test defects. Both are now closed.

### The authority defect

The designated maintainer ruled on it directly, then issued the ratification act itself. Both are
reproduced verbatim at
[`maintainer-ruling-section7-ratification.md`](maintainer-ruling-section7-ratification.md). The
ruling's substance is that the intended ratification WAS the full AMD-009 §7, that the structural
reshape stands as intended, and that what failed was the RECORDING of it, not the work; it set the
cure order — fix the test defects first, re-review the post-binding charter drift, rebind the
package, record an explicit §7 ratification, and only then re-attest. The act is the maintainer's
own text:

> Ratify AMD-009 §7 in full: BF3 is a conformance-exhaustion and correction-dispatch audit; create
> BA0, BS0, BCSS0, and BRT0 as mandatory B2/B3 predecessors together with BV0 and BF3; supersede
> the retraction procedure and the conflicting AMD-005/AMD-006 text as AMD-009 §7 states;
> authorize no production error-on-bad-output path; do not accept BF3 or unlock B2/B3.

It ratifies §7's TEXT and names no commit, so the content identity below records the bytes §7 is
applied to at landing rather than a tree the maintainer inspected. It settles AUTHORITY and
nothing else: BF3 is not accepted, B2/B3 are not unlocked, the four correction blocks are created
but not accepted, and no production error-on-bad-output path is authorized anywhere. It is not
license to green any outstanding verification.

- The product ruling is **not** rewritten or weakened. It remains a genuine maintainer artifact
  and remains valid for exactly what its own text says: the AMD-009 §1/§2 no-production-error
  direction. What is superseded is the *reading* of it as full-§7 ratification.
- The over-claim is recorded, not erased —
  [`amd009-ratification-packet.md`](amd009-ratification-packet.md) states plainly what the earlier
  record claimed and why it was wrong, AMD-009 §8 separates the two maintainer acts, and
  [`amd009-unratified-package.md`](amd009-unratified-package.md) is demoted to a historical record
  under its original filename so links keep resolving.
- The two live documents that still read UNRATIFIED — [`scope-memo.md`](scope-memo.md) and
  [`../framework-conformance/bf3-safety-retraction-scope.md`](../framework-conformance/bf3-safety-retraction-scope.md)
  — carry dated status notices; the memo's historical body is preserved unedited.
- The package is **rebound by content**, not by commit: seven files, their git blob OIDs, and a
  combined SHA-256, all fixed before the commit that lands them and reproducible from any
  checkout. The identity is in the packet.
- The **template ledger** now carries `BA0`, `BS0`, `BCSS0` and `BRT0`, which it was missing.
  `--mode template` had been FAILING on that gap; both modes now validate clean.

### The post-binding charter drift

Five charters changed `+51/−36` after the earlier bound identity `9e457ca78` and had never been
accepted. Two independent external seats reviewed exactly that drift; both returned `BLOCKING`,
and both located the fault in AUTHORITY rather than content — every finding row still matched
`dispositions.md`, and every test the drifted text names exists with the stated ignore status and
assertions. The full record, both verbatim reports, and the per-finding disposition are at
[`charter-drift-review.md`](charter-drift-review.md). Three drift findings produced text changes
here: AMD-009 §5's BRT0 bullet and `BRT0.md`'s procedure now follow the re-measured BND rows and
name `dispositions.md` as their authority instead of describing a superseded provisional state,
and each charter's status line cites the rebound content identity so the ratified bytes are
unambiguous.

### The five test defects

Each was closed test-first, with the discriminating assertion proven RED against the pre-fix tree
before the fix, and every mutation plant proven present, unique and new before its run was
trusted.

- **`BS0.md`'s SV-4 row contradicted the tree.** It said there is no ignored correct-behaviour
  target "because the projector defines the correct surface", and its exits said "the three
  ignored" targets. The tree has four: SV-4's
  `an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript`
  (`public_api_typescript_observation.rs:505`, `#[ignore]`d) runs and fails for its stated reason.
  The row, the exit count and the procedure now say what the tree proves. Both drift seats
  verified the corrected text against the test independently.
- **Bundler public-spelling coverage.** The built artifact exports `Verter`, `VerterVue`,
  `VerterSvelte`, `unpluginFactory` and `default`; the probe drove two. It now executes the raw
  Vue-pinned `unpluginFactory` and the auto-carrier `Verter` over both carriers, and measures
  `default === VerterVue` rather than asserting the alias. A partition test requires every exported
  spelling to be either executed — with its named probe case present and proven to have run — or
  classified out of scope with a reason, and makes membership in both classes a failure. Proven RED
  twice: dropping the `unpluginFactory` row reports it in NEITHER class, and renaming its probe
  case reports that the claimed execution did not run. No behavioural divergence was found in the
  newly executed routes.
- **The Rollup acceptance target greened against a lying boolean.** It asserted the probe's derived
  `publicTransformHasMap`. Acceptance now validates the map ARTIFACT — non-null, `version: 3`,
  non-empty `mappings`, non-empty `sources` — across the Rollup target and both Vite targets, and
  absence is asserted as a null artifact rather than a false boolean. Proven RED in both
  directions: hard-coding the boolean true with `map` still null leaves the ignored target failing
  at the artifact, and planting a structurally valid but empty map turns the green Vite test RED
  while the old boolean oracle still reported `true`.
- **The documented invocations could go vacuously green.** Commenting out the three `mod`
  declarations left all three reporting `running 0 tests` / `ok` / exit 0. A census module now
  lives OUTSIDE all three suites, so deleting them cannot delete the check. Its three tests are
  named so each documented substring filter matches the census even when its suite is gone, and
  each performs independent discovery — re-executing the test binary with `--list --format=terse`,
  requiring the child to exit 0, witnessing its own path in the listing so an empty listing can
  never read as a pass, and asserting a floor on the tests the suite itself contributes. Proven RED
  per suite: with a `mod` line commented out, the invocation goes from `running 0 tests`/ok/exit 0
  to `running 1 test` → FAILED, naming the suite and its observed zero.
- **The JS executor spec pinned no runtime version and was cold-fragile.** Both `it(...)` blocks
  now assert the mounted module's bound `runtime.version` against the harness's own pin authority
  (`SVELTE_DOMAIN.packageVersion`), never a literal — planting `9.9.9` in `resolveBoundRuntime`
  fails both. The timeout defect was an INVERTED DEADLINE NESTING, not slowness: the child's
  `spawnSync` deadline was 30 s while the parent `it(...)` used vitest's 5 s default, so a cold run
  was killed before the child's own timeout could report why. The parent budget is now explicitly
  above the child deadline, with the ordering stated in a comment.

### One further defect the cure surfaced and closed

Fixing the map-artifact assertions exposed that the public Svelte virtual-script product publishes
a structurally valid but semantically EMPTY map — `mappings: "A"`, one unmapped segment — while the
Vue product carries 16 segments of which 12 are mapped. Tightening the acceptance target would have
flipped a green target RED with no owner, so it is recorded instead as a green characterization
(`the_public_svelte_virtual_script_map_currently_maps_nothing_where_vue_maps_most_of_its_output`)
that pins the exact current shape on both public Svelte routes and a floor on Vue's, and as an
observation in [`dispositions.md`](dispositions.md). It is not a new finding row: Svelte client map
provenance is already owned as SV-3 by BS0, with its own correct-behaviour target.

### Verification after the cure (run directly by the track orchestrator)

Run in a dedicated worktree after `pnpm install --frozen-lockfile` and `pnpm build:ts` (both rc=0),
on the toolchain the repository pins — `clippy 0.1.97 (8bab26f4f6)`. Every figure was observed
directly, not taken from a worker's report.

- `node scripts/gate.mjs --build-jobs 4 --test-threads 4 --memory-limit 12GiB` — **VERDICT: PASS
  (all three surfaces green)**, exit 0.
  - Surface 1 (nextest, process isolation): **24387 run, 24387 passed**, 0 failed, 0 timed out,
    0 tolerated, 584 skipped.
  - Surface 2 (in-process `verter_session` libtests from the same archive): 3 suites clean,
    0 tolerated failures.
  - Surface 3 (shipped `no-debug-assertions` cfg, `verter_session` + `verter_scheduler`):
    **8621 run, 8621 passed**, 566 skipped.
  - Build-prerequisite preflight `SATISFIED`; freshness-tooling preflight
    `already-present — tolerance DISABLED`, so the proto/TS byte-pin ran genuinely and a freshness
    failure would have been a hard failure.
- `cargo fmt --all --check` — clean. `cargo clippy --workspace --all-targets -- -D warnings` —
  clean. `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` — clean.
  `cargo check --workspace --release` — clean.
- Every suite in [`test-invocations.md`](test-invocations.md) at its recorded invocation matched
  its recorded expectation exactly: Svelte official-conformance `running 19` → 16/3;
  PublicApi-TSC-declaration `running 8` → 7/1; IDE-TSX `running 3` → 3/0; product-route inventory
  `running 24` → 22/2; batch route `running 9` → 8/1; transport equivalence `running 16` → 15/1;
  the `the_bundler` filter `running 5` → 4/1. The three inventory/batch/transport counts each
  include that suite's census test.
- All **eight** `#[ignore]`d conformance targets were run individually with `--ignored`; each
  reported `running 1 test` and **FAILED at its own named assertion**, not at setup, a missing file
  or a compile error. They are, by name rather than by line so the record cannot go stale:
  `each_flags_for_a_keyed_runes_each_match_the_official_compiler`,
  `a_runes_props_read_in_the_instance_script_compiles_to_a_runtime_module` and
  `the_client_source_map_covers_every_required_authored_anchor` in
  `svelte_official_conformance_gate.rs`;
  `an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript` in
  `public_api_typescript_observation.rs`;
  `the_standalone_css_route_publishes_valid_requested_maps_for_passthrough_and_transformed_css`
  and `a_refused_combined_request_publishes_no_product_at_all` in
  `framework_product_surface_tests.rs`;
  `a_svelte_batch_matches_the_single_file_route_item_for_item` in `svelte_batch_route_tests.rs`;
  and `the_bundler_rollup_inline_transform_preserves_requested_source_maps` in
  `transport_route_equivalence_tests.rs` — the last now failing on PARITY against the host's own
  published map artifact, not on a derived boolean and not on envelope shape.
- `packages/framework-conformance-harness` full `npx vitest --run`: **640/640 across 30 files.**
- `node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state
  docs/arch/architecture-lock/ledger/program-state.toml --mode live` — OK, 62 blocks; and
  `node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state
  docs/arch/refactor/rev11/templates/program-state.template.toml --mode template` — OK, 62 blocks.
  (All three flags are mandatory; the bare command prints usage and exits 2.) Template mode had
  been FAILING before this cure.

The gate above ran on the tree without this record's final section. After committing it, the three
guards a tracked document can affect — `tracked_files_contain_no_machine_specific_path_markers`,
`tracked_paths_are_portable`, and `every_critical_rule_in_docs_has_registered_guard` — were re-run
directly against the final tree.

## Re-attestation, and the gaps it surfaced (all since closed — see Status)

A bounded per-mandate re-attestation ran against the cure delta — external CLI seats only, scoped to
two questions about that delta, with an exit-criteria enumeration required per seat. Four rounds ran,
each scoped to the delta the previous round produced. The full record, all ten verbatim reports and
the per-finding disposition are at [`reattestation.md`](reattestation.md).

**Every round found real defects in the checks this cure introduced** — a map oracle that validated
envelope shape rather than preservation, an export partition satisfied by a spelling that never ran,
a census that trusted a path string the suite itself owned, and a recorded residue that was simply
FALSE. All were fixed or, where genuinely open, restated accurately. Three residues remain recorded
and are named in that file: invocation attribution on the bundler probe, the witness decoy, and the
all-four-`mod` removal that no in-binary check can decide.

**The block is not acceptance-recommended, and the reason is not the cure.** The cure's own scope —
the authority defect and the five verified test defects — is closed. What blocks acceptance is
pre-existing and was recorded before this cure began: the seats' exit-criteria enumeration returned
`NOT-EVIDENCED` for BF3's procedure item 3 (no committed plant drives a wrong-render candidate
through the candidate-vs-golden runtime comparison), item 4 (several bundler lanes in the route
inventory remain read-verified citations rather than driven results), and item 6 (AT-2's gating test
measures a different failure class than the row it gates), and for the matching "Required exits"
sentences. The charter's own words settle it: *"`UNPROVEN` records an open proof gap and cannot
count as exhaustion."* Those gaps were outside this cure's scope, are unchanged by it, and must be
closed before this block can be recommended.

> **Superseded in part.** Items 3 and 4, and all three residues, were subsequently worked — see
> [Closing the exhaustion gaps](#closing-the-exhaustion-gaps) below. WORKED is not CLOSED for all of
> them: items 3 and 4 are satisfied — item 4's last residue, the recompile write, was closed
> afterwards by attributing it against the shipped artifact — and item 6 was ESCALATED, then closed
> by a maintainer act amending the ratified row it gates. See the Status section for the current
> position on each.

## Closing the exhaustion gaps

The three exit criteria the re-attestation returned `NOT-EVIDENCED`, plus the three residues that
record named as open, were worked directly. Every change below is test-, probe- or evidence-only:
`git diff` against this section's base touches no production Rust outside `#[cfg(test)]` modules and
no `packages/*/src` file at all.

### Procedure 3 — the runtime axis now has its own planted defect

The gate claimed six discrimination axis families — parse, real-package link, structural,
diagnostics, mapping, runtime — and `the_gate_detects_a_planted_defect_on_every_applicable_axis_family`
planted a defect for five of them (mapping twice: content integrity and anchor coverage). The RUNTIME
comparison, which mounts the candidate and the golden against the pinned official client runtime and
compares rendered markup, had none: nothing proved it would notice a candidate that renders the
WRONG markup.

That comparison was extracted into ONE helper (`compare_mounted_render`) which the live gate
(`every_emitting_client_request_mounts_and_renders_what_the_golden_renders`) now drives, so the plant
exercises the same code rather than a copy of it. `the_runtime_comparison_detects_a_planted_wrong_render`
then retemplates the `basic-runes` golden's `<p>zero</p>` — the markup the `alternate` branch actually
renders, since `count` starts at 0 — into a marker, proves the plant applied through the existing
`assert_plant_applied` (absent before, present exactly once after, bytes changed), and requires the
comparison to report a divergence **while the planted module still MOUNTS**. That last assertion is
the point: it makes the plant a wrong RENDER rather than a crash, so the test cannot pass by proving
only that a broken module is noticed. It also asserts the planted markup breaks the recorded render
pin, so the two independent catches are both shown.

The unplanted control for this axis is the live gate itself, which mounts the real shipped-route
candidate against the golden and is green; the plant test additionally brackets itself with a
pristine-against-pristine run before and after, which pins the bound runtime version and the recorded
markup and proves no plant leaked into shared harness state.

Discrimination was proven, not assumed: with `compare_mounted_render` temporarily forced to report no
divergence, the new test FAILS naming both rendered strings; restored byte-identically (hash
re-verified), it passes.

### Procedure 4 — the bundler route aliases are DRIVEN, not cited

The route inventory admitted the bundler style, load, recompile and CSS-scoping lanes and the
runtime-render batch lane as "read-verified citations, not driven". A source investigation found that
two of them were in fact already REACHED by the existing probe and simply never observed. All five are
now driven, each with a route-identity comparison against the in-process host product and each with a
negative control:

| lane | site | now |
|---|---|---|
| style + `listVirtualFiles` | `packages/unplugin/src/index.ts:68`, `:63` | DRIVEN — the Svelte wrapper's style requests are loaded and compared against the host's `Style{0}` product, whole-map included; a carrier with no `<style>` publishes zero style requests |
| load | `index.ts:668` | DRIVEN — a `?vue&type=template` request falls through to the host route; compared against the host's `Template` node; an unregistered carrier must answer `missing` |
| runtime-render batch | `index.ts:101` | DRIVEN — the Vue rollup INLINE product is captured and compared against `compile_many(RuntimeRender)` |
| non-Vite CSS scoping + its native wrapper | `index.ts:863`, `core/compiler.ts:64` | DRIVEN — a rollup-shaped two-call sequence ending in a `&scoped&lang.scss` style request, compared against the shared style processor, asserting both the `[data-v-…]` attribute and a `v-bind()` rewrite that proves WHICH cached profile's component id was used |
| recompile | `index.ts:803` | DRIVEN — `buildStart` is driven over a real two-file fixture and both published modules match the in-process host's products; the recompile WRITE itself is attributed separately, see below |

The recompile lane's write was, at that point, the one residue, and it is now closed — against the
SHIPPED native artifact, not a feature-enabled build. The earlier record named a
`session_metrics`-enabled native build as the closure condition, on the reading that the host metrics
channel was the only thing that could count the call. That reading was wrong, and the correction is
recorded rather than quietly swapped: the metrics channel is one way to count the call, not the only
one. An observation of `host.getVirtualFile` taken WHILE `buildStart` runs names the recompile call,
and the observation is taken at the native module boundary on the same `@verter/native` the plugin
resolves, by a wrapper that delegates and returns the real value. The hook reaches that call at TWO
places, not one — the recompile block and the SVELTE branch's compiled-style read — a precision a
review seat was right to insist on; the lane's fixture is two `.vue` files so the Svelte branch
cannot fire, the two reads are distinguishable by request shape (a bare canonical versus a
`?verter&type=style&index=…` request) and the test asserts equality against the bare child canonical,
and reading 2 below ties the observation to the cross-file block by turning its flag off on the same
fixture.

Three readings, in `the_bundler_cross_file_recompile_write_is_attributed_to_the_recompile_call`: the
lane observes exactly one read and its `rawId` is the CHILD; the same drive with `crossFileOptimize`
off observes NONE while still publishing both host products, so zero is an absent recompile and not
an absent lane; and a third drive in which the boundary substitutes a marked value for that one
return publishes, for the child and only the child, the host's own product followed by exactly those
bytes — asserted as an EQUALITY, never as a search for the marker inside generated output. What the
recompile call returned is therefore what the route cached and served, which is the write.

Each reading was proven discriminating by a plant in the PLUGIN SOURCE — dropping the call, dropping
the cache write while keeping the call, and entering the block regardless of the flag — each proven
present, unique and new before its run and restored to a byte-identical build. The middle plant is
the one that matters: it leaves the call intact and makes only the write assertion go red, which is
what separates "the write happened" from "the call happened". The full plant table is in
[`test-invocations.md`](test-invocations.md). It remains true, and unchanged, that a recompiled
module is byte-identical to the pre-compiled one (`prop_constness_overrides: None` in
`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`) — which is exactly why the
published products alone cannot attribute the write and an observation of the call was needed. The
inventory's blanket sentence is replaced by a per-alias status list.

The lane's claim was also CORRECTED rather than defended. It previously called
`compute_cross_file_optimizations()` on a fresh in-process host and read the non-empty result as
evidence that the plugin's guarded block iterated. A different host in a different process cannot
witness that, and an adversarial seat proved it by disabling the plugin's cross-file block and finding
the test still green. The claim and the measurement behind it are both gone; the test now states only
what it measures.

One honesty note carried into the assertion text, the inventory and the evidence index: the
runtime-render lane is documented to publish the same `Main` bytes as the host-backed lane, so the
identity comparison proves the bundler publishes the host's render product but cannot discriminate
WHICH host lane produced it.

### Procedure 6 — the atomicity regression, and an escalation

`FC-ATOMIC-001`'s batch half was carried by a single test driving ONE failure class. It is now a
table over every failing-entry class reachable through the public `compile_many` API, on both lanes,
each row proving CLASS ENTRY with a class-specific assertion before asserting the entry publishes no
code, no source map and no output language, and each row asserting a cleanly-publishing neighbour.
Two controls keep a diagnostic from being read as a refusal: an ordinary success, and a warning-only
compile that still publishes. Two whole classes are recorded NOT REACHABLE with their source reasons, plus
the one lane of a driven class that has no reachable input, rather than given an `#[ignore]`d target
that would fail for the wrong reason. Sixteen mutations of the code
under test — every product-publishing site, every class-proof rendering, and four shape mutations —
were each proven applied and each went RED.

The row that gates this criterion, `AT-2`, was ESCALATED and is now AMENDED under a maintainer act.
Its ratified text asserted that a batch entry publishes a product together with a genuine typed
refusal. An enumeration of all nine `CompileBatchEntry` construction sites shows eight are atomic by
hardcoded literal, the typed refusal itself lands on an atomic arm, and the single non-atomic
construction has no demonstrated reachable input; one path remains UNKNOWN and unreproduced. An
independent, unprimed disposition ruling held that the row as written is not an evidenced defect and
recommended a specific amending act, which no track-level actor could issue.

That act now exists. The maintainer's standing ruling of 2026-08-17 on bug handling and the type
waiver ([`maintainer-standing-ruling-bugs-and-types.md`](maintainer-standing-ruling-bugs-and-types.md),
verbatim) establishes that a bug found during the program is captured as an `#[ignore]`d test with
the fix deferred, and that a finding which is not a demonstrated, reproduced defect must not carry a
required-RED target. Applied to `AT-2` — the application is the program orchestrator's reading of a
general rule, and the ruling file says so — that rejects the original claim, reclassifies the row as
a latent construction hazard with reachability unproven, retains the DEFER to `BA0`, and drops the
requirement that a Svelte-refusal atomicity target be RED (that target would fail only because the
separate ratified row `RT-1` prevents Svelte classification at all — a stub by the ruling's own
terms).

Two byte locations changed under that authority, and only those two: the `AT-2` row in
[`dispositions.md`](dispositions.md) and the AT-2 obligation lines in
[`../../charters/BA0.md`](../../charters/BA0.md). Every other row of the ratified findings table is
untouched. The hazard's PRECONDITION is now carried by an `#[ignore]`d characterization,
`the_host_backed_success_construction_is_never_fed_a_response_that_carries_an_error`, which asserts
through the public batch API that the host-backed SUCCESS construction is fed by a response carrying
no error-severity diagnostic, and that a genuine FAILURE answers `Err` and never reaches that
construction. It passes today by design and turns RED if a successful host-backed response ever
carries an error. It deliberately does NOT claim the construction READS its error list rather than
hardcoding it — a review seat proved that a hardcoded-empty plant leaves it green, and the test says
so rather than implying otherwise. Two plants settle the surrounding mechanism: flipping the
producing diagnostic to error severity routes the whole response to `Err` and an atomic arm
(`code_len=0`), which is WHY the construction is never fed one; and merging an error diagnostic into
the success response downstream of that gate makes the entry carry a 480-byte product beside the
error — the hazard is real as a construction property, and only its reachability is unproven. The full evidence and the verbatim
independent ruling remain at [`at2-deviation-memo.md`](at2-deviation-memo.md) (now marked
discharged) and [`at2-disposition-ruling.md`](at2-disposition-ruling.md).

### The three recorded residues

- **Invocation attribution — CLOSED.** The recorded hole was that nothing required a driven export to
  have been APPLIED, so a probe could print an export's true readings while sourcing its drive results
  from a sibling. The test-owned observer — a node script that lives in the Rust source, which the
  probe cannot forge — now DRIVES each export itself through an apply-counting `Proxy` and records the
  apply count, the plugin keys ITS OWN invocation returned, and both carrier include decisions. Every
  export the test classifies as executed must carry a non-zero apply count from the test's own
  invocation and must agree with it. The exact recorded full-forgery plant, reconstructed faithfully
  (true evidence, plausible plugin keys, drive skipped), PASSES the pre-change tree and FAILS the
  post-change tree — that two-sided measurement is the discrimination proof. The `rollupIsCallable`
  reading the observer gathered and discarded, and the default-alias identity that was the probe's
  word alone, are now compared too. Residue, stated: the observer drives the factory and the include
  decision, never a carrier transform, so per-carrier product BYTES remain the probe's word — judged
  where it matters by host parity.
- **The witness decoy — NARROWED, and said so.** A census row was bound to "a test of this path exists",
  a free `&str` each suite handed over, so a module defining a same-named witness could be borrowed to
  clear another suite's floor. A row is now a reference to the witness function ITEM, with the path
  coming back from the compiler (`std::any::type_name` on the zero-sized function-item type, verified
  in-crate to equal the `module_path!()` spelling and verified to degrade to `fn()` under pointer
  coercion, hence the `&F` signature). The per-suite constants are deleted. The recorded decoy plant is
  now `error[E0425]`. What remains, stated plainly rather than claimed closed: anyone editing the
  census file itself can still name any resolvable item, and that was measured to pass.
- **All four `mod` declarations removed at once — raised to a build error, residue dispositioned.**
  The census and its three suites protected each other pairwise, but one edit removing all four
  removed every party to the argument. An unrelated long-lived sibling module now makes a real,
  discriminating assertion through the census (that no census row counts ITS tests, which would
  otherwise let a suite clear its floor on them), which anchors the registration from outside the set.
  Two mutations were measured, both under `--features transport-authoritative`, and they are different
  facts:

  | mutation | diagnostics |
  |---|---|
  | remove `mod suite_census;` ALONE | seven `error[E0433]`, one per consuming site, across four modules — `framework_product_surface_tests.rs:66,69`, `svelte_batch_route_tests.rs:67,70`, `transport_route_equivalence_tests.rs:79,82`, and the outside anchor `script_facts_tests.rs:32` |
  | remove ALL FOUR `mod` declarations | ONE `error[E0433]` — `script_facts_tests.rs:32:28`, `could not find suite_census in framework`. The six in-set sites vanish with the modules that held them; the outside anchor is the one party the edit does not remove, and it is what makes this a build error at all |

  The general
  execution-attestation problem is NOT closed by that and is not claimed to be — a source-tree scanner
  is forbidden as a landed guard here — so it is dispositioned as ledger row **GI-21** in
  [`../../../gate-integrity-ledger.md`](../../../gate-integrity-ledger.md), owned by the gate-integrity
  block with a named acceptance test.

### Verification after closing the exhaustion gaps (run directly by the track orchestrator)

Run in a dedicated worktree after `pnpm install --frozen-lockfile` and `pnpm build:ts` (both rc=0).
Every figure below was observed directly, not taken from a worker's report.

- `node scripts/gate.mjs --build-jobs 4 --test-threads 4 --memory-limit 12GiB` — **VERDICT: PASS
  (all three surfaces green)**, exit 0, terminal summary present.
  - Surface 1 (nextest, process isolation): **24390 run, 24390 passed**, 0 failed, 0 timed out,
    0 tolerated, 584 skipped.
  - Surface 2 (in-process `verter_session` libtests from the same archive): 3 suites clean,
    0 tolerated failures.
  - Surface 3 (shipped `no-debug-assertions` cfg, `verter_session` + `verter_scheduler`):
    **8624 run, 8624 passed**, 566 skipped.
  - Build-prerequisite preflight `SATISFIED`; freshness-tooling preflight
    `already-present — tolerance DISABLED`, so the proto/TS byte-pin ran genuinely and a freshness
    failure would have been a hard failure.
  - An EARLIER run of this same gate FAILED, and the failure is recorded rather than smoothed over:
    `phase_archaeology_test_files_count_zero` flagged an assertion message this work had added which
    read `style block 0` — numbered-block vocabulary the repository forbids in source. It was
    reworded and the gate re-run in full. A partial or aborted gate is not a verdict; both runs are
    stated.
- `cargo fmt --all --check` — clean. `cargo clippy --workspace --all-targets -- -D warnings` —
  clean. `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` — clean.
  `cargo check --workspace --release` — clean.
- Every suite in [`test-invocations.md`](test-invocations.md), at its recorded invocation, matched
  its recorded expectation exactly: Svelte official-conformance `running 20` → 17 passed/3 ignored;
  PublicApi-TSC-declaration `running 8` → 7/1; IDE-TSX `running 3` → 3/0; product-route inventory
  `running 24` → 22/2; batch route `running 11` → 10/1; transport equivalence `running 21` → 20/1;
  the `the_bundler` filter `running 9` → 8/1.
- All **eight** `#[ignore]`d conformance targets were run individually with `--ignored`; each
  reported `running 1 test` and **FAILED at its own named assertion inside its own file**, not at
  setup, a missing file or a compile error:
  `each_flags_for_a_keyed_runes_each_match_the_official_compiler`,
  `a_runes_props_read_in_the_instance_script_compiles_to_a_runtime_module` and
  `the_client_source_map_covers_every_required_authored_anchor` in
  `svelte_official_conformance_gate.rs`;
  `an_untyped_svelte_props_destructure_publishes_its_authored_props_to_typescript` in
  `public_api_typescript_observation.rs`;
  `the_standalone_css_route_publishes_valid_requested_maps_for_passthrough_and_transformed_css` and
  `a_refused_combined_request_publishes_no_product_at_all` in `framework_product_surface_tests.rs`;
  `a_svelte_batch_matches_the_single_file_route_item_for_item` in `svelte_batch_route_tests.rs`;
  and `the_bundler_rollup_inline_transform_preserves_requested_source_maps` in
  `transport_route_equivalence_tests.rs`.
- `packages/framework-conformance-harness` full `npx vitest --run`: **640/640 across 30 files.**
- The out-of-process bundler probe, before and after the work:
  `node packages/unplugin/scripts/probe-bundler-route.mjs` — exit 0, `loaded: true`, `fresh: true`,
  `erroredCases: []`.
- Both ledger validator modes — OK, 62 blocks each.

The gate above ran on the tree without this section. After committing it, the three guards a tracked
document can affect — `tracked_files_contain_no_machine_specific_path_markers`,
`tracked_paths_are_portable`, and `every_critical_rule_in_docs_has_registered_guard` — were re-run
directly against the final tree.

## Proposed ledger transition — the §7 cure round (superseded)

Kept as the record of what that round proposed. The CURRENT proposal is at the end of this file,
under "Proposed ledger transition — the acceptance round".

The program orchestrator owns `docs/arch/architecture-lock/ledger/program-state.toml`; this record
does not write it. The proposed BF3 field set was:

| field | value |
|---|---|
| `status` | `BLOCKED` |
| `base_sha` | the cure commit's parent on the program branch |
| `candidate_sha` / `candidate_tree` | the squashed cure commit and its tree |
| `accepted_sha` / `accepted_tree` | empty — the block is NOT accepted |
| `charter_digest` | `sha256` of `charters/BF3.md` |
| `context_packet_digest` | unchanged |
| `evidence_digest` | `sha256` of this file |
| `conformance_review` / `architecture_review` / `adversarial_review` | `BLOCKING` — no seat issued a PASS on the block, and the exit-criteria gaps above are unresolved |
| `maintainer_decision` | `PENDING` — the ratification act settles §7's authority and explicitly withholds block acceptance |

## Status

The authority defect is closed by a direct maintainer act on §7, and every defect this record
previously listed as verified-and-unfixed is closed with a discriminating test that was proven to
fail before it was fixed. What the act explicitly does NOT do is accept this block:
`maintainer_decision` stays `PENDING`, B2 and B3 stay locked, the four correction blocks are created
but not accepted, and no production error-on-bad-output path is authorized anywhere.

The three exit criteria the re-attestation returned `NOT-EVIDENCED` no longer stand alike. Their
current position, criterion by criterion:

| charter item | position now |
|---|---|
| procedure 3, and its matching "Required exits" sentence | **SATISFIED.** The runtime axis has its own committed plant driving a wrong-render candidate through the same `compare_mounted_render` helper the live gate uses; the plant is proven applied, the planted module still MOUNTS, and the comparison was proven to go red when the helper is forced to report no divergence. |
| procedure 4, and its matching "Required exits" sentence | **EVIDENCED.** All five cited bundler aliases are DRIVEN with route-identity comparisons and negative controls. The last residue — the recompile WRITE — is attributed against the SHIPPED native artifact rather than the `session_metrics` build the earlier record named: the `getVirtualFile` call the recompile block makes is observed at the native module boundary while `buildStart` runs, a `crossFileOptimize`-off control observes none on the same fixture while still publishing both host products, and a marked substitution for that one return is what the route publishes for the child and only the child. Three plants in the plugin source prove the three readings discriminate, including one that keeps the call and drops the write. |
| procedure 6, and its matching "Required exits" sentence | **EVIDENCED. The amendment is authorized by a maintainer act that names the row** ([`maintainer-act-at2-amendment.md`](maintainer-act-at2-amendment.md)), issued after a review seat correctly refused the earlier general-ruling chain. The atomicity regression is a driven table over every failing-entry class the public API can reach, with class-entry proofs and sixteen proven-applied mutations. The row that GATES the criterion, `AT-2`, was escalated as not an evidenced defect and is now AMENDED: rejected as a demonstrated defect, reclassified as a latent construction hazard with reachability unproven, DEFER to `BA0` retained, and carried by an `#[ignore]`d characterization instead of a required-RED target. Item 6 binds "every genuine defect"; under the amended row AT-2 is not one, so it raises no defect-specific regression obligation, and every row that IS a genuine defect has its precise discriminating regression. **The authority, in order:** the amendment was first taken under the maintainer's GENERAL standing ruling of 2026-08-17, which does not name `AT-2`; both closing seats agreed the ORIGINAL claim is not demonstrated but split on that chain — Grok LAND, Codex BLOCKING, asking for an explicit act naming `AT-2` or a revert. Codex was right, and the first remedy it named was taken: the maintainer issued an act naming `AT-2`, authorizing exactly the bytes already present and changing none of them. |

The three recorded residues were worked to three different ends, stated as such: invocation
attribution is CLOSED; the witness decoy is NARROWED, with the surviving hole measured and named;
and the all-four-`mod` removal is RAISED TO A BUILD ERROR, with the general execution-attestation
remainder dispositioned as ledger row **GI-21** rather than claimed closed.

All three exit criteria the re-attestation returned `NOT-EVIDENCED` are closed: procedure 3 by its
committed runtime plant, procedure 4 by attributing the last bundler residue against the shipped
artifact, and procedure 6 by the maintainer act that amends the row it gates.

**One open proof gap survives, and it is not counted as exhaustion.** The `AT-2` reachability
residual — `peek_last_good` skipping its validator on an empty fact rail, which a self-contained SFC
produces — was probed and NOT reproduced, and it is recorded as `UNKNOWN` in
[`dispositions.md`](dispositions.md) rather than as a closure. The charter is explicit that an
`UNPROVEN` result records an open proof gap and cannot count as exhaustion, so this record does not
count it as one. What bounds it is stated exactly:

- It is **not an inventory row.** Every retained product/route row in
  `framework_product_surface_inventory.json` carries an actual driven result; the residual is a
  reachability question about ONE construction shape, not an undriven surface.
- Its status is **what the maintainer act authorizes**: the act reclassifies `AT-2` as a latent
  HostBacked construction hazard *with reachability unproven*, retains the DEFER to `BA0`, and
  directs that it be carried as an `#[ignore]`d characterization. "Reachability unproven" is the
  recorded state of that row, not a gap the record claims to have closed.
- It is **dispatched**, not dropped: `BA0` owns removing the hazard, and the act says that if the
  hazard is later demonstrated reachable, that reproduction is a NEW finding with its own RED
  target.

Anyone reading this record should read that residual as open. The claim made here is narrower than
"everything is closed": it is that every criterion the re-attestation returned is closed, that every
retained inventory row has an actual result, and that the one surviving unknown is recorded as
unknown and owned by a named block.

Both closures were reviewed by two external CLI seats, run sequentially, each authoring its own
plants; the verbatim reports and the split verdict are at
[`exhaustion-closure-reviews.md`](exhaustion-closure-reviews.md).

**Procedure 6's closure carried a stated dependency and a live objection; both are now closed.** The
closure holds only if the `AT-2` amendment is validly authorized. It was first taken under a GENERAL
maintainer standing ruling that does not name `AT-2`, with the application to that row recorded as
the program orchestrator's. One review seat held that chain insufficient and asked for an explicit
maintainer act naming `AT-2` or a revert of the row and the matching `BA0` lines. **The seat was
right, and the first remedy was taken.** The maintainer issued an act that names `AT-2` and
authorizes, clause for clause, the four points the deviation memo had asked for; it is recorded
verbatim at [`maintainer-act-at2-amendment.md`](maintainer-act-at2-amendment.md), its effect on the
seat's report at [`exhaustion-closure-reviews.md`](exhaustion-closure-reviews.md), and its discharge
of the memo at [`at2-deviation-memo.md`](at2-deviation-memo.md).

**The discharge was confirmed by a seat, not declared by the actor who took the remedy.** Satisfying
an objection is no more self-certifying than recording one, so the finding was put back to a fresh
external conformance seat on this tree. It ruled Finding 2 DISCHARGED and procedure item 6
EVIDENCED — having run all nine genuine rows' correction targets individually and reported each
one's assertion line and observed failure — with no findings and a `PASS` verdict. Its verbatim
report is in [`exhaustion-closure-reviews.md`](exhaustion-closure-reviews.md).

The act authorizes bytes that were already in the tree — the `AT-2` row in
[`dispositions.md`](dispositions.md) and `charters/BA0.md` lines 28 and 37 — and **neither file is
touched by the commit that records it**, so no post-act drift can hide behind the act. `git diff`
over those two paths against the pre-act tip is empty.

**One discrepancy in that scope is recorded rather than resolved here, and it needs the
maintainer.** The act's scope bullet enumerates two locations in `BA0.md`, lines 28 and 37. The
block's actual `BA0.md` edit has THREE hunks: the row at line 28, the AT-2 procedure paragraph at
line 37, and a third at the Required-exits paragraph (`git diff --unified=0
b75fcebc33e3a100bbfff7af62fe2edceb4fcaf0..HEAD -- docs/arch/refactor/rev11/charters/BA0.md` shows
`@@ -54,5 +59,5 @@`). The third hunk carries the SAME instruction the act's operative clause drops —
that AT-2 prove the Svelte-refusal batch class with a RED target once RT-1 is corrected — so it is
within the act's operative sentence while sitting outside its locator enumeration. The enumeration
was inherited from [`at2-deviation-memo.md`](at2-deviation-memo.md), which named the same two
locations and missed the third.

This record does NOT claim the act covers that hunk. Track-level actors may not widen a maintainer
act any more than they may amend a ratified row, and the whole point of the naming act was to stop
exactly that kind of inference. The disposition is: the bytes stand as landed and byte-unchanged,
the discrepancy is stated here and in
[`maintainer-act-at2-amendment.md`](maintainer-act-at2-amendment.md), and the maintainer is asked to
either confirm the act reaches the third hunk or direct that it be reverted to its ratified text —
under which `BA0` would again instruct a Svelte-refusal RED target that the act's own reasoning
rejects. **Acceptance should not be granted before that confirmation.**

**That is not the same as acceptance, and this record does not claim it.** The §7 ratification act
explicitly withheld block acceptance, `maintainer_decision` stays `PENDING`, B2 and B3 stay locked,
and the four correction blocks exist but are not accepted. What changed is the ground on which this
record previously withheld a recommendation: the two open exit criteria that stood in the way are no
longer open. Whether the block is accepted remains the maintainer's decision, on the evidence
recorded here.

## The architecture mandate

The closing round commissioned two seats. This block is foundational class and acceptance needs
three, so a full architecture mandate was owed. That was an orchestrator scoping error, not a seat
finding, and it is now discharged: the mandate ran over the block as a whole — every numbered
procedure item and every "Required exits" sentence enumerated independently, each verdict required to
cite evidence the seat had personally verified, with an item carried on this record's say-so counting
as `NOT-EVIDENCED` by default. Two rounds, both external CLI seats, verbatim reports and per-finding
dispositions at [`architecture-mandate-review.md`](architecture-mandate-review.md).

**Round 1: `NOT-EVIDENCED`, seven findings.** Three were real and new. One was fixed as code, one as
a record correction, one escalated:

- A genuine defect row, `TR-1`, had no correction-oriented regression. Its only test asserts BOTH
  current transport shapes, so it goes RED if either is corrected — a test that fails on the fix
  cannot be the gate for the fix. Every other genuine row carries an `#[ignore]`d correct-behaviour
  target beside its characterization; `TR-1` was the only one that did not, and procedure item 6
  requires one for every genuine defect. **Fixed:**
  `the_transports_report_a_missing_node_the_same_way` is added, `#[ignore]`d, and proven RED at its
  parity assertion with the staleness guard and both no-product assertions passing before it. It is
  deliberately shape-neutral — it asserts the two transports AGREE without asserting which spelling
  wins — so it passes under either correction and does not pre-empt `BRT0`'s design decision. No
  ratified row and no charter is edited to accommodate it.
- This record claimed no exit criterion remained `NOT-EVIDENCED` while `dispositions.md` recorded the
  `AT-2` reachability residual as an open proof gap. **Fixed as a record correction:** the universal
  claim is withdrawn above, and the residual is stated with its exact bounds.
- The maintainer act's `BA0.md` locator names lines 28 and 37 while the landed edit has three hunks.
  **Escalated**, not resolved — see the discrepancy recorded above.

Two round-1 findings were REJECTED with evidence, and one was declared outside this track's
ownership. All three rejections were put back to the seat in round 2 and upheld.

**Round 2: `BLOCKING`, no new findings, all four dispositions ruled CORRECT.** Procedure items 4 and
6, Required-exits sentences 2, 3 and 5, and Stub Prevention all moved to `EVIDENCED`. On Stub
Prevention the seat reversed its own round-1 finding by experiment: it planted an injected error into
the HostBacked success arm, proved the marker absent-before and once-after with a changed file SHA,
drove the `AT-2` characterization RED, restored the file byte-identically and re-ran green —
concluding that the `Vec::new()` counterexample attacks a property that test expressly disclaims.

**Two items remain open, and neither is closable at track level.**

1. **The `BA0.md` byte-scope discrepancy.** The seat ruled the escalation CORRECT and restated it:
   only the maintainer can confirm the act reaches the third hunk or direct that it be reverted. It
   independently confirmed there is no drift — `dispositions.md` and `BA0.md` have identical blob IDs
   on both sides of the delta. This is the acceptance blocker.
2. **Required-exits sentence 1 and the "exhaust the retained inventory" objective.** The `AT-2`
   reachability residual is recorded UNKNOWN, and the controlling independent consult says a residue
   must be demonstrated or conclusively closed before exhaustion is claimed. The seat's reading is
   precise: the naming act changes `AT-2`'s classification and its item-6 consequence, but its stated
   effect names item 6 only — it does not amend this exit. Closing it needs either a conclusive
   structural proof of unreachability, which is investigation rather than a record edit, or a
   maintainer act ruling that a recorded, bounded, dispatched open proof gap satisfies the exit.

A third row, **Verification Must Prove Execution**, is also `NOT-EVIDENCED`, and it is not a finding
against this block: the repository already carries that class as an open gate-integrity row (`GI-21`)
with its owner and resolution gate outstanding, and `CLAUDE.md` says of that rule in its own words
that it currently fails its own test. No block closes it by running more targeted filters.

## Verification for this delta

The delta is one test file plus evidence documents. Per the standing gate-scope ruling of 2026-08-17
— the full gate runs once at landing readiness, and a test-only change warrants targeted tests rather
than a gate run — no canonical gate was run for it, and the reasoning is recorded here rather than
assumed. The claim that it reaches nothing in production is VERIFIED, not asserted:
`git diff --name-status` over the delta lists one test module
(`crates/verter_session/src/framework/transport_route_equivalence_tests.rs`, a `#[cfg(test)]` module
behind the `transport-authoritative` feature) and evidence documents under
`docs/arch/refactor/rev11/evidence/BF3/`. No production implementation file is touched, so no
consumer of production code can observe it. The independent architecture seat re-checked the same
boundary and reported it identically.

What was run:

- `cargo fmt --all --check` — exit 0.
- `cargo test -p verter_session --lib --features transport-authoritative transport_route_equivalence -- --test-threads=1`
  — `running 23 tests` → 21 passed, 2 ignored.
- `cargo test -p verter_session --lib --features transport-authoritative the_transports_report_a_missing_node_the_same_way -- --ignored --test-threads=1`
  — `running 1 test` → FAILED at the parity assertion, which is the required RED.
- Both ledger validator modes.
- The architecture seat additionally ran, on this tree, the official-conformance (20), PublicApi/TSC
  (8), IDE (3), product/route (24), batch (12) and transport (23) suites, plus its own plant.
- The conformance confirm seat additionally ran, on this tree, all nine genuine rows' correction
  targets individually with `--ignored`, confirming each selected exactly one test and reporting the
  assertion line and observed failure for every one.

The gate figures recorded earlier in this file still stand for the production/test content they
measured: that content is unchanged since the gate-passed commit apart from this one added
`#[ignore]`d test, which the targeted runs above cover directly.

## Proposed ledger transition — the acceptance round

The program orchestrator owns `docs/arch/architecture-lock/ledger/program-state.toml`; this record
does not write it, and this track did not touch it. The proposed BF3 field set:

| field | value |
|---|---|
| `status` | `BLOCKED` |
| `base_sha` | `9104e0be7edb07fe5bbd477a903c457c9d825b5b` |
| `candidate_sha` / `candidate_tree` | this round's squashed commit and its tree |
| `accepted_sha` / `accepted_tree` | empty — the block is NOT accepted |
| `charter_digest` | unchanged — `charters/BF3.md` is not edited |
| `context_packet_digest` | unchanged |
| `evidence_digest` | `sha256` over the raw bytes of this file |
| `conformance_review` | `PASS` — re-issued by a fresh external seat on this tree, not inferred from the discharge: Finding 2 DISCHARGED, procedure item 6 EVIDENCED with all nine genuine rows' targets run individually and RED at their named assertions, no findings |
| `architecture_review` | `BLOCKING` — the mandate ran and returned `BLOCKING` on the `BA0.md` byte-scope escalation and the exhaustion exit; both need the maintainer |
| `adversarial_review` | unchanged from the closing round |
| `maintainer_decision` | `PENDING` |

**This record does not recommend acceptance.** Two items need a maintainer act first: confirmation
that the `AT-2` act reaches `BA0.md`'s third hunk (or a direction to revert it), and a decision on
whether the recorded `AT-2` reachability residual leaves the exhaustion exit open. Everything the
track could close is closed and independently verified; what remains is authority, and inferring it
is the exact defect this block has now been blocked for twice.
