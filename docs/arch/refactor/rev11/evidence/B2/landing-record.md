# B2 — landing record

Base `41e039c2f`. Candidate `32d3a2f07`. Dispatch context:
[`context-packet.md`](context-packet.md).

## What shipped

- Canonical parse-key/syntax-profile identities (`verter_language::parse_identity`)
  for Vue and Svelte carriers, so two requests differing only in
  canonically-equivalent options (custom-element ordering, etc.) encode
  identically. `ParseOptions` fields are mandatory, not `Option` — the crate never
  substitutes a default for an absent value; a caller that means "Vue's standard
  delimiters" opts in explicitly via `ParseOptions::vue_standard()`.
- Structural redesign of registered-carrier-geometry construction: the previous
  `CarrierAccessToken`/`RegisteredProjectorSeal` design was a remintable,
  non-capability value any external caller could obtain via a public function.
  Replaced with a closed `KnownRegisteredCompiler` enum behind
  `CarrierCompilerRegistry::project_registered`, which takes no caller-supplied
  compiler — no `&dyn CarrierCompiler`, no `Any` downcast on the public
  `CarrierCompiler` trait. Typed-carrier recovery for `verter_session` goes
  through monomorphic per-adapter openers installed at registry-build time.
- Diagnostic spans mandatory end-to-end: `HostDiagnostic`, `FfiDiagnostic`,
  `NapiDiagnostic`, and every intermediate producer now carry a real mapped
  `verter_span::Span`, never an `Option` with a fabricated fallback.
- Svelte's unsupported "loose" parse profile is rejected with a typed
  `SyntaxReject::UnsupportedProfile` before parsing, participating in the
  `SyntaxProfileId`/cache-key identity — not silently downgraded to strict.
- Svelte's carrier parser no longer hard-rejects the whole carrier on a
  recoverable mid-edit syntax state (unclosed tag, unterminated block); the
  tokenizer already produces a faithful, structurally usable tree for those
  states, which every IDE feature needs while the user is mid-edit. The
  compile-time official-conformance gate is unaffected.
- Vue's parse diagnostics flow through the same registered-artifact channel
  Svelte already used, instead of being rebuilt separately downstream from a raw
  carrier.
- Official parse-conformance evidence generation persists real per-invocation
  results (expected/actual/variant/match/verdict hash), not a bare
  classification.

## Review arc

Class `foundational`; all three mandates required. **Codex CLI is non-functional
in this environment** (`code-mode host is disabled` on every invocation,
including with the documented `--disable code_mode_host` workaround; reproduced
repeatedly with a bare `echo hello`). Substituted **kimi** for the conformance
seat throughout, with the substitution disclosed in every affected review prompt.

**Round 1** (recovered from a prior session, commit `f965e6576`) — conformance,
architecture, and adversarial all `BLOCKING`. Central findings: the capability
sealing above; `HostDiagnostic.span` was `Option` with a tested `0..0` fallback;
B2 was defaulting Vue delimiter options (B3's job); AMD-010's amended E1 evidence
exit was unresolvable opaque hashes; Svelte's loose profile fell through as
"unverifiable" instead of a typed reject; CSS/runtime-conformance/codegen work had
leaked into the block's diff; `SyntaxReject` was used for internal invariant
violations; Vue diagnostics bypassed the registered artifact.

**Round 1 fix** (6 commits, `5f0b0c285`) closed 5 of 9 findings; deferred the
capability-sealing redesign and the Vue-diagnostics-channel fix, citing a
process-global-lock constraint for the former. **Delta re-review** (kimi, default-
to-block) independently re-verified every claim and both DEFERs: `BLOCKING` — 7 of
9 items still open, including both DEFERs rejected as illegitimate (the capability
hole was still fully exploitable; the Vue-diagnostics gap was an original
conformance blocker, not a bounded follow-up).

**Round 2 fix** — a grok-4.6 xhigh architecture consult (see context packet)
produced a concrete redesign for the capability sealing, implemented in full along
with the remaining 6 items. **Round 3** (parallel: conformance=kimi,
architecture=grok-4.6 xhigh, adversarial=Claude subagent in its own worktree cut
from the candidate commit) — architecture `PASS`, adversarial `PASS` (real
plant/red/green mutation proof: a live external-crate attacker `CarrierCompiler`
proof-of-concept confirmed it cannot reach the registered-projection arms).
Conformance `BLOCKING`: independently re-checked against the pinned official Vue
oracle and found the round-2 implementer's own out-of-scope-revert deviation
(restoring parser-level `XVSlotMisplaced`, deleting `MissingSfcEntryBlock`/
`TemplateFunctionalUnsupported`) was itself a real conformance regression against
official `compiler-sfc` behavior for 6 named official-case rows, with the
committed evidence left stale/false after the revert. Also found one genuinely
red trybuild fixture in the full compile-fail suite (a stale `.stderr` baseline
after the capability-sealing visibility change) that a narrower targeted test run
had missed.

**Round 3 fix + targeted delta review (the round cap)** — re-derived ground truth
per affected row directly against the pinned official Vue checkout (not taken on
trust from either side); landed a per-check split matching the oracle exactly
(5 rows keep the parse-level checks, the 6th's checker correctly stays
compile-level-only) and regenerated the evidence for real. Fixed the trybuild
baseline; independently confirmed the second red fixture
(`deferred_callable_is_sealed_to_its_two_consumers`) was genuinely pre-existing
(zero diff on its fixture/source across the whole block range) and block-
unrelated rustc-wording drift. Delta review (kimi, re-deriving every citation
independently): `PASS`.

## Post-review-cap gate findings (mandatory fixes, not additional review rounds)

Two issues surfaced only at gate time, after the round cap was exhausted; both are
mandatory-before-landing regardless of review-round budget, not judgment calls
needing disposition:

- `crates/verter_ffi/src/convert/tests.rs` had 16 sites still comparing the
  now-mandatory `span_start`/`span_end` `u32` fields against `Some(k)` —
  `verter_ffi` was missing from every prior round's targeted crate list (a scoping
  gap in the round-2 fix brief). Mechanical fix, 126 tests pass.
- The full gate found 11 real (non-flaky) LSP/type-runtime test failures, all
  traced to one root cause: `SvelteCarrierCompiler::parse` was hard-rejecting the
  *entire* carrier whenever the tokenizer recorded any strict-parse fact — but the
  tokenizer is deliberately infallible and produces a faithful, usable tree for
  exactly those mid-typing states; that split is the whole IDE-vs-runtime
  contract. Confirmed as a genuine regression (not pre-existing) by running the
  same 9 non-real-provider failures against base `41e039c2f` in a separate
  worktree — all 9 passed there. Fixed by removing the hard-reject from `parse()`
  and keeping strict facts internal to the existing compile-time
  `official_reject_gate`. A second, independent cause on the Vue side (a test
  fixture using a bare `<div />` with no template/script wrapper, which the
  round-3 fix's now-correct `MissingSfcEntryBlock` check correctly flags) was
  fixed by correcting the fixture, not the check.

## Landing-hygiene fix (this manager, post-squash)

Audited full commit-message body text and the diff for program vocabulary and
machine-path leaks before squashing. Found and fixed one real item in production
test source: a doc comment in `official_parse_manifest_guard.rs` literally named
the `B2-parse-facet-{vue,svelte}.md` evidence files; reworded to remove the block
identifier (compile-verified, no behavior change).

**Disposition — left as-is, not a violation:** the same guard file matches a
literal `"B2"` token against an owner-tag column in the pre-existing, already-
accepted `docs/arch/refactor/rev11/evidence/framework-conformance/*.tsv` data
(established by an earlier block, BF2/BF3). This is a functional data-match, not
narrative program-vocabulary — renaming it unilaterally risks either silently
breaking the guard's own row selection or touching cross-block-owned data outside
this block's scope to safely rewrite. Flagged for a coordinated follow-up if a
cleaner resolution is wanted; not unwound here.

## Gate

`node scripts/gate.mjs --test-threads 8 --memory-limit 18GiB`, run three times:
once mid-cycle (found the Svelte hard-reject regression above), once after that
fix (clean except the documented pre-existing `failed_respawn_retries_within_budget_and_recovers`
flake, independently reproduced against base), and once more on the exact final
squashed+amended commit `32d3a2f07` for rigor. Final run: **VERDICT: PASS**, all
three surfaces green (`24459/24459`, 3/3 suites, `8645/8645`) — the flaky respawn
test passed on this run too.

Also on the landed tree: `cargo fmt --all --check` clean; `cargo clippy
--workspace --all-targets -- -D warnings` clean; `cargo clippy --target
wasm32-unknown-unknown -p verter_wasm -- -D warnings` clean; `cargo check
--workspace --release` clean. `pnpm test`: only `packages/typeinfo` (3 tests)
failed, independently confirmed pre-existing by running the identical tests
against base `41e039c2f` with a freshly built native binding (same 3 failures
there); every other package passed.

## Scope held

No CSS semantic work (a pre-existing carrier-boundary CSS-parsing revert was
re-verified genuinely reverted at round 3, not merely relocated). No framework
runtime/conformance-correction work beyond what the round-1/round-3 reviews
required reverting or correcting on B2's own parse facet. No codegen ownership
change beyond the round-3 per-check oracle-derived split, which left the
compile-level `collect_template_compile_diagnostics` copy exactly as B4's
predecessor commit had placed it. No type-correctness work opened. Every DEFER
from round 1 was either closed for real or rejected as illegitimate by round 3 —
no open DEFER remains on this block.

## Post-landing conformance re-review (rounds 4-7)

The initial landing above (candidate `32d3a2f07`) was recorded as landed-but-not-accepted:
its conformance mandate had been run by an off-roster tool substituted on a false
premise that the rostered seat was unavailable. A direct probe confirmed the
rostered seat was healthy, so the conformance mandate was re-run on it against the
landed tree, and the re-run surfaced real, previously-undetected defects. Closing
them took four further fix rounds, each independently re-reviewed (rostered
conformance seat plus an independent empirical re-verification agent that ran real
oracle-backed regenerations and test suites — no round's own self-report was taken
on trust):

- **Round 4** — `MissingSfcEntryBlock` (Vue) did not cover empty/self-closing
  `<script>`/`<script setup>` blocks with no `src`, which official `compiler-sfc`
  rejects; Svelte's strict-parse diagnostics (a missing attribute value, an
  unclosed tag, and similar recoverable syntax errors) were being silently
  dropped from the carrier's diagnostic channel instead of surfaced. Fixed both;
  regenerated both frameworks' parse-facet evidence for real against the pinned
  oracles (previously-false Svelte evidence — 27 rows recorded `pass` with
  diagnostics the tree could not actually produce — became genuinely accurate).
- **Round 5** — the round 4 fix itself had three defects: the Vue `src`-attribute
  check tested value-presence instead of attribute-presence; a Svelte tokenizer
  recovery point double-emitted one defect under two diagnostic channels; and,
  most seriously, mapping Svelte's strict-parse facts onto the diagnostic channel
  at `Error` severity reintroduced — one layer up, at `compile_entry` rather than
  at carrier publish — the exact fail-closed-over-a-recoverable-defect regression
  an earlier fix (mid-cycle in the original landing) had already resolved once.
  Added a `blocks_compile` distinction on the framework-neutral diagnostic type so
  a diagnostic can be fully IDE-visible without gating whether compilation
  produces output; proved the fix with a test that fails against round 4's code
  and passes against round 5's.
- **Round 6** — the `src` check was case-insensitive where official Vue's
  attribute-presence check is case-sensitive, and the JS verification harness had
  a matching bug plus a quoted-attribute-value false-positive; a round-5 test
  asserted the wrong diagnostic code for one unclosed-`<style>` shape, which
  investigation traced to a genuine (pre-existing) production misclassification,
  fixed narrowly (boundary-shape vs mid-construct, not full CSS-reader parity).
- **Round 7** — the FULL canonical gate (run for the first time on this fix
  chain, not just targeted crate suites) found a pre-existing integration test
  asserting `<script setup></script>` compiles successfully; verified against the
  pinned oracle that official Vue's `ignoreEmpty` default drops an empty
  script-setup exactly like an empty script, so the fix chain's behavior was
  correct and the pre-existing test encoded an unchecked assumption — corrected
  it. Also found and fixed a genuine duplicate-diagnostic bug on the Vue side
  (parse-time diagnostics double-counted when a compiled bundle's diagnostics
  merge on top of an already-parsed carrier's snapshot).

Two items were investigated and explicitly dispositioned as NOT requiring a fix
in this block:

- A trybuild fixture (`deferred_callable_is_sealed_to_its_two_consumers`) whose
  `.stderr` baseline references a `compose` associated function that no longer
  exists in `crates/verter_session/src/semantic_query/deferred_callable.rs` —
  confirmed, by running it standalone against both the landed tip and the base
  commit `41e039c2f` in separate worktrees, to fail identically at both (zero diff
  across the block's full commit range on the fixture, its baseline, or the
  source file it targets). Pre-existing, unrelated to this block; worth a small
  dedicated follow-up to re-bless the baseline.
- The `<style>` boundary-shape classifier (round 6) does not reproduce official
  Svelte's exact diagnostic code for every mid-construct unclosed-style shape
  (only the empty/`}`/`{`/`;`-boundary split its one recovery point needs,
  disclosed in its own doc comment) — confirmed the specific gap is byte-identical
  before and after round 6 (pre-existing, not newly introduced or falsely claimed
  fixed) and out of scope: achieving full CSS-reader diagnostic-code fidelity is
  CSS-classification work, which is suspended program-wide pending a later program
  stage.

Final gate (round 7 tip `efa167801`): `node scripts/gate.mjs --test-threads 8
--memory-limit 18GiB` — **VERDICT: PASS**, all three surfaces green
(`24470/24470`, 3/3 trybuild suites, `8649/8649`) on a second run (the first run
on this tip had found the round-7-fixed regression and one pre-existing flake/
one pre-existing load-dependent timeout, neither of which reproduced on the clean
re-run). `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D
warnings`, and `cargo check --workspace --release` all clean. `pnpm test`:
`packages/typeinfo` (3 tests, matching the original landing's already-disclosed
pre-existing failures) and `packages/framework-conformance-harness` (package-
install/closure-integrity tests, unrelated to Vue/Svelte parsing) failed; both
packages have zero diff across this block's entire commit range against base
`41e039c2f`, and the widespread `Unsupported engine: wanted >=22, current
v20.20.2` warnings across the whole install indicate a local Node-version
mismatch, not a code regression. Every other package passed.

Conformance mandate, re-run on the rostered seat against the final tip: **PASS**.
Architecture and adversarial mandates were not re-run — none of rounds 4-7
touched the item-2 structural-sealing redesign or introduced new architectural
surface those mandates evaluate; their original round-3 `PASS` verdicts stand.
