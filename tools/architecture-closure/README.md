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

- **`owner-lock`** — every direct predecessor of the lock target (the structural
  owners) and each of their transitive predecessors is implemented, the report
  records each structural owner's real dependency path, and the engine and
  observe inventories are complete. Later owners that are not yet implemented
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

**Plan export:** the fields below. One that is absent is `plan-field-missing`;
the validator never infers a population from prose.

| Field                                                                         | Meaning                                                                                                               |
| ----------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `tasks[].slug`, `train`, `predecessors`, `labels`, `semanticRole`, `optional` | The dependency graph and its classifications                                                                          |
| `tasks[].selected`                                                            | Boolean on every optional task: whether it was selected                                                               |
| `ledger[]`                                                                    | `{ slug, status }`, the trusted implementation status; an absent row is never implemented                             |
| `closure.namedOwners[]`                                                       | Owners required by contracts and amendments; each must have a path to the target (closure)                            |
| `closure.requiredOutcomes[]`                                                  | Outcome ids, each owned by exactly one report row (closure)                                                           |
| `closure.capabilitySurfaces[]`                                                | `{ surface, class, cases: [{ case, kind }] }`, the declared surfaces and their partial or unsupported cases (closure) |
| `closure.metricCells[]`                                                       | `{ row, evidenceRun, modes }`, the predeclared controlling cells of the benchmark runs (closure)                      |
| `closure.wsp6Counts[]`                                                        | Names of the required production counts (both modes)                                                                  |

[`closure-policy.json`](closure-policy.json) holds the named constants the rules
apply: targets, the conditional owner, named evidence rows, benchmark runs and
worker tag, required evidence lanes, required review trains, and inventory
vocabularies.

## Decision rules worth knowing

- A metric cell recorded `unavailable` with a reason is accepted and returned in
  `advisories`: unavailable performance evidence never fails acceptance. An
  omitted cell, a cell with no result or no reason, a failing cell and a cell
  measured on a worker without the benchmark tag all reject.
- A surface classed `preview` or `experimental-unranked` must report a partial
  result; a `stable` surface must report a complete one. Omitting a row is never
  equivalent to declaring it unsupported.
- Candidate identities are opaque tokens compared for equality. No locator,
  commit or Git-history validation exists here.

## Reason codes

| Area         | Codes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Export       | `plan-field-missing`, `plan-duplicate-task`, `plan-dangling-predecessor`, `plan-duplicate-ledger-row`, `plan-target-missing`                                                                                                                                                                                                                                                                                                                                                                                              |
| Report shape | `schema-invalid`, `report-mode-mismatch`, `report-target-mismatch`                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| Owners       | `owner-row-missing`, `owner-row-duplicate`, `required-owner-ledger-absent`, `ancestor-pending`, `owner-pathless`, `owner-unknown`, `conditional-owner-pathless`, `optional-row-unclassified`, `owner-classification-mismatch`, `owner-status-mismatch`, `owner-path-missing`, `owner-path-invalid`, `owner-outcome-missing`, `owner-final-owner-missing`, `owner-consumer-missing`, `owner-route-disposition-missing`, `surviving-unowned-route`, `owner-evidence-missing`, `outcome-unmapped`, `outcome-multiple-owners` |
| Owner lock   | `structural-owner-pending`, `structural-owner-row-missing`, `nonstructural-owner-unlisted`                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Reviews      | `train-review-missing`, `train-review-stale`, `train-checkpoint-uncovered`, `train-review-open-deferral`, `train-review-unresolved-finding`, `independent-review-missing`, `terminal-review-missing`                                                                                                                                                                                                                                                                                                                      |
| Capabilities | `capability-row-missing`, `capability-cell-missing`, `capability-cell-mismatch`, `capability-class-mismatch`, `capability-field-missing`, `capability-limitation-missing`, `capability-noncomplete-stands-in`, `capability-preview-claims-complete`, `capability-surface-unknown`                                                                                                                                                                                                                                         |
| Metrics      | `metric-cell-missing`, `metric-result-missing`, `metric-outcome-missing`, `metric-worker-not-allowed`, `metric-failing`, `metric-run-mismatch`, `metric-run-evidence-missing`, `metric-undeclared`                                                                                                                                                                                                                                                                                                                        |
| Evidence     | `evidence-lane-missing`, `ecrv8-evidence-missing`, `ecrx2-evidence-missing`, `rel3-evidence-missing`, `evidence-row-duplicate`, `evidence-ref-dangling`, `evidence-owner-missing`, `evidence-owner-unknown`, `evidence-command-missing`, `evidence-stale-candidate`, `evidence-executor-missing`, `evidence-prerequisites-missing`, `evidence-prerequisite-mismatch`, `evidence-summary-incomplete`, `evidence-failed`, `evidence-zero-work`, `evidence-unexpected-skip`                                                  |
| Inventories  | `engine-inventory-incomplete`, `moved-guard-not-executing`, `observe-inventory-incomplete`, `optional-default-hot-bookkeeping`, `wsp6-count-absent`, `wsp6-count-requires-capture`, `observe-off-versus-stripped-missing`                                                                                                                                                                                                                                                                                                 |
| Typed reuse  | `reuse-inventory-missing`, `refusal-as-complete`, `context-poisoned-reuse`, `arbitrary-incomplete-reuse`, `negative-cache-test-missing`, `per-fact-transport-inexact`, `unowned-graph-escape`                                                                                                                                                                                                                                                                                                                             |
| Exceptions   | `exception-undispositioned`, `exception-unexplained`, `open-deferral`                                                                                                                                                                                                                                                                                                                                                                                                                                                     |

## Tests

`node --test tools/architecture-closure/*.test.mjs`, also run by
`pnpm test:scripts` through
[`scripts/test-scripts.d/architecture-closure.json`](../../scripts/test-scripts.d/architecture-closure.json).
Fixtures under `fixtures/` are synthetic. Each negative control proves its
mutation applied, then requires exactly its reason code.
