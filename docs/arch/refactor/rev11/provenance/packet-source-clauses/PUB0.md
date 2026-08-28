# Exact operative source-clause attachment — PUB0

Schema: 1. Node: `PUB0`. Clause count: 7. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXISTING-NODE-AMENDMENT-PUB0

- Kind: `requirement`; source: `existing-node-amendments.md:95-128`; target: `node:PUB0`; text SHA-256: `487fd1382b9bcd0e90e07ea0ac7338b93d782b7d972da8ec27aa6856213dbcf4`.

~~~~markdown
## PUB0 — Versioned public request/result and capability truth

Add public-neutral forms for:

```text
DiagnosticRequest / DiagnosticBatch
SemanticOperationRequest / SemanticOperationOutcome
AuthoredTarget / SemanticOccurrence / PresentationFragment
RenamePlan / AuthoredEditIntent / AuthoredEditTransaction
EngineProvisioningPolicySummary / EngineResolutionReport / EngineActivationStatus
```

Mandatory outcome vocabulary:

```text
Complete
Ambiguous
NeedInputs
Unsupported
NotApplicable
Cancelled
Stale
Superseded
BudgetExceeded
Partial
OperationalFailure
```

Rules:

- no LSP positions, generated TSX paths, provider JSON handles, CLI formatting, or filesystem write fields in core results;
- WASM/MCP/FFI consumers report missing inputs truthfully;
- capabilities derive from accepted conformance/active receipts, never booleans maintained by clients;
- schema epochs and reserved-field policy apply to every new result domain.
~~~~

### SRC-EXP-L977-E368C210381D

- Kind: `context`; source: `successor-expansion.md:977-977`; target: `node:PUB0`; text SHA-256: `e368c210381d53bbf1e8dd7831fbb87345b0a72d733682b355a11cf0e3cdeacf`.

~~~~markdown
### `PUB0.md` — Versioned public request/result and capability truth
~~~~

### SRC-EXP-L979-AC3D83A7573A

- Kind: `forbidden`; source: `successor-expansion.md:979-984`; target: `node:PUB0`; text SHA-256: `ac3d83a7573a43dfb9d627131ddf8df319ad99bcf1346efbf963e9eb9ee500b7`.

~~~~markdown
**Intent:** make Rust, NAPI, WASM, LSP, MCP, and CLI consumers observe one semantic vocabulary and honest availability.
**Predecessors:** `ENC1`, `TIF1`, `LRA0`, `FMK0`, `COX0`, `PER0`.
**Subblocks:** (1) request/result envelope and schema epochs; (2) typed success/partial/ambiguous/NeedInputs/unsupported/not-applicable/cancelled/stale outcomes; (3) generated per-surface capability/maturity matrix; (4) prepared-input and filesystem boundaries; (5) cancellation/budget/encoding propagation; (6) compatibility and reserved-field policy.
**Acceptance:** differential fixtures return equivalent semantic facts across available surfaces; WASM reports missing inputs rather than empty success; LSP registers only full-participation applicable capabilities.
**Forbidden:** surface-specific semantic DTOs, boolean capability lies, implicit encoding, provider handles, or CLI presentation fields in core results.
**Deletion/abort:** delete duplicate public envelopes only after generated consumer parity; rescope when a surface cannot supply required inputs and mark the capability accordingly.
~~~~

### SRC-LEGACY-EXISTING-TYPEINFO-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:207-209`; target: `node:E1`; text SHA-256: `80907890ae0cdb9344c99bc73988697f7cbf0e12a81bb168db6b3940ec07dc06`.

~~~~markdown
### EXISTING-TYPEINFO-001

TypeInfo semantic value/query/public graph contracts remain owned by E/TCM/UAO/PUB authority; the checker and language service do not create a second TypeInfo engine. Related source: `docs/arch/native-typeinfo-parity.md`, blob `2041fbfbd635086ec718a84e314a53f89d1566ac` and child plans.
~~~~

### SRC-LEGACY-TRANSFER-2041FBFBD635

- Kind: `requirement`; source: `legacy-architecture-transfers.md:355-360`; target: `node:E1`; text SHA-256: `c0d46f5d4f4b7948eb0d04483d333de9bb4741019eab423d31ba0fad97877835`.

~~~~markdown
### LEGACY-TRANSFER-2041FBFBD635

- Original path: `docs/arch/native-typeinfo-parity.md`; Git blob: `2041fbfbd635086ec718a84e314a53f89d1566ac`; exact source SHA-256: `5039c1d88e71b4f2a9f5d4d52aac64ad4e535fa9e6c0fad3569427d8f5a736dc`.
- Exact retained source: `sources/legacy-architecture-transfers/native-typeinfo-parity.md`.
- Applicable authority: `E1`, `E2`, `E3`, `E4`, `TCM3`, `TCM4`, `TIF0`, `TIF1`, `UAO0`, `PUB0`, `NCK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-39DC88E896A4

- Kind: `requirement`; source: `legacy-architecture-transfers.md:334-339`; target: `node:TCM3`; text SHA-256: `1ce1f4e3213eaf5e0a79e27de9a289863d057100ca4a431542527de72b9dc4f0`.

~~~~markdown
### LEGACY-TRANSFER-39DC88E896A4

- Original path: `docs/arch/native-typeinfo-parity-adapters-final-lift.md`; Git blob: `39dc88e896a462763b1957a68576046517f4f642`; exact source SHA-256: `d4d1092e46eb1f05224f00a96758458ec70860804285a5556cf4835317129ff9`.
- Exact retained source: `sources/legacy-architecture-transfers/native-typeinfo-parity-adapters-final-lift.md`.
- Applicable authority: `TCM3`, `TCM4`, `TIF0`, `TIF1`, `PUB0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-499039C321E0

- Kind: `requirement`; source: `legacy-architecture-transfers.md:341-346`; target: `node:E4`; text SHA-256: `22cdc3736b0a9f4a75027d89ffdfd9d2e1a58cb342dbc7f31d75de15ec612935`.

~~~~markdown
### LEGACY-TRANSFER-499039C321E0

- Original path: `docs/arch/native-typeinfo-parity-cache-export-session.md`; Git blob: `499039c321e0eb76ba2a3bd9b526627c97290ee4`; exact source SHA-256: `6238efd4ae029fa0ef4b10b10e25321bed6956340b54587be5bf3214017ec25b`.
- Exact retained source: `sources/legacy-architecture-transfers/native-typeinfo-parity-cache-export-session.md`.
- Applicable authority: `E4`, `H1`, `TCM3`, `TIF0`, `PUB0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
