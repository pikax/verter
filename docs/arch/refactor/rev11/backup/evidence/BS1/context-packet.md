# BS1 — context packet

Base `6c3939734`. Candidate `fce37476e`. Predecessor B4, ACCEPTED.

## Scoping

A read-only scoping pass established that the charter's "Required exits" paragraph
(`FC-SVELTE-001`, `FC-HYDRATION-001`, `FC-TS-001`, `FC-ATOMIC-001`, `FC-ZERO-WORK-001`,
`FC-PERF-001`, "every corresponding BF3 guard is removed") is defined only in unratified
AMD-005 — only BS1's existence is authorised, per the 2026-08-20 maintainer ruling. No BF3
guard exists to remove (BF3 landed with zero production guards); the FC-* IDs appear nowhere
as a live test or gate. Treating that paragraph as binding acceptance criteria would import
unratified text, so this candidate does not claim those IDs closed. It targets the charter's
"Owned scope" text instead (Svelte-native client codegen correctness), which carries no
AMD-005 dependency.

The Svelte oracle pin (`svelte@5.56.8`, `official-core-oracles.md:10`) is current — no repin
needed, unlike BV1's Vue rc.1→rc.3 repin.

## Slice chosen

Seven already-`#[ignore]`d, RED-characterized client compiled-output defects in
`crates/verter_compiler/src/svelte/runtime/client_tests.rs`, measured against the pinned
official compiler, identified as the smallest real gate-provable first cut (not the CSS
covering-array crate, not the official-pack TSV exhaustion, not SSR/hydration cross-pairing —
all separate, larger, or out-of-scope trains).

## Review arc

Three mandates on the initial 7-fix candidate: codex (conformance, `gpt-5.6-sol`/high), grok
(architecture, `grok-4.6`/high, explicit default-to-BLOCK), Claude subagent (adversarial,
isolated worktree, genuine plant→RED→revert→GREEN against all 7 fixes plus 4 additional
adversarial probes). Adversarial: PASS, no findings. Codex and grok independently converged
BLOCKING on the same root defect (the single-name-destructure-each fix conflated binding NAME
with property KEY, correct only for object shorthand); codex additionally found a debug-only
panic risk in the function-declaration name-mapping fix (a `debug_assert!` reachable on
admitted `async`/generator declarations); grok additionally found a new zero-span production
withhold on the destructure-each emit path.

Fix round 1 addressed all three: the function-decl mapping now derives its anchor from the
real parsed name offset instead of a fixed keyword literal, degrading to unmapped rather than
asserting; the destructure-each fix is narrowed to a new `PatternShape::ShorthandSingleProperty`
(the only shape where local name is provably the correct property key), with renamed/array/rest
single-name decompositions failing closed at classification time with a REAL span — a legitimate
pre-compilation capability boundary, not a miscompile-avoidance withhold. A targeted codex
delta-review confirmed A and B closed, and surfaced one residual: a genuinely multi-name
each-item destructure (`{ a, b }`) still refuses through a DIFFERENT, PRE-EXISTING code path
(`pattern_single_binding`, present at base `6c3939734`, unrelated to this candidate's changes)
that also carries a placeholder span. Verified pre-existing via `git show
6c3939734:.../client_block_plan.rs`. Per the maintainer's standing bugs-and-types ruling this is
captured as one `#[ignore]`d, verified-real characterization test rather than fixed in this
slice — closing it needs the same real-span + per-property-read plumbing generalized to N
properties, a materially larger change than the single-name shape gate this round scoped to.

## Debt row proposed (for the program orchestrator to record)

**Finding:** `pattern_single_binding` (`client_block_plan.rs`, pre-existing at base) refuses a
genuinely multi-name each-item destructure with a placeholder `Span::new(0, 0)` instead of the
pattern's real location.
**Owner:** a future Svelte each-item destructure slice (no existing DAG block currently owns
this; BS1 itself if the train continues, or its successor).
**Acceptance:** the refusal carries the pattern's real span, OR the multi-name case is
implemented for real with correct per-property reads.
**Test:** `a_multi_name_each_item_destructure_refuses_with_a_placeholder_span`
(`client_tests.rs`), currently `#[ignore]`d, verified to pass when run un-ignored (pins today's
actual placeholder-span behavior).
