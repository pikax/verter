# Predeclared implementation ledger (schema 2)

- Status: accepted
- Date: 2026-09-01
- Supersedes: the schema-1 append-only `[[implemented]]` row format (format only; the trust model of `2026-08-28-trusted-implementation-ledger.md` is unchanged)
- Scope: the serialization and mutation model of `authority/state/implemented.toml`

## Context

Schema 1 recorded implementation by appending one `[[implemented]]` array-table row per node. Under autonomous multi-worker execution, several node candidates land close together, and each landing's ledger change must rebase mechanically onto whatever landed just before it. Appending multi-line blocks makes concurrent landings collide textually (both append at the file tail, or in overlapping regions), forcing either serial hand-merging or an LLM into a deterministic bookkeeping path. Appending also leaves "which nodes exist" implicit: the file cannot be validated for completeness, and a typo'd node id is only caught against the DAG at validation time, not by the file's own shape.

## Decision

`authority/state/implemented.toml` declares `schema = 2` and predeclares every DAG node exactly once under one `[implementation]` table:

```toml
[implementation]
"A0" = { status = "implemented", commit_message = "...", commit_date = "2026-08-10T01:40:28+01:00" }
"D4" = { status = "pending" }
```

- Every DAG node appears exactly once; unknown nodes are invalid; missing nodes are invalid.
- `status = "pending"` carries no evidence fields.
- `status = "implemented"` requires `commit_message` and `commit_date` and optionally `pull_request` — the same never-validated locator hints as before.
- No other status values exist. Transient orchestration states (claimed, implementing, reviewing, fixing, CI-red, ...) are runtime state owned by the orchestration controller and are structurally invalid here.
- Serialization is canonical: node ids sorted, one line per node, fixed field order. Implementing a node is a one-line diff of that node's line and nothing else.
- Deliberately flipping a line back to `status = "pending"` is the operation that marks a node unimplemented (the schema-1 "remove the row" correction, made explicit).
- `[[github_issue]]` and `[[github_train_issue]]` rows are unchanged in meaning and shape.

Because independent nodes own independent lines, concurrent ledger changes merge mechanically: `roadmap/0.1.0-tama/tools/merge-ledger.mjs` performs a deterministic three-way merge (main transitioned D5, candidate transitioned D4 → latest main plus the candidate's D4 transition) and fails closed when both sides changed the same node incompatibly. No LLM ever merges ledger state.

New DAG nodes are predeclared as pending in the same patch that adds them to a DAG module (`predeclareMissing` in `tools/ledger.mjs`); DAG validation fails until they are.

## Consequences

- The implementation patch still carries its own completion fact before squash and review — now as a one-line transition instead of an appended block.
- The trust model is untouched: row status is trusted, evidence is a loose locator, nothing resolves or validates against Git or GitHub, and readiness remains "every transitive DAG ancestor implemented".
- `tools/lib.mjs` derives the same in-memory `implemented` row list from the table, so `deriveState`, `programctl`, and githubctl consumers observe identical semantics.
- The file's own shape now validates completeness (361 predeclared nodes at migration time), and `validate-program-dag --strict` fails on unknown or missing nodes.

## Non-goals

This does not add lifecycle states, receipts, leases, or landing records to the ledger, and it does not make the merge helper an authority: the merged file is authoritative because it is reviewed and landed, not because the tool produced it.
