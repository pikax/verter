# Exact operative source-clause attachment — CLIL0

Schema: 1. Node: `CLIL0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1547-6E35C43139AA

- Kind: `context`; source: `successor-expansion.md:1547-1547`; target: `node:CLIL0`; text SHA-256: `6e35c43139aafdaf28daa148ad512735c4b603561adcc6e7375dd3d2245d124a`.

~~~~markdown
### `CLIL0.md` — Lint CLI adapter
~~~~

### SRC-EXP-L1549-01620671A9DC

- Kind: `forbidden`; source: `successor-expansion.md:1549-1554`; target: `node:CLIL0`; text SHA-256: `01620671a9dc912bd7b5e5abb8ed2dc89dfcabb5ece3bf851ff152cccc1a5725`.

~~~~markdown
**Intent:** add `verter lint` as a thin adapter over the independently promoted lint service and available rule packs.
**Predecessors:** `CLI1`, `LNT3`.
**Subblocks:** (1) file/project/stdin selection; (2) report/fix-policy flags; (3) native/external provenance and trust inputs; (4) human/JSON/SARIF reporters; (5) safe-fix preview/atomic write; (6) watch/performance/cancellation/failure tests.
**Acceptance:** process failure/timeout is not clean lint; `lint` writes only under an explicit safe-fix flag; available pack/capability truth is generated; CLI owns no rules.
**Forbidden:** arbitrary plugin execution in Rust, implicit fixes, duplicated diagnostics, formatter side effects, or CLI-owned suppression semantics.
**Deletion/abort:** delete standalone lint shells only after parity and zero consumers; disable external fallback unless its trusted-host gates pass.
~~~~
