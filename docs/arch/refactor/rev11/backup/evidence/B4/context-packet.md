# B4 — context packet

Base `664cab091`. Candidate `84c676689` (preceded by standalone infra commit `05779b05f`).

## Scope

Logical source units, fragment placement, source-space and assembly
mapping composition, and atomic compiler-artifact publication per the
ratified `docs/arch/refactor/rev11/contracts/fragment-assembly.md`
contract. Full ratified scope: `docs/arch/refactor/rev11/charters/B4.md`
(unchanged by this candidate).

Pre-implementation scoping (two independent codex/grok high-effort
consults) converged on: `CodeTransform` stays Converged as the
fragment-local edit engine; a new `verter_compiler::assembly` substrate
(Fragment/SourceUnit/ArtifactSet) replaces `generated_chunk.rs` +
session's `assemble_vue_main_module` + `compile_entry`'s Main slot; Vue
main-module assembly is the first producer proving the contract, since it
was the only existing multi-fragment case with typed map refusal already
and a live contract violation (a generated-text scan) to delete.

## Review arc

Round 1 (all three seats, concurrent) against the initial candidate:
BLOCKING on conformance and architecture — the substrate existed
(well-tested in isolation) but production barely used it; the real Vue
main-module assembler (`assemble_vue_main_module`/`map_compose.rs`,
including its own live generated-text scan) was untouched. Adversarial
PASS.

Two codex rulings resolved genuine scope-boundary questions rather than
being re-litigated round to round: (1) whether closing the script-rewrite
scan required BV1-owned script-producer changes — ruled B4-in-scope,
same structural fact-threading pattern already accepted for the
template-side fix; per-site evidence confirmed every emission site
already knows what it writes. (2) after the resulting cutover, whether
completing full request-driven multi-slot `compile_entry` consumption
(all six virtual-file kinds) was also B4's to finish now — ruled DEFER,
correctly assigned jointly to not-yet-run B5/C4 in the ratified ledger;
B4's own exact-product-set/atomic-publication obligation is the
plan/publish mechanism, which does exist and is proven on the Main
artifact.

Round 2 (all three seats): architecture PASS; conformance narrowed from 6
blocking findings to 2 residual mechanical ones (an unconverted panic on
one source-map chain call, stale evidence-doc text); adversarial found one
real, unambiguous rule violation (a landed name-keyed source-tree scanner
test) plus one narrow, non-blocking architectural gap. Round 3 (final,
targeted delta, no further architecture round needed per the round cap):
closed both mechanical residuals, deleted the scanner test (its invariant
already has structural/functional coverage from two other tests),
recorded the narrow gap as tracked debt rather than attempted.

A `cargo check --workspace --release`-only defect (two newly-added public
struct fields leaking an internal marker type past crate boundaries,
surfaced only under whole-workspace release reachability analysis, not
under `-p`-scoped or dev-profile checks) was found and fixed during
landing-readiness verification, independent of the review-seat rounds.
