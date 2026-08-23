# TCM0 §3 — Feature-ownership ledger

Scope: charter item 3. The trait defines **44** distinct methods (direct enumeration of every `fn` in
the trait body, `crates/verter_type_runtime/src/traits.rs:130-512`
— `pub trait TypeProvider: Send + Sync`; the `crates/verter_lsp/src/type_provider/traits.rs:6` re-export
is the same trait, not a second one). This ledger covers all 44 in **31 rows** — 8 methods that are pure
priority-tier variants of a base method (`open_file_background`/`open_file_normal`,
`load_file_background`/`load_file_normal`, `update_file_background`/`update_file_normal`,
`close_file_background`/`close_file_normal`) are folded into their base method's row (#2-5), the same
grouping convention the ledger already uses for `configure_paths`/`configure_paths_background` (#23) and
`update_workspace_folders`/`update_workspace_folders_background` (#28) — confirmed genuine, non-dead
variants with real distinct overrides and real production call sites (`tsserver/project_router.rs:1021-
1128`, `type_provider/lazy_managed.rs:277-350,634-708`, `type_provider/project_sync.rs:535,548,561,566`),
not silently dropped. `get_diagnostics_background` is the one priority variant confirmed dead
(zero non-wrapper callers) and is given its OWN row (#31) rather than folded, since a dead method is a
distinct finding from a live grouped variant.

**Owner vocabulary is exactly the four the charter names — no fifth invented:**
`TypeScriptLspDirect` (TypeScript's own semantic API answers the feature directly against the
content-mapper-produced generated file, using the wire `SpanMapFeature` mask — see
`package-lock-and-semantic-api.md` §3 — to decide legality per segment; no Verter-side relay of the
request/response); `VerterWithTypeSemanticOracle` (Verter answers using its own semantic engine but
consults the TypeScript `Program`/`Checker` sync/async API for underlying type facts it does not itself
resolve); `VerterNative` (answered entirely from Verter's own analysis, no TypeScript engine consulted);
`DisabledByExplicitApprovedContract` (the current capability has no owner under the new architecture and
is a **candidate** for removal — TCM0 records the candidate and its rationale; actual disposition still
requires the governance ratification the charter's acceptance clause demands, so every such row is
marked `CANDIDATE — governance ruling required`, never `REMOVED`).

**Rationale for the assignment pattern.** Once Verter is a content mapper (TCM2), the *reason* today's
`TypeProvider` exists at all — Verter acting as a client-side relay in front of a separately-managed
tsgo/tsserver process purely to answer standard TS-in-generated-file questions — goes away for the
subset of features that are pure "ask TypeScript about a position in the mapped file": TypeScript's own
language service now serves the editor directly, using the mapper's `SpanMap`/`diagnosticDirectives` to
map back to source, with **no Verter-owned code in the request path**. Only capabilities that need
Verter-specific knowledge the mapped file cannot carry (framework directive/slot/prop semantics, cross-
file macro surfaces, carrier lifecycle bookkeeping the content-mapper protocol has no field for) still
need a Verter-side answerer, and that answerer is `VerterWithTypeSemanticOracle` when it must ask the
oracle a type question, or `VerterNative` when it never needs to.

## Taxonomy note, 2026-08-23: the "Mapping class/mask" column is NOT the `projection_class` axis

The table's "Mapping class/mask" column predates, and is a DIFFERENT axis from,
`projection-class-contract.md`'s five ratified `projection_class` values
(`AuthoredVerbatim`/`AuthoredTransformed`/`SynthesizedHelper`/`ExternalUnit`/`DefinitionAnchor`). Two
labels in this column — `SessionLifecycle` and `TokenCompletion` — are NOT members of that ratified set,
and are not meant to be: they are this ledger's own capability-grouping labels for methods whose mapping
behaviour clusters together, used before `projection-class-contract.md` closed its five-class set.
Reconciled here rather than silently left to read as a sixth/seventh class:

- **`SessionLifecycle`** rows (#2-5's sub-rows) carry NO per-span wire mask at all — the column already
  says so explicitly ("no per-span mask"). These are project/file lifecycle operations
  (open/load/update/close), not spans on the generated file, so `projection_class` — which exists to
  classify a SPAN — does not apply to them. `SessionLifecycle` is retained as a lifecycle-method grouping
  label, not a `projection_class` value, and TCM2's terminal-mask emission (owned-scope item 10) never
  needs to resolve it to one.
- **`TokenCompletion`** rows (#6/#9's sub-rows) DO carry per-span masks (`Completion`, `Hover`) and DO
  need a `projection_class` at implementation time — this ledger does not perform that classification
  (a genuine, named TCM1/TCM2 task, not this integration's), but the mapping is straightforward from the
  class definitions already ratified: a `TypeScriptLspDirect`-owned plain-TS completion/hover span is
  `AuthoredVerbatim` (byte-for-byte authored script content); a `VerterWithTypeSemanticOracle`-owned
  directive/prop/slot/component-meta completion or hover span is `AuthoredTransformed` (derived from
  authored source through a lossy-but-reversible transform, per that class's own definition). TCM1/TCM2
  perform this per-row classification as part of implementing owned-scope item 10; it is not re-derived
  here.

## Ledger

| # | Method | Current impl(s) | Current production callers (representative) | Framework/region | New primary owner | Required TS capability | Mapping class/mask | Diagnostic behaviour | Failure behaviour | Conformance test | Perf cell | TCM4 deletes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `provider_id` | all 9 production impls | `server/nav_features.rs:643,1275,1341,1346` | n/a (identity) | `VerterNative` | none | n/a | n/a | n/a | new: `session_diagnostic_provenance_tag` | negligible | the `"tsgo"/"tsserver"/"extension"` string vocabulary — replaced by content-mapper's `InitializeResponse`/mapper-identity string |
| 2 | `open_file` / `open_file_background` / `open_file_normal` | `TsgoTypeProvider`, `TsgoOwnedProvider`, `TsserverTypeProvider`, `ProjectTsserverProvider`, `ExtensionTypeProvider`; the two priority variants are real, distinct overrides in `tsserver/project_router.rs:1021,1090` and `type_provider/lazy_managed.rs:634,678` (not dead defaults) | `type_provider/project_sync.rs:101,429`, `tsgo/overlay_core.rs:79`; priority variants dispatched by `lazy_managed.rs:277-350` (`PriorityLane`-based routing to normal/background) and consumed directly at `project_sync.rs:535,548` (background) | all frameworks | `TypeScriptLspDirect` for the editor-open lifecycle (TS's own `didOpen` now drives `transform()`); `VerterWithTypeSemanticOracle` for the oracle-session half Verter's own native features still need — the priority distinction (normal vs. background) becomes an oracle-session scheduling detail, not a separate ownership question, since it exists only to protect interactive traffic from bulk background opens | session initialize + `updateSnapshot{openFiles}` | class `SessionLifecycle` — no per-span mask | n/a | on failure, provider marked degraded (existing `resilient/forwarding.rs` semantics carry over) | new: `content_mapper_open_close_parity` | cold-open path — see topology-benchmark-plan.md | the whole relay-side "open a file in an externally-managed engine" call, all three priority variants — content mapper transform + editor didOpen supersede it |
| 3 | `load_file` / `load_file_background` / `load_file_normal` | same 3 engine impls + `ProjectTsserverProvider`; priority variants same override sites as #2 | `type_provider/project_sync.rs:413,456,1762,1784,2068`, `workspace_scanner.rs:789,804`; background variant also at `project_sync.rs:535` | all frameworks | `VerterWithTypeSemanticOracle` for all three (import-resolution-only load never needed a `TypeScriptLspDirect` split — it has no diagnostic/feature surface to hand to TypeScript directly) | `updateSnapshot{openFiles}` (import-resolution-only load) | `SessionLifecycle` | n/a | same as #2 | new: `import_only_load_no_diagnostics_regression` | background-tier, must not compete with interactive | the relay's "load without diagnosing" distinction, all three variants — the oracle-session equivalent replaces it 1:1, not deleted |
| 4 | `update_file` / `update_file_background` / `update_file_normal` | 3 engine impls; priority variants same override sites as #2 | `type_provider/project_sync.rs:445,461`, `virtual_types.rs:137`; background variant also at `project_sync.rs:561` | all frameworks | `TypeScriptLspDirect` (edits reach TS via editor `didChange` → mapper `transform()` again) with `VerterWithTypeSemanticOracle` mirror for the oracle session, for all three variants | `updateSnapshot{fileChanges}` | `SessionLifecycle` | n/a | same as #2 | new: as #2 | edit-latency-critical — first-class topology metric | the relay half; oracle-session update path survives |
| 5 | `close_file` / `close_file_background` / `close_file_normal` | 3 engine impls + router; priority variants same override sites as #2 | `background_drain.rs:1529`, `external_ts/membership_reconciler.rs:964,992,1083`, `server/lifecycle.rs:1038,1072`, `host_lifecycle.rs:586,986`; background variant also at `project_sync.rs:566` | all frameworks | `TypeScriptLspDirect` / `VerterWithTypeSemanticOracle` mirror, as #2/#4, for all three variants | `updateSnapshot{closeFiles}` (ref-counted, per §7 of the package-lock doc) | `SessionLifecycle` | n/a | same as #2 | new: as #2 | n/a | relay half deleted; oracle-session close survives |
| 6 | `get_completions` | 3 engine impls | `server/nav_features.rs:625,1149,1191,1224` | all frameworks (script region) | `TypeScriptLspDirect` for plain TS completions in mapped regions; `VerterWithTypeSemanticOracle` for directive/prop/slot-name completions (framework-specific, mapped file alone can't answer) | `SpanMapFeature.Completion` | class `TokenCompletion`, mask `Completion` | none (completion has no diagnostic surface) | empty list on failure (existing `Result` semantics) | existing `nav_features_completion*` suite, extend for split ownership | interactive-tier, tightest latency budget | the relay's full completion round-trip for plain-TS cases |
| 7 | `get_completion_details` (resolve) | default (identity) override only in `provider_adapter.rs:63` (component-meta query backend) | `provider_adapter.rs:63` | component-meta type-expansion | `VerterWithTypeSemanticOracle` (this is exactly the "Verter needs a type fact the mapped file alone won't carry" case) | oracle `Checker` symbol/type query | n/a (not a wire span feature) | n/a | n/a | existing `real_provider_tests/completion_detail.rs` | oracle round-trip, not interactive-tier | not deleted — this is the one method whose current shape IS the target shape |
| 8 | `supports_completion_resolve` | all engines except default-`false` | `server/lifecycle.rs:231` | capability flag | `VerterNative` (advertised capability becomes "does Verter's own resolver exist", not an engine flag) | none | n/a | n/a | n/a | existing | negligible | the per-engine variance — one Verter-owned answer replaces engine-dependent variance |
| 9 | `get_hover` | 3 engine impls | `server/custom_methods/mod.rs:681`, `server/nav_features.rs:87,128,270,349,427` | all frameworks | `TypeScriptLspDirect` for plain-TS hover; `VerterWithTypeSemanticOracle` for component-meta/prop/slot hover text | `SpanMapFeature.Hover`; upstream's PR #63936 note that "hover now concatenates results from multiple projections" is directly load-bearing here — Verter's own hover text over a `v-bind`/slot region is exactly a second projection TypeScript must be told to concatenate, not silently drop | class `TokenCompletion` shares `Hover` mask | none | `None`/empty on failure | existing `nav_features` hover suite + new multi-projection-concat regression | interactive-tier | the relay round-trip for plain-TS hover |
| 10 | `provider_wire_witness` | default (mint) + per-provider override | `extension_provider.rs:392` (self-called at each `get_hover` override) | wire-safety plumbing | `VerterNative` | none | n/a | n/a | n/a | existing `display_signature_seal.rs` | negligible | survives unchanged — it is a Rust-side type-safety seal, not an engine capability |
| 11 | `get_diagnostics` | 3 engine impls | `documents/mod.rs:1404`, `sync_orchestration.rs:111`, `server_utils.rs:1705`, `sync_coordinator.rs:1322,1565` | all frameworks — the busiest single method in the trait | `TypeScriptLspDirect` for compiler/checker diagnostics on mapped regions (content mapper's `DiagnosticDirectives`/`Expect`/`Ignore` policy, see diagnostic-ownership-matrix.md, supersedes today's merge logic for this class); `VerterNative` for lint/parse/style diagnostics (already Verter-only, per diagnostic-ownership-matrix.md A2-A4) | `SpanMapFeature` has no diagnostic-specific bit — diagnostics ride the mapper's own `DiagnosticDirectives` channel, not a feature mask | n/a — no `SpanMapFeature` mask applies to this class; see the previous column | see diagnostic-ownership-matrix.md for the full precedence/dedup ruling | empty list on failure, per existing semantics | existing `sync_coordinator` diagnostics suite, extended for the split | debounced background push — see topology-benchmark-plan.md | the TS-diagnostic half of today's `merge_diagnostics`/`same_mapped_diagnostic` (`type_provider/merge/diagnostics.rs`) — lint/parse merge logic survives, TS-merge logic does not |
| 12 | `get_definition` | 3 engine impls | `server/child_prop_rename.rs:462`, `nav_features_navigation.rs:100,323,431` | all frameworks | `TypeScriptLspDirect` (plain-TS defs) / `VerterWithTypeSemanticOracle` (component/slot cross-file defs the mapped file's own AST can't express) | `SpanMapFeature.Definition` | mask `Definition` | none | empty on failure | existing nav-features suite | interactive-tier | relay round-trip for plain-TS cases |
| 13 | `get_type_definition` | 3 engine impls | `nav_features_navigation.rs:556,645` | all frameworks | same split as #12 | `SpanMapFeature.TypeDefinition` | mask `TypeDefinition` | none | empty on failure | existing suite | interactive-tier | as #12 |
| 14 | `get_references` | 3 engine impls | `nav_features_navigation.rs:742,889` | all frameworks | same split as #12 | `SpanMapFeature.References` | mask `References` | none | empty on failure | existing suite | interactive-tier | as #12 |
| 15 | `get_rename_locations` | 3 engine impls | `nav_features_navigation.rs:1135`, `rename_prepare.rs:183` | all frameworks | `VerterWithTypeSemanticOracle` (rename that spans script+template needs Verter's own cross-region binding — see the Carrier IDE TS Surface Principle in CLAUDE.md; a pure `TypeScriptLspDirect` answer would miss the template-side occurrences) | `SpanMapFeature.Rename` | mask `Rename` | none | empty on failure, fail-closed per the multi-claimant carrier rule already governing rename elsewhere | existing suite + new cross-region rename regression | interactive-tier, allowed to be slower than hover/completion | the relay's script-only rename path — template-aware merge survives and grows |
| 16 | `get_signature_help` | 3 engine impls | `aux_features.rs:301,326` | all frameworks | `TypeScriptLspDirect` | `SpanMapFeature.SignatureHelp` | mask `SignatureHelp` | none | `None` on failure | existing suite | interactive-tier | relay round-trip |
| 17 | `get_code_actions` | 3 engine impls | `aux_features.rs:548,726` | all frameworks | split: plain-TS quick fixes → `TypeScriptLspDirect`; Verter-authored fixes (e.g. add-missing-prop) → `VerterNative` | `SpanMapFeature.CodeActions` | mask `CodeActions` | none | empty on failure | existing suite, split by action origin | interactive-tier | the TS-native subset of today's merged action list |
| 18 | `get_semantic_tokens` | 3 engine impls | `aux_features.rs:770` | all frameworks | `TypeScriptLspDirect` for script tokens; `VerterNative` for template/directive tokens (already a distinct token legend in practice) | `SpanMapFeature.SemanticTokens` | mask `SemanticTokens` | none | empty on failure | existing suite | interactive-tier | relay round-trip for the script-token subset |
| 19 | `get_document_highlights` | 3 engine impls | `aux_features.rs:198,262` | all frameworks | same split as #12 | `SpanMapFeature.DocumentHighlights` | mask `DocumentHighlights` | none | empty on failure | existing suite | interactive-tier | relay round-trip |
| 20 | `get_inlay_hints` | 3 engine impls | `aux_features.rs:873,959` | all frameworks | `TypeScriptLspDirect` | `SpanMapFeature.InlayHints` | mask `InlayHints` | none | empty on failure | existing suite | interactive-tier | relay round-trip |
| 21 | `resolve_completion` | default (`Ok(None)`) + overrides | `nav_features.rs:1361` | all frameworks | split, by analogy with #6/#7's completion split rather than #7 itself (#7 is sole-owner): `TypeScriptLspDirect` for plain items, `VerterWithTypeSemanticOracle` for Verter-authored completion items | oracle completion-details query | n/a | n/a | `None` on failure | existing suite | interactive-tier | relay half only |
| 22 | `shutdown` | default (`Ok(())`) + overrides | `main.rs:1091`, `server/lifecycle.rs:495` | session lifecycle | `VerterNative` (becomes "tear down the oracle session and the content-mapper process", not "ask an external engine to shut down") | mapper `closeProject` / oracle `API.close()` | n/a | n/a | best-effort, already tolerant | existing suite | teardown-latency metric | the per-engine variance in shutdown sequencing |
| 23 | `configure_paths` / `configure_paths_background` | default no-op (tsgo overrides real) | `background_init.rs:302` | tsgo-only today | `TypeScriptLspDirect` (paths/baseUrl now flow through the content-mapper's `openProject` config response, not a Verter-injected call) | mapper `OpenProjectResult` config identity | n/a | option diagnostics per mapper `OptionDiagnostic` | n/a | new: `paths_config_flows_through_mapper_not_injected` | one-time per project open | the Verter-injected config path entirely |
| 24 | `notify_carrier_changed` / `notify_carriers_changed` | default no-op (tsserver router overrides) | `tsserver/project_router.rs:877,888`, `external_ts/publish_coordinator.rs:393` | carrier lifecycle | `VerterWithTypeSemanticOracle` (still needed — the content-mapper protocol has no field for "a companion file changed out of band"; Verter's own oracle session must still be told) | oracle `updateSnapshot{fileChanges}` | n/a | n/a | n/a | new: `carrier_change_notify_oracle_session` | background-tier | none — survives, renamed onto the oracle session only |
| 25 | `register_carrier_member` / `register_carrier_metadata` | default no-op (tsgo overrides) | `type_provider/project_sync/virtual_types.rs:348`, `external_ts/membership_reconciler.rs:979` | tsgo-only today | `CANDIDATE — governance ruling required` | n/a until ruled | n/a | n/a | n/a | none yet | n/a | if approved: the entire relay-side carrier-registration call, since the content mapper's own `virtualFileName`/`canonicalSourceFileName`/`supplementalSourceFileNames` fields (package-lock-and-semantic-api.md §3) already carry this identity on the wire — a second Verter-side registration call may be pure duplication, but TCM0 does not have authority to rule that unilaterally |
| 26 | `activate_carrier_member` / `activate_carrier_members` | default no-op (tsgo overrides) | `external_ts/membership_reconciler.rs:780` | tsgo-only today | `CANDIDATE — governance ruling required` | n/a until ruled | n/a | n/a | n/a | none yet | n/a | same reasoning as #25 |
| 27 | `resync_open_files` | default (`Ok(())`) + tsgo override | `resync_singleflight.rs:145` | tsgo-only today | `VerterWithTypeSemanticOracle` (the oracle session still needs a bulk resync primitive; the content mapper's own `openProject`/`closeProject` do not resync file-open state) | `updateSnapshot{closeFiles,openFiles}` bulk | n/a | n/a | n/a | existing `resync_singleflight` tests, extended | background-tier, coalesced | the relay-engine resync path; oracle-session resync survives |
| 28 | `update_workspace_folders` / `_background` | default no-op (tsgo overrides) | `background_init.rs:283`, `server/lifecycle.rs:415,1137` | tsgo-only today | `VerterWithTypeSemanticOracle` | `updateSnapshot{openProjects,closeProjects}` | n/a | n/a | n/a | existing lifecycle tests | one-time per workspace-folder event | relay-side variant; oracle-session variant survives |
| 29 | `set_project_ownership` | default no-op + tsgo override | `background_init.rs:271` (sole call site) | tsgo-only, single call site | `VerterNative` (this is Verter's own multi-claimant-owner authority, per the Project-Bound External-TS Contract — it has nothing to do with the content-mapper protocol at all) | none | n/a | n/a | n/a | existing `configured_owner_resolution` tests | negligible | not deleted — unrelated to the TS-contract split, kept as-is |
| 30 | `child_pid` | default `None` + overrides | `main.rs:1225`, `server/lifecycle.rs:303` | process-management | `VerterNative` (reports the oracle-session process's pid, if one exists — the content-mapper process is spawned BY tsgo, not by Verter, so it has no pid to report from this side) | none | n/a | n/a | `None` when no process | existing `$/verter/typeProviderStarted` test | negligible | the per-relay-engine pid variance; oracle-session pid reporting survives |
| 31 | `get_diagnostics_background` | tsgo real override, all wrappers forward it | **zero non-test, non-wrapper call sites** (confirmed dead by exhaustive grep) | n/a | `DisabledByExplicitApprovedContract` | n/a | n/a | n/a | n/a | none — dead code | n/a | delete outright; already-dead code is not "capability removal" requiring the same governance bar, but recorded here since the charter demands every method be classified, not silently dropped from the ledger |

## Summary counts

Recomputed directly from the 31-row table above (a sole-owner row counts once; a split row counts once
per named owner, so per-owner totals sum to more than 31):

- 31 ledger rows covering all 44 trait methods (8 priority-tier variants folded into rows #2-5, 1
  already-grouped variant each in #23/#28 — see the file header), zero left as "TBD"/unclassified — the
  acceptance bar this ledger must clear.
- **Sole-owner rows (16):** `VerterNative` — #1,8,10,22,29,30 (6); `VerterWithTypeSemanticOracle` —
  #3,7,15,24,27,28 (6); `TypeScriptLspDirect` — #16,20,23 (3); `DisabledByExplicitApprovedContract` —
  #31 (1).
- **Split rows, two named owners each (13):** `TypeScriptLspDirect` + `VerterWithTypeSemanticOracle` —
  #2,4,5,6,9,12,13,14,19,21 (10 rows); `TypeScriptLspDirect` + `VerterNative` — #11,17,18 (3 rows).
- **Governance-pending rows (2):** #25, #26 — named `CANDIDATE — governance ruling required` rather than
  silently defaulted to any of the four, per the acceptance clause's ban on "an intentional capability
  removal without explicit governance approval." These are the one place this ledger is honestly
  incomplete. **Superseded, 2026-08-23**: an earlier draft of this paragraph routed the required consult
  through "TCM1/TCM2" — that routing is wrong and is corrected by the dedicated correction section below.
  The maintainer ruling is TCM0's OWN work (TCM0's charter forbids accepting the block with "an
  intentional capability removal lacking explicit governance approval," and rows #25-26 remain exactly
  that until the ruling lands), NOT TCM1/TCM2's and NOT TCM3's: TCM3's own `predecessors` in
  `program-dag.toml` are `["TCM0","TCM1"]`, so TCM3 cannot prepare or originate anything TCM0's own
  acceptance depends on. TCM3's charter separately carries TCM3-EC-G1, a downstream exit criterion
  satisfied by CITING the ruling TCM0 already obtained, not by TCM3 preparing it — see `OPEN-GAPS.md`
  items G-TCM0-ACCEPTANCE-ROWS-25-26 (TCM0's own gate) and G-TCM4-DELETION-ROWS-25-26 (TCM3-EC-G1's
  downstream gate) and the dedicated correction section below.
- **Per-owner totals** (sole + its share of split rows): `TypeScriptLspDirect` 16 (3 sole + 13 split);
  `VerterWithTypeSemanticOracle` 16 (6 sole + 10 split); `VerterNative` 9 (6 sole + 3 split);
  `DisabledByExplicitApprovedContract` 1. Check: 16+16+9+1 = 42 owner-slot mentions = 16 sole-owner rows
  (1 mention each) + 13 split rows (2 mentions each) = 16 + 26 = 42. Distinct rows: 16 + 13 + 2
  (governance-pending) = 31.

Several methods legitimately split across two owners for different sub-cases (e.g. `get_code_actions`:
plain-TS fixes vs Verter-authored fixes). This is recorded as a split, not left ambiguous — each row
names both owners and the exact discriminant between them, satisfying "a feature claimed by two owners"
only in the sense the charter permits (a stated, disjoint split), not the forbidden sense (an unresolved
contention over one undivided capability).

## Correction, 2026-08-23: split rows resolved into single-owner sub-rows

The reasoning above is sound, but the steering's acceptance invariants are literal: "no feature has two
primary owners" (steering, Acceptance invariants) and the per-row schema names ONE "new primary owner"
column. A 13-row table where each row names two owners under one "primary owner" heading reads as
exactly the forbidden shape even though the underlying split is legitimate. Closed here by splitting
each of the 13 two-owner rows into two single-owner sub-rows, each inheriting its parent row's other
columns (current impl/callers/framework/mapping-class-family/conformance-test-family) and narrowing only
to its own discriminant, owner, and mask. The parent row numbers (`#2` etc.) stay the stable
cross-reference identity; sub-rows are `#Na`/`#Nb`.

| Sub-row | Parent | Method | Discriminant (single, disjoint) | Primary owner (exactly one) | Mask/capability |
|---|---|---|---|---|---|
| #2a | #2 | `open_file`(+variants) | editor-open lifecycle (didOpen → mapper `transform()`) | `TypeScriptLspDirect` | `SessionLifecycle`, no per-span mask |
| #2b | #2 | `open_file`(+variants) | the oracle-session half Verter's own native features still need | `VerterWithTypeSemanticOracle` | session initialize + `updateSnapshot{openFiles}` |
| #4a | #4 | `update_file`(+variants) | edits reach TS via editor `didChange` → mapper `transform()` | `TypeScriptLspDirect` | `SessionLifecycle` |
| #4b | #4 | `update_file`(+variants) | oracle-session mirror update | `VerterWithTypeSemanticOracle` | `updateSnapshot{fileChanges}` |
| #5a | #5 | `close_file`(+variants) | editor `didClose` stops future `didChange`/`transform` traffic for that file | `TypeScriptLspDirect` | `SessionLifecycle` — **corrected 2026-08-23**: NOT `closeProject` (the confirmed protocol lifecycle is `OpenProject → repeated Transform → CloseProject`, `package-lock-and-semantic-api.md` §3 — `CloseProject` is PROJECT-scoped, not per-file; a per-file `didClose` requires no mapper-side call at all, it simply stops future `Transform` requests for that file within the still-open project) |
| #5b | #5 | `close_file`(+variants) | oracle-session mirror close | `VerterWithTypeSemanticOracle` | `updateSnapshot{closeFiles}` (ref-counted) |
| #6a | #6 | `get_completions` | plain TS completions in mapped regions | `TypeScriptLspDirect` | `SpanMapFeature.Completion` |
| #6b | #6 | `get_completions` | directive/prop/slot-name completions (framework-specific) | `VerterWithTypeSemanticOracle` | class `TokenCompletion`, mask `Completion` |
| #9a | #9 | `get_hover` | plain-TS hover | `TypeScriptLspDirect` | `SpanMapFeature.Hover` |
| #9b | #9 | `get_hover` | component-meta/prop/slot hover text (concatenated per upstream PR #63936's multi-projection note) | `VerterWithTypeSemanticOracle` | class `TokenCompletion` shares `Hover` mask |
| #11a | #11 | `get_diagnostics` | compiler/checker diagnostics on mapped regions | `TypeScriptLspDirect` | mapper `DiagnosticDirectives`/`Expect`/`Ignore` channel |
| #11b | #11 | `get_diagnostics` | lint/parse/style diagnostics (already Verter-only) | `VerterNative` | n/a — Verter's own diagnostic pipeline |
| #12a | #12 | `get_definition` | plain-TS definitions | `TypeScriptLspDirect` | `SpanMapFeature.Definition` |
| #12b | #12 | `get_definition` | component/slot cross-file defs the mapped file's own AST can't express | `VerterWithTypeSemanticOracle` | mask `Definition` |
| #13a | #13 | `get_type_definition` | plain-TS type definitions | `TypeScriptLspDirect` | `SpanMapFeature.TypeDefinition` |
| #13b | #13 | `get_type_definition` | cross-file component/slot type-definition cases | `VerterWithTypeSemanticOracle` | mask `TypeDefinition` |
| #14a | #14 | `get_references` | plain-TS references | `TypeScriptLspDirect` | `SpanMapFeature.References` |
| #14b | #14 | `get_references` | cross-file component/slot references | `VerterWithTypeSemanticOracle` | mask `References` |
| #17a | #17 | `get_code_actions` | plain-TS quick fixes | `TypeScriptLspDirect` | `SpanMapFeature.CodeActions` |
| #17b | #17 | `get_code_actions` | Verter-authored fixes (e.g. add-missing-prop) | `VerterNative` | n/a — Verter's own action provider |
| #18a | #18 | `get_semantic_tokens` | script tokens | `TypeScriptLspDirect` | `SpanMapFeature.SemanticTokens` |
| #18b | #18 | `get_semantic_tokens` | template/directive tokens (already a distinct token legend) | `VerterNative` | n/a — Verter's own token legend |
| #19a | #19 | `get_document_highlights` | plain-TS highlights | `TypeScriptLspDirect` | `SpanMapFeature.DocumentHighlights` |
| #19b | #19 | `get_document_highlights` | component/slot cross-file highlights | `VerterWithTypeSemanticOracle` | mask `DocumentHighlights` |
| #21a | #21 | `resolve_completion` | plain-TS completion-item resolution | `TypeScriptLspDirect` | n/a (wire completion-resolve) |
| #21b | #21 | `resolve_completion` | Verter-authored completion-item resolution | `VerterWithTypeSemanticOracle` | oracle completion-details query |

The original combined rows (#2,4,5,6,9,11,12,13,14,17,18,19,21 in the table above) are SUPERSEDED by
their `a`/`b` sub-rows for the purpose of the "one primary owner per row" acceptance bar; their other
column content (current impl, call sites, conformance test names, perf cell, TCM4-deletes text) is
unchanged and is read by reference from the parent row, not duplicated here. `get_rename_locations`
(#15) is NOT split by this correction — it already names a single owner
(`VerterWithTypeSemanticOracle`) with an explanatory note, not two competing owners, so it is unaffected.

**Clarification, 2026-08-23 — what "split" means for #2/#4/#5 versus #6/#9/#12-14/#19/#21.** Two distinct
shapes were folded under one "split row" label; they are disambiguated here rather than left to read as
one pattern:

- **#2, #4, #5 (`open_file`/`update_file`/`close_file`) are not contested ownership at all.** The
  `a`/`b` sub-rows are two ALWAYS-CO-OCCURRING side effects of one editor lifecycle event, historically
  bundled into one trait method — editor-open unconditionally drives BOTH the mapper `transform()` path
  AND the oracle-session's own `updateSnapshot`, every time, never one instead of the other. There is no
  routing decision here to get wrong; the sub-row split exists only so each side effect has its own named
  primary owner for the "one primary owner" acceptance bar, not because a caller must choose between them.
- **#6, #9, #12-14, #19, #21 are genuine region/target-shape routing splits**, and each discriminant is
  structural (decidable from the query's target, not a runtime judgment call): #6/#9 route by SOURCE
  REGION — a completion/hover position inside plain mapped script text is `TypeScriptLspDirect`; a
  position inside a directive/prop/slot/template-expression region Verter's own template analysis owns is
  `VerterWithTypeSemanticOracle` — these are DISJOINT byte ranges within the same document, not two
  owners answering the same span. #12-14/#19 route by TARGET SHAPE — a definition/type-definition/
  reference/highlight resolving entirely within the mapped file's own generated AST is
  `TypeScriptLspDirect`; one that requires crossing into Verter's component-meta graph (a cross-file
  component/slot/prop declaration) is `VerterWithTypeSemanticOracle` — this is a structural property of
  the resolved symbol (is it inside this generated file's own AST, or does resolving it require the
  component-meta graph), knowable from the query target, not a post-hoc judgment. #9 specifically is a
  REGION split, not a duplicate-answer case: upstream's documented multi-projection concatenation
  (`package-lock-and-semantic-api.md` §3, PR #63936) is how TWO DISJOINT-REGION results reach the editor
  in one response — it is not Verter fabricating a second answer over TypeScript's own span, which is
  exactly what "no duplicate result" forbids.

## Correction, 2026-08-23: rows #25-26 given two separate, non-circular gates, not left open-ended

The rows themselves stay `CANDIDATE — governance ruling required` (open) until the ruling actually
lands — it is the GATES that are closed here (assigned to a named owner, non-circular), not the rows'
disposition itself; see the per-row `CANDIDATE` marker preserved in the table above and the "Until the
ruling resolves..." bullet below.

Rows #25-26 (`register_carrier_member`/`register_carrier_metadata`,
`activate_carrier_member`/`activate_carrier_members`) were recorded as `CANDIDATE — governance ruling
required` with "none yet" in place of a disposition. Per the maintainer's direction that this train
proceeds and gaps are closed by assigning them to the block that owns the work — not left as an
unassigned "blocked" — these two rows are closed as follows:

- **They do NOT get a `DisabledByExplicitApprovedContract` owner now.** That owner class is reserved for
  removal explicitly approved through governance (charter's own rule: "Do not use this category to hide
  an unimplemented migration"); TCM0 has no such approval to cite.
- **The maintainer ruling itself is TCM0's OWN work, not TCM3-scoped work — see
  `OPEN-GAPS.md` G-TCM0-ACCEPTANCE-ROWS-25-26.** TCM3's own `predecessors` in `program-dag.toml` are
  `["TCM0", "TCM1"]`: TCM3 cannot be dispatched until TCM0 is ACCEPTED, so naming TCM3 as the ruling's
  preparer or gate here would make TCM0's acceptance depend on a block that cannot start until TCM0 is
  already accepted — an unsatisfiable cycle. The decision packet the ruling needs is already fully
  assembled by TCM0's own evidence (this section names the two legal outcomes and the reasoning record
  below), so the ruling can be requested directly off TCM0's own evidence, with no dependency on TCM3
  starting first. TCM0's charter forbids accepting the block with "an intentional capability removal
  lacking explicit governance approval," so this ruling gates TCM0's OWN acceptance.
- **TCM3-EC-G1 is a separate, downstream row (`OPEN-GAPS.md` G-TCM4-DELETION-ROWS-25-26), not the
  ruling's origin.** TCM3's rewritten charter (`charters/TCM3.md`) carries a numbered exit criterion
  (TCM3-EC-G1) requiring the SAME ruling be on record before TCM3 may be marked complete — satisfied by
  citing the ruling TCM0 already obtained, not by TCM3 re-deriving or preparing it. By the time TCM3
  exists to act, `program-dag.toml`'s ordering (`TCM3.predecessors = ["TCM0","TCM1"]`) guarantees TCM0's
  acceptance, and therefore the ruling, already landed. The ruling's two legal outcomes are: (a) approve
  deletion, on the grounds already recorded here — the content mapper's own `virtualFileName`/
  `canonicalSourceFileName`/`supplementalSourceFileNames` wire fields subsume the registration call's
  function, making a second Verter-side registration call pure duplication; or (b) retain the methods
  under `VerterWithTypeSemanticOracle` ownership if the ruling finds a surviving need TCM0's evidence
  did not identify. Either outcome is a closed row; TCM4 may only delete rows #25-26's code once
  TCM3-EC-G1 is satisfied (`deletion-closure.md`'s own gate on these rows is unchanged and reads the
  same ruling).
- Until the ruling resolves, rows #25-26 stay live code with a `CANDIDATE` marker — not orphaned, not
  silently defaulted, and not blocking TCM1/TCM2 (neither method is on their owned surface).
