# Trusted implementation ledger replaces Git identity validation

- Status: accepted
- Date: 2026-08-28
- Supersedes: ORC0's commit-SHA, tree, receipt, activation, and post-landing lifecycle
- Scope: Rev11 implementation completion and readiness

## Context

ORC0 introduced a machine-validated control plane around exact commit and tree identities. Completion depended on several related records and transitions: candidate identity, receipts, activation state, ancestry checks, and updates made after the implementation commit existed.

That design made ordinary squash workflows fragile. A harmless ordering difference or one incorrect identity could require coordinated repairs across several files before unrelated work could start. The concrete failure that prompted this decision was J1 being squashed before ORC0: the implementation was already landed, but the lifecycle could not recognize the usable state and blocked subsequent work. Repairing that bookkeeping would have taken more work than the information was worth.

Exact Git identity also conflicts with the desired patch workflow. If a ledger entry requires the final squash SHA, the implementation patch cannot contain its own final entry: the SHA does not exist until after the squash. That forces a second bookkeeping commit or a restamping cycle. Rebases and later squashes can change the identity again without changing whether the block was implemented.

The operational question is much simpler: is the ancestor recorded as implemented? Occasionally, a maintainer may also want a convenient way to find the associated commit. Exact proof is not required for either question. Rev11 already trusts implementing and reviewing agents to change only the intended scope, and GH0 provides the normal GitHub review and CI workflow without needing a second custom trust system.

## Decision

Rev11 uses `authority/state/implemented.toml` as a trusted implementation ledger.

The presence of one `[[implemented]]` row is the complete and authoritative statement that its `node_id` is implemented. Readiness checks only row presence for every transitive DAG ancestor. Tooling does not resolve, authenticate, or validate the row against Git or GitHub.

Each row carries human locator hints:

- `commit_message`: normally the planned squash subject, or another useful non-exact search phrase;
- `commit_date`: an approximate timezone-bearing date used to choose the nearest plausible commit when several messages are similar; and
- `pull_request`: an optional PR number, especially useful after GH0.

These fields are documentation, not identity. They do not need an exact match. The date is not a uniqueness key or proof. The PR number is not queried as a gate. If the row exists, it is trusted as correct.

The implementer adds the row to the same patch before the squash and before fresh review. The planned squash message and approximate date are therefore available without knowing a final SHA. The patch is reviewed and squashed once, and no follow-up ledger update is required.

## Consequences

- J1 and ORC0 are recorded by commit message and approximate timezone-bearing date; their implementation is not reopened or repaired.
- A completed ancestor cannot block new work merely because its commit was squashed, rebased, reordered, or assigned a different Git identity.
- Activation gates, conditional-predecessor state, and declared in-progress state do not participate in readiness. If work must block another node, it must be an ordinary DAG predecessor.
- The SHA/tree/ancestry validators, receipts, activation journal, leases, authority locks, finalization steps, and post-landing ledger updates are removed from the live lifecycle.
- Locator mistakes are corrected as ordinary documentation patches. They do not invalidate implementation or descendants.
- Removing an implementation row is the deliberate operation that marks a node unimplemented and can change descendant readiness.
- Reviews still enforce the charter and test the patch. This decision removes identity bookkeeping, not engineering review or verification.

## Non-goals

This decision does not make commit message, date, or PR number into a weaker validator. It deliberately removes validation. It also does not change content-provenance hashes used to identify imported source text; those hashes are archival metadata and never participate in implementation completion or readiness.

GitHub issue mappings are deliberately separate `[[github_issue]]` rows in the same ledger file. They map `node_id` to `gh_issue` for lookup and require a local `sync_to_github` mutation policy. The flag never affects readiness: it only permits one-way refresh (`true`) or protects a pre-existing issue (`false`). Mapping rows never count as implementation rows.
