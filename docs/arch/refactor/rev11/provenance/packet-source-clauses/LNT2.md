# Exact operative source-clause attachment — LNT2

Schema: 1. Node: `LNT2`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1419-1485F4875783

- Kind: `context`; source: `successor-expansion.md:1419-1419`; target: `node:LNT2`; text SHA-256: `1485f4875783df795bcbc95eb05c8b743ba8938bd438358394cef14fb4ac3481`.

~~~~markdown
### `LNT2.md` — Demand-driven lint service and ecosystem fallback
~~~~

### SRC-EXP-L1421-052FC46AF4E9

- Kind: `forbidden`; source: `successor-expansion.md:1421-1426`; target: `node:LNT2`; text SHA-256: `052fc46af4e9d443cfb5c8c6aeae2339ef5abcf59187354c7d8be77f4ae3854c`.

~~~~markdown
**Intent:** compose native lint and optional trusted external execution without duplication or authority leakage.
**Predecessors:** `LNTCFG0`.
**Subblocks:** (1) fact-demand planner and per-profile scheduling; (2) config/suppression read sets; (3) native diagnostic/result cache and cancellation; (4) rule-pack registration/selection; (5) trusted batched external process protocol; (6) Native/External provenance, dedupe, failure, timeout, and WASM capability truth.
**Acceptance:** unsupported external-only rule can be run only by explicit trusted-host policy; native and external ownership never both execute; process failure is not clean lint; WASM reports the external capability unavailable.
**Forbidden:** Node/plugin execution in Rust, silent fallback, duplicate fixes, unbounded subprocesses, or external diagnostics cached as native.
**Deletion/abort:** disable external fallback by default until sandbox/trust/budget gates pass; abort on non-deterministic ownership selection.
~~~~
