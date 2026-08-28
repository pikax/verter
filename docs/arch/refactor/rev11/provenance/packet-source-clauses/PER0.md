# Exact operative source-clause attachment — PER0

Schema: 1. Node: `PER0`. Clause count: 4. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1935-C36FEC9B0D78

- Kind: `context`; source: `compiler-proposal.md:1935-1935`; target: `node:PER0`; text SHA-256: `c36fec9b0d78f156feff1bd8f1bfc5c901d53480bfc3900d5bd77877eed425e1`.

~~~~markdown
## 11.5 `PER0`
~~~~

### SRC-COMP-L1937-2322F3485F7A

- Kind: `context`; source: `compiler-proposal.md:1937-1937`; target: `node:PER0`; text SHA-256: `2322f3485f7a746c6c358c61a0dcfd0d5f80c2151d963baa982a275ff51ffdb5`.

~~~~markdown
Keep `PER0` as the system-wide identity/cancellation/budget constitution. Add `CPER0`–`CPER3` as compiler-specific consumers; do not move compiler counters into `PER0` as a giant global schema.
~~~~

### SRC-EXP-L968-86074DF950E6

- Kind: `context`; source: `successor-expansion.md:968-968`; target: `node:PER0`; text SHA-256: `86074df950e6484ab5ac20c6d43efc51869804ee18de65b427e2a0fe4ba0fcb8`.

~~~~markdown
### `PER0.md` — Cache/backend identity, cancellation, budgets, and zero work
~~~~

### SRC-EXP-L970-DE4E3F21CBDF

- Kind: `forbidden`; source: `successor-expansion.md:970-975`; target: `node:PER0`; text SHA-256: `de4e3f21cbdff9edc69ee7db6fad74e47efec0c763d8f2dd6acc60ccfa0063ad`.

~~~~markdown
**Intent:** make performance and reuse correctness explicit across every future capability.
**Predecessors:** `DEM0`, `ENC1`, `TIF0`, `IDX0`, `PAR0`.
**Subblocks:** (1) consume Rev11/TCM1 plus `VID0/PAR0` prepared-artifact identities and the accepted generic observation identity with `TIF0` operation descriptors; (2) keep snapshot-independent `QueryIdentity` candidate lookup, G2-owned `(QueryIdentity, InputBasisId)` flight identity, and value-side candidate/result basis provenance as three distinct contracts; (3) validate/benchmark backend/artifact/process/project/snapshot/map/parser invalidation by revalidating candidate provenance without redefining any identity; (4) cancellation/stale-generation publication law; (5) per-operation budgets and audit events; (6) equivalent-work benchmark and RSS-soak harness.
**Acceptance:** candidate lookup remains snapshot-independent; same reported TS version with a different artifact cannot pass value-side reuse validation; process restart, source/map/profile epoch change, cancellation, and overflow reject stale admission; native no-projection artifacts survive backend changes; disabled profiles show zero attributable work.
**Forbidden:** backend-free type caches, sleep/idle completion inference, per-vertical singleflight, unbounded candidate collection, or performance claims without result equivalence.
**Deletion/abort:** delete only successor-local duplicate cache/coalescer paths proven displaced; never delete or shadow TCM1/G2 authority; abort when candidate/result provenance cannot carry and revalidate the complete observation basis—never enlarge `QueryIdentity` to make its lookup key reconstruct that basis.
~~~~
