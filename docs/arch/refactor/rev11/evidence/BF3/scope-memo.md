# BF3 scope memo — recorded deviation from the charter's production mechanism

**Status:** recorded deviation; the required
[`AMD-009`](../../amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md)
package is drafted but **UNRATIFIED** until the designated maintainer accepts it.
**Authority:** the independent architecture consult reproduced verbatim at
[`scope-consult-ruling.md`](scope-consult-ruling.md); the exact dispatch prompt is
[`scope-consult-prompt.md`](scope-consult-prompt.md).
**Base:** `program/architecture-lock` at `040084bf0`.

## 1. The conflict

BF3's charter ([`../../charters/BF3.md`](../../charters/BF3.md), "Required procedure per
successful cell", steps 3–5) mandates, for a known-wrong Svelte or non-Vue-runtime cell
that currently reports success: detect the affected request before publication from typed
data, return typed non-success, publish no partial product, and retract the whole
capability cell when the broken subset is not safely distinguishable — each bound to a
`BF3-RET-*` record with a correction owner and a removal ID.

That is a production mechanism. A standing project-wide maintainer rule, recorded
2026-08-13, states the opposite direction in general terms: a wrong output is a BUG, not an
error path; production must not gain typed refusals, publication guards, runtime tracking,
or known-divergence machinery consumed by production; the correct response is a failing
characterization test plus an actual fix. Test-side tracking remains explicitly permitted.

[`AMD-006`](../../amendments/AMD-006-vue-known-defect-correction.md) §1 encodes that
direction but scopes it textually to Vue VDOM/Vapor/SSR, reassigning those rows to BV0
correction; §4 states BF3 "retains the original procedure" for in-scope Svelte and
non-Vue-runtime cells. §8.1 records that all three independent review mandates raised
exactly this conflict as their shared round-1 blocking finding, and that a prior
architecture ruling resolved it `RETROACTIVE-NO-FORWARD-ONLY` — the general rule governs
BV0's Vue findings and future findings outside BF3's already-ratified retained inventory,
but does not repeal BF3's existing mechanism for its retained domain. The live ledger's
BF3 note mirrors that.

## 2. The consult and its ruling

An open-ended, option-free architecture consult was dispatched read-only against this
worktree with the full evidence on both sides — including the prior `RETROACTIVE-NO-
FORWARD-ONLY` ruling, presented as something to adjudicate rather than to confirm. Its
ruling, in summary (full text in [`scope-consult-ruling.md`](scope-consult-ruling.md)):

- BF3 builds **no** new production guard, typed refusal, artifact-withholding path,
  retraction table, or runtime tracking mechanism for incorrect-but-successful output. The
  prior ruling is "procedurally understandable but architecturally wrong"; ratification
  history explains why an implementer may not silently ignore current text, but does not
  justify preserving an inferior mechanism. There is no principled Svelte exception —
  consistent with the separate standing rule that Svelte is first-class alongside Vue.
- A typed refusal stays legitimate **only** when it expresses a real, independently
  specified capability boundary decided from the typed request before compilation. It must
  never be triggered by a fixture identity, a known compiler defect, an oracle mismatch, a
  syntax pattern selected because it currently miscompiles, or a version-specific
  known-divergence list. Svelte's existing `ServerGenerate` refusal
  (`crates/verter_compiler/src/svelte/runtime/client_compile.rs`, the SSR arm that fails
  closed before any emitter work) meets that bar: BF3 tests and records it and adds nothing
  to it. It is not a BF3 guard and carries no BF3 removal ID.
- BF3 is reshaped from a safety-retraction block into a **conformance-exhaustion and
  correction-dispatch** block: build and run a Svelte counterpart of the genuine
  shipped-path authoritative gate; exercise the exact six `svelte@5.56.8` client cells
  across every applicable axis; prove each axis discriminates via planted-defect controls;
  exhaust the remaining reachable-success product/route inventory; separate genuine
  compiler defects from harness/normalizer/source-content/route-assembly artifacts before
  assigning ownership; add a precise failing regression per genuine defect; route each to a
  correction owner rather than a runtime guard.
- Safety is provided by refusing to advance the program — B2/B3 stay locked until the
  corrections arising from BF3 are accepted — not by contaminating the shipped compiler
  with defect recognition.
- The stale `svelte@5.56.3` pin (root `package.json`, `crates/verter_svelte_conformance`,
  `crates/verter_compiler/tests/svelte_oracle_corpus`) is a separate conformance-
  infrastructure migration that belongs in its own authorized block, not folded into BF3
  and not left to BS1.
- No designated-maintainer-only *architectural* decision is required, but changing the
  normative program text does require maintainer ratification of a formal amendment.

## 3. What this block does with that ruling

BF3 executes the ruling's direction on the half it can execute unilaterally, and escalates
the half it cannot.

**Executed here.** BF3 adds no production mechanism. Its work is the probe, the
exhaustion, the discriminating regressions, and the per-failure disposition. Adding nothing
to production is the reversible direction: it neither creates a mechanism a later block
must delete nor commits the program to one.

**Escalated, not executed here.** Superseding the live normative text — BF3's charter title,
objective, procedure steps 3–5, step 7, the retained-retraction paragraph, its exit clause
demanding a guard plus removal ID, AMD-006 §4 and §8.1, the BF3 ledger note, the
`BF3-RET-*` scheme in
[`../framework-conformance/bf3-safety-retraction-scope.md`](../framework-conformance/bf3-safety-retraction-scope.md),
and any DAG edge for an immediate Svelte correction owner — is maintainer-ratified
amendment work. So is authorizing the separate `svelte@5.56.3` → `5.56.8` migration block.
Both are reported upward with this memo as their evidence.

**Ordering, and why it matters.** The mechanism question is only load-bearing if BF3's
retained inventory actually contains a genuine known-wrong successful cell. That is
unknown: the Rust seed loader
(`crates/verter_session/src/compile/map_equality_tests/bf2_seed_matrix.rs`) filters the
golden manifest to `vue/`, so no Svelte cell has ever been driven through the authoritative
comparator. BF3 therefore probes first and disposes second, and records no disposition
derived from a presumed outcome. This ordering is deliberate: BF3's own predecessor history
contains a probe conclusion ("0/36 Vue cells pass, retract wholesale") that a second
independent re-investigation proved materially overbroad — the cell count conflated
repeated axes, one axis's failures were a `sourcesContent` comparison artifact, and a
control was trivial. The corrected disposition turned out to be correction (BV0), not
retraction at all.

## 4. Inventory as established by direct inspection

- `packages/framework-conformance-harness/goldens/manifest.json` carries 48 entries: 36
  `vue/*` (BV0's domain, corrected and landed) and 12 `svelte/*`.
- The 12 Svelte entries are 3 fixtures x {client, server} x {dev0, dev1}: `basic-runes`
  (runes), `props-events` (runes), `legacy-slots` (legacy). BF3's "exact `svelte@5.56.8`
  client cells" is therefore exactly **6** cells; the 6 server cells correspond to the
  already-typed `ServerGenerate` refusal and are already non-successful.
- The harness's accepted entry point is `bin/check-candidate.mjs --authoritative`, whose
  axes are parse, link, structural, diagnostics, mapping, and runtime, and which turns any
  skipped axis into a hard failure under `--authoritative`.
- The remaining in-scope product/route inventory is the one enumerated in
  [`../framework-conformance/bf3-safety-retraction-scope.md`](../framework-conformance/bf3-safety-retraction-scope.md).
