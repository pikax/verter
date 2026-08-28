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

**One limit on that fourth owner, ruled 2026-08-24.**
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q5: **dead API surface has no capability owner and must not be labelled
`DisabledByExplicitApprovedContract`.** That class is for a live capability whose removal governance has
approved, not for code that already answers nobody. Row 31 (`get_diagnostics_background`) is the one row
this reaches: the ruling rejects its label and rules the method, its forwarding implementations and the
row itself for deletion — by a later, separately-scoped code-bearing slice. This block changes no source,
so that deletion has NOT been performed.

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

| # | Method | Current impl(s) | Current production callers (readable extract — the completeness claim is the derivation, not this cell) | Framework/region | New primary owner | Required TS capability | Mapping class/mask | Diagnostic behaviour | Failure behaviour | Conformance test | Perf cell | TCM4 deletes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `provider_id` | all 9 production impls | `server/nav_features.rs:643,1275,1341,1346` | n/a (identity) | `VerterNative` | none | n/a | n/a | n/a | new: `session_diagnostic_provenance_tag` | negligible | the `"tsgo"/"tsserver"/"extension"` string vocabulary — replaced by content-mapper's `InitializeResponse`/mapper-identity string |
| 2 | `open_file` / `open_file_background` / `open_file_normal` | `TsgoTypeProvider`, `TsgoOwnedProvider`, `TsserverTypeProvider`, `ProjectTsserverProvider`, `ExtensionTypeProvider`; the two priority variants are real, distinct overrides in `tsserver/project_router.rs:1021,1090` and `type_provider/lazy_managed.rs:634,678` (not dead defaults) | `type_provider/project_sync.rs:429`, `tsgo/overlay_core.rs:79` (corrected 2026-08-24: `project_sync.rs:101`, previously cited here, is a doc comment describing the single-open invariant, not a call site); priority variants dispatched by `lazy_managed.rs:277-350` (`PriorityLane`-based routing to normal/background) and consumed directly at `project_sync.rs:548` (background) and `:656` (normal) — `:535`, previously cited here, is `load_file_background`, row 3's method | all frameworks | `TypeScriptLspDirect` for the editor-open lifecycle (TS's own `didOpen` now drives `transform()`); `VerterWithTypeSemanticOracle` for the oracle-session half Verter's own native features still need — the priority distinction (normal vs. background) becomes an oracle-session scheduling detail, not a separate ownership question, since it exists only to protect interactive traffic from bulk background opens | session initialize + `updateSnapshot{openFiles}` | class `SessionLifecycle` — no per-span mask | n/a | on failure, provider marked degraded (existing `resilient/forwarding.rs` semantics carry over) | new: `content_mapper_open_close_parity` | cold-open path — see topology-benchmark-plan.md | the whole relay-side "open a file in an externally-managed engine" call, all three priority variants — content mapper transform + editor didOpen supersede it |
| 3 | `load_file` / `load_file_background` / `load_file_normal` | same 3 engine impls + `ProjectTsserverProvider`; priority variants same override sites as #2 | `type_provider/project_sync.rs:413,456`, `workspace_scanner.rs:789,804`; background variant also at `project_sync.rs:535`. Corrected 2026-08-24: `:1762,1784,2068`, previously cited here as production callers, sit inside `project_sync.rs`'s `#[cfg(test)] mod tests` (opens at `:677` and runs to end of file) — test references, not production evidence | all frameworks | `VerterWithTypeSemanticOracle` for all three (import-resolution-only load never needed a `TypeScriptLspDirect` split — it has no diagnostic/feature surface to hand to TypeScript directly) | `updateSnapshot{openFiles}` (import-resolution-only load) | `SessionLifecycle` | n/a | same as #2 | new: `import_only_load_no_diagnostics_regression` | background-tier, must not compete with interactive | the relay's "load without diagnosing" distinction, all three variants — the oracle-session equivalent replaces it 1:1, not deleted |
| 4 | `update_file` / `update_file_background` / `update_file_normal` | 3 engine impls; priority variants same override sites as #2 | `type_provider/project_sync.rs:445,461`, `virtual_types.rs:137`; background variant also at `project_sync.rs:561` | all frameworks | `TypeScriptLspDirect` (edits reach TS via editor `didChange` → mapper `transform()` again) with `VerterWithTypeSemanticOracle` mirror for the oracle session, for all three variants | `updateSnapshot{fileChanges}` | `SessionLifecycle` | n/a | same as #2 | new: as #2 | edit-latency-critical — first-class topology metric | the relay half; oracle-session update path survives |
| 5 | `close_file` / `close_file_background` / `close_file_normal` | 3 engine impls + router; priority variants same override sites as #2 | `background_drain.rs:1529`, `external_ts/membership_reconciler.rs:964,992,1083`, `server/lifecycle.rs:1038,1072`, `host_lifecycle.rs:586,986`; background variant also at `project_sync.rs:566` | all frameworks | `TypeScriptLspDirect` / `VerterWithTypeSemanticOracle` mirror, as #2/#4, for all three variants | `updateSnapshot{closeFiles}` (ref-counted, per §7 of the package-lock doc) | `SessionLifecycle` | n/a | same as #2 | new: as #2 | n/a | relay half deleted; oracle-session close survives |
| 6 | `get_completions` | 3 engine impls | `server/nav_features.rs:625,1149,1191,1224` | all frameworks (script region) | `TypeScriptLspDirect` for plain TS completions in mapped regions; `VerterWithTypeSemanticOracle` for directive/prop/slot-name completions (framework-specific, mapped file alone can't answer) | `SpanMapFeature.Completion` | class `TokenCompletion`, mask `Completion` | none (completion has no diagnostic surface) | empty list on failure (existing `Result` semantics) | existing `nav_features_completion*` suite, extend for split ownership | interactive-tier, tightest latency budget | the relay's full completion round-trip for plain-TS cases |
| 7 | `get_completion_details` (resolve) | default (identity) plus eight production overrides: `extension_provider.rs:308`, `tsserver/project_router.rs:688`, `type_provider/lazy_managed.rs:498`, `tsgo/composite.rs:1032`, `tsgo/shared.rs:1057`, `tsserver/ipc.rs:3632`, `tsgo/ipc.rs:3105`, `tsgo/owned.rs:530` | `provider_adapter.rs:63` is a component-meta query-backend call site | component-meta type-expansion | `VerterWithTypeSemanticOracle` (this is exactly the "Verter needs a type fact the mapped file alone won't carry" case) | oracle `Checker` symbol/type query | n/a (not a wire span feature) | n/a | n/a | existing `real_provider_tests/completion_detail.rs` | oracle round-trip, not interactive-tier | not deleted — this is the one method whose current shape IS the target shape |
| 8 | `supports_completion_resolve` | all engines except default-`false` | `server/lifecycle.rs:231` | capability flag | `VerterNative` (advertised capability becomes "does Verter's own resolver exist", not an engine flag) | none | n/a | n/a | n/a | existing | negligible | the per-engine variance — one Verter-owned answer replaces engine-dependent variance |
| 9 | `get_hover` | 3 engine impls | `server/custom_methods/mod.rs:681`, `server/nav_features.rs:87,128,270,349,427` | all frameworks | `TypeScriptLspDirect` for plain-TS hover; `VerterWithTypeSemanticOracle` for component-meta/prop/slot hover text | `SpanMapFeature.Hover`; upstream's PR #63936 note that "hover now concatenates results from multiple projections" is directly load-bearing here — Verter's own hover text over a `v-bind`/slot region is exactly a second projection TypeScript must be told to concatenate, not silently drop | class `TokenCompletion` shares `Hover` mask | none | `None`/empty on failure | existing `nav_features` hover suite + new multi-projection-concat regression | interactive-tier | the relay round-trip for plain-TS hover |
| 10 | `provider_wire_witness` | default (mint); no provider override anywhere | self-calls at `extension_provider.rs:392`, `tsserver/ipc.rs:3471`, `tsgo/ipc.rs:3234` | wire-safety plumbing | `VerterNative` | none | n/a | n/a | n/a | existing `display_signature_seal.rs` | negligible | survives unchanged — it is a Rust-side type-safety seal, not an engine capability |
| 11 | `get_diagnostics` | 3 engine impls | trait calls at `sync_orchestration.rs:111`, `sync_coordinator.rs:1322,1565`; `documents/mod.rs:1404` and `server_utils.rs:1705` call Verter-native diagnostics instead | all frameworks — the busiest single method in the trait | `TypeScriptLspDirect` for compiler/checker diagnostics on mapped regions (content mapper's `DiagnosticDirectives`/`Expect`/`Ignore` policy, see diagnostic-ownership-matrix.md, supersedes today's merge logic for this class); `VerterNative` for lint/parse/style diagnostics (already Verter-only, per diagnostic-ownership-matrix.md A2-A4) | `SpanMapFeature` has no diagnostic-specific bit — diagnostics ride the mapper's own `DiagnosticDirectives` channel, not a feature mask | n/a — no `SpanMapFeature` mask applies to this class; see the previous column | see diagnostic-ownership-matrix.md for the full precedence/dedup ruling | empty list on failure, per existing semantics | existing `sync_coordinator` diagnostics suite, extended for the split | debounced background push — see topology-benchmark-plan.md | the TS-diagnostic half of today's `merge_diagnostics`/`same_mapped_diagnostic` (`type_provider/merge/diagnostics.rs`) — lint/parse merge logic survives, TS-merge logic does not |
| 12 | `get_definition` | 3 engine impls | `server/child_prop_rename.rs:462`, `nav_features_navigation.rs:100,323,431` | all frameworks | `TypeScriptLspDirect` (plain-TS defs) / `VerterWithTypeSemanticOracle` (component/slot cross-file defs the mapped file's own AST can't express) | `SpanMapFeature.Definition` | mask `Definition` | none | empty on failure | existing nav-features suite | interactive-tier | relay round-trip for plain-TS cases |
| 13 | `get_type_definition` | 3 engine impls | `nav_features_navigation.rs:556,645` | all frameworks | same split as #12 | `SpanMapFeature.TypeDefinition` | mask `TypeDefinition` | none | empty on failure | existing suite | interactive-tier | as #12 |
| 14 | `get_references` | 3 engine impls | `nav_features_navigation.rs:742,889` | all frameworks | same split as #12 | `SpanMapFeature.References` | mask `References` | none | empty on failure | existing suite | interactive-tier | as #12 |
| 15 | `get_rename_locations` | 3 engine impls | `nav_features_navigation.rs:1135`, `rename_prepare.rs:183` | all frameworks | `VerterWithTypeSemanticOracle` (rename that spans script+template needs Verter's own cross-region binding — see the Carrier IDE TS Surface Principle in CLAUDE.md; a pure `TypeScriptLspDirect` answer would miss the template-side occurrences) | ~~`SpanMapFeature.Rename`~~ **NONE — see below** | ~~mask `Rename`~~ **CLEARED — bit 256 is NOT set on any wire span; see "row #15's wire-mask cells are superseded" below**  ~~ | none | empty on failure, fail-closed per the multi-claimant carrier rule already governing rename elsewhere | existing suite + new cross-region rename regression | interactive-tier, allowed to be slower than hover/completion | the relay's script-only rename path — template-aware merge survives and grows |
| 16 | `get_signature_help` | 3 engine impls | `aux_features.rs:301,326` | all frameworks | `TypeScriptLspDirect` | `SpanMapFeature.SignatureHelp` | mask `SignatureHelp` | none | `None` on failure | existing suite | interactive-tier | relay round-trip |
| 17 | `get_code_actions` | 3 engine impls | `aux_features.rs:548`. Corrected 2026-08-24: `:726`, previously cited alongside it, is inside the `#[cfg(test)]`-only helper `raw_provider_code_actions` (`aux_features.rs:692-729`) — a test reference, not a production caller; the surviving production call sites besides `:548` are the provider-composition forwarders at `tsserver/project_router.rs:796`, `type_provider/lazy_managed.rs:595`, `tsgo/composite.rs:1198` and `tsgo/shared.rs:1118` | all frameworks | split: plain-TS quick fixes → `TypeScriptLspDirect`; Verter-authored fixes (e.g. add-missing-prop) → `VerterNative` | `SpanMapFeature.CodeActions` | mask `CodeActions` | none | empty on failure | existing suite, split by action origin | interactive-tier | the TS-native subset of today's merged action list |
| 18 | `get_semantic_tokens` | 3 engine impls | `aux_features.rs:770` | all frameworks | `TypeScriptLspDirect` for script tokens; `VerterNative` for template/directive tokens (already a distinct token legend in practice) | `SpanMapFeature.SemanticTokens` | mask `SemanticTokens` | none | empty on failure | existing suite | interactive-tier | relay round-trip for the script-token subset |
| 19 | `get_document_highlights` | 3 engine impls | `aux_features.rs:198,262` | all frameworks | same split as #12 | `SpanMapFeature.DocumentHighlights` | mask `DocumentHighlights` | none | empty on failure | existing suite | interactive-tier | relay round-trip |
| 20 | `get_inlay_hints` | 3 engine impls | `aux_features.rs:873,959` | all frameworks | `TypeScriptLspDirect` | `SpanMapFeature.InlayHints` | mask `InlayHints` | none | empty on failure | existing suite | interactive-tier | relay round-trip |
| 21 | `resolve_completion` | default (`Ok(None)`) + overrides | `nav_features.rs:1361` | all frameworks | split, by analogy with #6/#7's completion split rather than #7 itself (#7 is sole-owner): `TypeScriptLspDirect` for plain items, `VerterWithTypeSemanticOracle` for Verter-authored completion items | oracle completion-details query | n/a | n/a | `None` on failure | existing suite | interactive-tier | relay half only |
| 22 | `shutdown` | default (`Ok(())`) + overrides | `main.rs:1091`, `server/lifecycle.rs:495` | session lifecycle | `VerterNative` (becomes "tear down the oracle session and the content-mapper process", not "ask an external engine to shut down") | mapper `closeProject` / oracle `API.close()` | n/a | n/a | best-effort, already tolerant | existing suite | teardown-latency metric | the per-engine variance in shutdown sequencing |
| 23 | `configure_paths` / `configure_paths_background` | default no-op; real overrides in tsgo and `extension_provider.rs:1294` | `background_init.rs:302` | tsgo and extension-host paths today | `TypeScriptLspDirect` (paths/baseUrl now flow through the content-mapper's `openProject` config response, not a Verter-injected call) | mapper `OpenProjectResult` config identity | n/a | option diagnostics per mapper `OptionDiagnostic` | n/a | new: `paths_config_flows_through_mapper_not_injected` | one-time per project open | the Verter-injected config path entirely |
| 24 | `notify_carrier_changed` / `notify_carriers_changed` | default no-op (tsserver router overrides) | `tsserver/project_router.rs:877,888`, `external_ts/publish_coordinator.rs:393` | carrier lifecycle | `VerterWithTypeSemanticOracle` (still needed — the content-mapper protocol has no field for "a companion file changed out of band"; Verter's own oracle session must still be told) | oracle `updateSnapshot{fileChanges}` | n/a | n/a | n/a | new: `carrier_change_notify_oracle_session` | background-tier | none — survives, renamed onto the oracle session only |
| 25 | `register_carrier_member` / `register_carrier_metadata` | default no-op; **tsserver-family** override is the only substantive impl (`tsserver/ipc.rs:3126`/`:3348`/`:3222`/`:3271`) — the tsgo-side `composite.rs:1283` override is a pure delegation. CORRECTED 2026-08-23; the prior cell said "tsgo overrides" | `type_provider/project_sync/virtual_types.rs:348`, `external_ts/membership_reconciler.rs:979` | tsserver-family ownership today; tsgo has a delegation only | `VerterWithTypeSemanticOracle` — **RETAINED** by `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q4 (preserves local content/position conversion and carrier-to-project routing) | `Program`/`Checker` type facts for the carrier members it registers | class `SessionLifecycle` — no per-span mask | n/a | existing `resilient/forwarding.rs` degraded-provider semantics carry over | successor-owned: a fixture proving the retained content/position conversion and carrier-to-project routing survive the mapper cutover | negligible per registration | **nothing, until TCM3 supplies and tests equivalent semantics.** Q4: TCM4 may remove the tsserver-specific methods only after that. The prior deletion rationale — that the mapper's `virtualFileName`/`canonicalSourceFileName`/`supplementalSourceFileNames` identity strings subsume this call — rested on an inverted premise and is not the basis of any retained obligation |
| 26 | `activate_carrier_member` / `activate_carrier_members` | default no-op; **tsserver-family** override is the only substantive impl (`tsserver/ipc.rs:3126`/`:3348`/`:3222`/`:3271`) — the tsgo-side `composite.rs:1283` override is a pure delegation. CORRECTED 2026-08-23; the prior cell said "tsgo overrides" | `external_ts/membership_reconciler.rs:780` | ~~tsgo-only today~~ **tsserver-family, not tsgo** | `VerterWithTypeSemanticOracle` — **RETAINED** by `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q4 (preserves oracle working-set activation) | `Program`/`Checker` type facts over the activated working set | class `SessionLifecycle` — no per-span mask | n/a | existing `resilient/forwarding.rs` degraded-provider semantics carry over | successor-owned: a fixture proving oracle working-set activation survives the mapper cutover | negligible per activation | **nothing, until TCM3 supplies and tests equivalent semantics** — same gate as #25 |
| 27 | `resync_open_files` | default (`Ok(())`) plus overrides in tsgo, `extension_provider.rs:1389`, `tsserver/project_router.rs:980`, and `tsserver/ipc.rs:4817` | `resync_singleflight.rs:145` | tsgo, extension-host, and tsserver-family paths today | `VerterWithTypeSemanticOracle` (the oracle session still needs a bulk resync primitive; the content mapper's own `openProject`/`closeProject` do not resync file-open state) | `updateSnapshot{closeFiles,openFiles}` bulk | n/a | n/a | n/a | existing `resync_singleflight` tests, extended | background-tier, coalesced | the relay-engine resync path; oracle-session resync survives |
| 28 | `update_workspace_folders` / `_background` | default no-op plus overrides in tsgo, `extension_provider.rs:1331`, `tsserver/ipc.rs:4781`, and `tsserver/project_router.rs:990` | `background_init.rs:283`, `server/lifecycle.rs:415,1137` | tsgo, extension-host, and tsserver-family paths today | `VerterWithTypeSemanticOracle` | `updateSnapshot{openProjects,closeProjects}` | n/a | n/a | n/a | existing lifecycle tests | one-time per workspace-folder event | relay-side variant; oracle-session variant survives |
| 29 | `set_project_ownership` | default no-op + tsgo override | `background_init.rs:271` (sole call site) | tsgo-only, single call site | `VerterNative` (this is Verter's own multi-claimant-owner authority, per the Project-Bound External-TS Contract — it has nothing to do with the content-mapper protocol at all) | none | n/a | n/a | n/a | existing `configured_owner_resolution` tests | negligible | not deleted — unrelated to the TS-contract split, kept as-is |
| 30 | `child_pid` | default `None` + overrides | `main.rs:1225`, `server/lifecycle.rs:303` | process-management | `VerterNative` (reports the oracle-session process's pid, if one exists — the content-mapper process is spawned BY tsgo, not by Verter, so it has no pid to report from this side) | none | n/a | n/a | `None` when no process | existing `$/verter/typeProviderStarted` test | negligible | the per-relay-engine pid variance; oracle-session pid reporting survives |
| 31 | `get_diagnostics_background` | tsgo real override, all wrappers forward it | **zero non-test, non-wrapper call sites** (confirmed dead by exhaustive grep) | n/a | **no capability owner** — `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q5 REJECTS the `DisabledByExplicitApprovedContract` label for this row: dead API surface has no capability owner | n/a | n/a | n/a | n/a | none — dead code | n/a | **deleted**, per Q5: `get_diagnostics_background`, its forwarding implementations, and this row. The deletion is a separate, later, code-bearing slice — not performed by this read-only block. Recorded here since the charter demands every method be classified, not silently dropped from the ledger |

## Summary counts

Recomputed directly from the 31-row table above (a sole-owner row counts once; a split row counts once
per named owner, so per-owner totals sum to more than 31):

- 31 ledger rows covering all 44 trait methods (8 priority-tier variants folded into rows #2-5, 1
  already-grouped variant each in #23/#28 — see the file header), zero left as "TBD"/unclassified — the
  acceptance bar this ledger must clear.
- **Sole-owner rows (17):** `VerterNative` — #1,8,10,22,29,30 (6); `VerterWithTypeSemanticOracle` —
  #3,7,15,24,25,26,27,28 (8); `TypeScriptLspDirect` — #16,20,23 (3). Rows #25 and #26 join this tier
  as `VerterWithTypeSemanticOracle` by `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
  Q4 (RETAINED), replacing their former `CANDIDATE — governance ruling required` marker.
- **Rows with no capability owner (1):** #31, whose `DisabledByExplicitApprovedContract` label the same
  ruling REJECTS (Q5: dead API surface has no capability owner) and which it rules for deletion by a
  later code-bearing slice.
- **Split rows, two named owners each (13):** `TypeScriptLspDirect` + `VerterWithTypeSemanticOracle` —
  #2,4,5,6,9,12,13,14,19,21 (10 rows); `TypeScriptLspDirect` + `VerterNative` — #11,17,18 (3 rows).
- **Formerly governance-pending rows (2), now RULED:** #25 and #26 carried
  `CANDIDATE — governance ruling required` because the acceptance clause bans "an intentional capability
  removal without explicit governance approval" and no ruling existed. That ruling now exists.
  `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
  Q4 **RETAINS both rows under `VerterWithTypeSemanticOracle`** — row 25 preserving local
  content/position conversion and carrier-to-project routing, row 26 preserving oracle working-set
  activation — and states the deletion gate: **TCM4 may remove the tsserver-specific methods only after
  TCM3 supplies and tests equivalent semantics.** These rows no longer gate TCM0, and the ledger is no
  longer incomplete at them. See `OPEN-GAPS.md` items G-TCM0-ACCEPTANCE-ROWS-25-26 and
  G-TCM4-DELETION-ROWS-25-26, both closed on that ruling, and the dedicated correction section below for
  the source-level finding that made the deletion argument untenable.
- **Per-owner totals** (sole + its share of split rows): `TypeScriptLspDirect` 16 (3 sole + 13 split);
  `VerterWithTypeSemanticOracle` 18 (8 sole + 10 split); `VerterNative` 9 (6 sole + 3 split);
  `DisabledByExplicitApprovedContract` 0 — the class has no member, row #31's former label having been
  rejected by Q5. Check: 16+18+9 = 43 owner-slot mentions = 17 owner-bearing sole rows (1 mention each) +
  13 split rows (2 mentions each) = 17 + 26 = 43. Distinct rows: 17 (sole) + 13 (split) + 1 (#31, ruled
  for deletion, no owner) = 31.

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

## Correction, 2026-08-23: rows #25-26 given two separate, non-circular gates — and the ruling that then landed

**Outcome, 2026-08-24.** The ruling this section was waiting for exists, and it RETAINS both rows.
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q4: retain rows 25 and 26 under `VerterWithTypeSemanticOracle` — row 25 preserves local content/position
conversion and carrier-to-project routing, row 26 preserves oracle working-set activation; TCM4 may remove
the tsserver-specific methods **only after TCM3 supplies and tests equivalent semantics**. The rows carry
that owner in the table above and no longer carry a `CANDIDATE` marker.

The reasoning below is retained because it remains the record of how the gates were separated and why the
deletion outcome was untenable. Read it as the packet the ruling decided, not as a live open question.

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

## The caller column is now backed by a derivation, not a sample

**Where the completeness claim lives.** It lives in
`typeprovider-call-sites.md`, generated by
`probes/typeprovider-call-site-derivation.mjs`. It does **not** live in the table's "Current production
callers" cells. Those cells are a readable, non-authoritative extract, silent about the rest; the six
source corrections below supersede inaccurate cells, and the header now says so. Read a cell to see *which kind of caller* a method has; read the
derivation for its classified own-spelling textual occurrences and disclosed blind spots.

**Why the change.** The column was previously labelled "representative". A sample omits exactly the call
site it should have caught, and the obligation is over every call site, so the claim cannot rest on a
hand-maintained cell. The derivation reads the method list out of the trait body itself
(`crates/verter_type_runtime/src/traits.rs`) and lexes every `.rs` file under `crates/`, so a method
added to that trait or an own-spelling textual call under `crates/` shows up on the next run; renamed and
macro-synthesised calls remain outside that derivation.

**Falsifying command.**
`node docs/arch/refactor/rev11/evidence/TCM0/probes/typeprovider-call-site-derivation.mjs --check`
re-derives from the live tree and exits 1 on any drift.

### The derived universe

44 trait methods, 3,130 `.rs` files under `crates/`, 2,551 classified occurrences:

| class | count | what it is |
|---|---|---|
| `trait-declaration` | 44 | the declarations in the trait body |
| `impl-production` | 328 | `fn <name>` definitions outside test regions |
| `impl-test` | 286 | definitions inside test regions (mock providers) |
| `call-production` | 203 | calls in non-test code from a `fn` of a different name |
| `call-forwarding` | 178 | calls from inside a `fn` of the SAME name — delegating wrappers |
| `call-trait-default` | 14 | calls inside the trait's own default bodies |
| `call-test` | 558 | calls in test regions |
| `ref-production` / `ref-test` | 55 / 41 | code mentions not followed by `(` |
| `doc-comment` / `comment` / `string-literal` | 395 / 200 / 249 | mentions in text, never counted as calls |

Per method, the derived call census (the full `file:line` enumeration is in the generated file):

| method | call prod | call fwd | call dflt | call test |
|---|---|---|---|---|
| `provider_id` | 6 | 2 | 0 | 3 |
| `supports_completion_resolve` | 1 | 3 | 0 | 0 |
| `open_file` | 12 | 3 | 2 | 100 |
| `load_file` | 14 | 4 | 2 | 14 |
| `update_file` | 7 | 3 | 2 | 17 |
| `close_file` | 17 | 5 | 2 | 11 |
| `get_completions` | 6 | 6 | 0 | 23 |
| `get_completion_details` | 1 | 5 | 0 | 7 |
| `get_hover` | 11 | 6 | 0 | 56 |
| `provider_wire_witness` | 3 | 0 | 0 | 4 |
| `get_diagnostics` | 8 | 5 | 1 | 42 |
| `get_definition` | 6 | 6 | 0 | 12 |
| `get_type_definition` | 3 | 6 | 0 | 6 |
| `get_references` | 3 | 6 | 0 | 5 |
| `get_rename_locations` | 2 | 6 | 0 | 5 |
| `get_signature_help` | 2 | 6 | 0 | 5 |
| `get_code_actions` | 1 | 6 | 0 | 8 |
| `get_semantic_tokens` | 1 | 6 | 0 | 11 |
| `get_document_highlights` | 2 | 6 | 0 | 5 |
| `get_inlay_hints` | 2 | 6 | 0 | 6 |
| `resolve_completion` | 2 | 6 | 0 | 8 |
| `shutdown` | 16 | 11 | 0 | 163 |
| `configure_paths` | 5 | 2 | 1 | 7 |
| `notify_carrier_changed` | 1 | 4 | 1 | 0 |
| `notify_carriers_changed` | 2 | 2 | 0 | 0 |
| `register_carrier_member` | 3 | 4 | 1 | 13 |
| `register_carrier_metadata` | 4 | 2 | 0 | 0 |
| `activate_carrier_member` | 2 | 3 | 1 | 2 |
| `activate_carrier_members` | 2 | 2 | 0 | 0 |
| `resync_open_files` | 2 | 5 | 0 | 2 |
| `update_workspace_folders` | 7 | 3 | 1 | 8 |
| `set_project_ownership` | 1 | 0 | 0 | 4 |
| `child_pid` | 4 | 5 | 0 | 8 |
| `open_file_background` | 5 | 3 | 0 | 0 |
| `load_file_background` | 5 | 4 | 0 | 0 |
| `update_file_background` | 5 | 3 | 0 | 0 |
| `close_file_background` | 5 | 4 | 0 | 0 |
| `get_diagnostics_background` | 1 | 4 | 0 | 2 |
| `configure_paths_background` | 2 | 1 | 0 | 1 |
| `update_workspace_folders_background` | 2 | 2 | 0 | 0 |
| `open_file_normal` | 5 | 3 | 0 | 0 |
| `load_file_normal` | 5 | 3 | 0 | 0 |
| `update_file_normal` | 4 | 3 | 0 | 0 |
| `close_file_normal` | 5 | 3 | 0 | 0 |

### What the derivation cannot see — stated here, not only in the script

A textual derivation is evidence only if its blind spots are named. These are reproduced from the
generator's own header (limits L1-L5 there):

- **Identifier collisions (L1).** `shutdown`, `child_pid`, `close_file`, `provider_id` are ordinary
  identifiers. A `.shutdown()` on something that is not a provider is textually identical to one that
  is. **The counts include textual collisions and can omit renamed or macro-synthesised calls, so neither
  bound is guaranteed.** Every
  derived row carries its enclosing item and a receiver snippet so a reader can adjudicate, and two such
  rows were adjudicated by reading them: `crates/verter_bench/examples/attribution_baseline.rs:195` and
  `crates/verter_bench/examples/currency_phase_probe.rs:685` are both `meta_host.shutdown()` on a
  `VerterHost`, not on a provider. They are collisions, they are counted, and naming them makes the
  over-counting risk concrete rather than merely a disclaimer.
- **Generic parameters (L2).** A call through `fn f<P: TypeProvider>(p: &P)` is found by name, but the
  bound is not proven textually. `crates/verter_type_runtime/src/resilient.rs`'s
  `replay_into<P: TypeProvider>` is exactly this shape and is a genuine call site for six methods.
- **Own-spelling dynamic dispatch is covered; renamed dispatch is not (L3).** `dyn TypeProvider` reaches a
  method by its own spelling, so such textual call sites appear; the `crates/verter_lsp/src/type_provider/traits.rs`
  re-export renames nothing, so it is covered too. A call reached under a DIFFERENT name — a renamed
  `use … as`, a renaming adapter, a macro-synthesised identifier — would be invisible. The derivation does
  not determine whether such a route exists; it establishes only the own-spelling occurrences it found.
- **Macro-pasted identifiers (L4)** carry no matchable text.
- **`crates/` only (L5).** The TypeScript packages are out of the trait's language.

### Two findings the sample did not carry

1. **`get_diagnostics_background` (row #31) has one non-test call whose enclosing `fn` is not the same
   method** — `crates/verter_lsp/src/tsgo/composite.rs:821`, inside the private helper
   `managed_diagnostics`. Following the chain: `get_diagnostics_background` (`composite.rs:997`, dispatching at `:999`) →
   `diagnostics_gated` (declared `:764`) → `managed_diagnostics` (called at `:771` and `:811`, declared
   `:815`) → `self.managed.get_diagnostics_background(path)` (`:821`). So row #31's "zero non-test, non-wrapper
   call sites" verdict stands — but it stands through a **two-hop private-helper wrapper chain**, not
   because a grep found nothing. The derivation classifies that site `call-production` on purpose: a
   differently-named helper is not a same-name forwarder, and hiding it inside the forwarding class
   would have concealed the one thing a reader needs to check.
2. **126 of the 203 derived production call sites are cited nowhere in this ledger.** That number is
   computed, not estimated: every `file.rs:NNN` pair anywhere in this document was collected and
   subtracted from the derived `call-production` set. The 77 that ARE cited are the sample; the 126 are
   what a sample omits. They cluster in the wrapper and adapter layers the per-row cells never reached —
   `crates/verter_type_runtime/src/resilient.rs` (28, including the `replay_into<P: TypeProvider>`
   restart replay), `crates/verter_lsp/src/type_provider/lazy_managed.rs` (28),
   `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs` (14),
   `crates/verter_type_runtime/src/provider_adapter.rs` (9),
   `crates/verter_dx_baseline/src/main.rs` (11), `crates/verter_lsp/src/type_provider/project_sync.rs`
   (6), `crates/verter_type_runtime/src/tsgo/ipc.rs` (4),
   `crates/verter_lsp/src/sync_coordinator.rs` (3), `crates/verter_lsp/src/tsgo/shared.rs` (3), and
   fifteen further files with one or two each. **That 126 is itself an upper bound**, for the same L1
   reason as the total: some of its `shutdown` entries — `crates/verter_tsgo_api/src/relay.rs:963`,
   `crates/verter_tsgo_api/src/jsonrpc/connection.rs:273` and their neighbours — are collisions on a
   common method name rather than provider calls, and the derivation counts them rather than guessing.
   The per-row cells are NOT expanded to list all 203: they stay a readable extract by design. What
   changes is that nothing rests on their being complete.

### One classification hazard, handled rather than disclosed

Several directories of test code live under `src/` and carry no `#[cfg(test)]` inside their own files:
`crates/verter_lsp/src/lib.rs` gates `real_provider_tests`, `test_harness`, `integration_tests` and
others through `#[cfg(test)] mod NAME;` declarations. Reading in-file attributes alone would have
promoted hundreds of test calls into the production column — an error in precisely the direction that
matters. The derivation resolves those file-module declarations, transitively, and a negative control
confirms it discriminates: a call planted in `real_provider_tests/completion.rs` classifies `call-test`,
while a call planted in `tsgo/composite.rs` classifies `call-production`.

### The derivation was proven to fail before it was believed

Three mutations, each verified present, unique and absent-before-planting, then reverted:

| planted | expected | observed |
|---|---|---|
| a new method in the trait body | method count rises from the trait, not from a list | 44 → 45 |
| a call in a non-test `impl` (`tsgo/composite.rs`) | appears as `call-production` | appeared, `call-production` |
| a call in a `#[cfg(test)] mod`-gated file (`real_provider_tests/completion.rs`) | appears as `call-test`, NOT production | appeared, `call-test` |

`--check` exited 1 under the plants and 0 after reverting them.

## Closure, 2026-08-23: capability coverage beyond the 44 trait methods (`G-LEDGER-SCOPE`)

`OPEN-GAPS.md`'s `G-LEDGER-SCOPE` row recorded that the steering's charter item 3 names capabilities
beyond the trait's 44 methods, and that nobody had checked row-by-row whether each is already one of the
44. That check has now been performed against the source. It produced three results, in ascending order
of consequence.

### 1. The "44" is correct, and the trait is the whole trait

Re-enumerated directly: `awk 'NR>=130 && NR<=512' crates/verter_type_runtime/src/traits.rs | grep -cE
"^    fn |^    async fn "` returns **44**. The trait body carries no `#[cfg(...)]`-gated methods, no
associated types or consts, and no capability-carrying supertrait (`crates/verter_type_runtime/src/
traits.rs:130` — `pub trait TypeProvider: Send + Sync`). The companion traits in the same crate
(`ConfiguredOwnerAuthority` `traits.rs:63`, `GeneratedQueryBackend` `backend.rs:151`, `ProviderNotifier`
`resilient.rs:63`, `ResilientBackend<P>` `resilient.rs:100`) add no provider capability. There is no
`TypeProviderExt` and no `impl dyn TypeProvider` inherent block. **The method-level coverage claim in
this ledger's header stands unmodified.**

### 2. Method coverage is not capability coverage — 14 named steering capabilities have no row

The steering's `At minimum cover:` list (`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md:278-311`)
names 32 capabilities. Checking each against the trait and against the LSP server's own handler table,
**thirteen are served by real production code with no `TypeProvider` hop in the adjudicated request path;
rename preparation is the exception and calls `get_rename_locations`. None has a separate row in this
ledger.** The per-capability walk below now names `folding`, `call hierarchy`,
`code lens`, `formatting`, `document symbol`, `selection range`, `prepareRename`, `workspace symbol`,
`linked editing`, `document link` or `document color`.

| Steering capability | Where it actually lives | Advertised at |
|---|---|---|
| rename preparation | `crates/verter_lsp/src/server/rename_prepare.rs:96` (own fail-closed admission policy at `:24`,`:38`; consumes `get_rename_locations` at `:181`) | `crates/verter_lsp/src/capabilities.rs:83` |
| formatting (+ on-type) | `crates/verter_lsp/src/features/formatting.rs:23`; on-type `server/aux_features.rs:1158` | `capabilities.rs:105` |
| call hierarchy | `crates/verter_lsp/src/features/call_hierarchy.rs:15`,`:107`,`:157` | `capabilities.rs:113` |
| code lens | `crates/verter_lsp/src/features/code_lens.rs:13` | `capabilities.rs:124` |
| folding | `crates/verter_lsp/src/features/folding_range.rs:14` | `capabilities.rs:91` |
| selection ranges | `crates/verter_lsp/src/server/aux_features.rs:112` (inline body) | `capabilities.rs:92` |
| document symbols | `crates/verter_lsp/src/features/document_symbol.rs:16` | `capabilities.rs:90` |
| component surface resolution | `VerterHost::resolve_framework_surface_with_audit` `crates/verter_session/src/typeinfo/framework_surface/executor.rs:91`; handler `crates/verter_lsp/src/server/custom_methods/component_meta.rs:110` | custom method, `crates/verter_lsp/src/main.rs:183` |
| template expression typing | IDE TSX projection `crates/verter_lsp/src/documents/mod.rs:298`; carrier↔TSX mapping `crates/verter_lsp/src/type_provider/merge/position.rs:142` | n/a — it is the projection itself |
| props | `AnalyzedMacroKind::DefineProps` `crates/verter_semantic/src/analysis/types.rs:1899`; `crates/verter_compiler/src/ide/template/props.rs` | n/a |
| events | `AnalyzedMacroKind::DefineEmits` `types.rs:1900`; `crates/verter_lsp/src/features/event_type_hints.rs:83` | n/a |
| slots and snippets | `AnalyzedMacroKind::DefineSlots` `types.rs:1904`; `crates/verter_lsp/src/server/component_resolve.rs:984`,`:1029` | n/a |
| directives | `crates/verter_lsp/src/features/hover_directive_names.rs:106`; `crates/verter_compiler/src/ide/template/directives.rs` | n/a |
| framework macros | `AnalyzedMacroKind` `types.rs:1898` (7 variants); `crates/verter_lsp/src/features/macro_actions.rs` | n/a |

Two further steering entries are **partially** covered — the provider half has a row, the Verter-native
half does not:

- **auto-imports.** The provider half rides `resolve_completion` (row #21; the trait doc names
  auto-import at `traits.rs:264`). Verter's own component auto-import and organize-imports path
  (`crates/verter_lsp/src/server/nav_features_completion_resolve.rs:53`,
  `crates/verter_lsp/src/features/organize_imports.rs`) has no row.
- **background semantic analysis.** The provider lane (the seven `*_background` methods, driven by
  `crates/verter_lsp/src/background_drain.rs:88`) is folded into rows #2-5/#23/#28/#31. Verter's own
  native semantic lane — `schedule_semantic_analysis` `crates/verter_lsp/src/documents/analysis.rs:136`,
  enabled at `crates/verter_lsp/src/server/lifecycle.rs:132` — has no row.

The steering's "all provider configuration and cache methods" clause (`:310`) likewise splits: the
configuration half is covered (rows #23, #28, #29, #1, #30, #10), but **there is no cache method on the
trait at all** — the provider-adjacent caches live outside it (`crates/verter_lsp/src/carrier_cache.rs:86`,
`ProviderSurfaceStore` `crates/verter_lsp/src/provider_surface_store/mod.rs:340`) and have no row.

The independent cross-check confirms the size of the gap from the other direction. Of the LSP requests
the server actually handles (`crates/verter_lsp/src/server/mod.rs:1423-1637`), **17 have no trait method
behind them** — `prepareRename`, `documentSymbol`, `foldingRange`, `selectionRange`, `codeLens`,
`linkedEditingRange`, `documentLink`, `documentColor`, `colorPresentation`, `formatting`,
`onTypeFormatting`, `workspace/symbol`, `prepareCallHierarchy`, `callHierarchy/incomingCalls`,
`callHierarchy/outgoingCalls`, plus the two custom-method families — and the **18 custom methods**
registered at `crates/verter_lsp/src/main.rs:136-195` have none either.

### 3. One steering capability has no `TypeProvider`/`verter_lsp` row, but IS served — by a typescript-plugin override

**Goto-implementation has no `TypeProvider` method and no `verter_lsp` dispatch handler** —
`capabilities.rs:56-212` sets no `implementation_provider` field (while `definition_provider`,
`type_definition_provider` and `references_provider` are all set, at `:79`, `:80`, `:81`); the
`impl LanguageServer for VerterLanguageServer` dispatch table (`server/mod.rs:1423-1637`) has no
`goto_implementation` method. Both of those hold.

**Corrected 2026-08-24 (round-2 review): the "exactly one hit" claim was wrong** — the search actually
returns **11 hits, not one**. Re-run independently: `grep -rn 'textDocument/implementation\|goto_implementation\|gotoImplementation\|implementation_provider\|ImplementationProvider\|GotoImplementationParams\|getImplementation' crates/ packages/*/src`. The breakdown:

- `crates/verter_tsgo_api/src/egress.rs:494` — the string `"textDocument/implementation"` inside the
  `NULL_VALID_METHODS` allowlist, a JSON-RPC routing table deciding whether a suppressed response may be
  completed with `result: null`. Pass-through transport, not a capability.
- Four test-only hits in `packages/typescript-plugin/src/index.spec.ts:201,202,3407,3472`.
- **Six production hits at `packages/typescript-plugin/src/index.ts:3095-3109`** — a genuine Verter-owned
  override of `languageService.getImplementationAtPosition` that carrier-routes the position
  (`editorCarrierPosition`/`usesEditorCarrierRouting`) to the owning runtime for carrier files and, for
  the direct-file case, remaps each returned `ImplementationLocation`'s `DocumentSpan`s back to source
  (`protocolSafeMappedSpans(editorRuntime, remapDocumentSpans(...))`) — the SAME carrier-routing pattern
  every other `TypeScriptLspDirect`-owned feature in this file uses.

So goto-implementation is **not absent** — it has no `TypeProvider` trait method and no `verter_lsp`-side
dispatch handler, but it IS served, at the typescript-plugin carrier-routing layer, the same layer that
serves every other `TypeScriptLspDirect` feature. Recording it as a proven absence was itself wrong; the
correct verdict is the same one section 2's 14 capabilities get: **located, not behind a `TypeProvider`
method** — here, at `packages/typescript-plugin/src/index.ts:3095`, advertised implicitly by TypeScript's
own `implementationProvider` capability (Verter never registers or advertises one of its own).

This does not change the charter's acceptance-bar analysis: TCM0 still may not be accepted with "an
intentional capability removal without explicit governance approval", and goto-implementation was never a
`TypeProvider` capability to remove, so it stays **out of scope for the ownership ledger's row structure**
— but "out of scope because it does not exist" is not the reason; "out of scope because, like the other 14,
its owner is Verter-adjacent production code with no trait method" is.

### Disposition

The ledger's acceptance claim is limited to what it proves; the later per-capability walk closes the
previously named capability residue:

- **Proven:** all 44 `TypeProvider` methods are classified, no method is unclassified, no method is
  claimed by two owners. This satisfies the charter's literal acceptance bar, which is phrased over
  methods: "an unclassified `TypeProvider` method".
- **Closed by the later per-capability walk:** the fourteen named capabilities, two partially-covered
  ones, and the cache clause now have individual verdicts; `closure-register.md` row `S3.e` owns the
  disposition. The inventory still records that 17 handled LSP requests and 18 custom methods have no
  ownership row, which is not an unanalysed residue against the `TypeProvider`-scoped acceptance bar.
  (The custom-method figure read 19 here and 18 twelve paragraphs
  above; 18 is correct — `grep -c '\.custom_method(' crates/verter_lsp/src/main.rs` returns 18, and the
  18 `$/…` method names it registers are distinct.)

**The acceptance bar against which the capability coverage was closed.** The steering scopes the inventory precisely:
*"TCM0 must inventory every method, call site, capability, and background consumer **of the current
`TypeProvider`**"* (`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md:275`), and its acceptance line is
*"TCM0 cannot be accepted until every existing `TypeProvider` capability has a complete row"* (`:370`),
with the charter's own clause reading *"unclassified `TypeProvider` methods"* (`:855`). Both qualifiers are
`TypeProvider`. The `At minimum cover:` list at `:277-311` is a checklist for that inventory — a guard
against missing something — not an assertion that all 32 named items ARE `TypeProvider` capabilities.

**Superseded in part by the walk below.** The two paragraphs that follow are the reasoning as it stood
before any path was walked; their SHAPE — a per-item verdict rather than a fabricated row — survives, and
their blanket claim "none has a provider anywhere in its request path" does NOT. One of the fourteen,
rename preparation, calls `TypeProvider::get_rename_locations` on its main path. Read
"Closure: every located capability, walked one at a time" below for the per-capability verdicts that
replace the uniform one.

For each of the 14, the correct entry against that bar is therefore not an ownership row but a recorded
verdict: **it is not a `TypeProvider` capability.** ~~None has a trait method; none has a provider anywhere
in its request path; each is served end-to-end by Verter's own code at the `file:line` given in the table
above.~~ (Struck: true of thirteen, false of rename preparation — see the walk below.) The steering's checklist is answered for all 32 items — 17 by an existing row, 14 by a
"not a `TypeProvider` capability" verdict with its real location cited, and one (`implementation`) by the
same verdict located at a different layer: `packages/typescript-plugin/src/index.ts:3095`'s carrier-routing
override, not a `TypeProvider` method and not a proven absence. That is complete coverage of the list the
steering actually asks for.

The gap is closed without fabricating rows. The per-capability walk establishes thirteen provider-free
paths and the rename-preparation exception, whose provider half is assigned to row #15; it supplies
individual verdicts instead of a uniform owner assignment for `formatting`, `callHierarchy`, or the other
located capabilities.
~~That is itself the finding: **the capability residue is uniformly `VerterNative` by construction, and it
is uniformly unaffected by TCM1-TCM4.**~~ **Struck.** That sentence was the unratified characterisation,
and the walk below shows it is wrong for one of the fourteen. What could not be done unilaterally was
assert a uniform verdict over 14 rows nobody had individually analysed — so they have now been analysed
individually, one path at a time, and the verdict is no longer uniform.

**`G-LEDGER-SCOPE`'s per-capability half is now closed by the walk below; the paragraph that follows
records why it was open and who owns whatever remains of it.** The findings above — the re-enumerated 44, the 14
located capabilities, the `implementation` re-verdict, the six corrected citations — land as evidence and
are not withdrawn. What is withdrawn is the "therefore CLOSED" verdict: this section's own text concedes a
residue TCM0 did not then ratify. **That was written when a successor block was to inherit the remainder, and
that construct does not exist** — the round limit was lifted and this block closed its own remainder under
act `4f0efc5e9`. The
per-capability walk below supplies the analysis the residue lacked. **Owner and status: see
`closure-register.md`**, which owns both; this paragraph owns neither.

## Closure: every located capability, walked one at a time

**SCOPE OF THE RATIFICATION — see the registry, which owns it.** The capability dispositions below are ratified by act **`90a25a43d`**. Its content, its scope and its limits are the registry's to state and are deliberately not repeated here: a value restated outside its owning store is a second store, and this file is not that store.

**What this file owns is the CONSEQUENCE for a reader of these rows**, which no act states: the dispositions rest on the per-capability walk below, and a later reader who changes any capability's request path, its owner cell, or the walk that derived it has changed the evidence the ratification was taken over. Such a reader must treat the disposition as reopened and obtain a fresh act — the ratification does not travel to evidence it was not taken over.

**What was withdrawn, and why.** The section above located fourteen steering-named capabilities to a
`file:line` and then characterised all fourteen at once — "none has a provider anywhere in its request
path", "the capability residue is uniformly `VerterNative` by construction". Its own closing paragraph
conceded the problem: that is "an ownership assignment for 14 rows it has not individually analysed". A
uniform verdict derived from examining none of the paths is not evidence. And a capability wrongly
excluded leaves the gap exactly where it was, so the fix is the walk, not a softer sentence.

**What replaces it.** Each capability's request path is now walked from its entry point by
`probes/capability-provider-hop-walk.mjs`; the output is `capability-provider-hop-walk.md`. The walk
indexes every non-test `fn` under `crates/` together with the type its enclosing `impl` owns, then
searches breadth-first for the shortest path to a **hop** — a call to one of the 44 methods derived from
the trait body, or a read of the `type_provider` handle. It covers the fourteen plus the three entries
this ledger recorded as only PARTIALLY covered (auto-imports' Verter-owned half, the Verter-owned
background-analysis lane, and the provider-adjacent caches), so "the provider half is the only half with
a provider on its path" is derived rather than assumed. Falsify with
`node docs/arch/refactor/rev11/evidence/TCM0/probes/capability-provider-hop-walk.mjs --check`.

**The verdicts differ per capability, which is the whole point.** The walk reports a hop for four of the
seventeen. Reading the source line each one printed, three are collisions and one is real.

| # | capability | entry point walked | walk | adjudicated | basis |
|---|---|---|---|---|---|
| 1 | rename preparation | `server/rename_prepare.rs:96` `handle_prepare_rename` | HOP (5 fns) | **HOP — REAL** | obtains the provider at `:161` and calls `TypeProvider::get_rename_locations` at `:181` |
| 2 | formatting (+ on-type) | `features/formatting.rs:23`; `server/aux_features.rs:1108`, `:1158` | NO-HOP (61) | NO-HOP | zero `type_provider`/`TypeProvider` occurrences in `features/formatting.rs` or in either 20/41-line handler body |
| 3 | call hierarchy | `features/call_hierarchy.rs:15`, `:107`, `:157`; `aux_features.rs:1213`, `:1238`, `:1263` | NO-HOP (21) | NO-HOP | zero occurrences in `features/call_hierarchy.rs` or the three handler bodies |
| 4 | code lens | `features/code_lens.rs:13`; `aux_features.rs:828` | NO-HOP (24) | NO-HOP | zero occurrences in either |
| 5 | folding | `features/folding_range.rs:14`; `aux_features.rs:90` | NO-HOP (15) | NO-HOP | zero occurrences; zero hop-bearing unresolved names |
| 6 | selection ranges | `aux_features.rs:112` (inline body) | NO-HOP (15) | NO-HOP | the 71-line handler body reads the document and its carrier blocks only |
| 7 | document symbols | `features/document_symbol.rs:16`; `aux_features.rs:40`, `:63` | NO-HOP (32) | NO-HOP | zero occurrences in either |
| 8 | component surface resolution | `server/custom_methods/component_meta.rs:110`; `verter_session/.../framework_surface/executor.rs:91` | HOP (116) | **NO-HOP — collision** | the hop is `self.scheduler.close_file(canonical_id)` at `verter_session/src/host_lifecycle.rs:986`: `Scheduler::close_file`, an inherent method sharing a name with `TypeProvider::close_file` |
| 9 | template expression typing | `documents/mod.rs:1138` `DocumentRegistry::get_ide` | NO-HOP (24) | NO-HOP | the projection is produced by `VerterHost::get_ide` under the registry's own `CompileProfile`; no engine is consulted |
| 10 | props | `features/component_diagnostics.rs:124`; `features/component_actions.rs:224` | NO-HOP (15) | NO-HOP | zero occurrences in either file |
| 11 | events | `features/event_type_hints.rs:83` | NO-HOP (7) | NO-HOP | zero occurrences in the file |
| 12 | slots and snippets | `server/component_resolve.rs:984`, `:1029` | NO-HOP (2) | NO-HOP | the file's four `type_provider` occurrences are a module import (`crate::type_provider::merge`), a comment, and the `resolve_barrel_type_provider_location` helper at `:805` — none on either slot path |
| 13 | directives | `features/hover_directive_names.rs:106` | NO-HOP (3) | NO-HOP | zero occurrences in the file |
| 14 | framework macros | `features/macro_actions.rs:97` `macro_code_actions` | HOP (1) | **NO-HOP — dead field** | the hop is `type_provider: None` at `:132`, initialising `SlotTypeContext.type_provider`, declared `pub type_provider: Option<()>, // placeholder for TypeProviderRef` at `:21` |
| 15 | auto-imports, Verter-owned half | `server/nav_features_completion_resolve.rs:53`; `features/organize_imports.rs:17` | NO-HOP (77) | NO-HOP | the provider half is row #21; the Verter-owned edit translation and organize-imports path reach no provider |
| 16 | background semantic analysis, Verter-owned lane | `documents/analysis.rs:143` `schedule_semantic_analysis` | HOP (645) | **NO-HOP — same collision as #8** | the hop is again `Scheduler::close_file` at `host_lifecycle.rs:986` |
| 17 | provider-adjacent caches | `carrier_cache.rs:86`, `:140`, `:153` | NO-HOP (3) | NO-HOP | pure hash/freshness predicates; zero unresolved receivers |

**One steering item is out of the walk's language and is settled separately.** `implementation` is served
by `packages/typescript-plugin/src/index.ts:3095`'s `getImplementationAtPosition` override, per the
correction recorded above. That is TypeScript, not `crates/`, so this walk does not cover it (limit L5);
its verdict — not a `TypeProvider` method, served at the plugin's carrier-routing layer over TypeScript's
own language service — stands on the citation above and is unchanged here.

**`implementation`'s disposition is bound by acts `15db16e7b` and `3a9ba83d9`**, and by `96773682d` which re-pins them. What they decide — survival, and the execution model — is the registry's to state and is not repeated here. This file records only what it owns: the capability is served today by the typescript-plugin carrier-routing override, that package is marked Deleted in the deletion manifest, and the legal candidate set this block derived is in `implementation-owner-candidates.md`.

### The one real finding: rename preparation is not a provider-free capability

`handle_prepare_rename` maps the carrier position to a TSX offset, waits for the configured-project
frontier, and then asks the provider:

```rust
// crates/verter_lsp/src/server/rename_prepare.rs:181
let Ok(locations) = type_provider
    .get_rename_locations(&ctx.tsx_path, tsx_offset)
    .await
```

admitting the rename only if one of the provider's locations maps back onto the authored token (`:199`
onward), after re-validating the captured surface (`:192`). Rename preparation therefore consumes a
`TypeProvider` method on its main path. **This was visible in the ledger's own citation all along** — the
located-capabilities table already reads "consumes `get_rename_locations` at `:181`" — and the uniform
verdict was asserted over it anyway. That is the concrete cost of characterising paths without walking
them, and it is why the fix had to be a walk.

Recorded narrowly:

- Rename preparation is a **`TypeProvider` capability in part**. Its provider half is row #15
  (`get_rename_locations`, owner `VerterWithTypeSemanticOracle`: a cross-region rename needs Verter's own
  script+template binding, so the reasoning that placed #15 there places the prepare gate's provider
  query there too). Its admission policy (`rename_prepare.rs:24`, `:38`), its position mapping and its
  fail-closed refusals are Verter's own and stay `VerterNative`.
- The other sixteen are answered with no engine in the request path. For those — and only those — the
  steering's checklist is answered by the verdict "not a `TypeProvider` capability", now evidenced per
  path instead of asserted over a set.

### An incidental defect, recorded rather than fixed here

`SlotTypeContext.type_provider` (`crates/verter_lsp/src/features/macro_actions.rs:21`) is `Option<()>`
with the comment `// placeholder for TypeProviderRef`, and its one construction site (`:132`) sets it to
`None`. That observed initializer does not constrain `Option<()>`, which can also hold `Some(())`; the
field is presently unused as a provider capability. It is
not a provider hop and changes no verdict above; it is named because a reader of the walk output will see
`type_provider` flagged there and is owed the reason it is not a finding. Removing it is production code,
which this block does not touch.

### What the walk cannot see, and what the verdicts therefore rest on

It over-approximates in one direction and under-approximates in the other. Both matter:

- **Over-approximation (L1).** Edges resolve by name and call shape, not by type, and the hop detector
  matches a method's spelling. Three of the four reported hops are collisions or a dead field, caught
  only by reading the line the walk printed. **A reported hop is a candidate, never a finding** — which
  is why all four were read before being counted. Three earlier revisions of this walk reported a hop for
  every capability, through `HandlerGuard::new`, `Box::new`, `tokio::spawn` and
  `crate::type_provider::specifier_rewrite` — a walk that answers HOP for everything answers nothing, and
  each of those four false rails is now closed by a resolution rule stated in the script's header.
- **Under-approximation (L2).** A call reached only through `dyn Trait` dispatch, a stored closure, or a
  macro-pasted name is not an edge, so `NO-HOP` is not a proof. Each `NO-HOP` therefore carries a second,
  independent signal in the "basis" column: the capability's own source file — or, where that file also
  serves provider-backed features, the handler body itself — contains no `type_provider` /
  `TypeProvider` occurrence at all. Two signals that fail for different reasons are what make the
  sixteen believable; neither alone would be.
- **Disclosed stopping points.** The walk records every unresolved receiver it declined to follow and
  reports how many of those names belong to functions that would themselves have been a hop. Where that
  count is non-zero it is `new` (1 of 468 definitions with that name), `spawn` or `ensure_loaded` — the
  same collision class, printed with its `K of N` so it reads as collision rather than reachability. No
  verdict here rests on an undisclosed truncation.

### The walk was proven to fail before it was believed

A two-edge provider call was planted under the `directives` entry point
(`features/hover_directive_names.rs`), verified present, unique and absent-before-planting: that
capability flipped `NO-HOP → HOP`, the report named both edges and printed the planted line, and
`--check` exited 1. After reverting, `--check` exited 0 and the verdict returned to `NO-HOP`.

## Correction, 2026-08-23: six factual attribution errors in the per-row citations

Spot-checking the table's call-site and override citations against source found six wrong. They are
corrected here rather than silently edited in the table, because one of them is load-bearing for a
pending governance ruling.

1. **Rows #25-26 — "tsgo overrides" / "tsgo-only today" is inverted.** This is the load-bearing one.
   Neither base tsgo engine overrides these methods: `crates/verter_type_runtime/src/tsgo/ipc.rs` and
   `crates/verter_type_runtime/src/tsgo/owned.rs` contain no `fn register_carrier_member`,
   `fn register_carrier_metadata`, `fn activate_carrier_member` or `fn activate_carrier_members`. The
   only substantive implementation is **tsserver-family**:
   `crates/verter_type_runtime/src/tsserver/ipc.rs:3126` (`register_carrier_member`), `:3348`
   (`register_carrier_metadata`), `:3222`/`:3271` (`activate_carrier_member(s)`). The one tsgo-side
   override, `crates/verter_lsp/src/tsgo/composite.rs:1283`/`:1298`/`:1313`, is a **pure delegation** —
   its whole body is `self.managed.register_carrier_member(...)` / `self.managed.activate_carrier_member(...)`,
   forwarding to the managed tsserver-family provider. The remaining overrides
   (`tsserver/project_router.rs:894`,`:937`,`:962`; `type_provider/lazy_managed.rs:759`,`:841`,`:878`;
   `resilient/forwarding.rs:91`,`:141`,`:165`) are routing and forwarding wrappers. The trait's own doc
   says so directly at `traits.rs:349-351`: *"`TsserverTypeProvider` overrides it to hydrate its
   `contents` cache and carrier→project map"*.

   **Consequence for the pending ruling.** The deletion rationale recorded above rests on the premise
   that these are a tsgo-lane relay artifact the content mapper's own wire identity fields subsume. The
   engine that actually implements them is tsserver, whose carrier-registration path hydrates a
   `contents` cache and a carrier→project map that the mapper's `virtualFileName`/
   `canonicalSourceFileName`/`supplementalSourceFileNames` fields do **not** obviously subsume — those
   are identity strings, not a content cache. The deletion outcome was therefore not supported by the
   reasoning as written. **This finding stands, and it is the one the ruling followed:**
   `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
   Q4 RETAINS both rows under `VerterWithTypeSemanticOracle`, gating any later removal of the
   tsserver-specific methods on TCM3 first supplying and testing equivalent semantics. The earlier
   conclusion that the ruling "must be re-requested on a corrected packet" is superseded — the ruling was
   taken and decided. See `OPEN-GAPS.md` `G-TCM0-ACCEPTANCE-ROWS-25-26`.

2. **Row #10, `provider_wire_witness` — "default (mint) + per-provider override".** There is no override
   anywhere. Every production reference is a self-call: `crates/verter_lsp/src/extension_provider.rs:392`,
   `crates/verter_type_runtime/src/tsserver/ipc.rs:3471`, `crates/verter_type_runtime/src/tsgo/ipc.rs:3234`.
   The trait doc states the reason at `traits.rs:200-203`: *"Provided (never overridden usefully — the
   witness type's field is non-public, so only this default body can produce one)"*.

3. **Row #7, `get_completion_details` — "override only in `provider_adapter.rs:63`".** That line is a
   *call site* inside `TypeProviderQueryBackend::query_members_at_offset`, not an override. There are
   eight production overrides: `extension_provider.rs:308`, `tsserver/project_router.rs:688`,
   `type_provider/lazy_managed.rs:498`, `tsgo/composite.rs:1032`, `tsgo/shared.rs:1057`,
   `tsserver/ipc.rs:3632`, `tsgo/ipc.rs:3105`, `tsgo/owned.rs:530`.

4. **Row #29, `set_project_ownership` — "default no-op + tsgo override".** No tsgo provider overrides it.
   The sole production override is `ExtensionTypeProvider` at
   `crates/verter_lsp/src/extension_provider.rs:1364` — the VS Code extension-host transport, not a tsgo
   lane. (The "sole call site `background_init.rs:271`" half is correct.) The row's `VerterNative`
   assignment is unaffected; its stated premise is not.

5. **Rows #23, #27, #28 — "tsgo-only today".** All three are also overridden outside the tsgo lane:
   `configure_paths` by `extension_provider.rs:1294`; `resync_open_files` by `extension_provider.rs:1389`,
   `tsserver/project_router.rs:980`, `tsserver/ipc.rs:4817`; `update_workspace_folders` by
   `extension_provider.rs:1331`, `tsserver/ipc.rs:4781`, `tsserver/project_router.rs:990`.

6. **Row #11, `get_diagnostics` — two of five cited callers are a different method.**
   `crates/verter_lsp/src/documents/mod.rs:1404` sits inside `DocumentRegistry::get_diagnostics`
   (defined `:1401`) and calls the two-argument `VerterHost::get_diagnostics(&canonical_id,
   &self.tsx_profile.read())` — Verter's own native diagnostics, not the one-argument trait method.
   `crates/verter_lsp/src/server_utils.rs:1705` calls that same `DocumentRegistry` method. The genuine
   trait call sites are `sync_orchestration.rs:111`, `sync_coordinator.rs:1322` and `:1565`.

None of these six changes any row's assigned OWNER except by removing a premise; rows #25-26 are the one
case where the premise was the rationale. The corrections are recorded here, in TCM0's own evidence,
rather than by rewriting the table's cells, so a reviewer can see what the ledger originally asserted and
what the source actually says.

## Correction, 2026-08-23: row #15's wire-mask cells are superseded — `Rename` is CLEARED, not masked

Row #15's "Required TS capability" and "Mapping class/mask" cells read `SpanMapFeature.Rename` and
"mask `Rename`", which a TCM2 implementer would read as an instruction to SET that bit on a mapper
segment. `projection-class-contract.md`'s terminal policy clears `Rename` from every wire mask
(`OWNER_WIRE_ELIGIBLE` = 13535, which excludes bit 256). Both cannot be right.

The contract is the correct side, and row #15's own owner column is the reason: rename is assigned to
`VerterWithTypeSemanticOracle` because *"a pure `TypeScriptLspDirect` answer would miss the template-side
occurrences"*. Setting `SpanMapFeature.Rename` on a mapper segment would invite TypeScript's own language
service to answer a rename Verter owns, producing exactly the partial, script-only edit that reason
predicts — and that the Project-Bound External-TS Contract's rename fail-closed rule (`CLAUDE.md`) exists
to prevent.

Row #15's capability/mask cells are therefore struck: they are a stale artifact of a draft written before
the mask contract existed. The row's OWNER is unchanged and remains correct. All eleven mask-bearing rows
were re-checked against the terminal policy; #15 is the only conflict.
