# Exact operative source-clause attachment — COX0

Schema: 1. Node: `COX0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

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
