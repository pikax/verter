# Exact operative source-clause attachment — ENCL0

Schema: 1. Node: `ENCL0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L833-57F6CB36508B

- Kind: `context`; source: `successor-expansion.md:833-833`; target: `node:ENCL0`; text SHA-256: `57f6cb36508b5f64d3361fa2cfc7177317d79e4c6582b3c8b488ab01374e4480`.

~~~~markdown
### `ENCL0.md` — LSP and editor coordinate-boundary cutover
~~~~

### SRC-EXP-L835-10E360311C14

- Kind: `forbidden`; source: `successor-expansion.md:835-840`; target: `node:ENCL0`; text SHA-256: `10e360311c149d6dec33077662260accb8dd6b2310af33f536e47b1832913505`.

~~~~markdown
**Intent:** make the editor boundary negotiate and convert coordinates exactly once while Rust core remains UTF-8-byte-only.
**Predecessors:** `ENC0`.
**Subblocks:** (1) LSP position-encoding handshake and capability truth; (2) ingress validation and UTF-16/UTF-32→UTF-8 conversion; (3) egress range/edit/location conversion; (4) line-index lifetime and incremental update rules; (5) astral/combining/ZWJ/CRLF/overflow property corpus; (6) UTF-8 fast-path allocation and latency benchmarks.
**Acceptance:** every admitted LSP encoding round-trips exactly; invalid boundaries are typed failures; UTF-8 requests allocate no conversion buffer; editor encoding never enters semantic/cache identity.
**Forbidden:** conversion inside parsers/resolvers/indexes, implicit UTF-16 defaults, saturation, or cached requester-encoding mirrors.
**Deletion/abort:** delete fixed-UTF-16 editor contracts only after all callers migrate; abort on an untagged editor range.
~~~~
