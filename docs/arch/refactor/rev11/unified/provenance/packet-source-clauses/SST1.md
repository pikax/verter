# Exact operative source-clause attachment — SST1

Schema: 1. Node: `SST1`. Clause count: 21. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1547-305FDF89D948

- Kind: `context`; source: `compiler-proposal.md:1547-1547`; target: `node:SST1`; text SHA-256: `305fdf89d948df7a64e08892356bc204e142e7a73270ea6c8d0471b8682e7e0f`.

~~~~markdown
## `SST1.md` — Svelte selector query plan and candidate-index architecture
~~~~

### SRC-COMP-L1549-EC99F3CC08AC

- Kind: `context`; source: `compiler-proposal.md:1549-1549`; target: `node:SST1`; text SHA-256: `ec99f3cc08ac03e4fdc0603edc878cba54ea058814c3dacdd48a56c2c99877a9`.

~~~~markdown
**Intent:** compile selectors and template topology into a sound, data-oriented query workload without changing semantic answers.
~~~~

### SRC-COMP-L1551-9BCE1C49B3FD

- Kind: `context`; source: `compiler-proposal.md:1551-1551`; target: `node:SST1`; text SHA-256: `9bce1c49b3fd5f6618ec08d14496e2b36ff682fa5605d78899afc9187261ccf8`.

~~~~markdown
**Problem:** scanning every element for every selector and cloning path structures can dominate large components, while always building an index can regress small components.
~~~~

### SRC-COMP-L1553-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1553-1553`; target: `node:SST1`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1555-005734067394

- Kind: `requirement`; source: `compiler-proposal.md:1555-1555`; target: `node:SST1`; text SHA-256: `005734067394a8227f50c90c2759695d026dd58941891364374f8c7810f6d382`.

~~~~markdown
- exact matcher semantics remain framework-owned and authoritative;
~~~~

### SRC-COMP-L1556-4B175DF768E2

- Kind: `requirement`; source: `compiler-proposal.md:1556-1556`; target: `node:SST1`; text SHA-256: `4b175df768e261cdcdce788da7f537d2bb19c51a719d78061b2647500b7b283e`.

~~~~markdown
- compile J selector structure into compact steps/subprogram ranges only when useful;
~~~~

### SRC-COMP-L1557-D57175E90D80

- Kind: `forbidden`; source: `compiler-proposal.md:1557-1557`; target: `node:SST1`; text SHA-256: `d57175e90d80547abf5649b7a643947bb64a1d187dca4222e3b49a61f87b3196`.

~~~~markdown
- use `SCP2` canonical topology, never runtime IR;
~~~~

### SRC-COMP-L1558-010BA78440AD

- Kind: `context`; source: `compiler-proposal.md:1558-1558`; target: `node:SST1`; text SHA-256: `010ba78440ad71ed30dd2db1ff3db0dde09d768b66377cefca65fb797b1cb76e`.

~~~~markdown
- define deterministic cost inputs:
~~~~

### SRC-COMP-L1560-7CB324D1AF52

- Kind: `context`; source: `compiler-proposal.md:1560-1566`; target: `node:SST1`; text SHA-256: `7cb324d1af523e5645c4c3490462f6f1783a206bdcd0c7af0dde8e3828b2db03`.

~~~~markdown
```text
  template node count
  selector count and step count
  positive-anchor availability
  dynamic/wildcard ratio
  posting cardinalities
  ```
~~~~

### SRC-COMP-L1568-10B6D9088503

- Kind: `context`; source: `compiler-proposal.md:1568-1568`; target: `node:SST1`; text SHA-256: `10b6d90885033e5c25c5279f9cb6f479ed208cb3f0dc034a1b1c2ef05eae0605`.

~~~~markdown
- support `DirectMatcher` and `IndexedMatcher`;
~~~~

### SRC-COMP-L1569-D873CF418C42

- Kind: `context`; source: `compiler-proposal.md:1569-1569`; target: `node:SST1`; text SHA-256: `d873cf418c42bf0346959b47eddc295b446c50401d45f4b308a8a7dbc3f27d93`.

~~~~markdown
- indexed postings for sound positive tag/id/class/attribute keys;
~~~~

### SRC-COMP-L1570-3360FDFCF7FC

- Kind: `context`; source: `compiler-proposal.md:1570-1570`; target: `node:SST1`; text SHA-256: `3360fdfcf7fcdbda270b628b4e1d40543349d71d72557b08ac34a86127b18865`.

~~~~markdown
- choose the rarest sound mandatory positive anchor using actual posting cardinality;
~~~~

### SRC-COMP-L1571-1355E7404600

- Kind: `forbidden`; source: `compiler-proposal.md:1571-1571`; target: `node:SST1`; text SHA-256: `1355e7404600ddf99c88be76ea1867df9d5a874e7f41f5792ba1cd0696ae660e`.

~~~~markdown
- negated predicates and unsafe pseudo branches never seed candidates;
~~~~

### SRC-COMP-L1572-BF207F03E1E2

- Kind: `context`; source: `compiler-proposal.md:1572-1572`; target: `node:SST1`; text SHA-256: `bf207f03e1e2a727de48e44eb577144bb80c1c90286e12e8618e0e05d16b2638`.

~~~~markdown
- dynamic/spread/maybe buckets are explicitly unioned into candidate sets;
~~~~

### SRC-COMP-L1573-461413062D89

- Kind: `requirement`; source: `compiler-proposal.md:1573-1573`; target: `node:SST1`; text SHA-256: `461413062d89edce3a9e6e08576dc5387e2327b38e25b87810eca5c0747bb953`.

~~~~markdown
- query planning is demand-only and may be skipped for tiny workloads.
~~~~

### SRC-COMP-L1575-21AF708254D0

- Kind: `context`; source: `compiler-proposal.md:1575-1575`; target: `node:SST1`; text SHA-256: `21af708254d0a1f259ada50909b9ec89cc3e243d8921f71164c5c6560cfc5ac8`.

~~~~markdown
**Suggested predecessors:** `SCP2`, `SST0`.
~~~~

### SRC-COMP-L1577-68876CC5ED5C

- Kind: `context`; source: `compiler-proposal.md:1577-1577`; target: `node:SST1`; text SHA-256: `68876cc5ed5cd8f95d632d15766551b26c53633b53beea66f13bd1e4eada999b`.

~~~~markdown
**Suggested subblocks:** selector-step representation, direct matcher baseline, feature postings, candidate rules/dynamic buckets, deterministic cost model, differential/performance tests.
~~~~

### SRC-COMP-L1579-D5BAA15BC1D5

- Kind: `acceptance`; source: `compiler-proposal.md:1579-1579`; target: `node:SST1`; text SHA-256: `d5baa15bc1d578c2df3dbb2243202eb86a7ce726c808f94d9a504085d701951a`.

~~~~markdown
**Acceptance:** candidate selection has no false negatives; direct and indexed paths feed the same exact verifier; small workloads avoid index construction; all candidate/index work is ledger-visible.
~~~~

### SRC-COMP-L1581-303CBE451AE8

- Kind: `forbidden`; source: `compiler-proposal.md:1581-1581`; target: `node:SST1`; text SHA-256: `303cbe451ae87e916e36f0ea002775e52cd8ba77fde744dae57d8cbabb039304`.

~~~~markdown
**Forbidden:** probabilistic rejection, negated anchors, always-on indexing, universal selector semantics, or pruning from candidate selection alone.
~~~~

### SRC-COMP-L1583-262C21E0AEDF

- Kind: `deletion`; source: `compiler-proposal.md:1583-1583`; target: `node:SST1`; text SHA-256: `262c21e0aedf9f9f5a6dd36ff56f7c8d60525563ea3098bd54f3a00bc190a071`.

~~~~markdown
**Deletion/abort:** preserve the exact matcher while replacing only physical execution; abort indexing if equivalent-work benefit is not demonstrated.
~~~~

### SRC-COMP-L1585-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1585-1585`; target: `node:SST1`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
