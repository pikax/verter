# Replacement-validation deviation ledger

This is the human narrative for the replacement-validation deviation ledger. The
machine-readable source of truth is the sidecar **`replacement-deviations.json`**
(schema `verter.replacement-deviations.v1`, validated by
`replacement-deviations.schema.json`). When the two disagree, the JSON wins.

See the charter — [`../verter-replacement-validation-campaign.md`](../verter-replacement-validation-campaign.md) —
for the campaign north star, the three workstreams, and the methodology this ledger
records.

## Hermeticity (non-negotiable)

Every entry is **hermetic**. The validation projects are local analysis inputs
only: no project name, path, source byte, identifier, or verbatim reference message
ever appears here. A deviation is reduced to a **generic, vendored, minimal repro**
(`genericReproFixture`) authored from scratch to exhibit the *class* of the
deviation; evidence is generic (TS code + node kind + a hand-authored snippet + a
message *shape*), never a project excerpt. Paths live only in the gitignored local
analysis config; the leak guard scans every committed artifact, including this file
and the sidecar.

## The five classes

Each deviation is classified before any change is made. The stored class names
(used in the sidecar's `class` field) are:

- **`VERTER_BUG`** — a genuine Verter defect (a lowering, resolution, mapping, or
  codegen error). Fix it in the lowest reusable owner crate, with a discriminating
  regression test on the generic repro.
- **`REFERENCE_WRONG`** — Verter is correct and the reference (Volar / vue-tsc /
  `@vue/compiler-sfc`) is wrong. Assert Verter's behavior and document the
  intentional divergence; never "fix" Verter to match a wrong baseline.
- **`HARNESS_OR_ORACLE_GAP`** — the defect is in the validation infrastructure, not
  in Verter or the reference. Requires a `subtype` of `HARNESS_BUG` (a harness
  defect — a sampling error, a stale fixture, a provider-config mistake) or
  `ORACLE_GAP` (a wrong/incomplete gold descriptor). Fix the harness, never the
  compiler.
- **`INTENTIONAL_DEVIATION`** — a documented, deliberate behavioral choice (e.g. a
  native-only API surface, or a tighter patch flag) where Verter intentionally
  differs from a reference.
- **`UNDECIDED`** — not yet ruled. Carries `undecidedReason` and `nextAction`, and
  cannot be marked final/approved until resolved (it escalates to the architect with
  the source-of-truth).

## The anti-motivated-misclassification rule

Any ruling that does **not** result in a Verter fix — `REFERENCE_WRONG`,
`INTENTIONAL_DEVIATION`, or `HARNESS_OR_ORACLE_GAP` — must carry all four
anti-misclassification fields, or the schema rejects it:

1. **`independentRepro`** — an independent reproduction that does not rely on
   Verter's own projection as the oracle.
2. **`sourceOfTruth`** — a named spec, Vue runtime behavior, TS behavior, or
   architect ruling. Never "because Verter says so".
3. **`reviewerApproval`** — explicit reviewer approval.
4. **`lockingAssertion`** — a regression assertion that LOCKS the intended behavior
   so it cannot silently drift.

This closes the loophole where a real Verter bug is waved off as "the reference is
wrong".

## The closed loop (per deviation)

```
detect (opaque id)
  → reduce to a generic minimal vendored repro   (hermetic; no project bytes)
    → reproduce generically (RED — fails pre-change, passes post-change)
      → classify (the five classes; escalate UNDECIDED to the architect)
        → fix (VERTER_BUG in the lowest reusable owner crate, or HARNESS_OR_ORACLE_GAP
               in the harness) with the regression / locking test in the same change
          → re-run the generic repro (GREEN)
            → re-run the originating project to confirm the deviation is gone
```

## Schema field summary

| field | required | notes |
| --- | --- | --- |
| `id` | always | stable opaque ledger id |
| `workstream` | always | `ide` \| `tsc` \| `build` |
| `class` | always | one of the five classes above |
| `genericReproFixture` | always | the hermetic repro — required before any landing |
| `status` | always | `draft` \| `final` (UNDECIDED stays `draft`) |
| `disposition` | always | `open` \| `fixed` \| `documented` \| `deferred` \| `blocked` |
| `subtype` | when `HARNESS_OR_ORACLE_GAP` | `HARNESS_BUG` \| `ORACLE_GAP` |
| `independentRepro`, `sourceOfTruth`, `reviewerApproval`, `lockingAssertion` | when non-`VERTER_BUG` affirmative | the four anti-misclassification fields |
| `undecidedReason`, `nextAction` | when `UNDECIDED` | and `status` must stay `draft` |
| `reference`, `oracleRuling`, `ownerCrate`, `regressionTest` | optional | evidence + provenance |

The ledger starts empty (`entries: []`); rows are added as deviations are found and
classified during the campaign.
