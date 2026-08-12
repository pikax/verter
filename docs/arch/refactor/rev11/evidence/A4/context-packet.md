# A4 context packet

Verbatim record of the bounded context given to the A4 implementer/fix workers, per governance's
L2 (block context) scope. These are the real dispatch-prompt bytes (recovered from the dispatching
orchestrator's own transcript after the worktree-local `.agent-run/` files were removed alongside
the worktree cleanup) — not a reconstruction. Three prompts were dispatched in sequence: the
initial implementer brief, a resume brief after a tool-permission stall, and a bounded fix-pass
brief after the three Foundational reviews.

**One byte-level normalisation, disclosed rather than silent.** The dispatch prompt named the main
checkout by its absolute machine path once. That exact string is a tracked-path violation the
`tracked_paths_no_machine_roots` guard rejects (discovered as A6 discovery D-1), so it is replaced
here with `<MACHINE_ROOT>`, matching the convention A6's own context packet uses. Nothing else is
altered. This repair changes this file's digest; the ledger's `block.A4.context_packet_digest` field
is updated in the same acceptance transition that records this fix.

## 1. Initial implementer brief (`a4-prompt.md`, second/clean dispatch — the first dispatch hit a
concurrent-duplicate-implementer collision on the same worktree and was discarded before any
source file was written; this is the brief for the single implementer whose work landed)

```
You are a bounded BLOCK IMPLEMENTER for the Verter Revision 11 architecture program, block A4.

Working directory: this repo (dedicated git worktree, branch work/a4-baseline-attribution, based on program/architecture-lock at 839645e3e). Do not touch <MACHINE_ROOT>/verter. You are the ONLY implementer running against this worktree — the tree was just reset to clean (a prior duplicate-session run corrupted it with a torn merge; that corruption has been discarded, nothing from it is in the tree).

AUTHORITY (read before acting, in this worktree):
- docs/arch/refactor/rev11/charters/A4.md (the ratified charter — binding)
- docs/arch/refactor/rev11/program.md section "A4 — Install measurement-only attribution and capture the baseline"
- docs/arch/refactor/rev11/governance.md (Foundational review class requirements)
- docs/arch/refactor/rev11/amendments/*.md — skim for anything mentioning A4 (none currently do; confirm)
- Predecessor: A3, accepted at c1aef669d9c1505e69faf0e327a9c1a5069c5798

OBJECTIVE (from charter, binding):
On the exact post-A3 lineage, make work, copies, allocations, tasks, retention, and determinism observable WITHOUT changing semantics. Install measurement-only attribution: count normalization, hashing, parses/reparses, preparation, planning, projection, resolver/relation/inference/flow, CSS work, rendering/maps/provenance/serialization/FFI, copies, allocations, arena live/capacity, tasks/flights/queues, admission/eviction, retained bytes, pins, and deterministic digests — all by logical identity. Instrumentation must be leaf-only, bounded, and disabled-overhead measured. It must NOT become semantic authority (nothing may branch on these counters). Then capture a baseline dataset and raw profiles.

IN SCOPE: only the deliverables named for A4 in program.md; evidence/source changes strictly necessary to produce them; deletion of obsolete evidence-only scaffolding created by this block before you finish.

OUT OF SCOPE: later production ownership/API cutovers; compatibility shims/parallel architecture paths/speculative services; numeric threshold selection after candidate direction is observed (A6's job); touching crates/verter_session/src/loop5_instrumentation.rs (pre-existing always-on counters — program.md assigns reconciling surviving instrumentation owners to A5, not A4; just measure its cost into the baseline if relevant).

ABORT/RESCOPE: if the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from charter assumptions — STOP, do not improvise a substitute design, and write status RESCOPE_REQUIRED with the exact contradiction to .agent-run/a4-report.yaml. Do not silently widen scope.

PRIOR-ART SEED (a previous run reached this design before being discarded for an unrelated process-hygiene reason — evaluate it on its merits, do not treat it as pre-approved, and diverge freely if your own analysis differs):
- Extend `verter_audit` (leaf observability substrate, depends only on `verter_span`, already a dependency of every crate holding a chokepoint: verter_compiler, verter_lsp, verter_mcp, verter_napi, verter_scheduler, verter_session, verter_wasm, verter_workspace). No new dependency edges needed; verter_semantic/verter_parser/verter_css_syntax don't depend on it and don't need to (observable at session/compiler call sites).
- Schema: closed `WorkSite` enum (or similar) via a declarative macro, each variant with a stable string id + domain (covering exactly the charter's named categories) + unit (Calls/Items/Bytes/Nanoseconds/Digest). Table indexed by site, `snapshot()`/`reset()`, `attribute!`/`attribute_n!`/`attribute_scope!`/`attribute_digest!` macros.
- No-semantic-authority held STRUCTURALLY: the table/snapshot/reset behind `#[cfg(feature = "attribution")]` (non-default) so a production build cannot resolve a reader path at all.
- Disabled-overhead: OFF-arm macro expansion must still type-check the site name/args (so a typo fails a DEFAULT build) but must NEVER evaluate the amount/digest argument and must emit zero code.
- Proof triad: (1) side-effect non-evaluation test with feature off; (2) negative-control compile-fail test — referencing attribution::snapshot() must fail to compile without the feature; (3) measured A/B wall-clock of the same workload feature-off vs feature-on.
- Harness precedent: crates/verter_workspace/src/currency_probe.rs (feature-gated `currency_probe`, `probe_scope!`/`probe_tally!` macros, driven by crates/verter_bench/examples/currency_phase_probe.rs) — right pattern, wrong scope (string-keyed, single-crate); A4 needs a typed cross-crate schema.
- evidence/ is currently flat (A0-summary.md etc). A4 needs a dataset + raw profiles: use evidence/A4-summary.md (flat, matching A0) plus an evidence/A4/ directory for dataset + raw profile dumps.

TDD / EVIDENCE DISCIPLINE (mandatory, no exceptions):
- Disabled-overhead proof: instrumentation OFF must be provably zero/negligible cost.
- Counter schema must be a real typed schema (not string-keyed ad hoc counters).
- No stubs, no always-true assertions, no unconditional default returns presented as instrumentation.
- Prove RED before / GREEN after for any new discriminating test.
- No second production authority; nothing may branch on these counters.

WORK PROCESS:
1. Explore the current Rust workspace (verter_semantic, verter_compiler, verter_session, verter_audit, etc.) to find leaf-only insertion points for each named category.
2. Design the counter schema (typed, by logical identity).
3. Implement instrumentation. Compile-check as you go (`cargo check -p verter_audit --all-targets`, then `--features attribution` variant).
4. Write discriminating tests (disabled-overhead proof, counter correctness, no-semantic-authority, negative-control compile-fail).
5. Capture a baseline dataset + raw profiles per charter's "Required evidence". Use docs/arch/refactor/rev11/evidence/A4-summary.md + evidence/A4/.
6. Run targeted tests for what you touched (`cargo test -p verter_audit`, plus any crate you instrumented). Do NOT run the full `node scripts/gate.mjs` unless you judge it required — mention the tradeoff in your report instead of just running it.
7. Ensure `git status --porcelain` shows only intended changes.
8. Do NOT commit — leave changes uncommitted. The orchestrator commits after verification.
9. Write your final structured report to .agent-run/a4-report.yaml (fields: block, status DONE|BLOCKED|RESCOPE_REQUIRED, changed files, tests red/green, disabled_overhead_proof, evidence paths, discoveries, deletions).

Follow "no phase archaeology": do not reference "A4"/"phase"/"block"/"rev11" in source code comments/identifiers under crates/packages/scripts — docs/evidence only.

Follow CLAUDE.md hard rules if encountered (no stubs, no co-author lines, no git push).
```

## 2. Resume brief (`a4-resume-prompt.md`, dispatched after the initial run stalled asking for
cargo-tool permission approval it did not have)

```
Resume block A4 (measurement-only attribution instrumentation) exactly where you left off in this worktree. Your prior uncommitted work is intact — do not discard or redesign it. You now have cargo approved (check/test/run/clippy/fmt) and may also make WIP commits freely as you go (new standing rule) — commit at sensible checkpoints with plain descriptive messages, no AI attribution, do not squash yourself (the orchestrator squashes at landing).

Pick up at: cargo check -p verter_audit --all-targets, then cargo check -p verter_audit --features attribution --all-targets. Fix any compile errors. Then continue the WORK PROCESS from the original brief (still at .agent-run/a4-prompt.md in this worktree — reread it for full scope/authority/out-of-scope/abort conditions):
- instrument the ~70 call sites across verter_session/workspace/compiler/scheduler per your schema
- trybuild negative-control compile-fail test
- baseline harness + evidence capture (docs/arch/refactor/rev11/evidence/A4-summary.md + evidence/A4/)
- run cargo test -p verter_audit and targeted tests for instrumented crates
- ensure git status --porcelain shows only intended changes (WIP commits are fine, just no stray junk)
- write final report to .agent-run/a4-report.yaml (status DONE|BLOCKED|RESCOPE_REQUIRED, changed files, tests red/green, disabled_overhead_proof, evidence paths, discoveries, deletions)

Do NOT run node scripts/gate.mjs unless you judge it required (note the tradeoff in your report instead).
```

(Dispatched first with `--allowedTools "Bash(cargo *) Bash(git *) Edit Write Read Glob Grep"`, per the
orchestrator's permission policy at that point in the session; superseded later in-session by a
standing rule to use `--dangerously-skip-permissions` for every worker/reviewer confined to its own
dedicated worktree — that already-running resume was allowed to finish under its original grant
rather than being killed and relaunched, since it was progressing correctly.)

## 3. Fix-pass brief (`a4-fix-prompt.md`, dispatched after all three Foundational reviews returned
verdicts against the pre-squash WIP tip `347436eff`)

```
You are a bounded FIX agent for block A4 (Verter Revision 11). Three independent reviews (conformance PASS-with-nits, architecture-conformance BLOCKING_FINDINGS, adversarial BLOCKING_FINDINGS) ran against candidate SHA 347436eff on this worktree/branch (work/a4-baseline-attribution). Apply ONLY the fixes below — no redesign, no scope expansion. Commit freely as WIP (plain descriptive messages, no AI attribution); do not squash.

FIX LIST (all confirmed real, cited with file/line where known):

1. [architecture finding] `crates/verter_bench/examples/attribution_baseline.rs:28` — module doc comment names the program/revision/block ("rev11/A4") in a path reference under `crates/`. Reword to drop the program/revision/block identity, e.g. "see the disabled-overhead evidence note" instead of the literal `docs/arch/refactor/rev11/evidence/A4/disabled-overhead.md` path. (No phase-archaeology rule technically scopes to `crates/*/src` only, but this is `crates/verter_bench/examples/`, and the reviewer correctly flagged the substance — fix it.)

2. [stale cross-reference, both reviewers flagged] `attribution/mod.rs` doc block and `crates/verter_audit/src/attribution/disabled_tests.rs:18` point at a nonexistent path `tests/cases/attribution_no_reader_path.rs`. The real file is `crates/verter_audit/tests/cases/attribution_compile_fail.rs`. Fix both doc pointers to the real path.

3. [F1, blocking, evidence-unsupported-number] `docs/arch/refactor/rev11/evidence/A4-summary.md` D5 / line ~102: "semantic_dispatch holds 216 ms inclusive across 4,216 dispatches" is presented as if comparable to wall-clock share, but it is 2.9x the run's actual wall clock because nested `ScopeGuard`s on recursive cold-build re-entry each record the full inclusive interval (double-counts by recursion depth). Fix: reword D5 to explicitly state the figure is a SUM OF INCLUSIVE NESTED INTERVALS, not a wall-clock share, and note it exceeds total wall clock for that reason (cite the ratio ~2.9x). Do not delete the finding — the qualitative claim ("resolution dominates") may still be defensible, but the number must be correctly framed. Also add a doc-level note to `WorkUnit::Nanoseconds` in `attribution/schema.rs` (or wherever it's documented) stating explicitly that inclusive nested scopes double-count and are not additive across recursion.

4. [F2, blocking] D6 title "Two thirds of allocations land outside any scope guard" — actual figure from the committed TSV is 55.8% by count (50.0% by bytes), not two thirds. Fix the title/text in A4-summary.md to state the correct percentage.

5. [F3, blocking] A4-summary.md line ~108: "Scoped regions account for ~110 MB inside semantic_dispatch alone" — the actual per-row value is 98.6 MB (109.8 MB was a mis-read running cumulative total, not the row value). Fix to 98.6 MB (or your own re-derived figure from the TSV).

6. [F4, blocking] D2 / A4-summary.md line ~98: "16,917 fact observations (413/file)... single most frequent operation" — 16,917 is the CALLS column (fan-out invocations); the actual per-observation AMOUNT column is 73,923 (1,803/file). Fix the cited number to correctly distinguish "calls" vs "items observed" — state both if useful, but do not conflate them. The qualitative claim may stand if still true under the corrected number.

7. [F7, minor] The baseline harness's determinism check prints "compiled_output run1=0 run2=0 AGREE" for a site with zero recorded observations — a vacuous, non-discriminating pass (0==0 always holds). Fix `crates/verter_bench/examples/attribution_baseline.rs` (or wherever `determinism_check` lives) to report N/A (not AGREE) when a site's call count is 0.

8. [F6, minor, hardening] `crates/verter_audit/src/attribution/scope.rs` — `ScopeGuard` is currently `Send`, which is a latent cross-thread mis-attribution hazard (not live today, but the substrate is meant for others to extend). Add `PhantomData<*const ()>` (or equivalent) to make it `!Send`. One line. Re-run `cargo check -p verter_audit --features attribution` and `cargo test -p verter_audit --features attribution` after to confirm nothing relies on Send.

9. [F5, policy — DISPOSITION, not a code fix] The adversarial reviewer flagged that the `attribution` feature (and its `compile-fail` companion) is never compiled or run by `node scripts/gate.mjs`, so the enabled arm and the compile-fail negative-control seal can silently rot. DISPOSITION: DEFER to A5, which program.md charters to "reconcile surviving instrumentation owners" (the natural point to decide gate wiring for whichever instrumentation rails survive A5's reconciliation against `loop5_instrumentation.rs`). Record this explicitly as a new "Known gaps / deferred" entry in `docs/arch/refactor/rev11/evidence/A4-summary.md`: title it clearly (e.g. "Gate coverage for the `attribution`/`compile-fail` features is deferred to A5"), name the debt (enabled arm + compile-fail trybuild seal uncompiled by the canonical gate), the durable owner (A5), and reference this review's finding id (adversarial F5). Do not modify `scripts/gate.mjs` in this block.

10. Optionally fix the Cargo.toml comment nit the conformance reviewer found (stale `-- --ignored` instruction in `crates/verter_audit/Cargo.toml` — the cfg_attr only ignores when the feature is OFF, so `--features compile-fail` alone runs it, no `-- --ignored` needed). Low priority but cheap.

AFTER FIXING:
- Run `cargo check -p verter_audit --all-targets` and `cargo check -p verter_audit --all-targets --features attribution` — both clean.
- Run `cargo test -p verter_audit`, `cargo test -p verter_audit --features attribution`, `cargo test -p verter_audit --features compile-fail` — all green, same or better pass counts than before.
- Run `cargo clippy -p verter_audit --all-targets --features attribution -- -D warnings` — clean.
- `git status --porcelain` shows only intended changes.
- Do NOT run the full `node scripts/gate.mjs`.
- Do NOT commit changes to files outside the fix list above.
- Write a short completion note to `.agent-run/a4-fix-report.yaml`: status DONE|BLOCKED, fixes_applied: [1..10 with brief note each], tests: {green: [...]}, commits: [sha list].
```

## Review mandates dispatched (Foundational class)

Conformance (sonnet, effort low), architecture-conformance (opus, effort medium), and
adversarial/performance (opus, effort high) reviews, each against the exact same candidate SHA
`347436eff` in its own dedicated worktree, each under the standard scope-control contract
(conformance review only, no redesign; block only on violated authority, unsupported/incorrect
evidence, or incomplete required closure). All three `--dangerously-skip-permissions`.

## Outcome

Conformance: PASS, nits only (stale doc pointers). Architecture: BLOCKING_FINDINGS, one finding
(phase/program-identity leak in a doc comment under `crates/`). Adversarial: BLOCKING_FINDINGS —
F1-F4 evidence-number corrections (216ms nested-recursion double-count misread as wall-clock
share; "two thirds" allocation-share overstatement, actual 55.8%; ~110MB cumulative-vs-row
mis-read, actual 98.6MB; calls-vs-items column conflation), F5 policy disposition (gate coverage
deferred to A5), F6 minor hardening (`!Send` on `ScopeGuard`), F7 minor (vacuous determinism
check), F8-F12 informational/no-fix-required. The adversarial reviewer independently re-derived
the disabled-overhead claim (CONFIRMED, tighter than the implementer's own measurement) and could
not break the no-semantic-authority or structural-safety claims — the blocking findings were
entirely in the evidence narrative, not the substrate.

One bounded fix pass applied (fix-pass brief above), impact-bounded reattestation (targeted
`cargo test`/`cargo clippy` reruns plus direct TSV spot-checks of the corrected numbers, not a
third review round), squash-landed as `147258e0be47b65fb872236599655d06bf4621f5`.
