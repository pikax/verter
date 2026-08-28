# Exact operative source-clause attachment — LRA0

Schema: 1. Node: `LRA0`. Clause count: 3. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXISTING-NODE-AMENDMENT-LRA0

- Kind: `requirement`; source: `existing-node-amendments.md:75-93`; target: `node:LRA0`; text SHA-256: `9c64d5273ff4a028cb48be82ee6f391dae69b0286869be2dbcc026cbaa326379`.

~~~~markdown
## LRA0 — Profile-scoped diagnostics, lint, fixes, and actions

Add:

- exact diagnostic class/origin/family/slice/rule/subject identity;
- authority state is separate from rule enablement/severity/suppression;
- parser, semantic checker, framework semantic, lint, provider, and project/configuration diagnostics remain distinct owners;
- diagnostic fixes are typed authored edit intents, never raw `TextEdit`/`WorkspaceEdit` payloads;
- safe/suggested/unsafe status requires complete conflict/precondition analysis;
- suppression is identity/provenance based, never message text;
- external/native shadow comparison is non-publishing;
- duplicate authority is rejected before consumer publication.

Acceptance additions:

- `lra0_diagnostic_identity_is_message_and_range_independent`
- `lra0_fix_requires_authored_intent_and_exact_basis`
- `lra0_shadow_observation_is_non_publishing`
- `lra0_duplicate_family_authority_fails_before_merge`
~~~~

### SRC-EXP-L950-737741E0B762

- Kind: `context`; source: `successor-expansion.md:950-950`; target: `node:LRA0`; text SHA-256: `737741e0b76239446c48a720239d65ee14568f9625c06d070cda9aa6c09bbdf3`.

~~~~markdown
### `LRA0.md` — Profile-scoped diagnostics, lint, fixes, and actions
~~~~

### SRC-EXP-L952-905C4A67A74B

- Kind: `forbidden`; source: `successor-expansion.md:952-957`; target: `node:LRA0`; text SHA-256: `905c4a67a74ba946413ea373036731c150065c484773f6bebb2e463834213a58`.

~~~~markdown
**Intent:** lock rule/action registration and safety without prematurely implementing every ecosystem rule.
**Predecessors:** `CFG0`, `TIF1`, `IDX0`.
**Subblocks:** (1) rule/action manifest keyed by exact vertical release; (2) fact-demand and applicability contracts; (3) diagnostic identity/suppression/provenance; (4) safe/suggested/unsafe edit classes and authored transaction basis; (5) common-neutral versus vertical-owned rule separation; (6) migrate representative Vue/Svelte rules and fixes.
**Acceptance:** inapplicable rules perform zero work; two profiles may use different rule epochs without collision; stale or conflicting edits are rejected; migrated rule diagnostics/fixes remain equivalent on pinned fixtures.
**Forbidden:** Vue-shaped global rule table, format-as-fix, executing third-party rule code, duplicate native/external diagnostics, or actions without exact source/map basis.
**Deletion/abort:** delete only the named representative rows/adapters migrated here; profile rows belong to their packs and shared registry deletion belongs solely to `LNT3`; abort if a “common” rule requires framework branching instead of a neutral fact contract.
~~~~
