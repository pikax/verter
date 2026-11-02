# Architecture-closure validator

Decides, from a supplied plan export and a candidate report, whether an
architecture closure holds. The answer is a pure function of those two files:
the validator reads no repository plan state, Git history or live service, and
never changes completion or readiness.

```bash
node tools/validate-architecture-closure.mjs --mode owner-lock|closure \
  --plan <export.json> --report <report.json>
```

Prints one JSON decision on stdout:
`{ ok, mode, target, errors: [{ code, subject, message }], pending, advisories }`.
Exit status: 0 accepted, 1 rejected, 2 usage error or unreadable input (no
decision was made).

- **`owner-lock`** — every structural owner the export names
  (`closure.structuralOwners`) has a real dependency path to the lock target and
  is implemented, as is every transitive predecessor of the lock; the report
  records each structural owner's real dependency path, and the engine and
  observe inventories match their declared populations. A structural owner whose
  edge to the lock was dropped is `owner-pathless`, never a later owner. Later owners that are not yet implemented
  are returned in `pending` and do not reject; the report must still list them.
- **`closure`** — the full population: every transitive predecessor of the
  closure target, its train reviews, the declared capability, metric and outcome
  populations, the required evidence lanes and named evidence rows, the
  inventories, the typed-reuse inventory and the exceptions.

`--plan` is trusted for the population and for status. `--report` is the claim
under test; a report row never adds to the required population and never vouches
for a status.

## Inputs

**Report:** [`schemas/architecture-closure.schema.json`](../../schemas/architecture-closure.schema.json).
The schema rejects structurally malformed reports (`schema-invalid`). A row's
semantic cells stay optional there so an omission is rejected for its own code.

**Plan export:** the fields below (an assembled evidence package for real runs).
A population that is absent, empty or has a malformed element is
`plan-field-missing` (only `closure.targetOutcomes` may be empty: it restricts
what the target may claim, so emptiness forbids rather than waives), and one
listing the same entry twice is `plan-duplicate-entry`; the validator never
infers a population from prose.
Every report section the export declares a population for is compared against
it: a declared entry with no report row, a row the export does not declare, or
two report rows with one identity each reject, so a report can neither drop the
row that would fail nor let a later duplicate shadow it.

| Field                                                                         | Meaning                                                                                                                                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tasks[].slug`, `train`, `predecessors`, `labels`, `semanticRole`, `optional` | The dependency graph and its classifications                                                                                                                                              |
| `tasks[].selected`                                                            | Boolean on every optional task: whether it was selected                                                                                                                                   |
| `ledger[]`                                                                    | `{ slug, status }`, the trusted implementation status; an absent row is never implemented                                                                                                 |
| `closure.structuralOwners[]`                                                  | The structural owners the lock names; each must have a path to the lock target (owner-lock)                                                                                               |
| `closure.namedOwners[]`                                                       | Owners required by contracts and amendments; each must have a path to the target (closure)                                                                                                |
| `closure.requiredOutcomes[]`                                                  | Outcome ids, each owned by exactly one implemented report row with a path to the target (closure)                                                                                         |
| `closure.targetOutcomes[]`                                                    | The terminal outcome ids the target itself may own; any other outcome on the target's row is ineligible (closure)                                                                         |
| `closure.capabilitySurfaces[]`                                                | `{ surface, class, cases: [{ case, kind }] }`, the declared surfaces and their partial or unsupported cases (closure)                                                                     |
| `closure.metricCells[]`                                                       | `{ row, evidenceRun, modes, evidenceKind }`, the predeclared controlling cells of the benchmark runs (closure)                                                                            |
| `closure.wsp6Counts[]`                                                        | Names of the required production counts (both modes)                                                                                                                                      |
| `closure.engineRelocations[]`                                                 | `{ from, to }`, the engine relocation inventory (both modes)                                                                                                                              |
| `closure.engineTestsBefore[]`, `engineTestsAfter[]`                           | The tests discovered before and after the moves; the report's mapping must be a bijection between them (both modes)                                                                       |
| `closure.engineGuardFamilies[]`                                               | `{ family, kind }`, the moved enforcement families (both modes)                                                                                                                           |
| `closure.futureCharterSurfaces[]`                                             | `{ surface, owner }`, the affected future charter surfaces (both modes)                                                                                                                   |
| `closure.observeOwners[]`                                                     | `{ owner, path, items: [{ item, classification, gate }] }`, the per-owner semantic-observe files and their rows (both modes); default-hot optional state is judged on these declared rows |
| `closure.reuseAdmissions[]`                                                   | `{ id, class }`, the typed-reuse admission rows (closure)                                                                                                                                 |

[`closure-policy.json`](closure-policy.json) holds the named constants the rules
apply: targets, the conditional owner, named evidence rows, benchmark runs and
worker tag, required evidence lanes, required review trains, and inventory
vocabularies.

## Decision rules worth knowing

- A metric cell whose result or any mode outcome is `unavailable` is explicitly
  unmeasured: with a reason it is accepted and returned in `advisories`, because
  unavailable performance evidence never fails acceptance. That allowance covers
  only cells whose `evidenceKind` is `performance`; correctness, lifecycle and
  accounting evidence is never optional, and its run's evidence row is always
  required. An omitted cell, a cell with no result or no reason, a `pass` with
  an unmeasured mode, any failing result or outcome (even beside an unavailable
  one) and a measurement from a worker without the benchmark tag all reject.
- A terminal summary must account for its selected work:
  `executed + expectedSkips` equals `selected`.
- A surface classed `preview` or `experimental-unranked` must report a partial
  result (neither complete nor unsupported); a `stable` surface must report a complete one. Omitting a row is never
  equivalent to declaring it unsupported.
- Candidate identities are opaque tokens compared for equality. No locator,
  commit or Git-history validation exists here.

## Reason codes

| Area         | Codes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Export       | `plan-field-missing`, `plan-duplicate-task`, `plan-duplicate-entry`, `plan-dangling-predecessor`, `plan-duplicate-ledger-row`, `plan-target-missing`                                                                                                                                                                                                                                                                                                                                                                                                  |
| Report shape | `schema-invalid`, `report-mode-mismatch`, `report-target-mismatch`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Owners       | `owner-row-missing`, `owner-row-duplicate`, `required-owner-ledger-absent`, `ancestor-pending`, `owner-pathless`, `owner-unknown`, `conditional-owner-pathless`, `optional-row-unclassified`, `owner-classification-mismatch`, `owner-status-mismatch`, `owner-path-missing`, `owner-path-invalid`, `owner-outcome-missing`, `owner-final-owner-missing`, `owner-consumer-missing`, `owner-route-disposition-missing`, `surviving-unowned-route`, `owner-evidence-missing`, `outcome-unmapped`, `outcome-multiple-owners`, `outcome-owner-ineligible` |
| Owner lock   | `structural-owner-pending`, `structural-owner-row-missing`, `nonstructural-owner-unlisted`                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Reviews      | `train-review-missing`, `train-review-duplicate`, `train-review-stale`, `train-checkpoint-uncovered`, `train-review-open-deferral`, `train-review-unresolved-finding`, `independent-review-missing`, `terminal-review-missing`                                                                                                                                                                                                                                                                                                                        |
| Capabilities | `capability-row-missing`, `capability-row-duplicate`, `capability-cell-duplicate`, `capability-cell-missing`, `capability-cell-mismatch`, `capability-class-mismatch`, `capability-field-missing`, `capability-limitation-missing`, `capability-noncomplete-stands-in`, `capability-preview-claims-complete`, `capability-preview-not-partial`, `capability-surface-unknown`                                                                                                                                                                          |
| Metrics      | `metric-cell-missing`, `metric-row-duplicate`, `metric-result-missing`, `metric-outcome-missing`, `metric-worker-not-allowed`, `metric-failing`, `metric-unmeasured-pass`, `metric-required-evidence-unavailable`, `metric-run-mismatch`, `metric-run-evidence-missing`, `metric-undeclared`                                                                                                                                                                                                                                                          |
| Evidence     | `evidence-lane-missing`, `ecrv8-evidence-missing`, `ecrx2-evidence-missing`, `rel3-evidence-missing`, `evidence-row-duplicate`, `evidence-ref-dangling`, `evidence-owner-missing`, `evidence-owner-unknown`, `evidence-command-missing`, `evidence-stale-candidate`, `evidence-executor-missing`, `evidence-prerequisites-missing`, `evidence-prerequisite-mismatch`, `evidence-summary-incomplete`, `evidence-failed`, `evidence-zero-work`, `evidence-partial-execution`, `evidence-unexpected-skip`                                                |
| Inventories  | `engine-inventory-incomplete`, `moved-guard-not-executing`, `observe-inventory-incomplete`, `optional-default-hot-bookkeeping`, `wsp6-count-absent`, `wsp6-count-duplicate`, `wsp6-count-requires-capture`, `observe-off-versus-stripped-missing`                                                                                                                                                                                                                                                                                                     |
| Typed reuse  | `reuse-inventory-missing`, `reuse-inventory-incomplete`, `refusal-as-complete`, `context-poisoned-reuse`, `arbitrary-incomplete-reuse`, `negative-cache-test-missing`, `per-fact-transport-inexact`, `unowned-graph-escape`                                                                                                                                                                                                                                                                                                                           |
| Exceptions   | `exception-undispositioned`, `exception-unexplained`, `open-deferral`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |

## Tests

`node --test tools/architecture-closure/*.test.mjs`, also run by
`pnpm test:scripts` through
[`scripts/test-scripts.d/architecture-closure.json`](../../scripts/test-scripts.d/architecture-closure.json).
Fixtures under `fixtures/` are synthetic. Each negative control proves its
mutation applied, then requires exactly its reason code.
