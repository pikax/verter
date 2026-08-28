# Exact operative source-clause attachment — COX0

Schema: 1. Node: `COX0`. Clause count: 7. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXISTING-NODE-AMENDMENT-COX0

- Kind: `requirement`; source: `existing-node-amendments.md:198-207`; target: `node:COX0`; text SHA-256: `27b8bd2d3447de7c453ea77f224221642aa476a8ac3e9ea402fbf185a7804672`.

~~~~markdown
## COX0 — Per-profile editor participation and coexistence

Add generated operation/family capability masks for:

- native semantic diagnostic families;
- navigation, references/hierarchy, rename, completion/resolve, hover/signature/inlay, edits;
- provider-backed residual operation families;
- engine availability/source status.

`WorkspaceOnly` must not publish interactive diagnostics/actions or run editor-only leaf features. `Disabled` performs zero hidden work. `auto` withdraws only overlapping capabilities.
~~~~

### SRC-EXP-L932-680571E04059

- Kind: `context`; source: `successor-expansion.md:932-932`; target: `node:COX0`; text SHA-256: `680571e0405904cfa5a20e494db631937699df56d99c9148704f2373f939e5aa`.

~~~~markdown
### `COX0.md` — Per-profile editor participation and coexistence
~~~~

### SRC-EXP-L934-49D0FFCCD051

- Kind: `forbidden`; source: `successor-expansion.md:934-939`; target: `node:COX0`; text SHA-256: `49d0ffccd051ec993e9ab56c9ddc6589e17bd96b34fcf7747df34a3a052a7b5e`.

~~~~markdown
**Intent:** allow Verter to stand down interactively while retaining explicitly requested workspace semantics.
**Predecessors:** `DEM0`, `IDX0`.
**Subblocks:** (1) public `auto|disabled|workspace|full` presets with clearer UI aliases evaluated during implementation; (2) effective `Disabled|WorkspaceOnly|Full`; (3) abstract per-profile, per-document-selector capability ownership mask; (4) editor-host extension observation via generated descriptor data; (5) dynamic register/unregister, diagnostic clearing, cancellation, and epoch transitions; (6) formatter-only, diagnostics-only, navigation-only, workspace-only, and full zero-work/audit tests.
**Acceptance:** installing/enabling a conflicting test extension under `auto` withdraws only overlapping capabilities while unrelated hover/completion/navigation/formatting remain available; `workspace` contributes only demanded bounded semantics; explicit `full` wins; Rust receives capability masks but no extension IDs.
**Forbidden:** file-extension heuristics in core, “workspace” publishing diagnostics/actions, mode changes serving stale results, or hidden processing in `disabled`.
**Deletion/abort:** remove old global on/off gates and per-framework client branches; abort if an LSP capability cannot be dynamically or truthfully withdrawn.
~~~~

### SRC-LEGACY-TRANSFER-005055FC605F

- Kind: `requirement`; source: `legacy-architecture-transfers.md:362-367`; target: `node:COX0`; text SHA-256: `c6193514cb19feb41e21572b3018ce98d6129198298108b48168dfd19c763aa8`.

~~~~markdown
### LEGACY-TRANSFER-005055FC605F

- Original path: `docs/arch/neovim-support-design.md`; Git blob: `005055fc605f4b97cd4433270cfdbe477758f7ff`; exact source SHA-256: `4c012b24a1b60fdc0679b0e7bf7b6d8dc7d16a64a5a0802bb22c315b40214472`.
- Exact retained source: `sources/legacy-architecture-transfers/neovim-support-design.md`.
- Applicable authority: `COX0`, `LSO9`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-44799D82168A

- Kind: `requirement`; source: `legacy-architecture-transfers.md:271-276`; target: `node:COX0`; text SHA-256: `abeeb11be42b77075e1dd4b001b9d06c5d7bc19c1b12bbc3f4a8a7ac545d69b6`.

~~~~markdown
### LEGACY-TRANSFER-44799D82168A

- Original path: `docs/arch/lapce-extension-design.md`; Git blob: `44799d82168abb95181b29990e1038e19c03a19a`; exact source SHA-256: `c3ed5e0d40c15a8dddec8373b4e0fc71fb32ecb8702fa28308eb47c59ef46238`.
- Exact retained source: `sources/legacy-architecture-transfers/lapce-extension-design.md`.
- Applicable authority: `COX0`, `LSO9`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-52CC5119F1F8

- Kind: `requirement`; source: `legacy-architecture-transfers.md:642-647`; target: `node:COX0`; text SHA-256: `1073dd252ca55e723d6e4917b3a7af2a31eb147d5f24f91ec74bbc5e95a1e336`.

~~~~markdown
### LEGACY-TRANSFER-52CC5119F1F8

- Original path: `docs/arch/zed-extension-design.md`; Git blob: `52cc5119f1f86e590251d3bd8015165911e8f277`; exact source SHA-256: `68b1903a0df76cb4520530b729bae6df3d0814ba76ef9e174d194571dc062065`.
- Exact retained source: `sources/legacy-architecture-transfers/zed-extension-design.md`.
- Applicable authority: `COX0`, `LSO9`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-E27C14F509BB

- Kind: `requirement`; source: `legacy-architecture-transfers.md:257-262`; target: `node:COX0`; text SHA-256: `48430205fc94724cff58518a27a53f9a7d55cc7d625abc30359c27bacb90867e`.

~~~~markdown
### LEGACY-TRANSFER-E27C14F509BB

- Original path: `docs/arch/helix-support-design.md`; Git blob: `e27c14f509bb53af0e290981f740a47a703642a5`; exact source SHA-256: `6ccde3af7ae4dab08276a634a7afe2b30a2584329168b566ff365d0c38cc32d5`.
- Exact retained source: `sources/legacy-architecture-transfers/helix-support-design.md`.
- Applicable authority: `COX0`, `LSO9`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
