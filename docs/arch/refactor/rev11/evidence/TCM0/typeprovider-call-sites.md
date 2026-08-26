# Derived: every `TypeProvider` call site in `crates/`

**This file is generated. Do not edit it.** Regenerate with `node docs/arch/refactor/rev11/evidence/TCM0/probes/typeprovider-call-site-derivation.mjs`; falsify with the same command and `--check`, which re-derives from the live tree and exits 1 on any drift.

The method list is read out of the `TypeProvider` trait body in `crates/verter_type_runtime/src/traits.rs` — it is never typed into the generator. The occurrence set is a lex over every `.rs` file under `crates/`. Both counts below are build outputs.

## What this derivation cannot see

Read these before reading the counts; the generator's own header carries the same list as L1-L5.

- **Name collisions (L1).** `shutdown`, `child_pid`, `close_file` and friends are ordinary identifiers. A `.shutdown()` on something that is not a provider is textually identical to one that is. Every row carries its enclosing item and a receiver snippet so a reader can adjudicate. **The counts can include textual collisions and omit renamed or macro-synthesised calls, so they are not a guaranteed upper or lower bound.**
- **Generic parameters (L2).** A call through `fn f<P: TypeProvider>(p: &P)` is found by name but the bound is not proven here.
- **Dynamic dispatch is covered; a rename would not be (L3).** `dyn TypeProvider` reaches a method by its own spelling, so every `dyn` call site appears, and the `verter_lsp` re-export of the trait renames nothing. A call reached under a DIFFERENT name — a renamed `use ... as`, a renaming adapter, a macro-synthesised identifier — would be invisible. None exists today; this derivation cannot prove that, only that no occurrence of the trait's own spelling was missed.
- **Macro-pasted identifiers (L4).** A name built by token pasting carries no matchable text.
- **`crates/` only (L5).** The TypeScript packages are out of the trait's language and are not scanned.

## Universe

- trait methods derived from the trait body: **44**
- `.rs` files under `crates/` walked: **3130**
- classified occurrences: **2551**

| class | count |
|---|---|
| `trait-declaration` | 44 |
| `impl-production` | 328 |
| `impl-test` | 286 |
| `call-production` | 203 |
| `call-forwarding` | 178 |
| `call-trait-default` | 14 |
| `call-test` | 558 |
| `ref-production` | 55 |
| `ref-test` | 41 |
| `doc-comment` | 395 |
| `comment` | 200 |
| `string-literal` | 249 |

## Per-method counts

`call-forwarding` is a call from inside a `fn` of the same name — a delegating wrapper passing the call down, not an independent consumer. It is separated because a method whose only non-test callers are forwarders has no live consumer at all.

| # | method | trait decl | impl prod | impl test | call prod | call fwd | call dflt | call test | ref prod | ref test | doc | comment | string |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `provider_id` | 1 | 9 | 15 | 6 | 2 | 0 | 3 | 28 | 32 | 11 | 1 | 10 |
| 2 | `supports_completion_resolve` | 1 | 9 | 1 | 1 | 3 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| 3 | `open_file` | 1 | 9 | 15 | 12 | 3 | 2 | 100 | 0 | 0 | 38 | 13 | 30 |
| 4 | `load_file` | 1 | 10 | 15 | 14 | 4 | 2 | 14 | 0 | 0 | 25 | 3 | 12 |
| 5 | `update_file` | 1 | 9 | 15 | 7 | 3 | 2 | 17 | 0 | 0 | 33 | 18 | 12 |
| 6 | `close_file` | 1 | 13 | 15 | 17 | 5 | 2 | 11 | 0 | 0 | 22 | 8 | 7 |
| 7 | `get_completions` | 1 | 9 | 15 | 6 | 6 | 0 | 23 | 0 | 0 | 9 | 6 | 7 |
| 8 | `get_completion_details` | 1 | 8 | 2 | 1 | 5 | 0 | 7 | 0 | 0 | 13 | 6 | 4 |
| 9 | `get_hover` | 1 | 9 | 15 | 11 | 6 | 0 | 56 | 0 | 0 | 16 | 9 | 10 |
| 10 | `provider_wire_witness` | 1 | 0 | 0 | 3 | 0 | 0 | 4 | 0 | 0 | 2 | 0 | 0 |
| 11 | `get_diagnostics` | 1 | 11 | 15 | 8 | 5 | 1 | 42 | 0 | 0 | 22 | 13 | 8 |
| 12 | `get_definition` | 1 | 9 | 15 | 6 | 6 | 0 | 12 | 0 | 0 | 13 | 11 | 8 |
| 13 | `get_type_definition` | 1 | 9 | 15 | 3 | 6 | 0 | 6 | 0 | 0 | 3 | 1 | 3 |
| 14 | `get_references` | 1 | 9 | 15 | 3 | 6 | 0 | 5 | 0 | 0 | 3 | 0 | 2 |
| 15 | `get_rename_locations` | 1 | 9 | 15 | 2 | 6 | 0 | 5 | 0 | 0 | 5 | 4 | 1 |
| 16 | `get_signature_help` | 1 | 9 | 15 | 2 | 6 | 0 | 5 | 0 | 0 | 3 | 0 | 2 |
| 17 | `get_code_actions` | 1 | 11 | 15 | 1 | 6 | 0 | 8 | 0 | 0 | 6 | 4 | 3 |
| 18 | `get_semantic_tokens` | 1 | 9 | 15 | 1 | 6 | 0 | 11 | 0 | 0 | 3 | 5 | 2 |
| 19 | `get_document_highlights` | 1 | 9 | 15 | 2 | 6 | 0 | 5 | 0 | 0 | 1 | 0 | 1 |
| 20 | `get_inlay_hints` | 1 | 9 | 15 | 2 | 6 | 0 | 6 | 0 | 0 | 1 | 0 | 0 |
| 21 | `resolve_completion` | 1 | 9 | 3 | 2 | 6 | 0 | 8 | 0 | 0 | 17 | 10 | 10 |
| 22 | `shutdown` | 1 | 19 | 6 | 16 | 11 | 0 | 163 | 22 | 9 | 104 | 62 | 87 |
| 23 | `configure_paths` | 1 | 5 | 6 | 5 | 2 | 1 | 7 | 0 | 0 | 19 | 17 | 25 |
| 24 | `notify_carrier_changed` | 1 | 5 | 1 | 1 | 4 | 1 | 0 | 0 | 0 | 4 | 0 | 0 |
| 25 | `notify_carriers_changed` | 1 | 4 | 1 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 26 | `register_carrier_member` | 1 | 6 | 2 | 3 | 4 | 1 | 13 | 0 | 0 | 7 | 4 | 2 |
| 27 | `register_carrier_metadata` | 1 | 4 | 1 | 4 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 28 | `activate_carrier_member` | 1 | 5 | 1 | 2 | 3 | 1 | 2 | 0 | 0 | 0 | 0 | 0 |
| 29 | `activate_carrier_members` | 1 | 5 | 1 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 30 | `resync_open_files` | 1 | 7 | 0 | 2 | 5 | 0 | 2 | 3 | 0 | 10 | 2 | 0 |
| 31 | `update_workspace_folders` | 1 | 8 | 3 | 7 | 3 | 1 | 8 | 0 | 0 | 0 | 0 | 1 |
| 32 | `set_project_ownership` | 1 | 1 | 1 | 1 | 0 | 0 | 4 | 0 | 0 | 0 | 0 | 0 |
| 33 | `child_pid` | 1 | 7 | 0 | 4 | 5 | 0 | 8 | 2 | 0 | 3 | 2 | 1 |
| 34 | `open_file_background` | 1 | 6 | 1 | 5 | 3 | 0 | 0 | 0 | 0 | 1 | 0 | 1 |
| 35 | `load_file_background` | 1 | 7 | 0 | 5 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 36 | `update_file_background` | 1 | 6 | 0 | 5 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 37 | `close_file_background` | 1 | 7 | 0 | 5 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 38 | `get_diagnostics_background` | 1 | 6 | 1 | 1 | 4 | 0 | 2 | 0 | 0 | 0 | 1 | 0 |
| 39 | `configure_paths_background` | 1 | 3 | 0 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 0 |
| 40 | `update_workspace_folders_background` | 1 | 5 | 0 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 41 | `open_file_normal` | 1 | 6 | 0 | 5 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 42 | `load_file_normal` | 1 | 6 | 0 | 5 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 43 | `update_file_normal` | 1 | 6 | 0 | 4 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 44 | `close_file_normal` | 1 | 6 | 0 | 5 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Every occurrence

Grouped by method, then by class, then by path. `context` is the enclosing `fn`/`impl`/`trait` header; `snippet` is the source text immediately preceding the occurrence on its line.

### `provider_id` — declared at `crates/verter_type_runtime/src/traits.rs:137`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:137` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:111` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:854` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:983` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:626` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:440` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:10` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2897` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:403` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2800` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:79` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2999` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:219` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:343` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:503` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:765` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:955` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:760` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:913` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:144` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1075` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:39` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:606` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:153` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:25` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/nav_features.rs:643` | `: &VerterLanguageServer, params: &CompletionParams, native_only: bool, ) -> Result<Option<CompletionResponse>>` | `let provider_id = tp.` |
| `crates/verter_lsp/src/server/nav_features.rs:1275` | `: &VerterLanguageServer, params: &CompletionParams, native_only: bool, ) -> Result<Option<CompletionResponse>>` | `tp.` |
| `crates/verter_lsp/src/server/nav_features.rs:1341` | `andle_completion_resolve( server: &VerterLanguageServer, mut item: CompletionItem, ) -> Result<CompletionItem>` | `if envelope_provider_id != tp.` |
| `crates/verter_lsp/src/server/nav_features.rs:1346` | `andle_completion_resolve( server: &VerterLanguageServer, mut item: CompletionItem, ) -> Result<CompletionItem>` | `tp.` |
| `crates/verter_lsp/src/tsgo/composite.rs:847` | `fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result` | `.field("managed", &self.managed.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:220` | `async fn activate(&self) -> Result<Arc<dyn TypeProvider>, TypeProviderError>` | `provider = provider.` |

**`call-forwarding`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:858` | `fn provider_id(&self) -> &'static str` | `self.managed.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:18` | `fn provider_id(&self) -> &'static str` | `(\|guard\| guard.as_ref().map(\|provider\| provider.` |

**`call-test`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/real_provider_tests/completion.rs:892` | `real_provider_test!( completion_carrier_fields_through_provider, fixture = , async fn run(session)` | `match session.provider().` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3000` | `fn provider_id(&self) -> &'static str` | `self.inner.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:106` | `(flavor = , worker_threads = 2)] async fn owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process()` | `assert_eq!(provider.` |

**`ref-production`** (28)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/nav_features.rs:643` | `: &VerterLanguageServer, params: &CompletionParams, native_only: bool, ) -> Result<Option<CompletionResponse>>` | `let` |
| `crates/verter_lsp/src/server/nav_features.rs:656` | `: &VerterLanguageServer, params: &CompletionParams, native_only: bool, ) -> Result<Option<CompletionResponse>>` | `` |
| `crates/verter_lsp/src/server_utils.rs:954` | `` | `let key = (result.source_id.clone(), result.` |
| `crates/verter_lsp/src/server_utils.rs:979` | `` | `let key = (result.source_id.clone(), result.` |
| `crates/verter_lsp/src/server_utils.rs:1032` | `` | `let key = (result.source_id.clone(), result.` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:136` | `` | `` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:150` | `a: &protocol::CompletionResolveData, provider_id: &str, tsx_path: Option<&str>, ) -> Option<serde_json::Value>` | `"provider_id":` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:173` | `` | `` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:179` | ` String, text_edit: Option<CompletionTextEdit>, provider_id: &str, tsx_path: Option<&str>, ) -> CompletionItem` | `.and_then(\|d\| mint_resolve_envelope(d,` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:252` | `` | `` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:312` | `ndex, tsx_path: Option<&str>, provider_id: &str, template_attr_context: bool, ) -> (Vec<CompletionItem>, bool)` | `.and_then(\|d\| mint_resolve_envelope(d,` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:348` | `ndex, tsx_path: Option<&str>, provider_id: &str, template_attr_context: bool, ) -> (Vec<CompletionItem>, bool)` | `` |
| `crates/verter_session/src/framework/script_facts.rs:455` | `` | `pub` |
| `crates/verter_session/src/framework/script_facts.rs:520` | `` | `pub` |
| `crates/verter_session/src/framework/script_facts.rs:953` | `&FrameworkRegistration, canonical: &str, request_ctx: Option<&dyn ResolverContext>, ) -> ScriptFactEvidence<T>` | `` |
| `crates/verter_session/src/framework/script_facts.rs:1077` | `&FrameworkRegistration, canonical: &str, request_ctx: Option<&dyn ResolverContext>, ) -> ScriptFactEvidence<T>` | `` |
| `crates/verter_workspace/src/engine.rs:3346` | `impl Engine` | `` |
| `crates/verter_workspace/src/engine.rs:3969` | `r, ctx: crate::types::ResolutionContext, captured_world: &Arc<CapturedResolutionWorld>, ) -> ResolutionOutcome` | `` |
| `crates/verter_workspace/src/resolution_currency.rs:192` | `pub(crate) fn with_external_provider_projection( mut self, result: &crate::types::ResolveResult, ) -> Self` | `+ result.` |
| `crates/verter_workspace/src/resolution_currency.rs:198` | `pub(crate) fn with_external_provider_projection( mut self, result: &crate::types::ResolveResult, ) -> Self` | `write_field(&mut identity, &result.` |
| `crates/verter_workspace/src/resolver.rs:280` | `impl ProjectResolver` | `pub fn source_id_from_provider_id(&self,` |
| `crates/verter_workspace/src/resolver.rs:281` | `pub fn source_id_from_provider_id(&self, provider_id: &str) -> Option<String>` | `let normalized = normalize_canonical_id(` |
| `crates/verter_workspace/src/resolver.rs:421` | `esult( &self, request: &ResolveRequest, source_id: String, resolution_kind: ResolutionKind, ) -> ResolveResult` | `let` |
| `crates/verter_workspace/src/resolver.rs:442` | `esult( &self, request: &ResolveRequest, source_id: String, resolution_kind: ResolutionKind, ) -> ResolveResult` | `relative_specifier(&importer_provider_id, &` |
| `crates/verter_workspace/src/resolver.rs:458` | `esult( &self, request: &ResolveRequest, source_id: String, resolution_kind: ResolutionKind, ) -> ResolveResult` | `` |
| `crates/verter_workspace/src/resolver.rs:472` | `resolve_result( &self, specifier: &str, source_id: String, resolution_kind: ResolutionKind, ) -> ResolveResult` | `let` |
| `crates/verter_workspace/src/resolver.rs:489` | `resolve_result( &self, specifier: &str, source_id: String, resolution_kind: ResolutionKind, ) -> ResolveResult` | `` |
| `crates/verter_workspace/src/types.rs:76` | `` | `pub` |

**`ref-test`** (32)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:1953` | `&str, specifier: &str, _ctx: verter_workspace::ResolutionContext, ) -> Option<verter_workspace::ResolveResult>` | `` |
| `crates/verter_lsp/src/server_tests.rs:21246` | `` | `` |
| `crates/verter_lsp/src/server_tests.rs:21264` | ` tsserver_resolve_envelope_item( provider_id: &str, provider_path: &str, entry_name: &str, ) -> CompletionItem` | `"provider_id":` |
| `crates/verter_lsp/src/server_tests.rs:21280` | `` | `` |
| `crates/verter_lsp/src/server_tests.rs:21295` | `] fn tsgo_resolve_envelope_item( provider_id: &str, provider_path: &str, entry_name: &str, ) -> CompletionItem` | `"provider_id":` |
| `crates/verter_lsp/src/type_provider/mock.rs:222` | `` | `` |
| `crates/verter_lsp/src/type_provider/mock.rs:538` | `impl MockTypeProvider` | `pub fn set_provider_id(&self,` |
| `crates/verter_lsp/src/type_provider/mock.rs:539` | `pub fn set_provider_id(&self, provider_id: &'static str)` | `self.state.lock().unwrap().` |
| `crates/verter_lsp/src/type_provider/mock.rs:539` | `pub fn set_provider_id(&self, provider_id: &'static str)` | `self.state.lock().unwrap().provider_id = Some(` |
| `crates/verter_lsp/src/type_provider/mock.rs:914` | `fn provider_id(&self) -> &'static str` | `self.state.lock().unwrap().` |
| `crates/verter_session/src/framework/script_facts_tests.rs:39` | `` | `` |
| `crates/verter_session/src/framework/script_facts_tests.rs:145` | `fn resolved_fact_key(canonical: &str) -> ResolvedFactKey` | `` |
| `crates/verter_workspace/src/resolver_tests.rs:673` | `#[test] fn provider_id_uses_original_path_for_non_vue()` | `let` |
| `crates/verter_workspace/src/resolver_tests.rs:678` | `#[test] fn provider_id_uses_original_path_for_non_vue()` | `` |
| `crates/verter_workspace/src/resolver_tests.rs:692` | `#[test] fn provider_paths_keep_vue_as_public_api_targets()` | `let` |
| `crates/verter_workspace/src/resolver_tests.rs:697` | `#[test] fn provider_paths_keep_vue_as_public_api_targets()` | `` |
| `crates/verter_workspace/src/resolver_tests.rs:701` | `#[test] fn provider_paths_keep_vue_as_public_api_targets()` | `!` |
| `crates/verter_workspace/src/resolver_tests.rs:715` | `#[test] fn provider_ide_id_appends_tsx_to_vue()` | `let` |
| `crates/verter_workspace/src/resolver_tests.rs:720` | `#[test] fn provider_ide_id_appends_tsx_to_vue()` | `` |
| `crates/verter_workspace/src/resolver_tests.rs:724` | `#[test] fn provider_ide_id_appends_tsx_to_vue()` | `resolver.source_id_from_provider_id(&` |
| `crates/verter_workspace/src/resolver_tests.rs:729` | `#[test] fn provider_ide_id_appends_tsx_to_vue()` | `Some(` |
| `crates/verter_workspace/src/resolver_tests.rs:926` | `#[test] fn resolve_relative_vue_import_returns_real_source_and_provider_api()` | `resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:928` | `#[test] fn resolve_relative_vue_import_returns_real_source_and_provider_api()` | `resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:964` | `#[test] fn resolve_workspace_alias_rewrites_to_shadow_provider_file()` | `resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:966` | `#[test] fn resolve_workspace_alias_rewrites_to_shadow_provider_file()` | `resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:1007` | `#[test] fn resolve_tsconfig_paths_before_base_url()` | `resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:1077` | `#[test] fn resolve_relative_paths_use_realpath_normalization()` | `resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:1079` | `#[test] fn resolve_relative_paths_use_realpath_normalization()` | `resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:1133` | `#[test] fn resolve_project_references_after_local_tsconfig_options()` | `assert_eq!(resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:1411` | `#[test] fn resolve_package_exports_prefers_types_for_root_imports()` | `assert_eq!(resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:1639` | `#[test] fn resolve_relative_unowned_to_owned_target()` | `resolved.` |
| `crates/verter_workspace/src/resolver_tests.rs:1641` | `#[test] fn resolve_relative_unowned_to_owned_target()` | `resolved.` |

**`doc-comment`** (11)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:21351` | `` | `/// Discriminating: the envelope says '` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:122` | `` | `/// 'completionItem/resolve' validates '` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:233` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:233` | `` | `er_id' is the active provider's ['TypeProvider::` |
| `crates/verter_lsp/src/type_provider/mock.rs:219` | `` | `ovider identity reported by ['MockTypeProvider::` |
| `crates/verter_lsp/src/type_provider/mock.rs:532` | `impl MockTypeProvider` | `/// Override the provider identity reported by '` |
| `crates/verter_session/src/framework/script_facts.rs:17` | `` | `//! 'file_language_id') plus '(` |
| `crates/verter_session/src/framework/script_facts.rs:21` | `` | `//! '(canonical,` |
| `crates/verter_type_runtime/src/protocol.rs:173` | `` | `/// '` |
| `crates/verter_workspace/src/resolver.rs:409` | `impl ProjectResolver` | `/// '` |
| `crates/verter_workspace/src/resolver_tests.rs:1609` | `` | `result should carry the correct owner metadata (` |

**`comment`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:27200` | `#[tokio::test] async fn virtual_file_completion_routes_actionable_handle_through_envelope()` | `// tsserver-kind mock so the envelope's '` |

**`string-literal`** (10)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/nav_features.rs:1334` | `andle_completion_resolve( server: &VerterLanguageServer, mut item: CompletionItem, ) -> Result<CompletionItem>` | `envelope.get("` |
| `crates/verter_lsp/src/server_tests.rs:21264` | ` tsserver_resolve_envelope_item( provider_id: &str, provider_path: &str, entry_name: &str, ) -> CompletionItem` | `"` |
| `crates/verter_lsp/src/server_tests.rs:21295` | `] fn tsgo_resolve_envelope_item( provider_id: &str, provider_path: &str, entry_name: &str, ) -> CompletionItem` | `"` |
| `crates/verter_lsp/src/server_tests.rs:27396` | `#[tokio::test] async fn virtual_file_completion_routes_actionable_handle_through_envelope()` | `envelope.get("` |
| `crates/verter_lsp/src/type_provider/merge/completion.rs:150` | `a: &protocol::CompletionResolveData, provider_id: &str, tsx_path: Option<&str>, ) -> Option<serde_json::Value>` | `"` |
| `crates/verter_lsp/src/type_provider/merge/tests.rs:546` | `#[test] fn merge_completions_emits_neutral_resolve_envelope()` | `envelope.get("` |
| `crates/verter_lsp/src/type_provider/merge/tests.rs:696` | `#[test] fn merge_completions_dedupe_preserves_import_capable_handle()` | `envelope.get("` |
| `crates/verter_lsp/tests/cases/carrier_routing_no_vue_gate.rs:342` | `fn is_vue_provider_path_builder(code: &str) -> bool` | `\|\| code.contains("` |
| `crates/verter_workspace/src/resolver_tests.rs:698` | `#[test] fn provider_paths_keep_vue_as_public_api_targets()` | `solve to .vue.verter.ts in the provider graph: {` |
| `crates/verter_workspace/src/resolver_tests.rs:1640` | `#[test] fn resolve_relative_unowned_to_owned_target()` | `"` |

### `supports_completion_resolve` — declared at `crates/verter_type_runtime/src/traits.rs:145`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:145` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:115` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:861` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:989` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:630` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:444` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:22` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2901` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:412` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2804` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:917` | `impl TypeProvider for MockTypeProvider` | `fn` |

**`call-production`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/lifecycle.rs:231` | `c fn handle_initialize( server: &VerterLanguageServer, params: InitializeParams, ) -> Result<InitializeResult>` | `.is_some_and(\|tp\| tp.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:862` | `fn supports_completion_resolve(&self) -> bool` | `self.managed.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:30` | `fn supports_completion_resolve(&self) -> bool` | `.map(\|provider\| provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:413` | `fn supports_completion_resolve(&self) -> bool` | `self.lsp.` |

**`doc-comment`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:3284` | `` | `/// active provider's '` |

### `open_file` — declared at `crates/verter_type_runtime/src/traits.rs:150`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:150` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:119` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:871` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:996` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:634` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:448` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:35` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2908` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:421` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2808` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:83` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3021` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:223` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:347` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:507` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:769` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:959` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:764` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:921` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:147` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1078` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:43` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:610` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:157` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:29` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (12)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:339` | `async fn apply_files( &self, files: &[BaselineFile], applied: &[AppliedSync], ) -> Result<(), Response>` | `SyncAction::Opened => provider.` |
| `crates/verter_lsp/src/tsgo/overlay_core.rs:79` | `fn inject(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1007` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:274` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:341` | `tent: String, access: Option<FileAccess>, lane: PriorityLane, update: bool, ) -> Result<(), TypeProviderError>` | `FileAccess::Open => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:429` | `pub async fn open_dts( &self, dts_path: &str, dts_content: &str, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:134` | `elf, path: &str, content: &str, lane: ProviderLane, verb: ProviderFileVerb, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_session/src/typeinfo/oracle_core/gen.rs:718` | ` tsgo_bin: &str, files: &[(String, String)], hover_rel: &str, hover_offset: u32, ) -> Result<String, GenError>` | `let _ = provider.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:196` | `n ensure_definition_file_loaded( &self, query_path: &str, definition_path: &str, ) -> Result<(), BackendError>` | `.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:271` | `_file<'a>( &'a self, file_id: &'a GeneratedFileId, revision: u64, content: &'a str, ) -> BackendFuture<'a, ()>` | `.` |
| `crates/verter_type_runtime/src/resilient.rs:838` | `async fn replay_into<P: TypeProvider>(&self, provider: &P, log_name: &str)` | `CachedFileMode::Open => provider.` |
| `crates/verter_type_runtime/src/resilient.rs:886` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Foreground => provider.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:875` | `fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:640` | `fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:426` | `fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `lsp.` |

**`call-trait-default`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:460` | `fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_type_runtime/src/traits.rs:498` | `fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |

**`call-test`** (100)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:1066` | `#[tokio::test] async fn known_good_script_setup_hover_resolves_through_tsgo_on_emitted_tsx()` | `.` |
| `crates/verter_dx_baseline/src/main_tests.rs:1187` | `#[tokio::test] #[ignore = ] async fn provider_resolves_barrel_reexport_through_rewritten_twin()` | `.` |
| `crates/verter_dx_baseline/src/main_tests.rs:1191` | `#[tokio::test] #[ignore = ] async fn provider_resolves_barrel_reexport_through_rewritten_twin()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:311` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:562` | `#[tokio::test] async fn extension_provider_get_code_actions_surfaces_single_and_combined_unused_fix()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:710` | `#[tokio::test] async fn extension_provider_combined_fix_uses_content_current_as_of_each_response()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:816` | `#[tokio::test] async fn open_stamps_the_owning_package_root_not_the_workspace_folder()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:840` | `#[tokio::test] async fn update_open_stamps_the_owning_package_root_too()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:870` | `#[tokio::test] async fn a_file_outside_every_nested_package_still_stamps_the_root_project()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:897` | `#[tokio::test] async fn without_an_ownership_authority_the_workspace_folder_is_the_last_resort()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:932` | `#[tokio::test] async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:999` | `#[tokio::test] async fn open_declares_the_owning_projects_config_file_alongside_its_root()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1024` | `#[tokio::test] async fn open_declares_a_jsconfig_owned_project_by_its_own_config_name()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1054` | `#[tokio::test] async fn update_open_reopen_declares_the_config_too()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1098` | `#[tokio::test] async fn without_an_ownership_authority_no_config_is_invented()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1189` | `#[tokio::test] async fn resync_rebinds_a_file_opened_before_the_ownership_authority_landed()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1251` | `#[tokio::test] async fn resync_closes_a_file_no_configured_project_owns_instead_of_rebinding_it()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1285` | `#[tokio::test] async fn an_authoritatively_unowned_file_fails_closed_rather_than_binding_an_invented_project()` | `let result = provider.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1320` | `#[tokio::test] async fn completion_details_propagate_a_refusal_instead_of_returning_the_previous_items()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1377` | `#[tokio::test] async fn semantic_tokens_decode_2020_and_remap_into_verter_legend_space()` | `provider.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1441` | `#[tokio::test] async fn semantic_tokens_drop_unmappable_classifications_instead_of_guessing()` | `provider.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1476` | `#[tokio::test] async fn inlay_hints_use_absolute_utf16_request_offsets_and_return_byte_positions()` | `provider.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:222` | `#[tokio::test] async fn restart_replays_cached_state_without_downgrading_loaded_files()` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3026` | `fn open_file( &self, path: &str, content: &str, ) -> crate::type_provider::traits::ProviderFuture<'_, ()>` | `self.inner.` |
| `crates/verter_lsp/src/server_tests.rs:229` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/src/server_tests.rs:353` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/src/server_tests.rs:513` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/src/server_tests.rs:793` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/src/server_tests.rs:974` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/src/test_harness.rs:815` | `pub(crate) async fn open_in_provider(&self, relative_path: &str, content: &str) -> String` | `.` |
| `crates/verter_lsp/src/test_harness.rs:844` | `pub(crate) async fn open_fixture_in_provider(&self, relative_path: &str) -> (String, String)` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:149` | `#[tokio::test] async fn lifecycle_is_cached_without_activation_and_replayed_once_on_first_query()` | `.` |
| `crates/verter_lsp/src/type_provider/mock.rs:771` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:152` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/tests/cases/quoted_prop_consumer_mistype_live.rs:256` | `#[tokio::test] async fn quoted_prop_consumer_mistype_surfaces_ts2322_tsserver()` | `.` |
| `crates/verter_lsp/tests/cases/quoted_prop_consumer_mistype_live.rs:261` | `#[tokio::test] async fn quoted_prop_consumer_mistype_surfaces_ts2322_tsserver()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:345` | `#[tokio::test] async fn test_e2e_tsserver_scoped_slot_types_from_generated_vue_outputs()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:349` | `#[tokio::test] async fn test_e2e_tsserver_scoped_slot_types_from_generated_vue_outputs()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:529` | `#[ignore = ] #[tokio::test] async fn test_e2e_tsserver_scoped_slot_types_with_in_memory_child_api()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:533` | `#[ignore = ] #[tokio::test] async fn test_e2e_tsserver_scoped_slot_types_with_in_memory_child_api()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:687` | `#[ignore = ] #[tokio::test] async fn test_e2e_tsserver_scoped_slot_types_with_plugin_and_open_child_ide()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:694` | `#[ignore = ] #[tokio::test] async fn test_e2e_tsserver_scoped_slot_types_with_plugin_and_open_child_ide()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:701` | `#[ignore = ] #[tokio::test] async fn test_e2e_tsserver_scoped_slot_types_with_plugin_and_open_child_ide()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:708` | `#[ignore = ] #[tokio::test] async fn test_e2e_tsserver_scoped_slot_types_with_plugin_and_open_child_ide()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:799` | `#[tokio::test] async fn test_e2e_tsserver_vfor_member_access_from_fixture_generated_vue_output()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:885` | `#[tokio::test] async fn test_e2e_tsserver_semantic_tokens_map_to_verter_legend()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:728` | `#[tokio::test(flavor = , worker_threads = 4)] async fn shared_provider_serves_real_vue_macro_carrier()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:732` | `#[tokio::test(flavor = , worker_threads = 4)] async fn shared_provider_serves_real_vue_macro_carrier()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:835` | `::test(flavor = , worker_threads = 4)] async fn shared_provider_serves_dual_claimant_carrier_with_real_types()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:839` | `::test(flavor = , worker_threads = 4)] async fn shared_provider_serves_dual_claimant_carrier_with_real_types()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:920` | `#[tokio::test(flavor = , worker_threads = 4)] async fn shared_provider_carrier_never_leaks_to_editor()` | `h.provider.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:921` | `#[tokio::test(flavor = , worker_threads = 4)] async fn shared_provider_carrier_never_leaks_to_editor()` | `h.provider.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1083` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1553` | `tokio::test(flavor = , worker_threads = 4)] async fn composite_overlays_shared_diagnostics_via_live_resolver()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1557` | `tokio::test(flavor = , worker_threads = 4)] async fn composite_overlays_shared_diagnostics_via_live_resolver()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1653` | `(flavor = , worker_threads = 4)] async fn composite_successful_shared_route_never_activates_managed_fallback()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1657` | `(flavor = , worker_threads = 4)] async fn composite_successful_shared_route_never_activates_managed_fallback()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1758` | `#[tokio::test] async fn composite_attach_failure_activates_managed_fallback_exactly_once()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1761` | `#[tokio::test] async fn composite_attach_failure_activates_managed_fallback_exactly_once()` | `composite.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:2001` | `async fn assert_template_member_served_typed(h: &CompositeHarness, topology: &str)` | `.` |
| `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_gen_spike.rs:134` | `spawn_with( tsconfig: &str, files: &[(&str, &str)], ) -> Option<(TsgoTypeProvider, String, tempfile::TempDir)>` | `let _ = provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:394` | `#[tokio::test] async fn a_respawned_provider_is_announced_structurally()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:960` | `pub(crate) async fn new(failures_before_success: usize) -> Self` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1128` | `#[tokio::test(start_paused = true)] async fn removed_carrier_is_absent_from_restart_replay()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1131` | `#[tokio::test(start_paused = true)] async fn removed_carrier_is_absent_from_restart_replay()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1165` | `#[tokio::test(start_paused = true)] async fn mid_restart_update_replays_current_not_stale_bytes()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1199` | `#[tokio::test(start_paused = true)] async fn restart_replay_equals_desired_membership_set()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1200` | `#[tokio::test(start_paused = true)] async fn restart_replay_equals_desired_membership_set()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1206` | `#[tokio::test(start_paused = true)] async fn restart_replay_equals_desired_membership_set()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1245` | `#[tokio::test(start_paused = true)] async fn mutation_racing_respawn_reaches_fresh_inner()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1420` | `#[tokio::test(start_paused = true)] async fn restart_replays_state_without_downgrading_loaded_files()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1481` | `#[tokio::test(start_paused = true)] async fn open_forwards_to_the_live_provider()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1836` | `tokio::test(start_paused = true)] async fn deliberate_shutdown_is_not_reported_as_a_crash_and_never_respawns()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1875` | `::test(start_paused = true)] async fn killer_request_is_quarantined_and_never_replayed_into_restarted_engine()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1914` | `#[tokio::test(start_paused = true)] async fn quarantine_clears_when_the_file_content_changes()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1959` | `#[tokio::test(start_paused = true)] async fn repeated_killer_request_does_not_burn_the_restart_budget()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:1217` | `#[tokio::test] async fn test_tsgo_hover_on_ts_file()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:1265` | `#[tokio::test] async fn test_tsgo_survives_workspace_configuration()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3156` | `#[tokio::test] async fn test_provider_operations_fail_after_process_death()` | `tokio::time::timeout(timeout, provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3763` | `#[tokio::test] async fn e2e_concurrent_requests_complete_without_deadlock()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4717` | `#[tokio::test] async fn a_refused_didopen_leaves_no_synced_entry_in_the_local_ledger()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4725` | `#[tokio::test] async fn a_refused_didopen_leaves_no_synced_entry_in_the_local_ledger()` | `if provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4950` | `#[tokio::test] async fn the_ledger_opens_once_changes_on_edit_and_reopens_only_after_a_close()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4957` | `#[tokio::test] async fn the_ledger_opens_once_changes_on_edit_and_reopens_only_after_a_close()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4958` | `#[tokio::test] async fn the_ledger_opens_once_changes_on_edit_and_reopens_only_after_a_close()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4966` | `#[tokio::test] async fn the_ledger_opens_once_changes_on_edit_and_reopens_only_after_a_close()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4990` | `#[tokio::test] async fn the_ledger_opens_once_changes_on_edit_and_reopens_only_after_a_close()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5059` | `#[tokio::test] async fn tsgo_sends_no_workspace_configuration_notification()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5110` | `#[tokio::test] async fn tsgo_semantic_tokens_arrive_in_verter_legend_space()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5202` | `#[tokio::test] async fn tsgo_inlay_hints_appear_for_inferred_types()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:389` | `#[tokio::test] async fn open_file_still_delivers_the_editor_open_on_the_lsp_surface()` | `let _ = TypeProvider::` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3691` | `async fn new() -> Self` | `.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs:138` | `async fn open_b_companions(provider: &TsgoOwnedProvider, src_dir: &Path)` | `.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs:142` | `async fn open_b_companions(provider: &TsgoOwnedProvider, src_dir: &Path)` | `.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs:183` | ` worker_threads = 2)] async fn owned_bare_vue_import_resolves_to_declaration_carrier_and_public_member_flows()` | `.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs:245` | `worker_threads = 2)] async fn owned_bare_vue_import_fails_closed_when_declaration_carrier_didopen_suppressed()` | `.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs:249` | `worker_threads = 2)] async fn owned_bare_vue_import_fails_closed_when_declaration_carrier_didopen_suppressed()` | `.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:128` | `(flavor = , worker_threads = 2)] async fn owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process()` | `.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:223` | `lavor = , worker_threads = 2)] async fn owned_api_oracle_resolves_multiple_projects_per_query_on_one_process()` | `provider.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:224` | `lavor = , worker_threads = 2)] async fn owned_api_oracle_resolves_multiple_projects_per_query_on_one_process()` | `provider.` |

**`doc-comment`** (38)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/background_drain.rs:69` | `` | `/// 'provider.` |
| `crates/verter_lsp/src/external_ts/publish_coordinator.rs:14` | `` | `//! This REPLACES the direct 'provider.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:15` | `` | `//! downgrading a 'load_file'd file to an '` |
| `crates/verter_lsp/src/server/mod.rs:613` | `` | `ore + plugin membership (NOT a direct 'provider.` |
| `crates/verter_lsp/src/server/sync_orchestration.rs:483` | `impl VerterLanguageServer` | `/// '` |
| `crates/verter_lsp/src/server_tests.rs:706` | `` | `/// 'Some((path, opened))': an '` |
| `crates/verter_lsp/src/server_tests.rs:1991` | `` | `/// observable '` |
| `crates/verter_lsp/src/server_tests.rs:30917` | `` | `/// '` |
| `crates/verter_lsp/src/tsgo/overlay_core.rs:7` | `` | `//! - **Lifecycle (OWNED-budgeted).** '` |
| `crates/verter_lsp/src/tsgo/overlay_core_tests.rs:153` | `` | `/// composite's '` |
| `crates/verter_lsp/src/type_provider/mock.rs:160` | `` | `/// When 'true', the file-op methods ('` |
| `crates/verter_lsp/src/type_provider/mock.rs:165` | `` | `/// Per-path failure injection: any '` |
| `crates/verter_lsp/src/type_provider/mock.rs:169` | `` | `/// underlying '` |
| `crates/verter_lsp/src/type_provider/mock.rs:252` | `` | `/// One-shot async gate for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:290` | `` | `hen set to 'Some((path, callback))', the FIRST '` |
| `crates/verter_lsp/src/type_provider/mock.rs:542` | `impl MockTypeProvider` | `one-shot side effect that fires the FIRST time '` |
| `crates/verter_lsp/src/type_provider/mock.rs:590` | `impl MockTypeProvider` | `/// Make every subsequent file-op ('` |
| `crates/verter_lsp/src/type_provider/mock.rs:600` | `impl MockTypeProvider` | `/// Make any '` |
| `crates/verter_lsp/src/type_provider/mock.rs:976` | `impl TypeProvider for MockTypeProvider` | `/// trait default would collapse it into '` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:101` | `` | `rrier-content authority, and a second 'provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:153` | `impl ProjectSync` | `s provider-sync unit tests that assert the raw '` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1089` | `` | `/// @ai-generated — open_tsx sends` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1617` | `` | `/ @ai-generated — load_tsx sends load_file (not` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1654` | `` | `/// @ai-generated — open_dts sends` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1675` | `` | `/ @ai-generated — load_dts sends load_file (not` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1821` | `` | `erated — load_tsx uses load_file, open_tsx uses` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1852` | `` | `/// a 'provider.` |
| `crates/verter_type_runtime/src/resilient.rs:138` | `` | `/// NOT through the file set / '` |
| `crates/verter_type_runtime/src/traits.rs:153` | `pub trait TypeProvider: Send + Sync` | `/// Unlike '` |
| `crates/verter_type_runtime/src/traits.rs:156` | `pub trait TypeProvider: Send + Sync` | `QUIRED, deliberately without a default. A 'self.` |
| `crates/verter_type_runtime/src/traits.rs:157` | `pub trait TypeProvider: Send + Sync` | `/// is silent: a wrapper that adds work to '` |
| `crates/verter_type_runtime/src/traits.rs:161` | `pub trait TypeProvider: Send + Sync` | `so explicitly by forwarding to ['TypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2915` | `impl TypeProvider for TsgoTypeProvider` | `/// Unlike '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:1196` | `` | `/// @ai-generated — TSGO processes` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:438` | `impl TypeProvider for TsgoOwnedProvider` | `oad-bearing, not a courtesy delegation: ['Self::` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:440` | `impl TypeProvider for TsgoOwnedProvider` | `/// 'load_file →` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:334` | `` | `/// The owned provider overrides '` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:336` | `` | `/// load through '` |

**`comment`** (13)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:201` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `//` |
| `crates/verter_lsp/src/extension_provider_tests.rs:511` | `#[tokio::test] async fn extension_provider_get_code_actions_surfaces_single_and_combined_unused_fix()` | `//` |
| `crates/verter_lsp/src/extension_provider_tests.rs:748` | `` | `// ('ExtensionTypeProvider::` |
| `crates/verter_lsp/src/server/sync_orchestration.rs:1313` | `pub(super) async fn ensure_current_file_synced(&self, uri: &Uri)` | `// Choose` |
| `crates/verter_lsp/src/server_tests.rs:16554` | `#[tokio::test(flavor = )] async fn open_dts_success_records_store_surface_and_failure_does_not()` | `arrier-API companion open: open_dts -> provider.` |
| `crates/verter_lsp/src/server_tests.rs:17182` | `#[tokio::test(flavor = )] async fn tsgo_barrel_eager_sync_follows_the_complete_reexport_closure()` | `// sync an observable '` |
| `crates/verter_lsp/src/server_tests.rs:17533` | `#[tokio::test(flavor = )] async fn barrel_eager_sync_terminates_on_reexport_cycle_and_still_syncs_terminal()` | `// carrier sync an observable '` |
| `crates/verter_lsp/src/sync_coordinator_tests.rs:1453` | `io::test(flavor = )] async fn coordinator_direct_ide_sync_must_not_pair_stale_content_with_a_mid_flight_edit()` | `// Pause the coordinator's '` |
| `crates/verter_lsp/src/tsgo/overlay_core.rs:78` | `fn inject(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `// state machine ('TypeProvider::` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:48` | `impl TypeProvider for MeasureMock` | `// the interactive '` |
| `crates/verter_type_runtime/src/resilient_tests.rs:986` | `pub(crate) async fn register_carriers(&self)` | `the engine's sole byte authority. An ordinary '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:1123` | `#[test] #[cfg(windows)] fn test_normalize_file_uri_cache_key_match()` | `// Simulate what` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:386` | `#[tokio::test] async fn open_file_still_delivers_the_editor_open_on_the_lsp_surface()` | `// '` |

**`string-literal`** (30)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:313` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:564` | `#[tokio::test] async fn extension_provider_get_code_actions_surfaces_single_and_combined_unused_fix()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:712` | `#[tokio::test] async fn extension_provider_combined_fix_uses_content_current_as_of_each_response()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:818` | `#[tokio::test] async fn open_stamps_the_owning_package_root_not_the_workspace_folder()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:842` | `#[tokio::test] async fn update_open_stamps_the_owning_package_root_too()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:872` | `#[tokio::test] async fn a_file_outside_every_nested_package_still_stamps_the_root_project()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:899` | `#[tokio::test] async fn without_an_ownership_authority_the_workspace_folder_is_the_last_resort()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:934` | `#[tokio::test] async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1001` | `#[tokio::test] async fn open_declares_the_owning_projects_config_file_alongside_its_root()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1026` | `#[tokio::test] async fn open_declares_a_jsconfig_owned_project_by_its_own_config_name()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1056` | `#[tokio::test] async fn update_open_reopen_declares_the_config_too()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1100` | `#[tokio::test] async fn without_an_ownership_authority_no_config_is_invented()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1191` | `#[tokio::test] async fn resync_rebinds_a_file_opened_before_the_ownership_authority_landed()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1253` | `#[tokio::test] async fn resync_closes_a_file_no_configured_project_owns_instead_of_rebinding_it()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1322` | `#[tokio::test] async fn completion_details_propagate_a_refusal_instead_of_returning_the_previous_items()` | `.expect("` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:264` | `#[tokio::test] async fn restart_replays_cached_state_without_downgrading_loaded_files()` | `"open files should replay via` |
| `crates/verter_lsp/src/test_harness.rs:817` | `pub(crate) async fn open_in_provider(&self, relative_path: &str, content: &str) -> String` | `.expect("provider` |
| `crates/verter_lsp/src/test_harness.rs:846` | `pub(crate) async fn open_fixture_in_provider(&self, relative_path: &str) -> (String, String)` | `.expect("provider` |
| `crates/verter_lsp/src/type_provider/mock.rs:959` | `fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `fail_or_ok(fail, "` |
| `crates/verter_lsp/tests/cases/editor_liveness_guards.rs:682` | `#[test] fn delegated_close_detector_discriminates_vue_evasion_from_approved_and_non_vue()` | `let _ = sync.` |
| `crates/verter_lsp/tests/cases/editor_liveness_guards.rs:813` | `#[test] fn delegated_close_detector_discriminates_vue_evasion_from_approved_and_non_vue()` | `let _ = sync.` |
| `crates/verter_lsp/tests/cases/editor_liveness_guards.rs:945` | `#[test] fn delegated_close_detector_discriminates_vue_evasion_from_approved_and_non_vue()` | `let _ = sync.` |
| `crates/verter_lsp/tests/cases/editor_liveness_guards.rs:1143` | `#[test] fn delegated_close_detector_discriminates_vue_evasion_from_approved_and_non_vue()` | `let _ = sync.` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1599` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `" fn configure_paths(&self) {}\n fn` |
| `crates/verter_session/tests/g_extts/shared_provider_live_wiring.rs:267` | `#[test] fn shared_provider_live_wiring_self_test_discriminates()` | `injection_failures("self.lsp.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1454` | `#[tokio::test(start_paused = true)] async fn restart_replays_state_without_downgrading_loaded_files()` | `"an opened file replays via` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2909` | `fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `tracing::debug!("TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:1134` | `#[test] #[cfg(windows)] fn test_normalize_file_uri_cache_key_match()` | `"` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3157` | `#[tokio::test] async fn test_provider_operations_fail_after_process_death()` | `assert!(result.is_ok(), "` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:399` | `#[tokio::test] async fn open_file_still_delivers_the_editor_open_on_the_lsp_surface()` | `.expect("` |

### `load_file` — declared at `crates/verter_type_runtime/src/traits.rs:162`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:162` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (10)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:153` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:881` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1006` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:645` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:458` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:455` | `impl ProjectSync` | `pub async fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:44` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2919` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:442` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2864` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:88` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3029` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:228` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:352` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:512` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:792` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:973` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:770` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:963` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:151` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1082` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:51` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:626` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:165` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:33` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (14)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:340` | `async fn apply_files( &self, files: &[BaselineFile], applied: &[AppliedSync], ) -> Result<(), Response>` | `SyncAction::Loaded => provider.` |
| `crates/verter_lsp/src/server_utils.rs:723` | `` | `project_sync.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1001` | `fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.features.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1015` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.features.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:283` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:342` | `tent: String, access: Option<FileAccess>, lane: PriorityLane, update: bool, ) -> Result<(), TypeProviderError>` | `FileAccess::Loaded => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:413` | `pub async fn load_dts( &self, dts_path: &str, dts_content: &str, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:131` | `elf, path: &str, content: &str, lane: ProviderLane, verb: ProviderFileVerb, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/workspace_scanner.rs:789` | `` | `.` |
| `crates/verter_lsp/src/workspace_scanner.rs:804` | `` | `.` |
| `crates/verter_type_runtime/src/resilient.rs:837` | `async fn replay_into<P: TypeProvider>(&self, provider: &P, log_name: &str)` | `CachedFileMode::Load => provider.` |
| `crates/verter_type_runtime/src/resilient.rs:891` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Foreground => provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4126` | `fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4206` | `fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |

**`call-forwarding`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:885` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:651` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:456` | `pub async fn load_file(&self, path: &str, content: &str) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:443` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

**`call-trait-default`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:464` | `fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_type_runtime/src/traits.rs:502` | `fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |

**`call-test`** (14)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/resilient_provider_tests.rs:214` | `#[tokio::test] async fn restart_replays_cached_state_without_downgrading_loaded_files()` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3034` | `fn load_file( &self, path: &str, content: &str, ) -> crate::type_provider::traits::ProviderFuture<'_, ()>` | `self.inner.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:157` | `#[tokio::test] async fn lifecycle_is_cached_without_activation_and_replayed_once_on_first_query()` | `.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1762` | `#[tokio::test] async fn load_file_sends_load_file()` | `sync.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1784` | `#[tokio::test] async fn load_file_propagates_provider_errors()` | `let result = sync.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:2068` | `#[tokio::test] async fn tsserver_still_syncs_non_carrier_shadow_files()` | `sync.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1413` | `#[tokio::test(start_paused = true)] async fn restart_replays_state_without_downgrading_loaded_files()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:101` | `#[tokio::test] async fn initialized_non_owning_transport_serves_hover_without_initialize_or_child()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:156` | `#[tokio::test] async fn initialized_non_owning_transport_pulls_diagnostics_strictly()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3239` | `#[tokio::test] async fn cached_content_resolves_equivalent_path_forms_after_load_file()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4406` | `#[tokio::test] async fn contents_cache_hit_still_sends_the_hover_request()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5009` | `#[tokio::test] async fn content_cached_by_load_file_never_counts_as_delivered_to_the_child()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5252` | `#[tokio::test] async fn non_owning_transport_semantic_tokens_fail_closed_until_witness_legend_arrives()` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:355` | `#[tokio::test] async fn load_file_is_local_only_and_never_opens_or_barriers_on_the_lsp_surface()` | `TypeProvider::` |

**`doc-comment`** (25)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/resilient_provider_tests.rs:15` | `` | `//! downgrading a '` |
| `crates/verter_lsp/src/server_tests.rs:30917` | `` | `/// 'open_file' / '` |
| `crates/verter_lsp/src/type_provider/mock.rs:160` | `` | `When 'true', the file-op methods ('open_file'/'` |
| `crates/verter_lsp/src/type_provider/mock.rs:165` | `` | `// Per-path failure injection: any 'open_file'/'` |
| `crates/verter_lsp/src/type_provider/mock.rs:590` | `impl MockTypeProvider` | `/// Make every subsequent file-op ('open_file'/'` |
| `crates/verter_lsp/src/type_provider/mock.rs:600` | `impl MockTypeProvider` | `/// Make any 'open_file'/'` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:104` | `` | `on-carrier real-file shadow verbs ('sync_file'/'` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:454` | `impl ProjectSync` | `/// Unlike 'sync_file', this uses '` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1617` | `` | `/// @ai-generated — load_tsx sends` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1675` | `` | `/// @ai-generated — load_dts sends` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1756` | `` | `///` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1756` | `` | `/// load_file sends` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1777` | `` | `///` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1821` | `` | `/// @ai-generated — load_tsx uses` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1852` | `` | `/// a 'provider.open_file'/'update_file'/'` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:2053` | `` | `/// real-file shadow ('sync_file'/'` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:1106` | `` | `/// 'versions' alone, never 'contents': '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3182` | `` | `(contents-cache false-miss across path forms): '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3194` | `` | `/// found. '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4998` | `` | `/// '` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:436` | `impl TypeProvider for TsgoOwnedProvider` | `/// '` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:440` | `impl TypeProvider for TsgoOwnedProvider` | `/// '` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:330` | `` | `/// The BACKGROUND load is local-only: '` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:335` | `` | `/// barrier. With no '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:1824` | `` | `/// 'updateOpen'. '` |

**`comment`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:47` | `impl TypeProvider for MeasureMock` | `// '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4125` | `fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `//` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2865` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `// For tsserver,` |

**`string-literal`** (12)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/resilient_provider_tests.rs:250` | `#[tokio::test] async fn restart_replays_cached_state_without_downgrading_loaded_files()` | `"loaded files should replay via` |
| `crates/verter_lsp/src/type_provider/mock.rs:972` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `Box::pin(async move { fail_or_ok(fail, "` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1785` | `#[tokio::test] async fn load_file_propagates_provider_errors()` | `assert!(result.is_err(), "` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1442` | `#[tokio::test(start_paused = true)] async fn restart_replays_state_without_downgrading_loaded_files()` | `"a loaded file replays via` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2920` | `fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `tracing::debug!("TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3241` | `#[tokio::test] async fn cached_content_resolves_equivalent_path_forms_after_load_file()` | `.expect("` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3260` | `#[tokio::test] async fn cached_content_resolves_equivalent_path_forms_after_load_file()` | `"the resolved content must be exactly what` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5012` | `#[tokio::test] async fn content_cached_by_load_file_never_counts_as_delivered_to_the_child()` | `"` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5020` | `#[tokio::test] async fn content_cached_by_load_file_never_counts_as_delivered_to_the_child()` | `"` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:358` | `#[tokio::test] async fn load_file_is_local_only_and_never_opens_or_barriers_on_the_lsp_surface()` | `.expect("` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:359` | `#[tokio::test] async fn load_file_is_local_only_and_never_opens_or_barriers_on_the_lsp_surface()` | `load.expect("` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:367` | `#[tokio::test] async fn load_file_is_local_only_and_never_opens_or_barriers_on_the_lsp_surface()` | `"` |

### `update_file` — declared at `crates/verter_type_runtime/src/traits.rs:164`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:164` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:163` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:891` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1010` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:656` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:468` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:53` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2944` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:446` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2898` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:93` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3037` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:232` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:356` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:516` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:796` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:977` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:774` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:990` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:154` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1085` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:55` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:618` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:173` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:37` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:341` | `async fn apply_files( &self, files: &[BaselineFile], applied: &[AppliedSync], ) -> Result<(), Response>` | `SyncAction::Updated => provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:335` | `tent: String, access: Option<FileAccess>, lane: PriorityLane, update: bool, ) -> Result<(), TypeProviderError>` | `(true, PriorityLane::Interactive) => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:445` | `pub async fn sync_dts( &self, dts_path: &str, dts_content: &str, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:461` | `pub async fn sync_file(&self, path: &str, content: &str) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:137` | `elf, path: &str, content: &str, lane: ProviderLane, verb: ProviderFileVerb, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:266` | `_file<'a>( &'a self, file_id: &'a GeneratedFileId, revision: u64, content: &'a str, ) -> BackendFuture<'a, ()>` | `.` |
| `crates/verter_type_runtime/src/resilient.rs:896` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Foreground => provider.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:895` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:662` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:451` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `lsp.` |

**`call-trait-default`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:468` | `fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_type_runtime/src/traits.rs:506` | `fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.` |

**`call-test`** (17)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:844` | `#[tokio::test] async fn update_open_stamps_the_owning_package_root_too()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1063` | `#[tokio::test] async fn update_open_reopen_declares_the_config_too()` | `.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:218` | `#[tokio::test] async fn restart_replays_cached_state_without_downgrading_loaded_files()` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3042` | `fn update_file( &self, path: &str, content: &str, ) -> crate::type_provider::traits::ProviderFuture<'_, ()>` | `self.inner.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:153` | `#[tokio::test] async fn lifecycle_is_cached_without_activation_and_replayed_once_on_first_query()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1170` | `#[tokio::test(start_paused = true)] async fn mid_restart_update_replays_current_not_stale_bytes()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1417` | `#[tokio::test(start_paused = true)] async fn restart_replays_state_without_downgrading_loaded_files()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1931` | `#[tokio::test(start_paused = true)] async fn quarantine_clears_when_the_file_content_changes()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3164` | `#[tokio::test] async fn test_provider_operations_fail_after_process_death()` | `tokio::time::timeout(timeout, provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4678` | `#[tokio::test] async fn writer_stall_watchdog_fires_crash_notify_when_the_child_stops_reading_stdin()` | `let _ = provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4813` | `#[tokio::test] async fn a_slow_but_progressing_child_does_not_trip_the_writer_stall_watchdog()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4896` | `#[tokio::test] async fn republishing_identical_bytes_sends_nothing_while_a_real_edit_still_syncs()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4903` | `#[tokio::test] async fn republishing_identical_bytes_sends_nothing_while_a_real_edit_still_syncs()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4904` | `#[tokio::test] async fn republishing_identical_bytes_sends_nothing_while_a_real_edit_still_syncs()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4912` | `#[tokio::test] async fn republishing_identical_bytes_sends_nothing_while_a_real_edit_still_syncs()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4927` | `#[tokio::test] async fn republishing_identical_bytes_sends_nothing_while_a_real_edit_still_syncs()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5024` | `#[tokio::test] async fn content_cached_by_load_file_never_counts_as_delivered_to_the_child()` | `provider.` |

**`doc-comment`** (33)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/artifact_overlay.rs:82` | `` | `/// '` |
| `crates/verter_dx_baseline/src/main_tests.rs:23` | `` | `/// When 'true', every apply ('open'/'load'/'` |
| `crates/verter_lsp/src/extension_provider.rs:102` | `impl<T: TsQueryTransport> ExtensionTypeProvider<T>` | `/// concurrent '` |
| `crates/verter_lsp/src/extension_provider.rs:1377` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `Nothing else repairs that binding. An ordinary '` |
| `crates/verter_lsp/src/extension_provider_tests.rs:47` | `` | `/// simulate a concurrent '` |
| `crates/verter_lsp/src/extension_provider_tests.rs:94` | `impl ScriptedTsQueryTransport` | `/// is returned — simulating a concurrent '` |
| `crates/verter_lsp/src/extension_provider_tests.rs:639` | `` | `/// taken once before the loop. A concurrent '` |
| `crates/verter_lsp/src/integration_tests.rs:3313` | `` | `/// directly to '` |
| `crates/verter_lsp/src/server_tests.rs:706` | `` | `/// 'Some((path, opened))': an 'open_file'/'` |
| `crates/verter_lsp/src/server_tests.rs:30916` | `` | `-to-definition performs ZERO carrier-companion '` |
| `crates/verter_lsp/src/tsgo/overlay_core.rs:7` | `` | `**Lifecycle (OWNED-budgeted).** 'open_file' / '` |
| `crates/verter_lsp/src/type_provider/mock.rs:161` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:166` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:169` | `` | `/// underlying 'open_file'/'` |
| `crates/verter_lsp/src/type_provider/mock.rs:243` | `` | `eam matching 'close_block', but for a one-shot '` |
| `crates/verter_lsp/src/type_provider/mock.rs:591` | `impl MockTypeProvider` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:600` | `impl MockTypeProvider` | `/// Make any 'open_file'/'load_file'/'` |
| `crates/verter_lsp/src/type_provider/mock.rs:653` | `impl MockTypeProvider` | `/// Test seam: pause the next '` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:154` | `impl ProjectSync` | `/// '` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:436` | `impl ProjectSync` | `/// 'provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1068` | `` | `/// @ai-generated — TSX sync sends` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1696` | `` | `/// @ai-generated — sync_dts sends` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1852` | `` | `/// a 'provider.open_file'/'` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:289` | `impl ProjectSync` | `/// direct 'provider.` |
| `crates/verter_session/tests/g_extts/provider_op_requires_resolved_project.rs:10` | `` | `uery_by_path(uri: &str)' / 'fn open_tsx(' / 'fn` |
| `crates/verter_type_runtime/src/contents_snapshot.rs:9` | `` | `//! '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2905` | `impl TypeProvider for TsgoTypeProvider` | `lish 'content' for 'path'. Identical to ['Self::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2917` | `impl TypeProvider for TsgoTypeProvider` | `/ IS eventually opened (user navigates to it), '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4690` | `` | `/// '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:1823` | `` | `/// companion CONTENTLESSLY. Used by '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:1859` | `` | `iately before the reopen send; if a concurrent '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:5050` | `` | `/// concurrent '` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:1186` | `` | `/// Helper: run the same logic as TypeProvider::` |

**`comment`** (18)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:276` | `#[tokio::test] async fn sync_artifacts_applies_update_file_then_probe_is_fresh()` | `// through TypeProvider::` |
| `crates/verter_lsp/src/extension_provider.rs:1001` | ` u32, end_offset: u32, diagnostics: &[ProviderDiagnosticContext], ) -> ProviderFuture<'_, Vec<TypeCodeAction>>` | `// '` |
| `crates/verter_lsp/src/extension_provider_tests.rs:148` | `s_query( &self, params: TsQueryParams, ) -> impl Future<Output = Result<Value, TypeProviderError>> + Send + '_` | `// '` |
| `crates/verter_lsp/src/extension_provider_tests.rs:748` | `` | `// ('ExtensionTypeProvider::open_file' / '` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1041` | `#[tokio::test] async fn update_open_reopen_declares_the_config_too()` | `// The re-open arm of '` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1050` | `#[tokio::test] async fn update_open_reopen_declares_the_config_too()` | `// prior text to diff against, '` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1126` | `` | `// and no later edit can fix it (an ordinary '` |
| `crates/verter_lsp/src/server/custom_methods/mod.rs:156` | `pub async fn on_did_change_ts_or_js_file(&self, params: OnDidChangeTsOrJsFileParams)` | `// Convert file:// URI to filesystem path —` |
| `crates/verter_lsp/src/server/sync_orchestration.rs:1313` | `pub(super) async fn ensure_current_file_synced(&self, uri: &Uri)` | `// Choose open_file vs` |
| `crates/verter_lsp/src/server_tests.rs:6067` | `#[tokio::test(flavor = )] async fn did_change_acknowledges_before_provider_refresh_for_all_carrier_modes()` | `// '` |
| `crates/verter_lsp/src/server_tests.rs:25659` | `#[tokio::test(flavor = )] async fn resync_aliased_imports_syncs_barrel_and_vue_deps_for_tsgo()` | `e should be synced to provider (via sync_file →` |
| `crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs:942` | `` | `// ('query_by_path' / 'open_tsx' / '` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:413` | `fn resync_open_files(&self) -> ProviderFuture<'_, ()>` | `// in-flight '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2994` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `// File not open yet — open it and track. '` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:1184` | `` | `// ──` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:1226` | `run_update_file_capture( old_content: Option<&str>, new_content: &str, file: &str, ) -> Vec<serde_json::Value>` | `// Run the same logic as` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3271` | `#[tokio::test] async fn resync_skips_stale_source_reopen_when_content_generation_advanced()` | `// 'const v = 2;' that '` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3285` | `#[tokio::test] async fn resync_skips_stale_source_reopen_when_content_generation_advanced()` | `// A concurrent '` |

**`string-literal`** (12)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:294` | `#[tokio::test] async fn sync_artifacts_applies_update_file_then_probe_is_fresh()` | `"syncArtifacts must call TypeProvider::` |
| `crates/verter_lsp/src/extension_provider_tests.rs:846` | `#[tokio::test] async fn update_open_stamps_the_owning_package_root_too()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1065` | `#[tokio::test] async fn update_open_reopen_declares_the_config_too()` | `.expect("` |
| `crates/verter_lsp/src/type_provider/mock.rs:1013` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `fail_or_ok(fail, "` |
| `crates/verter_session/tests/g_extts/provider_op_requires_resolved_project.rs:50` | `` | `"` |
| `crates/verter_session/tests/g_extts/provider_op_requires_resolved_project.rs:156` | `#[test] fn provider_op_scanner_discriminates()` | `" async fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2945` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `tracing::debug!("TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3165` | `#[tokio::test] async fn test_provider_operations_fail_after_process_death()` | `assert!(result.is_ok(), "` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4751` | `#[tokio::test] async fn a_refused_didopen_leaves_no_synced_entry_in_the_local_ledger()` | `` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2940` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `"tsserver` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2969` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `tracing::warn!("tsserver` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3001` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `"tsserver` |

### `close_file` — declared at `crates/verter_type_runtime/src/traits.rs:166`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:166` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (13)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:240` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:901` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1020` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:667` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:478` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:465` | `impl ProjectSync` | `pub async fn` |
| `crates/verter_scheduler/src/scheduler.rs:3085` | `impl Scheduler` | `pub fn` |
| `crates/verter_type_runtime/src/backend.rs:164` | `pub trait GeneratedQueryBackend: Send + Sync` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:297` | `impl GeneratedQueryBackend for TypeProviderAdapter` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:62` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2949` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:457` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3030` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:98` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3045` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:236` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:360` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:520` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:814` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:989` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:779` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1017` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:157` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1088` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:59` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:634` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:181` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:41` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (17)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/background_drain.rs:1529` | `vider_surface_store::ProviderSurfaceStore, stale_paths: &[(NonDeclProviderPathKind, String)], context: &str, )` | `NonDeclProviderPathKind::Shadow => sync.` |
| `crates/verter_lsp/src/external_ts/membership_reconciler.rs:964` | `ource, binding: ProjectBinding, companions: Vec<CarrierCompanion>, ) -> Result<ReconcileOutcome, ReconcileErr>` | `provider.` |
| `crates/verter_lsp/src/external_ts/membership_reconciler.rs:992` | `ource, binding: ProjectBinding, companions: Vec<CarrierCompanion>, ) -> Result<ReconcileOutcome, ReconcileErr>` | `provider.` |
| `crates/verter_lsp/src/external_ts/membership_reconciler.rs:1083` | `ply_absent( &self, source: &CanonicalSource, reason: AbsentReason, ) -> Result<ReconcileOutcome, ReconcileErr>` | `.` |
| `crates/verter_lsp/src/server/lifecycle.rs:1072` | `pub(super) async fn handle_did_close( server: &VerterLanguageServer, params: DidCloseTextDocumentParams, )` | `server.documents.host().scheduler().` |
| `crates/verter_lsp/src/server/provider_state.rs:1183` | `pub(super) async fn close_provider_paths(&self, paths: &[(ProviderPathKind, String)])` | `ProviderPathKind::Shadow => sync.` |
| `crates/verter_lsp/src/sync_coordinator.rs:1435` | `es: &crate::provider_surface_store::ProviderSurfaceStore, stale_paths: &[(NonDeclProviderPathKind, String)], )` | `NonDeclProviderPathKind::Shadow => sync.` |
| `crates/verter_lsp/src/tsgo/overlay_core.rs:83` | `fn retract(&self, path: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:366` | `async fn record_close( &self, path: String, lane: PriorityLane, ) -> Result<(), TypeProviderError>` | `PriorityLane::Interactive => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:450` | `pub async fn close_dts(&self, dts_path: &str) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:224` | `per) async fn close_tsx_in_lane( &self, tsx_path: &str, lane: ProviderLane, ) -> Result<(), TypeProviderError>` | `ProviderLane::Foreground => self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:256` | `c fn close_virtual_verter_types( &self, tsx_path: &str, lane: ProviderLane, ) -> Result<(), TypeProviderError>` | `ProviderLane::Foreground => self.provider.` |
| `crates/verter_lsp/src/workspace_scanner.rs:1226` | `es: &crate::provider_surface_store::ProviderSurfaceStore, stale_paths: &[(NonDeclProviderPathKind, String)], )` | `NonDeclProviderPathKind::Shadow => sync.` |
| `crates/verter_session/src/host_lifecycle.rs:586` | `pub fn close(&self)` | `self.scheduler.` |
| `crates/verter_session/src/host_lifecycle.rs:986` | `pub fn ensure_loaded(&self, canonical_id: &str) -> bool` | `self.scheduler.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:324` | `fn evict_file<'a>(&'a self, file_id: &'a GeneratedFileId) -> BackendFuture<'a, ()>` | `self.` |
| `crates/verter_type_runtime/src/resilient.rs:901` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Foreground => provider.` |

**`call-forwarding`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:904` | `fn close_file(&self, path: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:669` | `fn close_file(&self, path: &str) -> ProviderFuture<'_, ()>` | `ync move { self.provider_for_path(&path).await?.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:466` | `pub async fn close_file(&self, path: &str) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:308` | `fn close_file<'a>(&'a self, file_id: &'a GeneratedFileId) -> BackendFuture<'a, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:458` | `fn close_file(&self, path: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

**`call-trait-default`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:472` | `fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_type_runtime/src/traits.rs:510` | `fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()>` | `self.` |

**`call-test`** (11)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3046` | `fn close_file(&self, path: &str) -> crate::type_provider::traits::ProviderFuture<'_, ()>` | `self.inner.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:160` | `#[tokio::test] async fn lifecycle_is_cached_without_activation_and_replayed_once_on_first_query()` | `provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1793` | `#[tokio::test] async fn non_carrier_file_close_sends_close_file()` | `sync.` |
| `crates/verter_scheduler/src/scheduler.rs:8389` | `#[test] fn close_file_clears_overlay()` | `sched.` |
| `crates/verter_scheduler/src/scheduler.rs:14612` | `#[cfg(not(target_arch = ))] #[test] fn lifecycle_sweeps_drop_nodes_ref_before_dag_lock()` | `1 => sched_lifecycle.` |
| `crates/verter_scheduler/src/source_root_tests.rs:754` | `#[test] fn close_file_publishes_absent_and_the_prior_root_keeps_present()` | `scheduler.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1138` | `#[tokio::test(start_paused = true)] async fn removed_carrier_is_absent_from_restart_replay()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1205` | `#[tokio::test(start_paused = true)] async fn restart_replay_equals_desired_membership_set()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1380` | `#[tokio::test(start_paused = true)] async fn retracted_carrier_is_absent_from_restart_replay()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3167` | `#[tokio::test] async fn test_provider_operations_fail_after_process_death()` | `result = tokio::time::timeout(timeout, provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4977` | `#[tokio::test] async fn the_ledger_opens_once_changes_on_edit_and_reopens_only_after_a_close()` | `provider.` |

**`doc-comment`** (22)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/external_ts/membership_reconciler.rs:14` | `` | `tor's command API ('register_carrier_member' / '` |
| `crates/verter_lsp/src/server_tests.rs:686` | `` | `/// '` |
| `crates/verter_lsp/src/server_tests.rs:694` | `` | `/// 'Some((path, arrived, release))': a '` |
| `crates/verter_lsp/src/tsgo/composite.rs:77` | `` | `bound on a SHARED carrier retract issued from '` |
| `crates/verter_lsp/src/tsgo/composite.rs:164` | `impl SharedTsgoOverlay` | `/// Retract a carrier overlay off the managed '` |
| `crates/verter_lsp/src/tsgo/overlay_core.rs:755` | `impl<T: OverlayTransport> LazyOverlayCore<T>` | `/// neither hang nor delay the OWNED '` |
| `crates/verter_lsp/src/tsgo/overlay_core_tests.rs:559` | `` | `/// never-answering relay hung the composite '` |
| `crates/verter_lsp/src/type_provider/mock.rs:161` | `` | `/// 'update_file'/'` |
| `crates/verter_lsp/src/type_provider/mock.rs:170` | `` | `/// path. '` |
| `crates/verter_lsp/src/type_provider/mock.rs:230` | `` | `hen set to 'Some((path, arrived, release))', a '` |
| `crates/verter_lsp/src/type_provider/mock.rs:591` | `impl MockTypeProvider` | `/// 'update_file'/'` |
| `crates/verter_lsp/src/type_provider/mock.rs:605` | `impl MockTypeProvider` | `when only that kind's replacement sync fails. '` |
| `crates/verter_lsp/src/type_provider/mock.rs:631` | `impl MockTypeProvider` | `/// Test seam: make '` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:105` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1636` | `` | `/// @ai-generated — close_tsx sends` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:1717` | `` | `/// @ai-generated — close_dts sends` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:2053` | `` | `/// real-file shadow ('sync_file'/'load_file'/'` |
| `crates/verter_lsp/tests/cases/editor_liveness_guards.rs:21` | `` | `//! 'close_tsx' / 'close_dts' / '` |
| `crates/verter_lsp/tests/cases/editor_liveness_guards.rs:43` | `` | `/// ('for ... { close_tsx/close_dts/` |
| `crates/verter_scheduler/src/scheduler.rs:4381` | `impl Scheduler` | `/// that a concurrent 'invalidate()' / '` |
| `crates/verter_scheduler/src/scheduler.rs:14295` | `` | `/ it retires — exactly like 'invalidate()' and '` |
| `crates/verter_scheduler/src/scheduler.rs:14537` | `` | `applied to the lifecycle sweeps ('invalidate', '` |

**`comment`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/lifecycle.rs:1038` | `pub(super) async fn handle_did_close( server: &VerterLanguageServer, params: DidCloseTextDocumentParams, )` | `(Also keeps the required ordering vs 'scheduler.` |
| `crates/verter_lsp/src/server/lifecycle.rs:1039` | `pub(super) async fn handle_did_close( server: &VerterLanguageServer, params: DidCloseTextDocumentParams, )` | `// VFS overlay must clear before '` |
| `crates/verter_lsp/src/type_provider/mock.rs:1028` | `fn close_file(&self, path: &str) -> ProviderFuture<'_, ()>` | `// '` |
| `crates/verter_scheduler/src/scheduler.rs:3089` | `pub fn close_file(&self, id: &str)` | `// for the AB-BA-prevention rationale;` |
| `crates/verter_scheduler/src/scheduler.rs:3523` | `fn prepare_request(&self, request: QueuedRequest) -> Option<PreparedRequest>` | `// Source: None after removal — stale (e.g.` |
| `crates/verter_scheduler/src/scheduler.rs:3632` | `prepared_under_lock( &self, dag: &mut SchedulerDag, prepared: PreparedRequest, post: &mut AdmissionPostWork, )` | `// the same rule as 'invalidate()' and '` |
| `crates/verter_scheduler/src/scheduler.rs:4445` | `Dag, canonical: &Arc<str>, generation: u64, identity: &WorkNodeIdentity, ) -> Vec<crate::dag::SubmissionToken>` | `// 'invalidate', a '` |
| `crates/verter_scheduler/src/scheduler.rs:4601` | `fn handle_stage_complete( &self, file_id: &str, generation: u64, task_kind: TaskKind, incarnation: u64, )` | `// 'invalidate()' / '` |

**`string-literal`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:1048` | `fn close_file(&self, path: &str) -> ProviderFuture<'_, ()>` | `fail_or_ok(fail, "` |
| `crates/verter_lsp/tests/cases/decl_overlay_close_ownership.rs:533` | `#[test] fn safe_decl_delegation_with_non_decl_raw_close_is_not_flagged()` | `ProviderPathKind::Shadow => sync.` |
| `crates/verter_lsp/tests/cases/editor_liveness_guards.rs:581` | `#[test] fn guard_detector_discriminates_inline_close_from_delegation()` | `ProviderPathKind::Shadow => sync.` |
| `crates/verter_lsp/tests/cases/editor_liveness_guards.rs:608` | `#[test] fn guard_detector_discriminates_inline_close_from_delegation()` | `ProviderPathKind::Shadow => sync.` |
| `crates/verter_scheduler/src/source_root_tests.rs:761` | `#[test] fn close_file_publishes_absent_and_the_prior_root_keeps_present()` | `"'` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2950` | `fn close_file(&self, path: &str) -> ProviderFuture<'_, ()>` | `tracing::debug!("TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3168` | `#[tokio::test] async fn test_provider_operations_fail_after_process_death()` | `assert!(result.is_ok(), "` |

### `get_completions` — declared at `crates/verter_type_runtime/src/traits.rs:168`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:168` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:253` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1005` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1047` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:672` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:482` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:179` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2980` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:521` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3430` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:102` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3049` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:240` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:364` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:524` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:844` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1001` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:784` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1157` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:166` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1091` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:63` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:638` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:204` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:45` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:380` | `async fn on_query(&mut self, q: QueryRequest) -> Response` | `.` |
| `crates/verter_lsp/src/server/nav_features.rs:625` | `: &VerterLanguageServer, params: &CompletionParams, native_only: bool, ) -> Result<Option<CompletionResponse>>` | `.` |
| `crates/verter_lsp/src/server/nav_features.rs:1149` | `: &VerterLanguageServer, params: &CompletionParams, native_only: bool, ) -> Result<Option<CompletionResponse>>` | `.` |
| `crates/verter_lsp/src/server/nav_features.rs:1191` | `: &VerterLanguageServer, params: &CompletionParams, native_only: bool, ) -> Result<Option<CompletionResponse>>` | `.` |
| `crates/verter_lsp/src/server/nav_features.rs:1224` | `: &VerterLanguageServer, params: &CompletionParams, native_only: bool, ) -> Result<Option<CompletionResponse>>` | `tp.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:58` | ` query_members_at_offset( &self, path: &str, generated_offset: u32, ) -> Result<BackendTypeData, BackendError>` | `.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1021` | `ns( &self, path: &str, offset: u32, trigger_character: Option<&str>, ) -> ProviderFuture<'_, CompletionResult>` | `.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1054` | `ns( &self, path: &str, offset: u32, trigger_character: Option<&str>, ) -> ProviderFuture<'_, CompletionResult>` | `.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:683` | `ns( &self, path: &str, offset: u32, trigger_character: Option<&str>, ) -> ProviderFuture<'_, CompletionResult>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:493` | `ns( &self, path: &str, offset: u32, trigger_character: Option<&str>, ) -> ProviderFuture<'_, CompletionResult>` | `.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:202` | `ns( &self, path: &str, offset: u32, trigger_character: Option<&str>, ) -> ProviderFuture<'_, CompletionResult>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:527` | `ns( &self, path: &str, offset: u32, trigger_character: Option<&str>, ) -> ProviderFuture<'_, CompletionResult>` | `self.lsp.` |

**`call-test`** (23)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:317` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1324` | `#[tokio::test] async fn completion_details_propagate_a_refusal_instead_of_returning_the_previous_items()` | `.` |
| `crates/verter_lsp/src/future_size_measure_tests.rs:583` | `#[tokio::test] #[ignore = ] async fn measure_handler_future_sizes()` | `let fut = tp.` |
| `crates/verter_lsp/src/integration_tests.rs:4919` | `::test(flavor = , worker_threads = 2)] async fn integration_concurrent_completion_and_did_change_no_deadlock()` | `let result = mock_a.` |
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:353` | `async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession)` | `.` |
| `crates/verter_lsp/src/real_provider_tests/completion.rs:855` | `real_provider_test!( completion_carrier_fields_through_provider, fixture = , async fn run(session)` | `if let Ok(r) = session.provider().` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:28` | `RealProviderTestSession, provider_path: &str, offset: u32, ) -> Vec<verter_type_runtime::protocol::Completion>` | `.` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:351` | `real_provider_test!( completion_resolve_carries_auto_import_edit, fixture = , async fn run(session)` | `.` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:467` | `ovider_test!( completion_resolve_does_not_fabricate_import_for_local_symbol, fixture = , async fn run(session)` | `let Ok(result) = session.provider().` |
| `crates/verter_lsp/src/real_provider_tests/request_surface.rs:153` | `ovider_test!( completion_racing_an_edit_never_serves_a_torn_provider_result, fixture = , async fn run(session)` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3058` | `r>, ) -> crate::type_provider::traits::ProviderFuture< '_, crate::type_provider::protocol::CompletionResult, >` | `self.inner.` |
| `crates/verter_lsp/src/server_tests.rs:18774` | `tokio::test(flavor = )] async fn completion_with_real_tsserver_returns_fixture_vfor_member_access_properties()` | `.` |
| `crates/verter_lsp/src/server_tests.rs:20041` | `#[tokio::test(flavor = )] async fn completion_with_real_tsserver_recovers_when_current_file_sync_was_missed()` | `.` |
| `crates/verter_lsp/src/server_tests.rs:20226` | `tokio::test(flavor = )] async fn real_tsserver_slot_member_access_stays_typed_after_opening_child_and_parent()` | `.` |
| `crates/verter_lsp/src/server_tests.rs:20286` | `tokio::test(flavor = )] async fn real_tsserver_slot_member_access_stays_typed_after_opening_child_and_parent()` | `.` |
| `crates/verter_lsp/src/server_tests.rs:20322` | `tokio::test(flavor = )] async fn real_tsserver_slot_member_access_stays_typed_after_opening_child_and_parent()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1093` | `#[tokio::test] async fn feature_completion_denied_carrier_serves_native_only_no_owned_call()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1142` | `#[tokio::test] async fn feature_completion_bound_carrier_delegates_to_owned()` | `c.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:48` | `ry( provider: &TsserverTypeProvider, path: &str, offset: u32, ) -> Result<CompletionResult, TypeProviderError>` | `let mut last = provider.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:58` | `ry( provider: &TsserverTypeProvider, path: &str, offset: u32, ) -> Result<CompletionResult, TypeProviderError>` | `last = provider.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:2080` | `async fn assert_template_member_served_typed(h: &CompositeHarness, topology: &str)` | `.` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:194` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `let fut = tp.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4333` | `#[tokio::test] async fn contents_cache_miss_fails_closed_without_fabricating_positions()` | `let completions = provider.` |

**`doc-comment`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:10` | `` | `//! These exercise the REAL provider's '` |
| `crates/verter_lsp/src/tsgo/composite.rs:616` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:260` | `` | `/// One-shot async gate for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:301` | `` | `/// query ('get_hover' / '` |
| `crates/verter_lsp/src/type_provider/mock.rs:552` | `impl MockTypeProvider` | `/// ('get_hover' / '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2717` | `` | `/// ['TsgoTypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:809` | `` | `/// '` |
| `crates/verter_type_runtime/src/tsserver/completion_resolve_tests.rs:170` | `` | `/// '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:5284` | `` | `// request, so both the tsserver and extension '` |

**`comment`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:203` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `//` |
| `crates/verter_lsp/src/server/nav_features.rs:613` | `: &VerterLanguageServer, params: &CompletionParams, native_only: bool, ) -> Result<Option<CompletionResponse>>` | `//` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1075` | `` | `// ── COMPLETION + RESOLVE carrier features (` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2787` | `fn build_client_capabilities() -> serde_json::Value` | `// '` |
| `crates/verter_type_runtime/src/tsserver/completion_resolve_tests.rs:68` | `#[test] fn parse_tsserver_completion_preserves_external_module_resolve_handle()` | `// Stamped by '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:5241` | `pub fn parse_tsserver_completion(item: &serde_json::Value) -> Option<Completion>` | `// completion-site 'offset' is stamped by '` |

**`string-literal`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:319` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.expect("` |
| `crates/verter_lsp/src/future_size_measure_tests.rs:585` | `#[tokio::test] #[ignore = ] async fn measure_handler_future_sizes()` | `"TypeProvider::` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:196` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `"TypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2987` | `ns( &self, path: &str, offset: u32, trigger_character: Option<&str>, ) -> ProviderFuture<'_, CompletionResult>` | `"TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3009` | `ns( &self, path: &str, offset: u32, trigger_character: Option<&str>, ) -> ProviderFuture<'_, CompletionResult>` | `"TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:826` | `#[test] fn client_capabilities_advertise_completion_context_support()` | `"completion.contextSupport must be 'true' —` |
| `crates/verter_type_runtime/src/tsserver/completion_resolve_tests.rs:71` | `#[test] fn parse_tsserver_completion_preserves_external_module_resolve_handle()` | `"parser leaves the offset for` |

### `get_completion_details` — declared at `crates/verter_type_runtime/src/traits.rs:178`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:178` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:308` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1032` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1057` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:688` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:498` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3105` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:530` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3658` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:183` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:652` | `impl TypeProvider for MockTypeProvider` | `fn` |

**`call-production`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/provider_adapter.rs:63` | ` query_members_at_offset( &self, path: &str, generated_offset: u32, ) -> Result<BackendTypeData, BackendError>` | `.` |

**`call-forwarding`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1044` | `s<'a>( &'a self, path: &'a str, offset: u32, items: &'a [Completion], ) -> ProviderFuture<'a, Vec<Completion>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1063` | `s<'a>( &'a self, path: &'a str, offset: u32, items: &'a [Completion], ) -> ProviderFuture<'a, Vec<Completion>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:697` | `s<'a>( &'a self, path: &'a str, offset: u32, items: &'a [Completion], ) -> ProviderFuture<'a, Vec<Completion>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:507` | `s<'a>( &'a self, path: &'a str, offset: u32, items: &'a [Completion], ) -> ProviderFuture<'a, Vec<Completion>>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:536` | `s<'a>( &'a self, path: &'a str, offset: u32, items: &'a [Completion], ) -> ProviderFuture<'a, Vec<Completion>>` | `self.lsp.` |

**`call-test`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:359` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1331` | `#[tokio::test] async fn completion_details_propagate_a_refusal_instead_of_returning_the_previous_items()` | `.` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:97` | `real_provider_test!( completion_detail_enriches_member_signature, fixture = , async fn run(session)` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1102` | `#[tokio::test] async fn feature_completion_denied_carrier_serves_native_only_no_owned_call()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1149` | `#[tokio::test] async fn feature_completion_bound_carrier_delegates_to_owned()` | `let _ = c.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4122` | `#[tokio::test] async fn get_completion_details_bounds_enrichment_to_list_cap()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4199` | `#[tokio::test] async fn get_completion_details_enriches_full_small_list()` | `provider.` |

**`doc-comment`** (13)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:3` | `` | `//! GAP-1: TSGO inherited the 'TypeProvider::` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:8` | `` | `//! '` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:10` | `` | `ercise the REAL provider's 'get_completions' + '` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:11` | `` | `//! contract directly. Reverting TSGO's '` |
| `crates/verter_lsp/src/tsgo/composite.rs:618` | `` | `/// '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:518` | `` | `/// (['TsgoTypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:528` | `` | `/// list (['TsgoTypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2062` | `` | `/// ['crate::traits::TypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2711` | `` | `/ 'documentation' back in ['TsgoTypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2735` | `` | `y. Both resolve sites here (['TsgoTypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:736` | `` | `consumes the resolve round-trip at two sites — '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:5304` | `` | `/// Shared by the tsserver and extension '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:5340` | `` | `/// extension '` |

**`comment`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:219` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `//` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:43` | `` | `// GAP-1:` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:52` | `real_provider_test!( completion_detail_enriches_member_signature, fixture = , async fn run(session)` | `// completion list omits and '` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:109` | `real_provider_test!( completion_detail_enriches_member_signature, fixture = , async fn run(session)` | `// '` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1075` | `` | `ON + RESOLVE carrier features (get_completions,` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:771` | `#[test] fn client_capabilities_advertise_completion_item_resolve_support()` | `// no more. 'documentation' + 'detail' from` |

**`string-literal`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:361` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.expect("` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:99` | `real_provider_test!( completion_detail_enriches_member_signature, fixture = , async fn run(session)` | `.expect("` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:122` | `real_provider_test!( completion_detail_enriches_member_signature, fixture = , async fn run(session)` | `"` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:154` | `real_provider_test!( completion_detail_enriches_member_signature, fixture = , async fn run(session)` | `"` |

### `get_hover` — declared at `crates/verter_type_runtime/src/traits.rs:197`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:197` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:389` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1071` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1074` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:702` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:521` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:210` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3228` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:547` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3491` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:113` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3061` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:254` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:419` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:596` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:858` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1071` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:794` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1207` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:205` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1104` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:77` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:666` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:218` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:59` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (11)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:372` | `async fn on_query(&mut self, q: QueryRequest) -> Response` | `QueryMethod::Hover => match provider.` |
| `crates/verter_lsp/src/server/custom_methods/mod.rs:681` | `pub async fn get_binding_types(&self, params: GetAnalysisParams) -> Result<serde_json::Value>` | `if let Ok(Some(hover)) = tp.` |
| `crates/verter_lsp/src/server/nav_features.rs:87` | `on_details( server: &VerterLanguageServer, uri: &Uri, mut items: Vec<CompletionItem>, ) -> Vec<CompletionItem>` | `if let Ok(Some(info)) = tp.` |
| `crates/verter_lsp/src/server/nav_features.rs:128` | `b(super) async fn handle_hover( server: &VerterLanguageServer, params: HoverParams, ) -> Result<Option<Hover>>` | `if let Ok(Some(info)) = tp.` |
| `crates/verter_lsp/src/server/nav_features.rs:270` | `b(super) async fn handle_hover( server: &VerterLanguageServer, params: HoverParams, ) -> Result<Option<Hover>>` | `if let Ok(Some(info)) = tp.` |
| `crates/verter_lsp/src/server/nav_features.rs:349` | `b(super) async fn handle_hover( server: &VerterLanguageServer, params: HoverParams, ) -> Result<Option<Hover>>` | `tp.` |
| `crates/verter_lsp/src/server/nav_features.rs:427` | `b(super) async fn handle_hover( server: &VerterLanguageServer, params: HoverParams, ) -> Result<Option<Hover>>` | `tp.` |
| `crates/verter_session/src/typeinfo/oracle_core/gen.rs:724` | ` tsgo_bin: &str, files: &[(String, String)], hover_rel: &str, hover_offset: u32, ) -> Result<String, GenError>` | `provider.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:120` | `efinition_type_at_offset( &self, path: &str, generated_offset: u32, ) -> Result<BackendTypeData, BackendError>` | `.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:385` | `xpected_revision: u64, generated_offset: u32, query: BackendTypeQuery, ) -> BackendFuture<'a, BackendTypeData>` | `.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:434` | `xpected_revision: u64, generated_offset: u32, query: BackendTypeQuery, ) -> BackendFuture<'a, BackendTypeData>` | `.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1077` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1075` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:707` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:523` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `Box::pin(async move { self.activate().await?.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:217` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `move \|provider\| async move { provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:548` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `self.lsp.` |

**`call-test`** (56)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:1070` | `#[tokio::test] async fn known_good_script_setup_hover_resolves_through_tsgo_on_emitted_tsx()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:937` | `#[tokio::test] async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result()` | `provider.` |
| `crates/verter_lsp/src/future_size_measure_tests.rs:577` | `#[tokio::test] #[ignore = ] async fn measure_handler_future_sizes()` | `let fut = tp.` |
| `crates/verter_lsp/src/integration_tests.rs:3592` | `#[tokio::test] async fn integration_hover_merge_with_mock_type_provider()` | `let type_hover = mock.` |
| `crates/verter_lsp/src/integration_tests.rs:5507` | `#[tokio::test] async fn integration_hover_slot_merge_preserves_verter_info()` | `let type_hover = mock.` |
| `crates/verter_lsp/src/real_provider_tests/binding_types.rs:20` | `alProviderTestSession, provider_path: &str, offset: u32, ) -> Option<verter_type_runtime::protocol::HoverInfo>` | `if let Ok(Some(info)) = session.provider().` |
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:237` | `async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession)` | `.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:136` | `_for_restarting( provider: &ResilientProvider<MockTypeProvider, TestBackend>, crash_notify: &Notify, ) -> bool` | `if provider.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3069` | `2, ) -> crate::type_provider::traits::ProviderFuture< '_, Option<crate::type_provider::protocol::HoverInfo>, >` | `self.inner.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:176` | `#[tokio::test] async fn lifecycle_is_cached_without_activation_and_replayed_once_on_first_query()` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:177` | `#[tokio::test] async fn lifecycle_is_cached_without_activation_and_replayed_once_on_first_query()` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:233` | `#[tokio::test] async fn concurrent_first_queries_singleflight_the_managed_factory()` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:234` | `#[tokio::test] async fn concurrent_first_queries_singleflight_the_managed_factory()` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:305` | `#[tokio::test] async fn failed_activation_retries_after_cooldown_and_recovers()` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:310` | `#[tokio::test] async fn failed_activation_retries_after_cooldown_and_recovers()` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:333` | `#[tokio::test] async fn failed_activation_retries_after_cooldown_and_recovers()` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:591` | `#[tokio::test] async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries()` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:617` | `#[tokio::test] async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries()` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:681` | `#[tokio::test] async fn a_cancelled_activation_still_arms_the_retry_cooldown()` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:696` | `#[tokio::test] async fn a_cancelled_activation_still_arms_the_retry_cooldown()` | `provider.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:979` | `#[tokio::test] async fn feature_mixed_read_denied_carrier_serves_external_default_no_owned_call()` | `c.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1045` | `#[tokio::test] async fn feature_mixed_read_bound_carrier_delegates_to_owned()` | `c.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:362` | `#[tokio::test] async fn test_e2e_tsserver_scoped_slot_types_from_generated_vue_outputs()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1692` | `(flavor = , worker_threads = 4)] async fn composite_successful_shared_route_never_activates_managed_fallback()` | `h.composite.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1766` | `#[tokio::test] async fn composite_attach_failure_activates_managed_fallback_exactly_once()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1771` | `#[tokio::test] async fn composite_attach_failure_activates_managed_fallback_exactly_once()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:2065` | `async fn assert_template_member_served_typed(h: &CompositeHarness, topology: &str)` | `h.composite.` |
| `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_gen_spike.rs:154` | `async fn hover_probe_under(tsconfig: &str, source: &str, rhs: &str) -> Option<String>` | `provider.` |
| `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_gen_spike.rs:178` | `#[tokio::test(flavor = , worker_threads = 2)] async fn spike_hover_expands_and_is_confluent_with_authored()` | `provider.` |
| `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_gen_spike.rs:413` | `async fn hover_value(tsconfig: &str, off: usize) -> Option<String>` | `provider.` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:186` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `let fut = tp.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:529` | `async fn await_down(provider: &ResilientProvider<MockProvider, TestBackend>)` | `if provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1014` | `pub(crate) async fn await_down(&self)` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1053` | `pub(crate) async fn assert_carriers_answer_typed(&self)` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1627` | `async fn spin_until(provider: &ResilientProvider<MockProvider, FlakyBackend>, up: bool) -> bool` | `let answered = provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1677` | `#[tokio::test(start_paused = true)] async fn persistently_failing_respawn_exhausts_budget_and_stays_down()` | `provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1694` | `#[tokio::test(start_paused = true)] async fn persistently_failing_respawn_exhausts_budget_and_stays_down()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1731` | `async fn await_live(provider: &ResilientProvider<MockProvider, TestBackend>)` | `if provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1757` | `er, TestBackend>>, initial: &MockProvider, replacement: &MockProvider, companion: &'static str, offset: u32, )` | `async move { provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1785` | `er, TestBackend>>, initial: &MockProvider, replacement: &MockProvider, companion: &'static str, offset: u32, )` | `async move { provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1883` | `::test(start_paused = true)] async fn killer_request_is_quarantined_and_never_replayed_into_restarted_engine()` | `let quarantined = provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1895` | `::test(start_paused = true)] async fn killer_request_is_quarantined_and_never_replayed_into_restarted_engine()` | `let neighbor = provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1921` | `#[tokio::test(start_paused = true)] async fn quarantine_clears_when_the_file_content_changes()` | `matches!(provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1935` | `#[tokio::test(start_paused = true)] async fn quarantine_clears_when_the_file_content_changes()` | `let served = provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1970` | `#[tokio::test(start_paused = true)] async fn repeated_killer_request_does_not_burn_the_restart_budget()` | `let replayed = provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1996` | `#[tokio::test(start_paused = true)] async fn repeated_killer_request_does_not_burn_the_restart_budget()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:134` | `#[tokio::test] async fn initialized_non_owning_transport_serves_hover_without_initialize_or_child()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:1225` | `#[tokio::test] async fn test_tsgo_hover_on_ts_file()` | `let hover = provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:1274` | `#[tokio::test] async fn test_tsgo_survives_workspace_configuration()` | `let hover_result = provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3776` | `#[tokio::test] async fn e2e_concurrent_requests_complete_without_deadlock()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3777` | `#[tokio::test] async fn e2e_concurrent_requests_complete_without_deadlock()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3778` | `#[tokio::test] async fn e2e_concurrent_requests_complete_without_deadlock()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3779` | `#[tokio::test] async fn e2e_concurrent_requests_complete_without_deadlock()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3780` | `#[tokio::test] async fn e2e_concurrent_requests_complete_without_deadlock()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4328` | `#[tokio::test] async fn contents_cache_miss_fails_closed_without_fabricating_positions()` | `let hover = provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4410` | `#[tokio::test] async fn contents_cache_hit_still_sends_the_hover_request()` | `hover_task = tokio::spawn(async move { provider.` |

**`doc-comment`** (16)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:20` | `` | `/// Byte offsets the bridge fed to '` |
| `crates/verter_lsp/src/integration_tests.rs:3497` | `` | `/// 4. Query type_provider.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:119` | `` | `/// query observes the down state — a '` |
| `crates/verter_lsp/src/tsgo/composite.rs:606` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:175` | `` | `pted transient hover failures: while > 0, each '` |
| `crates/verter_lsp/src/type_provider/mock.rs:195` | `` | `/// As 'hang_definition', for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:301` | `` | `/// query ('` |
| `crates/verter_lsp/src/type_provider/mock.rs:363` | `impl MockTypeProvider` | `/// Script the next 'count' '` |
| `crates/verter_lsp/src/type_provider/mock.rs:395` | `impl MockTypeProvider` | `/// Wedge '` |
| `crates/verter_lsp/src/type_provider/mock.rs:552` | `impl MockTypeProvider` | `/// ('` |
| `crates/verter_session/src/typeinfo/oracle_core/hover_extract.rs:29` | `` | `//! ('` |
| `crates/verter_session/tests/cases/oracle_tsgo_forbidden.rs:197` | `` | `/// or a '` |
| `crates/verter_session/tests/cases/oracle_tsgo_forbidden.rs:312` | `` | `/// - '.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:92` | `` | `/// When set for a path, '` |
| `crates/verter_type_runtime/src/resilient_tests.rs:119` | `impl MockProvider` | `/// Make every subsequent '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:1693` | `` | `/// the 'MarkedString[]' normalization arms in '` |

**`comment`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_session/tests/cases/oracle_tsgo_forbidden.rs:381` | `#[test] fn tsgo_runtime_driver_checker_discriminates()` | `// (4) A '--lsp --stdio' argv or a '.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1042` | `pub(crate) async fn assert_carriers_answer_typed(&self)` | `// real '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3007` | `ns( &self, path: &str, offset: u32, trigger_character: Option<&str>, ) -> ProviderFuture<'_, CompletionResult>` | `// position for the engine (see '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3425` | `fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `// position for the engine (see '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3495` | `fn get_type_definition( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeLocation>>` | `// position for the engine (see '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3558` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `// position for the engine (see '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3607` | `fn get_rename_locations( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<RenameLocation>>` | `// position for the engine (see '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3668` | `fn get_signature_help( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Option<SignatureHelp>>` | `// position for the engine (see '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3859` | `n get_document_highlights( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>>` | `// position for the engine (see '` |

**`string-literal`** (10)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/future_size_measure_tests.rs:579` | `#[tokio::test] #[ignore = ] async fn measure_handler_future_sizes()` | `"TypeProvider::` |
| `crates/verter_lsp/src/integration_tests.rs:3622` | `#[tokio::test] async fn integration_hover_merge_with_mock_type_provider()` | `"mock` |
| `crates/verter_lsp/src/server_tests.rs:31941` | `#[tokio::test] async fn shipped_hover_has_no_feature_latency_deadline()` | `"the handler must have reached the wedged` |
| `crates/verter_session/tests/cases/oracle_tsgo_forbidden.rs:206` | `#[test] fn oracle_consumption_path_has_no_tsgo_spawn()` | `".` |
| `crates/verter_session/tests/cases/oracle_tsgo_forbidden.rs:334` | `fn src_has_tsgo_runtime_driver(src: &str) -> Option<&'static str>` | `(".` |
| `crates/verter_session/tests/cases/oracle_tsgo_forbidden.rs:334` | `fn src_has_tsgo_runtime_driver(src: &str) -> Option<&'static str>` | `(".get_hover(", "calls the tsgo hover RPC '.` |
| `crates/verter_session/tests/cases/oracle_tsgo_forbidden.rs:383` | `#[test] fn tsgo_runtime_driver_checker_discriminates()` | `!(src_has_tsgo_runtime_driver("let h = provider.` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:188` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `"TypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3229` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `tracing::debug!("TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3246` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `"TSGO` |

### `provider_wire_witness` — declared at `crates/verter_type_runtime/src/traits.rs:206`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:206` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`call-production`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:392` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `let witness = self.` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3234` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `let witness = self.` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3497` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `let witness = self.` |

**`call-test`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:65` | `#[test] fn normalized_hover_mirrors_display_signature_as_plain_string()` | `MockProvider::default().` |
| `crates/verter_lsp/src/type_provider/mock.rs:24` | `pub fn test_display_signature(value: &str) -> DisplaySignature` | `ure::from_provider_wire(MockTypeProvider::new().` |
| `crates/verter_type_runtime/src/provider_adapter.rs:563` | `fn test_display_signature(value: &str) -> DisplaySignature` | `MockTypeProvider::default().` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:136` | `#[test] fn display_signature_exposes_only_the_labelled_accessor()` | `WitnessOnlyProvider.` |

**`doc-comment`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/protocol.rs:334` | `` | `/// (['crate::traits::TypeProvider::` |
| `crates/verter_type_runtime/tests/cases/compile-fail/display_signature_witness_forge.rs:2` | `` | `//! impl ('` |

### `get_diagnostics` — declared at `crates/verter_type_runtime/src/traits.rs:210`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:210` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (11)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/documents/mod.rs:1411` | `impl DocumentRegistry` | `pub fn` |
| `crates/verter_lsp/src/extension_provider.rs:470` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:992` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1031` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:712` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:526` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_session/src/host_manage/analysis_io.rs:2011` | `impl VerterHost` | `pub fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:223` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3378` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:509` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3836` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:118` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3072` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:258` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:423` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:600` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:862` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1075` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:799` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1253` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:160` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1107` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:81` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:681` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:236` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:63` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:469` | `async fn on_diagnostics(&mut self, d: DiagnosticsRequest) -> Response` | `match provider.` |
| `crates/verter_lsp/src/server/sync_orchestration.rs:111` | `pub(super) async fn compute_full_diagnostics(&self, uri: &Uri) -> Vec<Diagnostic>` | `match tp.` |
| `crates/verter_lsp/src/server_utils.rs:1705` | `g, CachedVerterDiagEntry>, vfs_workspace: Option<&verter_workspace::FilesystemWorkspace>, ) -> Vec<Diagnostic>` | `let host_diags = documents.` |
| `crates/verter_lsp/src/sync_coordinator.rs:1497` | `oordinatorDeps, tp: &dyn TypeProvider, canonical_id: &str, verter_diags: Vec<Diagnostic>, ) -> Vec<Diagnostic>` | `match tp.` |
| `crates/verter_lsp/src/sync_coordinator.rs:1740` | `vider, encoding: PositionEncodingKind, canonical_id: &str, verter_diags: Vec<Diagnostic>, ) -> Vec<Diagnostic>` | `match tp.` |
| `crates/verter_lsp/src/tsgo/composite.rs:823` | ` managed_diagnostics( &self, path: &str, background: bool, ) -> Result<Vec<TypeDiagnostic>, TypeProviderError>` | `self.managed.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:429` | `fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `let _ = lsp.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:452` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `let _ = lsp.` |

**`call-forwarding`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/documents/mod.rs:1414` | `pub fn get_diagnostics(&self, uri: &Uri) -> Option<verter_session::DiagnosticsSnapshot>` | `.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:717` | `fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:528` | `fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `Box::pin(async move { self.activate().await?.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:228` | `fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:510` | `fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `self.lsp.` |

**`call-trait-default`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:476` | `fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `self.` |

**`call-test`** (42)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:1199` | `#[tokio::test] #[ignore = ] async fn provider_resolves_barrel_reexport_through_rewritten_twin()` | `.` |
| `crates/verter_dx_baseline/src/main_tests.rs:1203` | `#[tokio::test] #[ignore = ] async fn provider_resolves_barrel_reexport_through_rewritten_twin()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:414` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:941` | `#[tokio::test] async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result()` | `provider.` |
| `crates/verter_lsp/src/integration_tests.rs:537` | `#[test] fn integration_diagnostics_for_valid_sfc()` | `let diags = registry.` |
| `crates/verter_lsp/src/integration_tests.rs:566` | `#[test] fn integration_pull_diagnostics_valid_sfc()` | `let verter_diags = match registry.` |
| `crates/verter_lsp/src/integration_tests.rs:597` | `#[test] fn integration_pull_diagnostics_invalid_template()` | `let verter_diags = match registry.` |
| `crates/verter_lsp/src/integration_tests.rs:2759` | `#[test] fn multithread_did_change_with_concurrent_reads()` | `reg4.` |
| `crates/verter_lsp/src/integration_tests.rs:3238` | `#[test] fn stress_test_no_deadlock_under_heavy_concurrent_load()` | `let _ = reg.` |
| `crates/verter_lsp/src/integration_tests.rs:4100` | `#[tokio::test] async fn diagnostic_pull_does_not_force_sync()` | `let _ = mock.` |
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:199` | `async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession)` | `.` |
| `crates/verter_lsp/src/real_provider_tests/diagnostics.rs:217` | `vider_test!( diagnostics_svelte_invalid_component_prop_remains_type_checked, fixture = , async fn run(session)` | `Some(path) => session.provider().` |
| `crates/verter_lsp/src/real_provider_tests/diagnostics.rs:616` | `ession: &RealProviderTestSession, provider_path: &str, ) -> Vec<verter_type_runtime::protocol::TypeDiagnostic>` | `match session.provider().` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3079` | ` ) -> crate::type_provider::traits::ProviderFuture< '_, Vec<crate::type_provider::protocol::TypeDiagnostic>, >` | `self.inner.` |
| `crates/verter_lsp/src/server_tests.rs:33052` | `[tokio::test(flavor = , worker_threads = 4)] async fn a_style_only_edit_does_not_erase_the_files_diagnostics()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:464` | `#[tokio::test] async fn gate_bound_carrier_delegates_to_owned()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:480` | `#[tokio::test] async fn gate_empty_snapshot_carrier_fails_closed_to_empty()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:496` | `#[tokio::test] async fn gate_no_project_carrier_fails_closed_to_empty()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:513` | `#[tokio::test] async fn gate_non_carrier_path_is_not_gated()` | `.` |
| `crates/verter_lsp/tests/cases/quoted_prop_consumer_mistype_live.rs:268` | `#[tokio::test] async fn quoted_prop_consumer_mistype_surfaces_ts2322_tsserver()` | `.` |
| `crates/verter_lsp/tests/cases/quoted_prop_consumer_mistype_live.rs:299` | `#[tokio::test] async fn quoted_prop_consumer_mistype_surfaces_ts2322_tsserver()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1562` | `tokio::test(flavor = , worker_threads = 4)] async fn composite_overlays_shared_diagnostics_via_live_resolver()` | `h.composite.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1669` | `(flavor = , worker_threads = 4)] async fn composite_successful_shared_route_never_activates_managed_fallback()` | `h.composite.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:2040` | `async fn assert_template_member_served_typed(h: &CompositeHarness, topology: &str)` | `h.composite.` |
| `crates/verter_session/src/compile_content_publish_fence_tests.rs:480` | `fn stored_diagnostics(host: &VerterHost, profile: &CompileProfile) -> Option<usize>` | `host.` |
| `crates/verter_session/src/framework/framework_product_surface_tests.rs:1282` | `#[test] fn the_diagnostics_route_answers_from_the_compile_slot_it_names()` | `host.` |
| `crates/verter_session/src/framework/framework_product_surface_tests.rs:1296` | `#[test] fn the_diagnostics_route_answers_from_the_compile_slot_it_names()` | `.` |
| `crates/verter_session/src/host_resolve_tests.rs:3831` | `#[test] fn duplicate_attribute_regression_does_not_appear_through_host_pipeline()` | `.` |
| `crates/verter_session/src/host_resolve_tests.rs:4029` | `#[test] fn evict_advances_the_diagnostics_epoch_whose_diagnostics_it_clears()` | `host.` |
| `crates/verter_session/src/host_resolve_tests.rs:4043` | `#[test] fn evict_advances_the_diagnostics_epoch_whose_diagnostics_it_clears()` | `host.` |
| `crates/verter_session/src/host_resolve_tests.rs:4162` | `#[test] fn ensure_compiled_skips_non_sfc_files()` | `let diags = host.` |
| `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_gen_spike.rs:282` | `#[tokio::test(flavor = , worker_threads = 2)] async fn spike_clean_probe_has_zero_diagnostics()` | `time::timeout(Duration::from_secs(15), provider.` |
| `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_gen_spike.rs:473` | `#[tokio::test(flavor = , worker_threads = 2)] async fn spike_nolib_forces_off_bundled_libs()` | `time::timeout(Duration::from_secs(15), provider.` |
| `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_gen_spike.rs:495` | `#[tokio::test(flavor = , worker_threads = 2)] async fn spike_nolib_forces_off_bundled_libs()` | `time::timeout(Duration::from_secs(15), provider.` |
| `crates/verter_session/tests/cases/g_misc0/host_tests.rs:1573` | `#[test] fn get_diagnostics_without_compilation()` | `let diags = host.` |
| `crates/verter_session/tests/cases/g_misc0/host_tests.rs:2069` | `#[test] fn get_diagnostics_nonexistent_returns_none()` | `.` |
| `crates/verter_session/tests/cases/g_misc0/host_tests.rs:2083` | `#[test] fn get_diagnostics_no_profile_match_returns_none()` | `assert!(host.` |
| `crates/verter_session/tests/cases/svelte_compiler_block1.rs:979` | `#[test] fn svelte_projector_diagnostic_reaches_diagnostics_snapshot()` | `.` |
| `crates/verter_session/tests/cases/svelte_compiler_block1.rs:1013` | `#[test] fn recoverable_svelte_parse_diagnostic_reaches_host_once_with_authored_span()` | `.` |
| `crates/verter_session/tests/cases/svelte_compiler_block1.rs:1049` | `#[test] fn well_formed_svelte_has_no_parse_diagnostics()` | `.` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:202` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `let fut = tp.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3172` | `#[tokio::test] async fn test_provider_operations_fail_after_process_death()` | `result = tokio::time::timeout(timeout, provider.` |

**`doc-comment`** (22)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/integration_tests.rs:2687` | `` | `///` |
| `crates/verter_lsp/src/real_provider_tests/diagnostics.rs:3` | `` | `//! These exercise the provider's own '` |
| `crates/verter_lsp/src/sync_coordinator.rs:843` | `` | `/// '` |
| `crates/verter_lsp/src/sync_coordinator_tests.rs:3643` | `` | `/// 'latest_diagnostics', so after an edit '` |
| `crates/verter_lsp/src/sync_coordinator_tests.rs:4162` | `` | `/// '` |
| `crates/verter_lsp/src/tsgo/overlay_core.rs:12` | `` | `- **Query (OFF the foreground-sync budget).** '` |
| `crates/verter_lsp/src/type_provider/mock.rs:199` | `` | `/// As 'hang_definition', for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:410` | `impl MockTypeProvider` | `/// Wedge '` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:15` | `` | `fall-through (the pre-fix composite delegated '` |
| `crates/verter_session/src/compile_content_publish_fence_tests.rs:487` | `` | `agnostics' has no key to reject a stale write: '` |
| `crates/verter_session/src/framework/framework_product_surface_tests.rs:1258` | `` | `/// '` |
| `crates/verter_session/src/host_manage.rs:4` | `` | `//! ['VerterHost::` |
| `crates/verter_session/src/host_manage/analysis_io.rs:7` | `` | `//! '` |
| `crates/verter_session/src/host_manage/analysis_io.rs:2047` | `impl VerterHost` | `/// ['Self::` |
| `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:3040` | `impl VerterHost` | `/// the CURRENT buffer: '` |
| `crates/verter_session/tests/cases/g_misc0/host_tests.rs:1548` | `` | `/// @ai-generated -` |
| `crates/verter_session/tests/cases/g_misc0/host_tests.rs:2064` | `` | `/// @ai-generated -` |
| `crates/verter_session/tests/cases/g_misc0/host_tests.rs:2073` | `` | `/// @ai-generated -` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:20` | `` | `ject') BEFORE delegating to 'TsgoOwnedProvider::` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:192` | `impl TsgoOwnedProvider` | `s DISTINCT from the user-facing ['TypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:439` | `impl TypeProvider for TsgoOwnedProvider` | `/// adds a synchronous '` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:334` | `` | `vider overrides 'open_file' with a synchronous '` |

**`comment`** (13)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:260` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `//` |
| `crates/verter_lsp/src/server/lifecycle.rs:898` | `pub(super) async fn handle_did_change( server: &VerterLanguageServer, params: DidChangeTextDocumentParams, )` | `// '` |
| `crates/verter_lsp/src/server_tests.rs:23007` | `#[test] fn compute_verter_diagnostics_bypasses_cache_after_host_recompile()` | `compile with the tsx_profile (same as documents.` |
| `crates/verter_lsp/src/sync_coordinator_tests.rs:2064` | `#[tokio::test(flavor = )] async fn hanging_provider_diagnostics_do_not_starve_verter_owned_batch()` | `// reached '` |
| `crates/verter_lsp/src/sync_coordinator_tests.rs:2069` | `#[tokio::test(flavor = )] async fn hanging_provider_diagnostics_do_not_starve_verter_owned_batch()` | `// 'MockTypeProvider::` |
| `crates/verter_lsp/src/tsgo/composite.rs:868` | `impl TypeProvider for TsgoCompositeProvider` | `// The query path ('` |
| `crates/verter_session/tests/cases/g_misc0/host_tests.rs:1545` | `` | `// Task 5:` |
| `crates/verter_session/tests/cases/g_misc0/host_tests.rs:1572` | `#[test] fn get_diagnostics_without_compilation()` | `//` |
| `crates/verter_session/tests/cases/g_misc0/host_tests.rs:2061` | `` | `//` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:853` | `#[test] fn no_fallback_to_inferred_anywhere()` | `te body before delegating to TsgoOwnedProvider::` |
| `crates/verter_type_runtime/src/tsgo/owned_tests.rs:352` | `#[tokio::test] async fn load_file_is_local_only_and_never_opens_or_barriers_on_the_lsp_surface()` | `// '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3516` | `fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>` | `// COLD-build recovery (mirrors '` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:136` | `(flavor = , worker_threads = 2)] async fn owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process()` | `// proof, distinct from the user-facing` |

**`string-literal`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:416` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.expect("` |
| `crates/verter_lsp/src/server/request_surface_guard_tests.rs:258` | `#[test] fn background_diagnostics_paths_use_captured_surface_and_revalidate()` | `for forbidden in [".` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:859` | `#[test] fn no_fallback_to_inferred_anywhere()` | `REQ-1) before delegating to TsgoOwnedProvider::` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:204` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `"TypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3388` | `fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `"` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3397` | `fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `"` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3173` | `#[tokio::test] async fn test_provider_operations_fail_after_process_death()` | `assert!(result.is_ok(), "` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3177` | `#[tokio::test] async fn test_provider_operations_fail_after_process_death()` | `"` |

### `get_definition` — declared at `crates/verter_type_runtime/src/traits.rs:212`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:212` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:541` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1084` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1078` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:722` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:531` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:234` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3409` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:551` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4012` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:121` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3082` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:262` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:427` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:604` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:866` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1079` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:804` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1284` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:209` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1110` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:85` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:685` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:240` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:67` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:390` | `async fn on_query(&mut self, q: QueryRequest) -> Response` | `QueryMethod::Definition => match provider.` |
| `crates/verter_lsp/src/server/child_prop_rename.rs:462` | `pe_provider: &dyn crate::type_provider::traits::TypeProvider, parent_tsx_path: &str, parent_tsx_offset: u32, )` | `.` |
| `crates/verter_lsp/src/server/nav_features_navigation.rs:100` | `tion( server: &VerterLanguageServer, params: GotoDefinitionParams, ) -> Result<Option<GotoDefinitionResponse>>` | `if let Ok(type_defs) = tp.` |
| `crates/verter_lsp/src/server/nav_features_navigation.rs:323` | `tion( server: &VerterLanguageServer, params: GotoDefinitionParams, ) -> Result<Option<GotoDefinitionResponse>>` | `let type_defs = tp.` |
| `crates/verter_lsp/src/server/nav_features_navigation.rs:431` | `tion( server: &VerterLanguageServer, params: GotoDefinitionParams, ) -> Result<Option<GotoDefinitionResponse>>` | `match tp.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:101` | `efinition_type_at_offset( &self, path: &str, generated_offset: u32, ) -> Result<BackendTypeData, BackendError>` | `.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1093` | `fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1079` | `fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:727` | `fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:533` | `fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `Box::pin(async move { self.activate().await?.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:239` | `fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:552` | `fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `self.lsp.` |

**`call-test`** (12)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/future_size_measure_tests.rs:571` | `#[tokio::test] #[ignore = ] async fn measure_handler_future_sizes()` | `let fut = tp.` |
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:133` | `async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession)` | `.` |
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:171` | `async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession)` | `.` |
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:372` | `async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession)` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3090` | `2, ) -> crate::type_provider::traits::ProviderFuture< '_, Vec<crate::type_provider::protocol::TypeLocation>, >` | `self.inner.` |
| `crates/verter_lsp/src/test_harness.rs:1074` | `fn raw_provider_definitions( &self, uri: &Uri, position: Position, ) -> Vec<verter_type_runtime::TypeLocation>` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:984` | `#[tokio::test] async fn feature_mixed_read_denied_carrier_serves_external_default_no_owned_call()` | `c.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1049` | `#[tokio::test] async fn feature_mixed_read_bound_carrier_delegates_to_owned()` | `!c.` |
| `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_gen_spike.rs:252` | `#[tokio::test(flavor = , worker_threads = 2)] async fn spike_definition_primitive_binds_to_intended_decl()` | `provider.` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:178` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `let fut = tp.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4339` | `#[tokio::test] async fn contents_cache_miss_fails_closed_without_fabricating_positions()` | `provider.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:156` | `(flavor = , worker_threads = 2)] async fn owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process()` | `.` |

**`doc-comment`** (13)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/child_prop_rename.rs:8` | `` | `/! ('defineProps<ImportedType>()') declaration '` |
| `crates/verter_lsp/src/server/child_prop_rename.rs:52` | `` | `/// '` |
| `crates/verter_lsp/src/server/child_prop_rename.rs:352` | `impl VerterLanguageServer` | `/// '` |
| `crates/verter_lsp/src/server/child_prop_rename.rs:415` | `impl VerterLanguageServer` | `/// declaration target by a single provider '` |
| `crates/verter_lsp/src/server/child_prop_rename.rs:655` | `` | `/// '` |
| `crates/verter_lsp/src/server/nav_features_navigation_tests.rs:243` | `` | `/// whose provider '` |
| `crates/verter_lsp/src/server_tests.rs:30635` | `` | `/// (production default); the mock's '` |
| `crates/verter_lsp/src/tsgo/composite.rs:608` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:181` | `` | `/// As 'fail_next_hovers', for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:190` | `` | `/// When 'true', '` |
| `crates/verter_lsp/src/type_provider/mock.rs:371` | `impl MockTypeProvider` | `/// Script the next 'count' '` |
| `crates/verter_lsp/src/type_provider/mock.rs:387` | `impl MockTypeProvider` | `/// Make every subsequent '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4226` | `` | `e per-target content lookup 'get_references' / '` |

**`comment`** (11)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/child_prop_rename.rs:77` | `Uri, Range)], prop_name: &str, spells_prop_name: impl Fn(&Uri, Range, &str) -> bool, ) -> Option<(Uri, Range)>` | `// member resolves '` |
| `crates/verter_lsp/src/server/child_prop_rename.rs:381` | `pub(super) fn classify_child_prop_rename( &self, uri: &Uri, position: &Position, ) -> ChildPropRenameClass` | `// upgrades it via a provider '` |
| `crates/verter_lsp/src/server/child_prop_rename.rs:474` | `pe_provider: &dyn crate::type_provider::traits::TypeProvider, parent_tsx_path: &str, parent_tsx_offset: u32, )` | `// '` |
| `crates/verter_lsp/src/server/mod.rs:70` | `` | `// '` |
| `crates/verter_lsp/src/server/nav_features_navigation.rs:1092` | `sync fn handle_rename( server: &VerterLanguageServer, params: RenameParams, ) -> Result<Option<WorkspaceEdit>>` | `// mid-capture. The declaration '` |
| `crates/verter_lsp/src/server/nav_features_navigation.rs:1121` | `sync fn handle_rename( server: &VerterLanguageServer, params: RenameParams, ) -> Result<Option<WorkspaceEdit>>` | `// '` |
| `crates/verter_lsp/src/server/nav_features_navigation_tests.rs:309` | `#[test] fn confirmed_unknown_declaration_usage_only_fails_closed()` | `// whose provider` |
| `crates/verter_lsp/src/server/nav_features_navigation_tests.rs:403` | `#[test] fn confirmed_imported_usage_only_fails_closed()` | `// closed. Even when '` |
| `crates/verter_lsp/src/server_tests.rs:10124` | `#[tokio::test] async fn goto_type_definition_delegates_to_provider()` | `ovider was called with get_type_definition (not` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:261` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `// HEAPinside Box::pin for TsgoTypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3581` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `lback inside 'parse_lsp_location'), exactly as '` |

**`string-literal`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/future_size_measure_tests.rs:573` | `#[tokio::test] #[ignore = ] async fn measure_handler_future_sizes()` | `"TypeProvider::` |
| `crates/verter_lsp/src/server_tests.rs:10137` | `#[tokio::test] async fn goto_type_definition_delegates_to_provider()` | `"handler should NOT call` |
| `crates/verter_lsp/src/server_tests.rs:30703` | `#[tokio::test] async fn production_definition_handler_fails_closed_when_the_provider_wedges()` | `e handler must have actually reached the wedged` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:180` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `"TypeProvider::` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:280` | `#[tokio::test] #[ignore = ] async fn measure_type_runtime_future_sizes()` | `"unboxed` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3410` | `fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `tracing::debug!("TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3427` | `fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `"TSGO` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:158` | `(flavor = , worker_threads = 2)] async fn owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process()` | `.expect("` |

### `get_type_definition` — declared at `crates/verter_type_runtime/src/traits.rs:214`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:214` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:584` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1100` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1082` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:732` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:536` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:245` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3475` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:555` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4066` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:124` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3093` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:266` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:431` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:608` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:870` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1083` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:813` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1331` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:213` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1113` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:89` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:694` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:244` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:71` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:397` | `async fn on_query(&mut self, q: QueryRequest) -> Response` | `match provider.` |
| `crates/verter_lsp/src/server/nav_features_navigation.rs:556` | `tion( server: &VerterLanguageServer, params: GotoDefinitionParams, ) -> Result<Option<GotoDefinitionResponse>>` | `if let Ok(type_defs) = tp.` |
| `crates/verter_lsp/src/server/nav_features_navigation.rs:645` | `tion( server: &VerterLanguageServer, params: GotoDefinitionParams, ) -> Result<Option<GotoDefinitionResponse>>` | `let type_defs = tp.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1114` | `fn get_type_definition( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeLocation>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1087` | `fn get_type_definition( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeLocation>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:741` | `fn get_type_definition( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeLocation>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:545` | `fn get_type_definition( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeLocation>>` | `.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:254` | `fn get_type_definition( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeLocation>>` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:560` | `fn get_type_definition( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeLocation>>` | `self.lsp.` |

**`call-test`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:249` | `async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession)` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3101` | `2, ) -> crate::type_provider::traits::ProviderFuture< '_, Vec<crate::type_provider::protocol::TypeLocation>, >` | `self.inner.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:884` | `#[tokio::test] async fn feature_external_only_denied_carrier_serves_empty_no_owned_call()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:932` | `#[tokio::test] async fn feature_external_only_bound_carrier_delegates_to_owned()` | `!c.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:958` | `#[tokio::test] async fn feature_external_only_plain_ts_is_ungated()` | `!c.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4344` | `#[tokio::test] async fn contents_cache_miss_fails_closed_without_fabricating_positions()` | `.` |

**`doc-comment`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:599` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:188` | `` | `/// As 'fail_next_definitions', for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:379` | `impl MockTypeProvider` | `/// Script the next 'count' '` |

**`comment`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:10124` | `#[tokio::test] async fn goto_type_definition_delegates_to_provider()` | `// Verify the provider was called with` |

**`string-literal`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:10130` | `#[tokio::test] async fn goto_type_definition_delegates_to_provider()` | `"handler should delegate to` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3480` | `fn get_type_definition( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeLocation>>` | `tracing::debug!("TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3497` | `fn get_type_definition( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeLocation>>` | `"TSGO` |

### `get_references` — declared at `crates/verter_type_runtime/src/traits.rs:217`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:217` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:631` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1121` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1090` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:746` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:550` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:260` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3545` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:563` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4124` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:127` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3104` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:274` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:439` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:616` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:878` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1091` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:822` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1363` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:221` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1120` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:97` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:702` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:252` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:79` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:404` | `async fn on_query(&mut self, q: QueryRequest) -> Response` | `QueryMethod::References => match provider.` |
| `crates/verter_lsp/src/server/nav_features_navigation.rs:742` | ` handle_references( server: &VerterLanguageServer, params: ReferenceParams, ) -> Result<Option<Vec<Location>>>` | `if let Ok(type_refs) = tp.` |
| `crates/verter_lsp/src/server/nav_features_navigation.rs:889` | ` handle_references( server: &VerterLanguageServer, params: ReferenceParams, ) -> Result<Option<Vec<Location>>>` | `match tp.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1130` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1091` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:751` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:552` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `Box::pin(async move { self.activate().await?.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:265` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:564` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `self.lsp.` |

**`call-test`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:289` | `async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession)` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3112` | `2, ) -> crate::type_provider::traits::ProviderFuture< '_, Vec<crate::type_provider::protocol::TypeLocation>, >` | `self.inner.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:991` | `#[tokio::test] async fn feature_mixed_read_denied_carrier_serves_external_default_no_owned_call()` | `c.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1053` | `#[tokio::test] async fn feature_mixed_read_bound_carrier_delegates_to_owned()` | `!c.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4351` | `#[tokio::test] async fn contents_cache_miss_fails_closed_without_fabricating_positions()` | `provider.` |

**`doc-comment`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:610` | `` | `/// '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4226` | `` | `// file. Mirrors the per-target content lookup '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:1799` | `` | `/ 'parse_lsp_locations_per_target' (the helper '` |

**`string-literal`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3546` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `tracing::debug!("TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3560` | `fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>` | `"TSGO` |

### `get_rename_locations` — declared at `crates/verter_type_runtime/src/traits.rs:219`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:219` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:675` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1137` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1094` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:756` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:555` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:271` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3591` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:567` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4179` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:130` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3003` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:278` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:443` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:620` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:882` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1095` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:831` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1392` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:225` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1123` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:101` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:710` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:256` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:83` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/nav_features_navigation.rs:1135` | `sync fn handle_rename( server: &VerterLanguageServer, params: RenameParams, ) -> Result<Option<WorkspaceEdit>>` | `match tp.` |
| `crates/verter_lsp/src/server/rename_prepare.rs:183` | `es::Uri, position: &tower_lsp_server::ls_types::Position, anchor: tower_lsp_server::ls_types::Range, ) -> bool` | `.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1152` | `fn get_rename_locations( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<RenameLocation>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1099` | `fn get_rename_locations( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<RenameLocation>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:765` | `fn get_rename_locations( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<RenameLocation>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:564` | `fn get_rename_locations( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<RenameLocation>>` | `.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:280` | `fn get_rename_locations( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<RenameLocation>>` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:572` | `fn get_rename_locations( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<RenameLocation>>` | `self.lsp.` |

**`call-test`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:322` | `async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession)` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3012` | ` ) -> crate::type_provider::traits::ProviderFuture< '_, Vec<crate::type_provider::protocol::RenameLocation>, >` | `let recorded = self.inner.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1174` | `#[tokio::test] async fn feature_rename_denied_carrier_serves_native_only_no_owned_call()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1196` | `#[tokio::test] async fn feature_rename_bound_carrier_delegates_to_owned()` | `!c.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4356` | `#[tokio::test] async fn contents_cache_miss_fails_closed_without_fabricating_positions()` | `.` |

**`doc-comment`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2974` | `` | `/// except that '` |
| `crates/verter_lsp/src/server_tests.rs:32372` | `` | `/// ('TsgoCompositeProvider::` |
| `crates/verter_lsp/src/tsgo/composite.rs:622` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:268` | `` | `/// One-shot async gate for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:697` | `impl MockTypeProvider` | `/// Pause the next '` |

**`comment`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/nav_features_navigation.rs:1143` | `sync fn handle_rename( server: &VerterLanguageServer, params: RenameParams, ) -> Result<Option<WorkspaceEdit>>` | `//` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1287` | `#[tokio::test] async fn instance_member_anchor_consults_the_provider_and_ships_nothing_when_it_answers_empty()` | `// No configured response: 'MockTypeProvider::` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1516` | `#[tokio::test] async fn prepare_rename_at_an_instance_member_declines_when_the_provider_answers_empty()` | `// No configured response: '` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1163` | `` | `// ── RENAME carrier feature (` |

**`string-literal`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3609` | `fn get_rename_locations( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<RenameLocation>>` | `"TSGO` |

### `get_signature_help` — declared at `crates/verter_type_runtime/src/traits.rs:225`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:225` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:755` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1159` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1102` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:770` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:569` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:286` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3652` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:575` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4270` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:133` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3115` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:286` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:451` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:628` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:890` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1103` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:840` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1427` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:239` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1130` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:109` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:718` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:264` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:91` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/aux_features.rs:301` | `signature_help( server: &VerterLanguageServer, params: SignatureHelpParams, ) -> Result<Option<SignatureHelp>>` | `if let Ok(type_sig) = tp.` |
| `crates/verter_lsp/src/server/aux_features.rs:326` | `signature_help( server: &VerterLanguageServer, params: SignatureHelpParams, ) -> Result<Option<SignatureHelp>>` | `if let Ok(type_sig) = tp.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1171` | `fn get_signature_help( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Option<SignatureHelp>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1107` | `fn get_signature_help( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Option<SignatureHelp>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:779` | `fn get_signature_help( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Option<SignatureHelp>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:578` | `fn get_signature_help( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Option<SignatureHelp>>` | `.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:295` | `fn get_signature_help( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Option<SignatureHelp>>` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:580` | `fn get_signature_help( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Option<SignatureHelp>>` | `self.lsp.` |

**`call-test`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:945` | `#[tokio::test] async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result()` | `provider.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3123` | ` -> crate::type_provider::traits::ProviderFuture< '_, Option<crate::type_provider::protocol::SignatureHelp>, >` | `self.inner.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:893` | `#[tokio::test] async fn feature_external_only_denied_carrier_serves_empty_no_owned_call()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:936` | `#[tokio::test] async fn feature_external_only_bound_carrier_delegates_to_owned()` | `c.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4362` | `#[tokio::test] async fn contents_cache_miss_fails_closed_without_fabricating_positions()` | `let signature_help = provider.` |

**`doc-comment`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:601` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:197` | `` | `/// As 'hang_definition', for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:402` | `impl MockTypeProvider` | `/// Wedge '` |

**`string-literal`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:31989` | `#[tokio::test] async fn shipped_signature_help_has_no_feature_latency_deadline()` | `"the handler must have reached the wedged` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3670` | `fn get_signature_help( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Option<SignatureHelp>>` | `"TSGO` |

### `get_code_actions` — declared at `crates/verter_type_runtime/src/traits.rs:241`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:241` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (11)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:903` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1178` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1110` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:784` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:583` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_napi/src/lib.rs:2315` | `#[napi] impl NapiVerterHost` | `pub fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:301` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3695` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:583` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4428` | `impl TypeProvider for TsserverTypeProvider` | `fn` |
| `crates/verter_wasm/src/lib.rs:740` | `#[wasm_bindgen(js_class = VerterHost)] impl WasmVerterHost` | `pub fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:136` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3126` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:294` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:459` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:636` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:898` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1111` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:849` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1464` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:253` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1137` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:117` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:726` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:272` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:99` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/aux_features.rs:548` | `_code_action( server: &VerterLanguageServer, params: CodeActionParams, ) -> Result<Option<CodeActionResponse>>` | `tp.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1198` | ` u32, end_offset: u32, diagnostics: &[ProviderDiagnosticContext], ) -> ProviderFuture<'_, Vec<TypeCodeAction>>` | `.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1118` | ` u32, end_offset: u32, diagnostics: &[ProviderDiagnosticContext], ) -> ProviderFuture<'_, Vec<TypeCodeAction>>` | `.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:796` | ` u32, end_offset: u32, diagnostics: &[ProviderDiagnosticContext], ) -> ProviderFuture<'_, Vec<TypeCodeAction>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:595` | ` u32, end_offset: u32, diagnostics: &[ProviderDiagnosticContext], ) -> ProviderFuture<'_, Vec<TypeCodeAction>>` | `.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:319` | ` u32, end_offset: u32, diagnostics: &[ProviderDiagnosticContext], ) -> ProviderFuture<'_, Vec<TypeCodeAction>>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:591` | ` u32, end_offset: u32, diagnostics: &[ProviderDiagnosticContext], ) -> ProviderFuture<'_, Vec<TypeCodeAction>>` | `.` |

**`call-test`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:573` | `#[tokio::test] async fn extension_provider_get_code_actions_surfaces_single_and_combined_unused_fix()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:728` | `#[tokio::test] async fn extension_provider_combined_fix_uses_content_current_as_of_each_response()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:970` | `#[tokio::test] async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result()` | `.` |
| `crates/verter_lsp/src/server/aux_features.rs:726` | ` uri: &Uri, range: Range, diagnostics: &[Diagnostic], ) -> Vec<crate::type_provider::protocol::TypeCodeAction>` | `tp.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3137` | ` ) -> crate::type_provider::traits::ProviderFuture< '_, Vec<crate::type_provider::protocol::TypeCodeAction>, >` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1219` | `#[tokio::test] async fn feature_code_actions_denied_carrier_serves_native_only_no_owned_call()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1241` | `#[tokio::test] async fn feature_code_actions_bound_carrier_delegates_to_owned()` | `!c.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1258` | `#[tokio::test] async fn feature_code_actions_plain_ts_is_ungated()` | `!c.` |

**`doc-comment`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:484` | `` | `F2 (review finding): the extension provider's '` |
| `crates/verter_lsp/src/server/aux_features.rs:685` | `` | `_code_action'] does, then calls the provider's '` |
| `crates/verter_lsp/src/server_tests.rs:22395` | `` | `/// '` |
| `crates/verter_lsp/src/server_tests.rs:22545` | `` | `/// '` |
| `crates/verter_lsp/src/tsgo/composite.rs:625` | `` | `/// '` |
| `crates/verter_type_runtime/src/protocol.rs:757` | `` | `/// shape before calling ['TypeProvider::` |

**`comment`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/aux_features.rs:498` | `_code_action( server: &VerterLanguageServer, params: CodeActionParams, ) -> Result<Option<CodeActionResponse>>` | `// The provider's '` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1205` | `` | `// ── CODE-ACTIONS carrier feature (` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1208` | `` | `// MERGES the provider's '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2835` | `fn build_client_capabilities() -> serde_json::Value` | `// '` |

**`string-literal`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:575` | `#[tokio::test] async fn extension_provider_get_code_actions_surfaces_single_and_combined_unused_fix()` | `.expect("` |
| `crates/verter_lsp/src/extension_provider_tests.rs:730` | `#[tokio::test] async fn extension_provider_combined_fix_uses_content_current_as_of_each_response()` | `.expect("` |
| `crates/verter_lsp/src/server_tests.rs:22499` | `#[tokio::test] async fn code_action_threads_diagnostic_code_to_type_provider_and_maps_edit_back()` | `let threaded = threaded.expect("` |

### `get_semantic_tokens` — declared at `crates/verter_type_runtime/src/traits.rs:249`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:249` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:1028` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1206` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1121` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:801` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:600` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:326` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3795` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:594` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4566` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:145` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3140` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:304` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:469` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:646` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:908` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1121` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:860` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1487` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:263` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1146` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:127` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:736` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:282` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:109` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/aux_features.rs:770` | `s_full( server: &VerterLanguageServer, params: SemanticTokensParams, ) -> Result<Option<SemanticTokensResult>>` | `if let Ok(type_tokens) = tp.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1215` | `fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1122` | `fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:806` | `fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:602` | `fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>>` | `Box::pin(async move { self.activate().await?.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:331` | `fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>>` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:595` | `fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>>` | `self.lsp.` |

**`call-test`** (11)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:949` | `#[tokio::test] async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result()` | `provider.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1379` | `#[tokio::test] async fn semantic_tokens_decode_2020_and_remap_into_verter_legend_space()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1443` | `#[tokio::test] async fn semantic_tokens_drop_unmappable_classifications_instead_of_guessing()` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3147` | `, ) -> crate::type_provider::traits::ProviderFuture< '_, Vec<crate::type_provider::protocol::SemanticToken>, >` | `self.inner.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:901` | `#[tokio::test] async fn feature_external_only_denied_carrier_serves_empty_no_owned_call()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:940` | `#[tokio::test] async fn feature_external_only_bound_carrier_delegates_to_owned()` | `!c.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:893` | `#[tokio::test] async fn test_e2e_tsserver_semantic_tokens_map_to_verter_legend()` | `.` |
| `crates/verter_lsp/tests/cases/tsserver_e2e_generated_outputs.rs:902` | `#[tokio::test] async fn test_e2e_tsserver_semantic_tokens_map_to_verter_legend()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5113` | `#[tokio::test] async fn tsgo_semantic_tokens_arrive_in_verter_legend_space()` | `let tokens = provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5258` | `#[tokio::test] async fn non_owning_transport_semantic_tokens_fail_closed_until_witness_legend_arrives()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5307` | `#[tokio::test] async fn non_owning_transport_semantic_tokens_fail_closed_until_witness_legend_arrives()` | `.` |

**`doc-comment`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:603` | `` | `/// '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2156` | `` | `/// ['TypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2495` | `impl TsgoTypeProvider` | `the legend exists). Without a retained legend '` |

**`comment`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/shared.rs:601` | `pub async fn establish_shared( params: EstablishSharedParams<'_>, ) -> Result<Self, EstablishError>` | `// Without it '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2305` | `ignal( tsgo_bin: &str, root_uri: &str, crash_notify: Option<Arc<Notify>>, ) -> Result<Self, TypeProviderError>` | `// advertised order). No legend ⇒` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2853` | `fn build_client_capabilities() -> serde_json::Value` | `// '` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:2424` | `` | `// ──` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:2428` | `#[tokio::test] async fn test_get_semantic_tokens_cache_miss_returns_empty()` | `// Simulate what` |

**`string-literal`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3808` | `fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>>` | `"TSGO` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5341` | `#[test] fn client_capabilities_advertise_semantic_tokens_with_the_published_vocabulary()` | `manticTokens.requests.full must be advertised —` |

### `get_document_highlights` — declared at `crates/verter_type_runtime/src/traits.rs:251`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:251` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:1083` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1222` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1125` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:811` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:605` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:337` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3840` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:598` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4630` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:148` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3150` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:308` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:473` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:650` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:912` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1125` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:865` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1501` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:274` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1149` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:131` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:740` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:286` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:113` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/aux_features.rs:198` | `t( server: &VerterLanguageServer, params: DocumentHighlightParams, ) -> Result<Option<Vec<DocumentHighlight>>>` | `if let Ok(type_highlights) = tp.` |
| `crates/verter_lsp/src/server/aux_features.rs:262` | `t( server: &VerterLanguageServer, params: DocumentHighlightParams, ) -> Result<Option<Vec<DocumentHighlight>>>` | `tp.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1235` | `n get_document_highlights( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1130` | `n get_document_highlights( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:820` | `n get_document_highlights( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:614` | `n get_document_highlights( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>>` | `.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:346` | `n get_document_highlights( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>>` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:603` | `n get_document_highlights( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>>` | `self.lsp.` |

**`call-test`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:953` | `#[tokio::test] async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result()` | `provider.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3158` | `rate::type_provider::traits::ProviderFuture< '_, Vec<crate::type_provider::protocol::TypeDocumentHighlight>, >` | `self.inner.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:998` | `#[tokio::test] async fn feature_mixed_read_denied_carrier_serves_external_default_no_owned_call()` | `c.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1057` | `#[tokio::test] async fn feature_mixed_read_bound_carrier_delegates_to_owned()` | `!c.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4369` | `#[tokio::test] async fn contents_cache_miss_fails_closed_without_fabricating_positions()` | `.` |

**`doc-comment`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:612` | `` | `/// '` |

**`string-literal`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3861` | `n get_document_highlights( &self, path: &str, offset: u32, ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>>` | `"TSGO` |

### `get_inlay_hints` — declared at `crates/verter_type_runtime/src/traits.rs:257`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:257` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:1162` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1242` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1133` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:825` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:619` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:352` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3886` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:606` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4716` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (15)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:155` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3161` | `impl crate::TypeProvider for RenameErrorProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:316` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:481` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:658` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:920` | `impl TypeProvider for GatedDeclOverlayProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:1133` | `impl TypeProvider for LostContentCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:874` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1520` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:283` | `impl TypeProvider for MarkerOwned` | `fn` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1156` | `impl TypeProvider for OwnedBaselineDouble` | `fn` |
| `crates/verter_type_runtime/src/future_size_measure_tests.rs:139` | `impl TypeProvider for MeasureMock` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:748` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:294` | `impl TypeProvider for MockProvider` | `fn` |
| `crates/verter_type_runtime/tests/cases/display_signature_seal.rs:121` | `impl TypeProvider for WitnessOnlyProvider` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/aux_features.rs:873` | `handle_inlay_hint( server: &VerterLanguageServer, params: InlayHintParams, ) -> Result<Option<Vec<InlayHint>>>` | `if let Ok(type_hints) = tp.` |
| `crates/verter_lsp/src/server/aux_features.rs:959` | `handle_inlay_hint( server: &VerterLanguageServer, params: InlayHintParams, ) -> Result<Option<Vec<InlayHint>>>` | `match tp.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1257` | `et_inlay_hints( &self, path: &str, start_offset: u32, end_offset: u32, ) -> ProviderFuture<'_, Vec<InlayHint>>` | `.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1140` | `et_inlay_hints( &self, path: &str, start_offset: u32, end_offset: u32, ) -> ProviderFuture<'_, Vec<InlayHint>>` | `.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:835` | `et_inlay_hints( &self, path: &str, start_offset: u32, end_offset: u32, ) -> ProviderFuture<'_, Vec<InlayHint>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:629` | `et_inlay_hints( &self, path: &str, start_offset: u32, end_offset: u32, ) -> ProviderFuture<'_, Vec<InlayHint>>` | `.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:368` | `et_inlay_hints( &self, path: &str, start_offset: u32, end_offset: u32, ) -> ProviderFuture<'_, Vec<InlayHint>>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:612` | `et_inlay_hints( &self, path: &str, start_offset: u32, end_offset: u32, ) -> ProviderFuture<'_, Vec<InlayHint>>` | `self.lsp.` |

**`call-test`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:957` | `#[tokio::test] async fn a_refused_project_propagates_instead_of_reading_as_an_empty_result()` | `provider.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1478` | `#[tokio::test] async fn inlay_hints_use_absolute_utf16_request_offsets_and_return_byte_positions()` | `.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:3170` | ` u32, ) -> crate::type_provider::traits::ProviderFuture< '_, Vec<crate::type_provider::protocol::InlayHint>, >` | `self.inner.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1005` | `#[tokio::test] async fn feature_mixed_read_denied_carrier_serves_external_default_no_owned_call()` | `c.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1064` | `#[tokio::test] async fn feature_mixed_read_bound_carrier_delegates_to_owned()` | `!c.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5206` | `#[tokio::test] async fn tsgo_inlay_hints_appear_for_inferred_types()` | `.` |

**`doc-comment`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:614` | `` | `/// '` |

### `resolve_completion` — declared at `crates/verter_type_runtime/src/traits.rs:269`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:269` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:1223` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1051` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1066` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:840` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:512` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:375` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:3929` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:539` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3757` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:884` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1541` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:192` | `impl TypeProvider for MarkerOwned` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:432` | `async fn on_resolve_completion(&mut self, r: ResolveCompletionRequest) -> Response` | `match provider.` |
| `crates/verter_lsp/src/server/nav_features.rs:1361` | `andle_completion_resolve( server: &VerterLanguageServer, mut item: CompletionItem, ) -> Result<CompletionItem>` | `tp.` |

**`call-forwarding`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1064` | `tion( &self, path: &str, data: CompletionResolveData, ) -> ProviderFuture<'_, Option<CompletionResolveResult>>` | `provider.` |
| `crates/verter_lsp/src/tsgo/shared.rs:1071` | `tion( &self, path: &str, data: CompletionResolveData, ) -> ProviderFuture<'_, Option<CompletionResolveResult>>` | `self.features.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:849` | `tion( &self, path: &str, data: CompletionResolveData, ) -> ProviderFuture<'_, Option<CompletionResolveResult>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:518` | `tion( &self, path: &str, data: CompletionResolveData, ) -> ProviderFuture<'_, Option<CompletionResolveResult>>` | `Box::pin(async move { self.activate().await?.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:391` | `tion( &self, path: &str, data: CompletionResolveData, ) -> ProviderFuture<'_, Option<CompletionResolveResult>>` | `move \|provider\| async move { provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:544` | `tion( &self, path: &str, data: CompletionResolveData, ) -> ProviderFuture<'_, Option<CompletionResolveResult>>` | `self.lsp.` |

**`call-test`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:392` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:464` | `#[tokio::test] async fn extension_provider_resolve_rejects_non_tsserver_handle_without_transport_call()` | `.` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:367` | `real_provider_test!( completion_resolve_carries_auto_import_edit, fixture = , async fn run(session)` | `session.provider().` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:479` | `ovider_test!( completion_resolve_does_not_fabricate_import_for_local_symbol, fixture = , async fn run(session)` | `if let Ok(Some(resolved)) = session.provider().` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:526` | `#[tokio::test] async fn resolve_completion_delegates_to_the_inner_provider()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1111` | `#[tokio::test] async fn feature_completion_denied_carrier_serves_native_only_no_owned_call()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1151` | `#[tokio::test] async fn feature_completion_bound_carrier_delegates_to_owned()` | `c.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4287` | `#[tokio::test] async fn resolve_completion_returns_some_when_only_label_details_present()` | `provider.` |

**`doc-comment`** (17)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:28` | `` | `//! 'verter_type_runtime::TypeProvider::` |
| `crates/verter_lsp/src/extension_provider_tests.rs:457` | `` | `/// '` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:17` | `` | `//! * '` |
| `crates/verter_lsp/src/server_tests.rs:21276` | `` | `/// 'TsgoTypeProvider::` |
| `crates/verter_lsp/src/server_tests.rs:21304` | `` | `/// Dispatch reaches '` |
| `crates/verter_lsp/src/server_tests.rs:21308` | `` | `/// envelope MUST have '` |
| `crates/verter_lsp/src/server_tests.rs:21352` | `` | `/// provider reports '"tsserver"'. '` |
| `crates/verter_lsp/src/server_tests.rs:21387` | `` | `/// '` |
| `crates/verter_lsp/src/tsgo/composite.rs:620` | `` | `/// '` |
| `crates/verter_type_runtime/src/traits.rs:144` | `pub trait TypeProvider: Send + Sync` | `/// providers with a real ['TypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2713` | `` | `/// ['TsgoTypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2736` | `` | `/// and ['TsgoTypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:737` | `` | `/// folds back 'detail' + 'documentation'; '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:743` | `` | `/// NOT advertised here (` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4252` | `` | `/// The tsgo '` |
| `crates/verter_type_runtime/src/tsserver/completion_resolve_tests.rs:194` | `` | `/// This pins the documented limitation: '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:5318` | `` | `/ fields. Shared by the tsserver and extension '` |

**`comment`** (10)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:231` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `//` |
| `crates/verter_lsp/src/real_provider_tests/completion_detail.rs:310` | `` | `// the provider's '` |
| `crates/verter_lsp/src/server_tests.rs:21435` | `#[tokio::test] async fn completion_resolve_envelope_resolves_for_both_provider_kinds()` | `// provider's real '` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1076` | `` | `//` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:1962` | `fn parse_completion_item(item: &serde_json::Value, content: Option<&str>) -> Option<Completion>` | `// opaque 'data' blob, replayed verbatim by '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2821` | `fn build_client_capabilities() -> serde_json::Value` | `// it is omitted (` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:772` | `#[test] fn client_capabilities_advertise_completion_item_resolve_support()` | `// 'additionalTextEdits' from` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:773` | `#[test] fn client_capabilities_advertise_completion_item_resolve_support()` | `// from` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:775` | `#[test] fn client_capabilities_advertise_completion_item_resolve_support()` | `erty, so we never claim resolve-support for it (` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:965` | `#[test] fn client_capabilities_do_not_overclaim_unhandled_features()` | `// by` |

**`string-literal`** (10)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:394` | `#[tokio::test] async fn extension_provider_transport_mock_drives_completion_resolve_and_diagnostics()` | `.expect("` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:540` | `#[tokio::test] async fn resolve_completion_delegates_to_the_inner_provider()` | `"` |
| `crates/verter_lsp/src/server_tests.rs:21343` | `#[tokio::test] async fn completion_resolve_dispatches_neutral_envelope_to_provider()` | `esolve envelope must dispatch to the provider's` |
| `crates/verter_lsp/src/server_tests.rs:21376` | `#[tokio::test] async fn completion_resolve_fails_closed_on_provider_id_mismatch()` | `"a provider-id mismatch must FAIL CLOSED —` |
| `crates/verter_lsp/src/tsgo/composite.rs:650` | `fn name(self) -> &'static str` | `ProviderFeature::ResolveCompletion => "` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1113` | `#[tokio::test] async fn feature_completion_denied_carrier_serves_native_only_no_owned_call()` | `.expect("` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1116` | `#[tokio::test] async fn feature_completion_denied_carrier_serves_native_only_no_owned_call()` | `"a denied carrier's` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1132` | `#[tokio::test] async fn feature_completion_denied_carrier_serves_native_only_no_owned_call()` | `"denied: no owned` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:1155` | `#[tokio::test] async fn feature_completion_bound_carrier_delegates_to_owned()` | `"a BOUND carrier delegates` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:382` | `tion( &self, path: &str, data: CompletionResolveData, ) -> ProviderFuture<'_, Option<CompletionResolveResult>>` | `"` |

### `shutdown` — declared at `crates/verter_type_runtime/src/traits.rs:278`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:278` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (19)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/mod.rs:1432` | `impl LanguageServer for VerterLanguageServer` | `async fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1345` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1361` | `impl SharedTsgoOverlay` | `async fn` |
| `crates/verter_lsp/src/tsgo/shared.rs:1143` | `impl TypeProvider for TsgoSharedProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:854` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:933` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_napi/src/meta.rs:192` | `#[napi] impl NapiMetaProject` | `pub fn` |
| `crates/verter_session/src/component_meta_host.rs:263` | `impl ComponentMetaHost` | `pub fn` |
| `crates/verter_session/src/meta.rs:369` | `impl MetaProject` | `pub fn` |
| `crates/verter_tsgo_api/src/attach.rs:605` | `impl TsgoAttach<Owned>` | `async fn` |
| `crates/verter_tsgo_api/src/relay.rs:904` | `impl LspRelay` | `pub async fn` |
| `crates/verter_tsgo_api/src/transport/pipe.rs:83` | `impl StdioPipeTransport` | `pub async fn` |
| `crates/verter_type_runtime/src/backend.rs:182` | `pub trait GeneratedQueryBackend: Send + Sync` | `fn` |
| `crates/verter_type_runtime/src/provider_adapter.rs:467` | `impl GeneratedQueryBackend for TypeProviderAdapter` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:397` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4011` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:637` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4783` | `impl TypeProvider for TsserverTypeProvider` | `fn` |
| `crates/verter_wasm/src/lib.rs:1898` | `#[wasm_bindgen(js_class = MetaProject)] impl WasmMetaProject` | `pub fn` |

**`impl-test`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:77` | `impl RenameFixture` | `async fn` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:74` | `impl FrontierFixture` | `async fn` |
| `crates/verter_lsp/src/sync_coordinator_tests.rs:28` | `impl tower_lsp_server::LanguageServer for NoopLanguageServer` | `async fn` |
| `crates/verter_lsp/src/test_harness.rs:1297` | `impl RealProviderTestSession` | `pub(crate) async fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1088` | `impl RealRecoveryHarness` | `pub(crate) async fn` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3825` | `impl RealReloadHarness` | `async fn` |

**`call-production`** (16)

| site | context | snippet |
|---|---|---|
| `crates/verter_bench/examples/attribution_baseline.rs:195` | `fn run_once(corpus: &[SourceFile]) -> (f64, usize)` | `meta_host.` |
| `crates/verter_bench/examples/currency_phase_probe.rs:685` | `fn run_meta_mode()` | `meta_host.` |
| `crates/verter_dx_baseline/src/main.rs:485` | `async fn on_shutdown(&mut self) -> Response` | `let _ = r.provider.` |
| `crates/verter_lsp/src/main.rs:1091` | `: Option<verter_tsgo_api::toolchain::discovery::ResolutionRequest>, ) -> Result<Arc<dyn TypeProvider>, String>` | `let teardown = lsp.` |
| `crates/verter_lsp/src/server/lifecycle.rs:495` | `pub(super) async fn handle_shutdown(server: &VerterLanguageServer) -> Result<()>` | `let _ = tp.` |
| `crates/verter_lsp/src/tsgo/overlay_core.rs:92` | `fn teardown(&self) -> ProviderFuture<'_, ()>` | `self.` |
| `crates/verter_lsp/src/tsgo/resilient.rs:61` | `in::Pin< Box< dyn std::future::Future<Output = Result<TsgoOwnedProvider, TypeProviderError>> + Send + 'a, >, >` | `let _ = inner.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:203` | `async fn activate(&self) -> Result<Arc<dyn TypeProvider>, TypeProviderError>` | `let _ = provider.` |
| `crates/verter_relay_shim/src/main.rs:1089` | `async fn run_relay(args: ShimArgs) -> Result<ShimExit, String>` | `relay.` |
| `crates/verter_tsgo_api/src/attach.rs:621` | `pub async fn teardown(self) -> TsgoApiResult<()>` | `self.` |
| `crates/verter_tsgo_api/src/control/server.rs:691` | `riter_task<W>(mut write: W, mut out_rx: mpsc::Receiver<Vec<u8>>) where W: AsyncWrite + Unpin + Send + 'static,` | `let _ = write.` |
| `crates/verter_tsgo_api/src/jsonrpc/connection.rs:273` | `ter_task<W>(mut writer: W, mut out_rx: mpsc::Receiver<Outbound>) where W: AsyncWrite + Unpin + Send + 'static,` | `let _ = writer.` |
| `crates/verter_tsgo_api/src/relay.rs:963` | `<W>(mut server_write: W, mut server_rx: mpsc::Receiver<Vec<u8>>) where W: AsyncWrite + Unpin + Send + 'static,` | `let _ = server_write.` |
| `crates/verter_type_runtime/src/resilient.rs:640` | `, inner: Arc<RwLock<Option<Arc<P>>>>, log_name: &'static str, ) where P: TypeProvider + Send + Sync + 'static,` | `let _ = provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2295` | `ignal( tsgo_bin: &str, root_uri: &str, crash_notify: Option<Arc<Notify>>, ) -> Result<Self, TypeProviderError>` | `let _ = TypeProvider::` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2320` | `ignal( tsgo_bin: &str, root_uri: &str, crash_notify: Option<Arc<Notify>>, ) -> Result<Self, TypeProviderError>` | `let _ = TypeProvider::` |

**`call-forwarding`** (11)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1348` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `shared.` |
| `crates/verter_lsp/src/tsgo/composite.rs:1350` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:862` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `if let Err(error) = provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:939` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `provider.` |
| `crates/verter_napi/src/meta.rs:194` | `#[napi] pub fn shutdown(&self) -> Result<()>` | `self.inner.` |
| `crates/verter_session/src/component_meta_host.rs:264` | `pub fn shutdown(&self)` | `self.inner.project.` |
| `crates/verter_tsgo_api/src/transport/pipe.rs:84` | `pub async fn shutdown(&mut self)` | `let _ = self.stdin.` |
| `crates/verter_type_runtime/src/provider_adapter.rs:474` | `fn shutdown(&self) -> BackendFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:404` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `let _ = provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:642` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `lsp.` |
| `crates/verter_wasm/src/lib.rs:1900` | `pub fn shutdown(&self) -> Result<(), JsValue>` | `self.inner.` |

**`call-test`** (163)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:1073` | `#[tokio::test] async fn known_good_script_setup_hover_resolves_through_tsgo_on_emitted_tsx()` | `let _ = provider.` |
| `crates/verter_dx_baseline/src/main_tests.rs:1206` | `#[tokio::test] #[ignore = ] async fn provider_resolves_barrel_reexport_through_rewritten_twin()` | `let _ = provider.` |
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:595` | `#[tokio::test(flavor = )] async fn carrier_dx_enhanced_both_engines_both_frameworks_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/carrier_dx_tests.rs:623` | `#[tokio::test(flavor = )] async fn carrier_dx_enhanced_both_engines_both_frameworks_tsgo()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/completion.rs:286` | `#[tokio::test(flavor = )] async fn svelte_contract_tsgo_template_completion_survives_provider_specialization()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:82` | `:test(flavor = )] async fn vue_carrier_resolves_aliased_import_and_ambient_under_configured_project_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:136` | `#[tokio::test(flavor = )] async fn vue_carrier_surfaces_semantic_type_error_on_source_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:173` | `#[tokio::test(flavor = )] async fn svelte_carrier_surfaces_semantic_type_error_on_source_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:270` | `kio::test(flavor = )] async fn vue_carrier_semantic_type_error_rides_owned_provider_sync_membership_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:303` | `okio::test(flavor = )] async fn vue_carrier_surfaces_semantic_type_error_through_resilient_provider_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:323` | `kio::test(flavor = )] async fn vue_carrier_resolves_aliased_import_and_ambient_under_configured_project_tsgo()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:336` | `st(flavor = )] async fn svelte_carrier_resolves_aliased_import_and_ambient_under_configured_project_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:349` | `::test(flavor = )] async fn svelte_carrier_resolves_aliased_import_and_ambient_under_configured_project_tsgo()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:397` | `#[tokio::test(flavor = )] async fn vue_carrier_surfaces_semantic_type_error_on_source_tsgo()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:430` | `#[tokio::test(flavor = )] async fn carrier_under_solution_style_leaf_resolves_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:462` | `#[tokio::test(flavor = )] async fn carrier_in_multiroot_monorepo_resolves_cross_package_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/import_matrix.rs:197` | `#[tokio::test(flavor = )] async fn type_only_import_is_not_carrier_linked_as_value_component_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/import_matrix.rs:256` | `#[ignore = ] #[tokio::test(flavor = )] async fn namespaced_component_tag_resolves_member_props_tsgo()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/import_matrix.rs:295` | `#[tokio::test(flavor = )] async fn carrier_diagnostics_resolve_path_alias_tsgo()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/import_matrix.rs:383` | `#[tokio::test(flavor = )] async fn import_nodenext_packages_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/import_matrix.rs:392` | `#[tokio::test(flavor = )] async fn import_nodenext_packages_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/import_matrix.rs:434` | `#[tokio::test(flavor = )] async fn nodenext_package_imports_subpath_populates_resolved_canonical_id_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/import_matrix.rs:471` | `#[tokio::test(flavor = )] async fn node_modules_raw_vue_carrier_resolves_props_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/import_matrix.rs:805` | `#[tokio::test(flavor = )] async fn dynamic_import_component_is_carrier_linked_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/rename.rs:316` | `#[tokio::test(flavor = )] async fn rename_cross_file_imported_prop_refuses_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/rename.rs:343` | `#[tokio::test(flavor = )] async fn rename_cross_file_imported_prop_refuses_tsgo()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/rename.rs:457` | `#[tokio::test(flavor = )] async fn parent_did_open_prewarms_imported_child_carrier_api()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/rename.rs:481` | `#[tokio::test(flavor = )] async fn rename_cross_file_prop_child_closed_unprewarmed_refuses_tsserver()` | `session.` |
| `crates/verter_lsp/src/real_provider_tests/semantic_tokens.rs:273` | `d: TestProviderKind, relative: &str, call_needle: &str, template_needle: &str, expected_template_kind: &str, )` | `session.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:323` | `#[tokio::test] async fn vue_define_props_member_refuses_before_provider_rename()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:367` | `#[tokio::test] async fn svelte_props_member_refuses_without_provider_passthrough()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:409` | `#[tokio::test] async fn svelte_untyped_props_shorthand_refuses_before_provider_rename()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:436` | `#[tokio::test] async fn svelte_untyped_props_shorthand_refuses_before_provider_rename()` | `unaffected.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:473` | `#[tokio::test] async fn svelte_rename_refuses_when_script_fact_validation_is_unavailable()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:608` | `#[tokio::test] async fn svelte_untyped_props_alias_public_key_refuses_but_local_binding_renames()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:650` | `#[tokio::test] async fn svelte_props_refuses_native_edit_that_would_touch_same_named_public_key()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:690` | `#[tokio::test] async fn svelte_props_open_rest_call_bindings_refuse_before_provider_rename()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:726` | `#[tokio::test] async fn svelte_undestructured_props_binding_refuses_before_provider_rename()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:767` | `#[tokio::test] async fn ordinary_svelte_binding_elsewhere_in_props_file_still_renames()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:814` | `#[tokio::test] async fn vue_runtime_options_and_svelte_legacy_prop_declarations_refuse()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:855` | `#[tokio::test] async fn unresolved_component_prop_usage_refuses_before_provider_rename()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:893` | `#[tokio::test] async fn ordinary_vue_and_svelte_script_bindings_still_produce_workspace_edits()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:966` | `#[tokio::test] async fn script_anchor_renames_exactly_the_two_lexical_script_occurrences()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:983` | `#[tokio::test] async fn template_instance_member_anchor_fails_closed_without_a_provider()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:999` | `#[tokio::test] async fn directive_value_instance_member_anchor_fails_closed_without_a_provider()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1031` | `#[tokio::test] async fn prepare_rename_refuses_the_instance_member_anchor_but_allows_the_script_anchor()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1060` | `#[tokio::test] async fn references_from_the_script_declaration_stop_at_the_script_occurrences()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1088` | `#[tokio::test] async fn references_at_an_instance_member_anchor_do_not_report_the_script_const()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1113` | `#[tokio::test] async fn script_setup_rename_still_spans_script_and_template()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1135` | `#[tokio::test] async fn script_setup_rename_from_the_template_spans_both_regions()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1158` | `#[tokio::test] async fn plain_template_text_matching_the_name_is_not_renamed()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1181` | `#[tokio::test] async fn a_v_for_variable_shadowing_the_binding_is_not_renamed()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1269` | `#[tokio::test] async fn instance_member_anchor_edit_is_provider_owned_and_never_the_script_const()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1304` | `#[tokio::test] async fn instance_member_anchor_consults_the_provider_and_ships_nothing_when_it_answers_empty()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1351` | `#[tokio::test] async fn instance_member_suppression_spans_the_whole_token_and_stops_at_its_end()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1368` | `#[tokio::test] async fn instance_member_suppression_spans_the_whole_token_and_stops_at_its_end()` | `setup.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1498` | `::test] async fn prepare_rename_handshake_offers_the_instance_member_then_renames_only_provider_occurrences( )` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1532` | `#[tokio::test] async fn prepare_rename_at_an_instance_member_declines_when_the_provider_answers_empty()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1569` | `#[tokio::test] async fn prepare_rename_at_an_instance_member_declines_when_the_provider_errors()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1622` | `#[tokio::test] async fn prepare_rename_declines_when_provider_locations_do_not_map_onto_the_authored_token()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1733` | `#[tokio::test] async fn prepare_rename_declines_when_the_provider_maps_onto_a_different_authored_token()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1796` | `#[tokio::test] async fn rename_revalidates_after_a_yes_prepare_and_ships_nothing_when_the_answer_moved()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1855` | `#[tokio::test] async fn instance_member_rename_refuses_a_provider_answer_that_edits_a_different_occurrence()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1957` | `#[tokio::test] async fn rename_refuses_when_the_merge_drops_a_provider_location()` | `mappable.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:1976` | `#[tokio::test] async fn rename_refuses_when_the_merge_drops_a_provider_location()` | `partial.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2251` | `#[tokio::test] async fn a_style_v_bind_reference_refuses_the_rename_and_says_why()` | `native_only.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2283` | `#[tokio::test] async fn a_style_v_bind_reference_refuses_the_rename_and_says_why()` | `refusing.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2303` | `#[tokio::test] async fn a_style_v_bind_reference_refuses_the_rename_and_says_why()` | `control.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2507` | `#[tokio::test] async fn instance_member_rename_ships_both_authored_occurrences_when_both_legs_map()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2543` | `#[tokio::test] async fn instance_member_rename_refuses_a_dropped_companion_leg_the_anchor_proof_cannot_see()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2604` | `#[tokio::test] async fn type_only_import_rename_refuses_a_dropped_companion_leg_it_proves_nothing_about()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2648` | `#[tokio::test] async fn type_only_import_rename_still_serves_the_provider_when_nothing_is_dropped()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2721` | `#[tokio::test] async fn a_svelte_carrier_models_no_markup_occurrence_despite_having_a_template_snapshot()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2786` | `#[tokio::test] async fn svelte_rename_refuses_a_dropped_companion_leg_over_its_unenumerated_markup()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2905` | `#[tokio::test] async fn rename_refuses_when_the_host_analysis_is_ahead_of_the_open_document()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2941` | `#[tokio::test] async fn native_rename_refuses_when_the_host_analysis_is_ahead_of_the_open_document()` | `f.` |
| `crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:2970` | `#[tokio::test] async fn prepare_rename_refuses_when_the_host_analysis_is_ahead_of_the_open_document()` | `f.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:428` | `::test(start_paused = true)] async fn references_on_child_declaration_fail_closed_until_imported_api_is_live()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:473` | `::test(start_paused = true)] async fn references_on_child_declaration_include_parent_usage_after_publication()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:559` | `#[tokio::test(flavor = )] async fn production_edit_debounce_delivers_imported_api_companion()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:625` | `io::test(start_paused = true)] async fn opening_an_imported_carrier_keeps_its_delivered_api_companion_loaded()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:705` | `sed = true)] async fn references_fail_closed_while_the_delivered_api_is_stale_then_serve_after_republication()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:744` | `okio::test(start_paused = true)] async fn an_api_neutral_edit_keeps_the_frontier_ready_without_republication()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:818` | `#[tokio::test(start_paused = true)] async fn references_serve_through_a_barrel_reexport_after_publication()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:870` | `[tokio::test(start_paused = true)] async fn opening_the_barrel_document_keeps_the_project_references_surface()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:917` | `o::test(start_paused = true)] async fn opening_the_barrel_before_publication_still_serves_project_references()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:1031` | `#[tokio::test(start_paused = true)] async fn a_paused_delivery_with_no_publish_still_claims_its_shadow()` | `fixture.` |
| `crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:1130` | `::test(start_paused = true)] async fn a_snapshot_published_during_delivery_is_never_claimed_as_that_delivery()` | `fixture.` |
| `crates/verter_lsp/src/server_tests.rs:18791` | `tokio::test(flavor = )] async fn completion_with_real_tsserver_returns_fixture_vfor_member_access_properties()` | `provider.` |
| `crates/verter_lsp/src/server_tests.rs:18808` | `tokio::test(flavor = )] async fn completion_with_real_tsserver_returns_fixture_vfor_member_access_properties()` | `provider.` |
| `crates/verter_lsp/src/server_tests.rs:18937` | `lavor = )] async fn completion_with_real_tsserver_recovers_fixture_vfor_member_access_immediately_after_open()` | `provider.` |
| `crates/verter_lsp/src/server_tests.rs:19066` | ` fn completion_with_real_tsserver_recovers_fixture_vfor_member_access_on_dot_trigger_immediately_after_open( )` | `provider.` |
| `crates/verter_lsp/src/server_tests.rs:20058` | `#[tokio::test(flavor = )] async fn completion_with_real_tsserver_recovers_when_current_file_sync_was_missed()` | `provider.` |
| `crates/verter_lsp/src/server_tests.rs:20244` | `tokio::test(flavor = )] async fn real_tsserver_slot_member_access_stays_typed_after_opening_child_and_parent()` | `provider.` |
| `crates/verter_lsp/src/server_tests.rs:20346` | `tokio::test(flavor = )] async fn real_tsserver_slot_member_access_stays_typed_after_opening_child_and_parent()` | `provider.` |
| `crates/verter_lsp/src/test_harness.rs:1298` | `pub(crate) async fn shutdown(self)` | `let _ = self.provider.` |
| `crates/verter_lsp/src/test_harness.rs:1650` | `` | `session.` |
| `crates/verter_lsp/src/test_harness.rs:1676` | `` | `session.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:318` | `#[tokio::test] async fn failed_activation_retries_after_cooldown_and_recovers()` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:378` | `#[tokio::test] async fn failed_activation_retries_after_cooldown_and_recovers()` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:597` | `#[tokio::test] async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries()` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:642` | `#[tokio::test] async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries()` | `.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:696` | `async fn teardown(mut h: Harness)` | `let _ = h.provider.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1523` | `async fn teardown_composite(mut h: CompositeHarness)` | `let _ = h.composite.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1781` | `#[tokio::test] async fn composite_attach_failure_activates_managed_fallback_exactly_once()` | `composite.` |
| `crates/verter_session/src/component_meta_host_tests.rs:75` | `#[test] fn shutdown_prevents_further_operations()` | `host.` |
| `crates/verter_session/src/meta_tests.rs:1412` | `#[test] fn shutdown_marks_project_dead()` | `project.` |
| `crates/verter_session/src/meta_tests.rs:1428` | `#[test] fn shutdown_is_idempotent()` | `project.` |
| `crates/verter_session/src/meta_tests.rs:1429` | `#[test] fn shutdown_is_idempotent()` | `project.` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:217` | `#[test] fn meta_project_shutdown_then_new_project_in_same_process_is_clean()` | `project_a.` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:220` | `#[test] fn meta_project_shutdown_then_new_project_in_same_process_is_clean()` | `project_a.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:427` | `o::test(flavor = , worker_threads = 2)] async fn control_dispatch_drives_full_attach_lifecycle_through_relay()` | `lb.relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:488` | `okio::test(flavor = , worker_threads = 2)] async fn wait_initialized_times_out_when_editor_never_initializes()` | `lb.relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:566` | `avor = , worker_threads = 2)] async fn abnormal_control_termination_retracts_open_carriers_non_destructively()` | `lb.relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:669` | `#[tokio::test(flavor = , worker_threads = 2)] async fn sent_but_unsynced_open_is_retracted_on_session_end()` | `lb.relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:721` | `:test(flavor = , worker_threads = 2)] async fn detach_close_carriers_false_opts_out_of_the_session_end_drain()` | `lb.relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:733` | `#[tokio::test] async fn control_hello_rejects_wrong_nonce()` | `lb.relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:745` | `#[tokio::test] async fn control_methods_require_hello_first()` | `lb.relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:792` | `#[tokio::test] async fn control_hello_wrong_protocol_returns_typed_error_code()` | `relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:975` | `kio::test(flavor = , worker_threads = 2)] async fn detach_omitted_params_fails_closed_and_retracts_via_drain()` | `lb.relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:1026` | `o::test(flavor = , worker_threads = 2)] async fn detach_malformed_params_fails_closed_and_retracts_via_drain()` | `lb.relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:1082` | `#[tokio::test(flavor = , worker_threads = 2)] async fn session_end_drain_is_bounded_against_a_wedged_writer()` | `relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:1176` | ` = , worker_threads = 2)] async fn session_end_drain_retracts_failed_close_residual_and_never_closed_carrier()` | `relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:1273` | ` = , worker_threads = 2)] async fn session_end_drain_retracts_failed_close_residual_and_never_closed_carrier()` | `relay2.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:1370` | `:test(flavor = , worker_threads = 2)] async fn relay_round_trip_handlers_are_bounded_against_a_wedged_writer()` | `relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:1398` | `:test(flavor = , worker_threads = 2)] async fn relay_round_trip_handlers_are_bounded_against_a_wedged_writer()` | `relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:1445` | `:test(flavor = , worker_threads = 2)] async fn relay_round_trip_handlers_are_bounded_against_a_wedged_writer()` | `relay.` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:1475` | `:test(flavor = , worker_threads = 2)] async fn relay_round_trip_handlers_are_bounded_against_a_wedged_writer()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:724` | `#[tokio::test] async fn relay_pass_through_is_byte_identical_preserving_key_order()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:759` | `#[tokio::test] async fn relay_passes_editor_request_and_server_response_untouched()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:788` | `#[tokio::test] async fn relay_forwards_server_to_client_notifications_untouched()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:839` | `#[tokio::test] async fn relay_reserves_verter_namespace_rejects_editor_verter_id()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:874` | `#[tokio::test] async fn relay_injects_didopen_onto_server_stream()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:934` | `#[tokio::test] async fn relay_injected_request_demuxes_to_verter_not_editor()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:968` | `#[tokio::test] async fn relay_reemits_api_session_request_and_parses_handle()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1069` | `#[tokio::test] async fn relay_suppresses_carrier_publish_diagnostics_for_open_overlay()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1147` | `#[tokio::test] async fn relay_suppresses_in_flight_carrier_frame_after_did_close()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1211` | `#[tokio::test] async fn relay_strips_carrier_entries_from_mixed_workspace_symbol_response()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1250` | `#[tokio::test] async fn relay_forwards_carrier_free_frame_byte_identical_while_overlay_open()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1349` | `#[tokio::test] async fn relay_answers_all_carrier_apply_edit_to_server_never_editor()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1466` | `#[tokio::test] async fn relay_answers_mixed_apply_edit_to_server_never_editor()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1558` | `#[tokio::test] async fn relay_answers_reserved_id_server_request_to_server_never_editor()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1644` | `#[tokio::test] async fn relay_answers_editor_definition_with_neutral_when_carrier_only()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1754` | `#[tokio::test] async fn relay_prunes_pending_request_on_cancel_no_fabricated_reply()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1814` | `#[tokio::test] async fn relay_demuxes_verter_response_before_egress_suppression()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1848` | `#[tokio::test] async fn injected_didopen_precedes_sync_barrier()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1937` | `#[tokio::test] async fn relay_captures_in_band_initialize_witness_as_handshake_passes()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:1995` | `#[tokio::test] async fn relay_witness_carries_the_servers_semantic_token_legend()` | `relay.` |
| `crates/verter_tsgo_api/src/relay_tests.rs:2026` | `#[tokio::test] async fn relay_signals_stopped_on_editor_disconnect()` | `relay.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1090` | `pub(crate) async fn shutdown(&self)` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1652` | `#[tokio::test] async fn failed_respawn_retries_within_budget_and_recovers()` | `harness.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1840` | `tokio::test(start_paused = true)] async fn deliberate_shutdown_is_not_reported_as_a_crash_and_never_respawns()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3684` | `#[tokio::test] async fn managed_shutdown_kills_and_reaps_unresponsive_owned_child()` | `out(std::time::Duration::from_secs(7), provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4451` | `#[tokio::test] async fn shutdown_disarms_the_eof_crash_signal()` | `provider.` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3827` | `async fn shutdown(&self)` | `.` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3877` | `#[tokio::test] async fn consecutive_timeouts_fire_crash_notify()` | `recovery.` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3994` | `#[tokio::test] async fn real_publication_refresh_admits_plugin_carriers()` | `real.` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:4011` | `#[tokio::test] async fn real_source_identity_observes_targeted_refresh_without_reload_projects()` | `real.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs:212` | ` worker_threads = 2)] async fn owned_bare_vue_import_resolves_to_declaration_carrier_and_public_member_flows()` | `provider.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs:271` | `worker_threads = 2)] async fn owned_bare_vue_import_fails_closed_when_declaration_carrier_didopen_suppressed()` | `provider.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:165` | `(flavor = , worker_threads = 2)] async fn owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process()` | `provider.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:258` | `lavor = , worker_threads = 2)] async fn owned_api_oracle_resolves_multiple_projects_per_query_on_one_process()` | `provider.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:293` | `#[tokio::test(flavor = , worker_threads = 2)] async fn owned_provider_is_one_process_no_second_spawn()` | `provider.` |

**`ref-production`** (22)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:120` | ` out: &mut W, bridge: &mut Bridge) where R: tokio::io::AsyncBufRead + Unpin, W: tokio::io::AsyncWrite + Unpin,` | `let (resp,` |
| `crates/verter_dx_baseline/src/main.rs:122` | ` out: &mut W, bridge: &mut Bridge) where R: tokio::io::AsyncBufRead + Unpin, W: tokio::io::AsyncWrite + Unpin,` | `if` |
| `crates/verter_scheduler/src/scheduler.rs:1158` | `` | `pub(crate)` |
| `crates/verter_scheduler/src/scheduler.rs:1368` | `tor>, cpu_pool: Arc<crate::pool::SchedulerCpuPool>, io_pool: Arc<crate::pool::SchedulerIoPool>, ) -> Arc<Self>` | `` |
| `crates/verter_scheduler/src/scheduler.rs:1461` | `ool::SchedulerCpuPool>, #[cfg(not(target_arch = ))] io_pool: Arc<crate::pool::SchedulerIoPool>, ) -> Arc<Self>` | `` |
| `crates/verter_scheduler/src/scheduler.rs:1683` | `impl Scheduler` | `if self.` |
| `crates/verter_scheduler/src/scheduler.rs:2144` | `#[cfg(not(target_arch = ))] pub fn reset(&self)` | `self.` |
| `crates/verter_scheduler/src/scheduler.rs:2215` | `#[cfg(not(target_arch = ))] pub fn reset(&self)` | `self.` |
| `crates/verter_scheduler/src/scheduler.rs:3155` | ` fn driver_loop_native( weak: std::sync::Weak<Scheduler>, receiver: crossbeam_channel::Receiver<Submission>, )` | `if scheduler.` |
| `crates/verter_scheduler/src/scheduler.rs:6827` | `te::job::CompletionHandle<T>, caller_kind: crate::caller_kind::CallerKind, ) -> crate::job::CompletionState<T>` | `if self.` |
| `crates/verter_scheduler/src/scheduler.rs:7077` | `fn drop(&mut self)` | `self.` |
| `crates/verter_session/src/meta.rs:194` | `` | `` |
| `crates/verter_session/src/meta.rs:206` | `pub fn new(host: VerterHost) -> Arc<Self>` | `` |
| `crates/verter_session/src/meta.rs:212` | `fn check_alive(&self) -> Result<(), MetaError>` | `if self.` |
| `crates/verter_session/src/meta.rs:371` | `pub fn shutdown(&self)` | `.` |
| `crates/verter_session/src/meta.rs:387` | `pub fn is_shutdown(&self) -> bool` | `self.` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:327` | `` | `let mut` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:332` | `` | `` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:338` | `` | `` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:345` | `` | `let mut` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:350` | `` | `` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:356` | `` | `` |

**`ref-test`** (9)

| site | context | snippet |
|---|---|---|
| `crates/verter_scheduler/src/job.rs:485` | `#[test] fn completion_state_variants()` | `let` |
| `crates/verter_scheduler/src/job.rs:486` | `#[test] fn completion_state_variants()` | `assert!(!` |
| `crates/verter_scheduler/src/job.rs:487` | `#[test] fn completion_state_variants()` | `assert!(` |
| `crates/verter_tsgo_api/src/jsonrpc/connection_tests.rs:307` | `#[test] fn jsonrpc_message_omits_null_params_and_keeps_real_params()` | `let` |
| `crates/verter_tsgo_api/src/jsonrpc/connection_tests.rs:309` | `#[test] fn jsonrpc_message_omits_null_params_and_keeps_real_params()` | `` |
| `crates/verter_tsgo_api/src/jsonrpc/connection_tests.rs:312` | `#[test] fn jsonrpc_message_omits_null_params_and_keeps_real_params()` | `assert_eq!(` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4501` | `#[test] fn jsonrpc_body_omits_null_params_and_keeps_real_params()` | `let` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4503` | `#[test] fn jsonrpc_body_omits_null_params_and_keeps_real_params()` | `` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4506` | `#[test] fn jsonrpc_body_omits_null_params_and_keeps_real_params()` | `assert_eq!(` |

**`doc-comment`** (104)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main.rs:194` | `impl Bridge` | `/// stop (` |
| `crates/verter_dx_baseline/src/protocol.rs:482` | `` | `/// '` |
| `crates/verter_lsp/src/server/lifecycle.rs:5` | `` | `//!` |
| `crates/verter_lsp/src/test_harness.rs:652` | `` | `/// ['Self::` |
| `crates/verter_lsp/src/test_harness.rs:667` | `` | `a completed session with another process until` |
| `crates/verter_lsp/src/test_harness.rs:1292` | `impl RealProviderTestSession` | `///` |
| `crates/verter_lsp/src/test_harness.rs:1295` | `impl RealProviderTestSession` | `/// still holds a handle on a slow` |
| `crates/verter_lsp/src/tsgo/composite.rs:1356` | `impl SharedTsgoOverlay` | `transport down (best-effort), then let managed` |
| `crates/verter_lsp/src/tsgo/composite.rs:1360` | `impl SharedTsgoOverlay` | `/// non-establishing 'current' accessor, so` |
| `crates/verter_lsp/src/tsgo/overlay_core.rs:258` | `impl<T: OverlayTransport> LazyOverlayCore<T>` | `/// (used by the non-establishing retract /` |
| `crates/verter_lsp/src/tsgo/shared.rs:38` | `` | `no process and never sends a second initialize,` |
| `crates/verter_lsp/src/tsgo/shared.rs:388` | `` | `ion. It owns no process and sends no initialize/` |
| `crates/verter_lsp/src/tsgo/transport_cell.rs:146` | `impl<T> LazyTransport<T>` | `///` |
| `crates/verter_lsp/tests/cases/stdio_launch_smoke.rs:7` | `` | `//! 'initialized' / '` |
| `crates/verter_napi/src/meta.rs:303` | `#[napi] impl NapiMetaSession` | `/// Throws on project-level` |
| `crates/verter_relay_shim/src/main.rs:36` | `` | `//! ORIGINATES 'exit'/'` |
| `crates/verter_relay_shim/src/main.rs:487` | `` | `/// reached steady state, so there is no clean-` |
| `crates/verter_relay_shim/src/main.rs:515` | `impl ChildSetupGuard` | `/// Used when a` |
| `crates/verter_relay_shim/src/main.rs:625` | `` | `/// The shim itself received a Unix` |
| `crates/verter_relay_shim/src/main.rs:630` | `` | `/// The Unix` |
| `crates/verter_relay_shim/src/main.rs:659` | `#[cfg(unix)] impl ShutdownSignals` | `/// Await the FIRST` |
| `crates/verter_relay_shim/src/main.rs:672` | `#[cfg(unix)] impl ShutdownSignals` | `/// Await a` |
| `crates/verter_relay_shim/src/main.rs:688` | `` | `f the ['run_relay'] setup-window race: EITHER a` |
| `crates/verter_relay_shim/src/main.rs:690` | `` | `/// no` |
| `crates/verter_relay_shim/src/main.rs:693` | `` | `/// A` |
| `crates/verter_relay_shim/src/main.rs:710` | `` | `/// Resolve the setup-window race. A delivered` |
| `crates/verter_relay_shim/src/main.rs:711` | `` | `hen setup was interrupted mid-flight, because a` |
| `crates/verter_relay_shim/src/main.rs:717` | `` | `/// re-check of the` |
| `crates/verter_relay_shim/src/main.rs:784` | `` | `Object / 'setsid' / 'PR_SET_PDEATHSIG') and NO` |
| `crates/verter_relay_shim/src/main.rs:809` | `` | `he control endpoint, and tear down on the first` |
| `crates/verter_relay_shim/src/main_tests.rs:206` | `` | `/// F1 — the Unix` |
| `crates/verter_relay_shim/src/main_tests.rs:301` | `` | `Unix teardown select must be 'biased' with the` |
| `crates/verter_relay_shim/src/main_tests.rs:373` | `` | `/// F2 — the setup window must RACE a delivered` |
| `crates/verter_relay_shim/src/main_tests.rs:376` | `` | `/// the` |
| `crates/verter_relay_shim/src/main_tests.rs:419` | `` | `/// F2 — the setup-window race DECISION: a` |
| `crates/verter_relay_shim/src/main_tests.rs:451` | `` | `/// F3 — a` |
| `crates/verter_relay_shim/tests/cases/shim_live.rs:1345` | `` | `/// Platform-neutral on purpose: the` |
| `crates/verter_relay_shim/tests/cases/shim_live.rs:1451` | `` | `/// gate is SOUND because the shim installs its` |
| `crates/verter_relay_shim/tests/cases/shim_live.rs:1905` | `` | `// THE PORTABLE regression test for the runtime` |
| `crates/verter_relay_shim/tests/cases/shim_live.rs:2066` | `` | `///` |
| `crates/verter_relay_shim/tests/cases/shim_live.rs:2069` | `` | `reachable. The tests that DO discriminate that` |
| `crates/verter_relay_shim/tests/support/fake_tsgo_heartbeat.rs:133` | `` | `/// the` |
| `crates/verter_scheduler/src/dag_supersede_index_tests.rs:15` | `` | `er / record / insert / remove / drain / scrub /` |
| `crates/verter_scheduler/src/dag_supersede_index_tests.rs:404` | `` | `/// complete / cancel / drain / scrub /` |
| `crates/verter_scheduler/src/driver.rs:37` | `` | `/// Wake the driver so it can observe` |
| `crates/verter_scheduler/src/scheduler.rs:942` | `` | `/// Final drain on` |
| `crates/verter_scheduler/src/scheduler.rs:1333` | `impl Scheduler` | `/// 'Arc' allows 'Drop' to run (sets` |
| `crates/verter_scheduler/src/scheduler.rs:2911` | `impl Scheduler` | `/// Signals` |
| `crates/verter_scheduler/src/scheduler.rs:3128` | `impl Scheduler` | `/// external Arc is dropped or the` |
| `crates/verter_scheduler/src/scheduler.rs:13852` | `` | `the DAG, deletes the 'FileNode' and drains the` |
| `crates/verter_session/src/component_meta_host.rs:262` | `impl ComponentMetaHost` | `/// Terminal` |
| `crates/verter_session/src/host_audit_runtime.rs:276` | `impl HostAuditRuntime` | `///` |
| `crates/verter_session/src/host_batch_coordinator.rs:62` | `` | `//! cancellation or` |
| `crates/verter_session/src/meta.rs:193` | `` | `/// Terminal` |
| `crates/verter_session/src/meta.rs:210` | `impl MetaProject` | `/// Check the` |
| `crates/verter_session/src/meta.rs:367` | `impl MetaProject` | `/// Terminal` |
| `crates/verter_session/src/types.rs:2656` | `impl CompileFailure` | `/// (cancellation / supersession /` |
| `crates/verter_session/tests/cases/g_misc0/sampler_thread_joined_at_host_drop.rs:6` | `` | `//! owner thread so` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:17` | `` | `y invalidation across repeated edits; scheduler` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:74` | `` | `/// and signals` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:183` | `` | `/// Scheduler` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:185` | `` | `/// or DAG-waiter state that leaks across a` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:207` | `` | `/// 'MetaProject::` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:210` | `` | `///` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:21` | `` | `//! 4. The 'exit'-sending teardown ('` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:25` | `` | `"exit"' method literal appears ONLY inside the '` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:38` | `` | `//! 'pub async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:39` | `` | `//! site outside '` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:163` | `` | `/// Predicate 4: the 'exit'-sending teardown ('` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:199` | `` | `/// (the '` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:199` | `` | `shutdown' body), and at least one exists there (` |
| `crates/verter_tsgo_api/src/attach.rs:46` | `` | `//! engine gets the full private '` |
| `crates/verter_tsgo_api/src/attach.rs:49` | `` | `//! pipe, never 'exit'/'` |
| `crates/verter_tsgo_api/src/attach.rs:50` | `` | `//! the 'exit'-sending '` |
| `crates/verter_tsgo_api/src/attach.rs:133` | `` | `/// 'exit'/'` |
| `crates/verter_tsgo_api/src/attach.rs:142` | `` | `/// never 'exit'/'` |
| `crates/verter_tsgo_api/src/attach.rs:616` | `impl TsgoAttach<Owned>` | `/// OWNED teardown: the full private '` |
| `crates/verter_tsgo_api/src/attach.rs:617` | `impl TsgoAttach<Owned>` | `/// The private '` |
| `crates/verter_tsgo_api/src/attach.rs:635` | `impl TsgoAttach<NonOwning>` | `/// 'exit'/'` |
| `crates/verter_tsgo_api/src/attach.rs:666` | `impl TsgoAttach<NonOwning>` | `/// 'exit'/'` |
| `crates/verter_tsgo_api/src/attach.rs:677` | `impl TsgoAttach<NonOwning>` | `/// no 'exit'/'` |
| `crates/verter_tsgo_api/src/attach.rs:702` | `impl TsgoAttach<NonOwning>` | `erlays and drop the '--api' pipe, never 'exit'/'` |
| `crates/verter_tsgo_api/src/attach_tests.rs:4` | `` | `//! ('teardown' → '` |
| `crates/verter_tsgo_api/src/control/messages.rs:184` | `` | `d connection. Lifecycle writes, initialization,` |
| `crates/verter_tsgo_api/src/control/server.rs:226` | `impl ControlServer` | `for Verter's OWN overlays only — never 'exit'/'` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:1301` | `` | `/// mode) and '` |
| `crates/verter_tsgo_api/src/jsonrpc/connection.rs:35` | `` | `ler has none ('Value::Null'). LSP methods like '` |
| `crates/verter_tsgo_api/src/jsonrpc/connection_tests.rs:295` | `` | `/// '` |
| `crates/verter_tsgo_api/src/relay.rs:730` | `` | `/// The three pump/writer tasks; aborted on` |
| `crates/verter_type_runtime/src/lib.rs:9` | `` | `//! - Minimal session lifecycle (start,` |
| `crates/verter_type_runtime/src/resilient.rs:249` | `` | `/// Deliberate-teardown intent. Set by '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:508` | `` | `/// Deliberate-teardown intent. Set by '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:508` | `` | `eardown intent. Set by 'shutdown()' BEFORE the '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:588` | `` | `ler has none ('Value::Null'). LSP methods like '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:1219` | `` | `down_intent' disarms that signal: a deliberate '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:1220` | `` | `/// sending '` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2136` | `` | `/// originate` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:2329` | `impl TsgoTypeProvider` | `/// never originates '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3647` | `` | `/// Regression test for Fix 4:` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4439` | `` | `/// A deliberate '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4466` | `` | `/// The positive control: WITHOUT a` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4487` | `` | `/// '` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:619` | `` | `/// @ai-generated — tsserver` |
| `crates/verter_wasm/src/lib.rs:2028` | `#[wasm_bindgen(js_class = MetaSession)] impl WasmMetaSession` | `/// Throws on project-level` |

**`comment`** (62)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:321` | `#[tokio::test] async fn sync_artifacts_applies_update_file_then_probe_is_fresh()` | `//` |
| `crates/verter_dx_baseline/src/main_tests.rs:1558` | `#[tokio::test] async fn dispatch_loop_handles_valid_frame_then_clean_eof()` | `// A valid` |
| `crates/verter_lsp/src/server/lifecycle.rs:493` | `pub(super) async fn handle_shutdown(server: &VerterLanguageServer) -> Result<()>` | `acefully shut down the type provider (sends LSP` |
| `crates/verter_lsp/src/server/mod.rs:94` | `` | `nguageServer' covering initialize, initialized,` |
| `crates/verter_lsp/tests/cases/client_lifetime.rs:86` | `fn initialize_with_standard_client_pid(lsp: &mut Child, client_pid: u32)` | `// lifetime should drive this test's` |
| `crates/verter_lsp/tests/cases/stdio_launch_smoke.rs:319` | `#[test] fn verter_lsp_initialize_handshake_returns_capabilities()` | `// ──` |
| `crates/verter_lsp/tests/cases/stdio_launch_smoke.rs:324` | `#[test] fn verter_lsp_initialize_handshake_returns_capabilities()` | `// Read until the` |
| `crates/verter_lsp/tests/cases/stdio_launch_smoke.rs:351` | `#[test] fn verter_lsp_initialize_handshake_returns_capabilities()` | `// capabilities, and answered '` |
| `crates/verter_relay_shim/src/main.rs:107` | `fn main() -> ExitCode` | `// default signal disposition and re-raise. The` |
| `crates/verter_relay_shim/src/main.rs:109` | `fn main() -> ExitCode` | `ever completes, and a 'Drop'/'shutdown_timeout'` |
| `crates/verter_relay_shim/src/main.rs:114` | `fn main() -> ExitCode` | `dvertisement removed, the child reaped), so the` |
| `crates/verter_relay_shim/src/main.rs:730` | `olve_setup_race<T>( outcome: SetupOutcome<T>, pending_signal_after_setup: Option<i32>, ) -> SetupResolution<T>` | `// A setup ERROR does NOT win over a` |
| `crates/verter_relay_shim/src/main.rs:823` | `async fn run_relay(args: ShimArgs) -> Result<ShimExit, String>` | `// Install the Unix` |
| `crates/verter_relay_shim/src/main.rs:824` | `async fn run_relay(args: ShimArgs) -> Result<ShimExit, String>` | `// exists a` |
| `crates/verter_relay_shim/src/main.rs:890` | `async fn run_relay(args: ShimArgs) -> Result<ShimExit, String>` | `ol accept loop. On Unix this is RACED against a` |
| `crates/verter_relay_shim/src/main.rs:971` | `async fn run_relay(args: ShimArgs) -> Result<ShimExit, String>` | `dow. On Unix, RACE the fallible setup against a` |
| `crates/verter_relay_shim/src/main.rs:975` | `async fn run_relay(args: ShimArgs) -> Result<ShimExit, String>` | `ard reaps as the 'Err' unwinds). Windows has no` |
| `crates/verter_relay_shim/src/main.rs:985` | `async fn run_relay(args: ShimArgs) -> Result<ShimExit, String>` | `select above polled its signal arm just once. A` |
| `crates/verter_relay_shim/src/main.rs:1016` | `async fn run_relay(args: ShimArgs) -> Result<ShimExit, String>` | `// Unix` |
| `crates/verter_relay_shim/src/main.rs:1018` | `async fn run_relay(args: ShimArgs) -> Result<ShimExit, String>` | `// select is BIASED so a` |
| `crates/verter_relay_shim/src/main_tests.rs:429` | `#[cfg(unix)] #[test] fn setup_signal_wins_over_later_setup_error()` | `// A delivered` |
| `crates/verter_relay_shim/tests/cases/shim_live.rs:1491` | `[tokio::test(flavor = , worker_threads = 2)] async fn shim_sigterm_reaps_owned_child_and_reraises_the_signal()` | `gate: the advertisement is published AFTER the` |
| `crates/verter_scheduler/src/dag.rs:2380` | `impl SchedulerDag` | `// completion / supersede / fail /` |
| `crates/verter_scheduler/src/dag_supersede_index_tests.rs:449` | `#[test] fn reverse_index_matches_scan_after_removals_and_shutdown()` | `// remove-owner + per-file` |
| `crates/verter_scheduler/src/scheduler.rs:2214` | `#[cfg(not(target_arch = ))] pub fn reset(&self)` | `// 6. Unset` |
| `crates/verter_scheduler/src/scheduler.rs:2238` | `pub fn quiesce(&self)` | `// Signal` |
| `crates/verter_scheduler/src/scheduler.rs:3650` | `prepared_under_lock( &self, dag: &mut SchedulerDag, prepared: PreparedRequest, post: &mut AdmissionPostWork, )` | `// drains the` |
| `crates/verter_scheduler/src/scheduler.rs:7076` | `fn drop(&mut self)` | `// Set` |
| `crates/verter_scheduler/src/scheduler.rs:7084` | `fn drop(&mut self)` | `// signals` |
| `crates/verter_scheduler/src/scheduler.rs:7096` | `fn drop(&mut self)` | `// Signal` |
| `crates/verter_scheduler/src/scheduler.rs:8418` | `#[test] fn shutdown_signals_pending_handles()` | `// Drop triggers` |
| `crates/verter_scheduler/src/scheduler.rs:17153` | `#[cfg(not(target_arch = ))] #[test] fn wait_or_drive_does_not_hold_dag_lock_while_blocked()` | `// and the test signals` |
| `crates/verter_session/src/host_audit_runtime.rs:505` | `fn drop(&mut self)` | `// too — the observable asserts a clean` |
| `crates/verter_session/src/host_audit_runtime.rs:527` | `#[cfg(not(target_arch = ))] fn sampler_loop(state: Arc<SamplerState>)` | `//` |
| `crates/verter_session/src/host_audit_runtime.rs:539` | `#[cfg(not(target_arch = ))] fn sampler_loop(state: Arc<SamplerState>)` | `// ABOVE this block would let the` |
| `crates/verter_session/src/host_audit_runtime.rs:562` | `#[cfg(not(target_arch = ))] fn sampler_loop(state: Arc<SamplerState>)` | `// read the resulting` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:219` | `#[test] fn meta_project_shutdown_then_new_project_in_same_process_is_clean()` | `// Idempotent — a second` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:175` | `fn shutdown_visibility_failures(src: &str) -> Vec<String>` | `// Reject EVERY visibility modifier on '` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:176` | `fn shutdown_visibility_failures(src: &str) -> Vec<String>` | `// 'pub(super)'` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:301` | `#[test] fn non_owning_attach_lifecycle()` | `// 'async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:302` | `#[test] fn non_owning_attach_lifecycle()` | `//` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:470` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `// The` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:470` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `n-visibility predicate: a PUBLIC 'exit'-sending` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:472` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `// passes; a source with no` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:508` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `// The '"exit"'-only-in-` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:509` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `// '"exit"' send site is inside the` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:510` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `// outside fails; a` |
| `crates/verter_shipped_cfg_contract/src/lib.rs:308` | `#[test] fn scheduler_shutdown_and_restart_replays_cleanly_under_shipped_cfg()` | `// the` |
| `crates/verter_tsgo_api/src/actor/mod.rs:404` | `async fn run(mut self)` | `// Drain a control message first (` |
| `crates/verter_tsgo_api/src/attach_tests.rs:425` | `` | `// and drops the --api pipe, NEVER exit/` |
| `crates/verter_tsgo_api/src/control/server_tests.rs:416` | `o::test(flavor = , worker_threads = 2)] async fn control_dispatch_drives_full_attach_lifecycle_through_relay()` | `// stays ALIVE. A` |
| `crates/verter_tsgo_api/src/jsonrpc/connection_tests.rs:210` | `#[tokio::test] async fn abandoned_request_is_pruned()` | `// and the connection is still usable for` |
| `crates/verter_tsgo_api/tests/cases/attach_live.rs:228` | `okio::test(flavor = , worker_threads = 2)] async fn attach_api_over_spawned_lsp_sees_didopen_overlay_carrier()` | `// connection, so teardown() takes the full` |
| `crates/verter_tsgo_api/tests/cases/attach_live.rs:313` | `#[tokio::test(flavor = , worker_threads = 2)] async fn one_process_serves_both_api_checker_and_lsp_feature()` | `connection ⇒ teardown() dispatches to the full` |
| `crates/verter_type_runtime/src/resilient.rs:639` | `, inner: Arc<RwLock<Option<Arc<P>>>>, log_name: &'static str, ) where P: TypeProvider + Send + Sync + 'static,` | `// their bounded` |
| `crates/verter_type_runtime/src/resilient.rs:977` | `tate<P, B>>, crash_notify: Arc<Notify>) where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `// TEARDOWN DISCRIMINATION: a deliberate '` |
| `crates/verter_type_runtime/src/resilient.rs:981` | `tate<P, B>>, crash_notify: Arc<Notify>) where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `// clean editor` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1826` | `tokio::test(start_paused = true)] async fn deliberate_shutdown_is_not_reported_as_a_crash_and_never_respawns()` | `// "crashed. Restarting" on every clean editor` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4021` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `// Never send` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4027` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `// Best-effort: try` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4043` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `// Idempotent/concurrent` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3657` | `#[tokio::test] async fn shutdown_completes_within_timeout_when_provider_unresponsive()` | `// Simulate the` |

**`string-literal`** (87)

| site | context | snippet |
|---|---|---|
| `crates/verter_dx_baseline/src/main_tests.rs:324` | `#[tokio::test] async fn sync_artifacts_applies_update_file_then_probe_is_fresh()` | `other => panic!("expected` |
| `crates/verter_dx_baseline/src/main_tests.rs:1560` | `#[tokio::test] async fn dispatch_loop_handles_valid_frame_then_clean_eof()` | `let input: &[u8] = b"{\"type\":\"` |
| `crates/verter_dx_baseline/src/main_tests.rs:1567` | `#[tokio::test] async fn dispatch_loop_handles_valid_frame_then_clean_eof()` | `s.contains(r#""type":"` |
| `crates/verter_dx_baseline/src/main_tests.rs:1568` | `#[tokio::test] async fn dispatch_loop_handles_valid_frame_then_clean_eof()` | `"expected a` |
| `crates/verter_dx_baseline/src/protocol.rs:708` | `#[test] fn each_message_type_tag_is_exactly_as_specified()` | `assert!(matches!(parse(r#"{"type":"` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:322` | `#[tokio::test] async fn failed_activation_retries_after_cooldown_and_recovers()` | `"` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:382` | `#[tokio::test] async fn failed_activation_retries_after_cooldown_and_recovers()` | `"` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:599` | `#[tokio::test] async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries()` | `.expect("` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:603` | `#[tokio::test] async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries()` | `"` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:644` | `#[tokio::test] async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries()` | `.expect("` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:648` | `#[tokio::test] async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries()` | `"` |
| `crates/verter_lsp/tests/cases/stdio_launch_smoke.rs:322` | `#[test] fn verter_lsp_initialize_handshake_returns_capabilities()` | `&json!({ "jsonrpc": "2.0", "id": 2, "method": "` |
| `crates/verter_lsp/tests/cases/stdio_launch_smoke.rs:327` | `#[test] fn verter_lsp_initialize_handshake_returns_capabilities()` | `ssage = next_message(&rx, &mut child, "awaiting` |
| `crates/verter_lsp/tests/cases/stdio_launch_smoke.rs:331` | `#[test] fn verter_lsp_initialize_handshake_returns_capabilities()` | `"` |
| `crates/verter_lsp/tests/cases/stdio_launch_smoke.rs:339` | `#[test] fn verter_lsp_initialize_handshake_returns_capabilities()` | `"never received a` |
| `crates/verter_relay_shim/src/main_tests.rs:224` | `#[test] fn shutdown_signal_install_precedes_spawn_and_disarm()` | `.expect("the` |
| `crates/verter_relay_shim/src/main_tests.rs:235` | `#[test] fn shutdown_signal_install_precedes_spawn_and_disarm()` | `"the Unix` |
| `crates/verter_relay_shim/src/main_tests.rs:242` | `#[test] fn shutdown_signal_install_precedes_spawn_and_disarm()` | `"the` |
| `crates/verter_relay_shim/src/main_tests.rs:326` | `#[test] fn teardown_select_prioritizes_the_shutdown_signal()` | `"the` |
| `crates/verter_relay_shim/src/main_tests.rs:414` | `#[test] fn setup_window_races_shutdown_signal_before_disarm()` | `"the` |
| `crates/verter_relay_shim/src/main_tests.rs:433` | `#[cfg(unix)] #[test] fn setup_signal_wins_over_later_setup_error()` | `"a delivered` |
| `crates/verter_scheduler/src/dag_supersede_index_tests.rs:461` | `#[test] fn reverse_index_matches_scan_after_removals_and_shutdown()` | `"global` |
| `crates/verter_scheduler/src/scheduler.rs:8422` | `#[test] fn shutdown_signals_pending_handles()` | `ate.is_some(), "handle should be resolved after` |
| `crates/verter_scheduler/src/scheduler.rs:14074` | `#[cfg(not(target_arch = ))] #[test] fn signal_file_shutdown_at_is_scoped_to_one_generation()` | `ation's waiter must survive a generation-scoped` |
| `crates/verter_session/src/host_compile_atomic_upsert_tests.rs:313` | `#[test] fn upsert_batch_completion_mapping_preserves_error_strings()` | `upsert_req("/map-` |
| `crates/verter_session/src/host_compile_atomic_upsert_tests.rs:313` | `#[test] fn upsert_batch_completion_mapping_preserves_error_strings()` | `upsert_req("/map-shutdown.vue", &good_template("` |
| `crates/verter_session/src/host_compile_atomic_upsert_tests.rs:427` | `#[test] fn upsert_batch_result_indices_map_to_prepared_canonicals()` | `upsert_req("/zip-` |
| `crates/verter_session/src/host_compile_atomic_upsert_tests.rs:505` | `#[test] fn upsert_batch_result_indices_map_to_prepared_canonicals()` | `upsert_req("/zip-` |
| `crates/verter_session/tests/cases/shared_process_contract.rs:226` | `#[test] fn meta_project_shutdown_then_new_project_in_same_process_is_clean()` | `project's` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:168` | `fn shutdown_visibility_failures(src: &str) -> Vec<String>` | `if !src.contains("async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:170` | `fn shutdown_visibility_failures(src: &str) -> Vec<String>` | `"the owned teardown 'async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:179` | `fn shutdown_visibility_failures(src: &str) -> Vec<String>` | `"pub async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:180` | `fn shutdown_visibility_failures(src: &str) -> Vec<String>` | `"pub(crate) async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:181` | `fn shutdown_visibility_failures(src: &str) -> Vec<String>` | `"pub(super) async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:185` | `fn shutdown_visibility_failures(src: &str) -> Vec<String>` | `"'` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:215` | `fn exit_only_in_shutdown_failures(src: &str, span: (usize, usize)) -> Vec<String>` | `"'` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:222` | `fn exit_only_in_shutdown_failures(src: &str, span: (usize, usize)) -> Vec<String>` | `n '\"exit\"' method literal exists OUTSIDE the '` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:223` | `fn exit_only_in_shutdown_failures(src: &str, span: (usize, usize)) -> Vec<String>` | `(byte offsets {outside:?}) — '` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:304` | `#[test] fn non_owning_attach_lifecycle()` | `et shutdown_span = fn_body_span(&src, "async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:306` | `#[test] fn non_owning_attach_lifecycle()` | `"could not extract the 'async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:474` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `pub async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:480` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `"a 'pub async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:485` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `pub(crate) async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:491` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `"a 'pub(crate) async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:494` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:498` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `"a private 'async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:505` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `"a source with no 'async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:513` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:515` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `n_exit_ok = fn_body_span(src_exit_ok, "async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:518` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `"a source whose sole '\"exit\"' site is inside` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:522` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:524` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `it_leak = fn_body_span(src_exit_leak, "async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:529` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `"an '\"exit\"' send site outside` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:532` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:534` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `uous = fn_body_span(src_exit_vacuous, "async fn` |
| `crates/verter_session/tests/g_extts/non_owning_attach_lifecycle.rs:537` | `#[test] fn non_owning_attach_lifecycle_self_test_discriminates()` | `"a` |
| `crates/verter_session/tests/g_extts/shared_mode_requires_full_ts_lsp_proxy.rs:375` | `#[test] fn shared_mode_requires_full_ts_lsp_proxy_self_test_discriminates()` | `c fn teardown(self) -> TsgoApiResult<()> { self.` |
| `crates/verter_tsgo_api/src/attach_tests.rs:518` | `#[tokio::test] async fn non_owning_detach_retracts_overlays_and_never_exits_or_kills()` | `!methods.iter().any(\|m\| m == "` |
| `crates/verter_tsgo_api/src/attach_tests.rs:519` | `#[tokio::test] async fn non_owning_detach_retracts_overlays_and_never_exits_or_kills()` | `"NON-OWNING teardown must NEVER send '` |
| `crates/verter_tsgo_api/src/fake_engine.rs:316` | `fn serve_lsp(scenario: Scenario)` | `("` |
| `crates/verter_tsgo_api/src/jsonrpc/connection_tests.rs:307` | `#[test] fn jsonrpc_message_omits_null_params_and_keeps_real_params()` | `let shutdown = super::jsonrpc_message(Some(3), "` |
| `crates/verter_tsgo_api/src/jsonrpc/connection_tests.rs:310` | `#[test] fn jsonrpc_message_omits_null_params_and_keeps_real_params()` | `"null request params must be OMITTED, got {` |
| `crates/verter_tsgo_api/src/relay_tests.rs:108` | `#[tokio::test] async fn carrier_channel_refuses_exit_shutdown_initialize_and_arbitrary()` | `for method in ["exit", "` |
| `crates/verter_tsgo_api/src/relay_tests.rs:138` | `#[tokio::test] async fn carrier_channel_refuses_exit_shutdown_initialize_and_arbitrary()` | `for denied in ["exit", "` |
| `crates/verter_tsgo_api/tests/cases/real_engine.rs:217` | `[tokio::test(flavor = , worker_threads = 2)] async fn real_engine_initialize_update_snapshot_and_diagnostics()` | `handle.close().await.expect("clean` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1092` | `pub(crate) async fn shutdown(&self)` | `.expect("` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1851` | `tokio::test(start_paused = true)] async fn deliberate_shutdown_is_not_reported_as_a_crash_and_never_respawns()` | `"a deliberate` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1858` | `tokio::test(start_paused = true)] async fn deliberate_shutdown_is_not_reported_as_a_crash_and_never_respawns()` | `"a deliberate` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4030` | `fn shutdown(&self) -> ProviderFuture<'_, ()>` | `let _ = transport.request("` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3660` | `#[tokio::test] async fn shutdown_completes_within_timeout_when_provider_unresponsive()` | `let _ = transport.request("` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3686` | `#[tokio::test] async fn managed_shutdown_kills_and_reaps_unresponsive_owned_child()` | `.expect("managed` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3687` | `#[tokio::test] async fn managed_shutdown_kills_and_reaps_unresponsive_owned_child()` | `.expect("managed` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3696` | `#[tokio::test] async fn managed_shutdown_kills_and_reaps_unresponsive_owned_child()` | `"managed` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4451` | `#[tokio::test] async fn shutdown_disarms_the_eof_crash_signal()` | `provider.shutdown().await.expect("` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4462` | `#[tokio::test] async fn shutdown_disarms_the_eof_crash_signal()` | `"a deliberate` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4501` | `#[test] fn jsonrpc_body_omits_null_params_and_keeps_real_params()` | `let shutdown = jsonrpc_body(Some(7), "` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4504` | `#[test] fn jsonrpc_body_omits_null_params_and_keeps_real_params()` | `"null request params must be OMITTED, got {` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:1479` | `#[tokio::test] async fn carrier_refresh_receipt_waits_for_deferred_plugin_graph_application()` | `panic!("unexpected transport` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:1500` | `#[tokio::test] async fn carrier_refresh_receipt_waits_for_deferred_plugin_graph_application()` | `panic!("unexpected transport` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:1843` | `#[tokio::test] async fn workspace_symbol_frontier_opens_only_one_bootstrap_per_project()` | `panic!("unexpected` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3829` | `async fn shutdown(&self)` | `.expect("` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:4384` | `async fn receive_tsserver_request( stdin_rx: &mut mpsc::Receiver<TsserverStdinMessage>, ) -> serde_json::Value` | `panic!("unexpected` |
| `crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs:212` | ` worker_threads = 2)] async fn owned_bare_vue_import_resolves_to_declaration_carrier_and_public_member_flows()` | `provider.shutdown().await.expect("` |
| `crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs:271` | `worker_threads = 2)] async fn owned_bare_vue_import_fails_closed_when_declaration_carrier_didopen_suppressed()` | `provider.shutdown().await.expect("` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:165` | `(flavor = , worker_threads = 2)] async fn owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process()` | `provider.shutdown().await.expect("` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:258` | `lavor = , worker_threads = 2)] async fn owned_api_oracle_resolves_multiple_projects_per_query_on_one_process()` | `provider.shutdown().await.expect("` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:293` | `#[tokio::test(flavor = , worker_threads = 2)] async fn owned_provider_is_one_process_no_second_spawn()` | `provider.shutdown().await.expect("` |

### `configure_paths` — declared at `crates/verter_type_runtime/src/traits.rs:286`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:286` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:1294` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1267` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:712` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:423` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:617` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`impl-test`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:325` | `impl TypeProvider for SlowConfigurePathsProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:490` | `impl TypeProvider for TriggerSensitiveCompletionProvider` | `fn` |
| `crates/verter_lsp/src/server_tests.rs:667` | `impl TypeProvider for DotTriggerRequiredCompletionProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:893` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1559` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:303` | `impl TypeProvider for MockProvider` | `fn` |

**`call-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/background_init.rs:302` | `pub(super) async fn background_init(args: BackgroundInitArgs) -> Result<()>` | `if let Err(e) = tp.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:236` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:386` | `paths( &self, base_url: String, paths: serde_json::Value, background: bool, ) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_type_runtime/src/resilient.rs:831` | `async fn replay_into<P: TypeProvider>(&self, provider: &P, log_name: &str)` | `.` |
| `crates/verter_type_runtime/src/resilient.rs:912` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `provider.` |

**`call-forwarding`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1268` | `fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:618` | `fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()>` | `self.lsp.` |

**`call-trait-default`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:484` | `fn configure_paths_background( &self, base_url: &str, paths: serde_json::Value, ) -> ProviderFuture<'_, ()>` | `self.` |

**`call-test`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/resilient_provider_tests.rs:226` | `#[tokio::test] async fn restart_replays_cached_state_without_downgrading_loaded_files()` | `.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:291` | `#[tokio::test] async fn restart_replays_all_cached_path_configs()` | `.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:295` | `#[tokio::test] async fn restart_replays_all_cached_path_configs()` | `.` |
| `crates/verter_lsp/src/test_harness.rs:601` | `pub(crate) async fn build(self) -> Option<RealProviderTestSession>` | `let _ = provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:162` | `#[tokio::test] async fn lifecycle_is_cached_without_activation_and_replayed_once_on_first_query()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1422` | `#[tokio::test(start_paused = true)] async fn restart_replays_state_without_downgrading_loaded_files()` | `.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5044` | `#[tokio::test] async fn tsgo_sends_no_workspace_configuration_notification()` | `provider.` |

**`doc-comment`** (19)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:206` | `` | `/// Fires when '` |
| `crates/verter_lsp/src/server_tests.rs:209` | `` | `/// Never notified — '` |
| `crates/verter_lsp/src/server_tests.rs:3909` | `` | `ring the provider await. The rebuild pauses in '` |
| `crates/verter_lsp/src/svelte_assets.rs:9` | `` | `//! it through '` |
| `crates/verter_lsp/src/svelte_assets.rs:631` | `` | `/// object for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:277` | `` | `/// One-shot async gate for '` |
| `crates/verter_lsp/src/type_provider/mock.rs:713` | `impl MockTypeProvider` | `/// Pause the next '` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:105` | `` | `/// 'close_file'), and '` |
| `crates/verter_session/src/framework/svelte_jsx_assets.rs:10` | `` | `//! consumers — '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:10` | `` | `//! '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:60` | `` | `//! inside 'ExtensionTypeProvider::` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:66` | `` | `t falls inside the matched '{ … }' body of the '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:142` | `` | `//! placed after the '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:254` | `` | `/// request-name occurrence — the '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:261` | `` | `/// The impl-block discriminant: the '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:303` | `` | `nction-signature line. Used both to LOCATE the '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:306` | `` | `rs in any order until the 'fn ' keyword, so 'fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:360` | `` | `/// * the '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:519` | `` | `/// locator behind BOTH the '` |

**`comment`** (17)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:4647` | `#[tokio::test] async fn initialized_returns_before_background_configure_paths_completes()` | `// '` |
| `crates/verter_lsp/src/server_tests.rs:4676` | `#[tokio::test] async fn initialized_returns_before_background_configure_paths_completes()` | `// proves '` |
| `crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs:961` | `` | `// tsserver '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:689` | `#[test] fn no_fallback_to_inferred_anywhere()` | `ngle allow-listed file (and there, only inside '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:705` | `#[test] fn no_fallback_to_inferred_anywhere()` | `// tsserver must NOT define a '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:709` | `#[test] fn no_fallback_to_inferred_anywhere()` | `// '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:756` | `#[test] fn no_fallback_to_inferred_anywhere()` | `ACTLY ONE live occurrence, and ONLY inside the '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1135` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `// ── the '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1237` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `'calls_named_fn' would be fooled). Mirrors the` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1495` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `// occurrence inside the '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1501` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `// fn' so the body-span scan finds '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1570` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `// brace-matched '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1597` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `// the` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1611` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `// '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1612` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `on wrongly accepts this (count==1, enclosing ==` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1634` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `// A STALE ANCHOR: no '` |
| `crates/verter_workspace/src/config.rs:721` | `` | `// Raw Paths Extraction (for tsserver` |

**`string-literal`** (25)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/server_tests.rs:4659` | `#[tokio::test] async fn initialized_returns_before_background_configure_paths_completes()` | `"initialized() should not wait for` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:260` | `` | `const ALLOWLISTED_INFERRED_CONFIG_FN: &str = "` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:410` | `fn allowlisted_request_violations(src: &str) -> Vec<String>` | `ope after its closing brace) — only the in-body` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:712` | `#[test] fn no_fallback_to_inferred_anywhere()` | `&& line.trim_start().starts_with("fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:716` | `#[test] fn no_fallback_to_inferred_anywhere()` | `}:{}: the tsserver transport must NOT define a '` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1137` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `" fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1139` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `.starts_with("fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1140` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `"a tsserver` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1504` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `" fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1506` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `Some("` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1507` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `"a trait-impl` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1520` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `fn_name_introduced_on_line(" pub(super) fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1521` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `Some("` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1525` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `ame_introduced_on_line(" pub(crate) async fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1526` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `Some("` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1572` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `" fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1578` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `"the single` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1585` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `" fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1599` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `" fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1615` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `" fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1623` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `"an occurrence after the` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1628` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `allowlisted_request_violations(&in_impl(" fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1640` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `"a missing` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1647` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `" fn` |
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1679` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `e live extension_provider.rs must expose a real` |

### `notify_carrier_changed` — declared at `crates/verter_type_runtime/src/traits.rs:309`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:309` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1279` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:872` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:724` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:70` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3103` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:1052` | `impl TypeProvider for MockTypeProvider` | `fn` |

**`call-production`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsserver/project_router.rs:888` | `fn notify_carriers_changed<'a>( &'a self, companion_paths: &'a [String], ) -> ProviderFuture<'a, ()>` | `self.` |

**`call-forwarding`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1280` | `fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:877` | `fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:736` | `fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()>` | `provider.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:76` | `fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()>` | `provider.` |

**`call-trait-default`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:323` | `fn notify_carriers_changed<'a>( &'a self, companion_paths: &'a [String], ) -> ProviderFuture<'a, ()>` | `self.` |

**`doc-comment`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/project_sync.rs:393` | `impl ProjectSync` | `eting content authority. The publish path (and '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:570` | `impl TsserverTransport` | `/// '` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:1366` | `` | `/// Capture the wire frames the '` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:4312` | `` | `/// caller does not even use — '` |

### `notify_carriers_changed` — declared at `crates/verter_type_runtime/src/traits.rs:317`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:317` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsserver/project_router.rs:882` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:740` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:80` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3121` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:1060` | `impl TypeProvider for MockTypeProvider` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/external_ts/publish_coordinator.rs:393` | `blished_companions( &self, companion_paths: &[String], ) -> Result<(), verter_type_runtime::TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:298` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `.` |

**`call-forwarding`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:755` | `fn notify_carriers_changed<'a>( &'a self, companion_paths: &'a [String], ) -> ProviderFuture<'a, ()>` | `provider.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:87` | `fn notify_carriers_changed<'a>( &'a self, companion_paths: &'a [String], ) -> ProviderFuture<'a, ()>` | `provider.` |

### `register_carrier_member` — declared at `crates/verter_type_runtime/src/traits.rs:353`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:353` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1283` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:894` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:759` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:340` | `impl ProjectSync` | `pub async fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:91` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3152` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:1071` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:188` | `impl TypeProvider for MockProvider` | `fn` |

**`call-production`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:254` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_type_runtime/src/resilient.rs:852` | `async fn replay_into<P: TypeProvider>(&self, provider: &P, log_name: &str)` | `provider.` |
| `crates/verter_type_runtime/src/resilient.rs:935` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `.` |

**`call-forwarding`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1290` | `, source_path: &str, companion_path: &str, content: &str, project_file_name: &str, ) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:910` | `, source_path: &str, companion_path: &str, content: &str, project_file_name: &str, ) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:785` | `, source_path: &str, companion_path: &str, content: &str, project_file_name: &str, ) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:348` | `e_path: &str, companion_path: &str, content: &str, project_file_name: &str, ) -> Result<(), TypeProviderError>` | `.` |

**`call-trait-default`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:376` | `h: &'a str, companion_path: &'a str, content: &'a str, project_file_name: &'a str, ) -> ProviderFuture<'a, ()>` | `self.` |

**`call-test`** (13)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/resilient_provider_tests.rs:339` | `#[tokio::test] async fn register_carrier_member_forwards_and_replays_after_restart()` | `.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:428` | `#[tokio::test(flavor = )] async fn registration_racing_respawn_replay_reaches_fresh_inner()` | `.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:462` | `#[tokio::test(flavor = )] async fn registration_racing_respawn_replay_reaches_fresh_inner()` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:294` | `#[tokio::test] async fn failed_activation_retries_after_cooldown_and_recovers()` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed_tests.rs:579` | `#[tokio::test] async fn failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:570` | `async fn register_recovery_carriers<P: TypeProvider>(provider: &P)` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:989` | `pub(crate) async fn register_carriers(&self)` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1282` | `#[tokio::test(start_paused = true)] async fn carrier_registration_racing_respawn_reaches_fresh_inner()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1320` | `#[tokio::test(start_paused = true)] async fn carrier_registration_survives_respawn_contentlessly()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1359` | `#[tokio::test(start_paused = true)] async fn retracted_carrier_is_absent_from_restart_replay()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1368` | `#[tokio::test(start_paused = true)] async fn retracted_carrier_is_absent_from_restart_replay()` | `.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1502` | `#[tokio::test(start_paused = true)] async fn register_carrier_forwards_to_the_live_provider()` | `.` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3709` | `async fn register_carriers(&self)` | `.` |

**`doc-comment`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/external_ts/membership_reconciler.rs:14` | `` | `//! single-writer actor's command API ('` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:394` | `` | `/// Production path: a '` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:404` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:224` | `` | `/// '` |
| `crates/verter_lsp/src/type_provider/mock.rs:615` | `impl MockTypeProvider` | `/// Test seam: make '` |
| `crates/verter_type_runtime/src/resilient.rs:137` | `` | `/ respawn this is replayed via the CONTENTLESS '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:1836` | `` | `/// Populated by ['TypeProvider::` |

**`comment`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/real_provider_tests/external_ts_baseline.rs:276` | `` | `// override '` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:419` | `#[tokio::test(flavor = )] async fn registration_racing_respawn_replay_reaches_fresh_inner()` | `// replacement's '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:2997` | `fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `// here (it enters only via '` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:2136` | `#[tokio::test] async fn resync_reopens_source_with_content_and_active_carrier_contentlessly()` | `e tsconfig dir), exactly like the publish-time '` |

**`string-literal`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/resilient_provider_tests.rs:360` | `#[tokio::test] async fn register_carrier_member_forwards_and_replays_after_restart()` | `"` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1518` | `#[tokio::test(start_paused = true)] async fn register_carrier_forwards_to_the_live_provider()` | `"` |

### `register_carrier_metadata` — declared at `crates/verter_type_runtime/src/traits.rs:369`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:369` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsserver/project_router.rs:920` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:795` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:116` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3374` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:1103` | `impl TypeProvider for MockTypeProvider` | `fn` |

**`call-production`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/external_ts/membership_reconciler.rs:979` | `ource, binding: ProjectBinding, companions: Vec<CarrierCompanion>, ) -> Result<ReconcileOutcome, ReconcileErr>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:261` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_type_runtime/src/resilient.rs:859` | `async fn replay_into<P: TypeProvider>(&self, provider: &P, log_name: &str)` | `provider.` |
| `crates/verter_type_runtime/src/resilient.rs:945` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `.` |

**`call-forwarding`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsserver/project_router.rs:932` | `h: &'a str, companion_path: &'a str, content: &'a str, project_file_name: &'a str, ) -> ProviderFuture<'a, ()>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:831` | `, source_path: &str, companion_path: &str, content: &str, project_file_name: &str, ) -> ProviderFuture<'_, ()>` | `.` |

### `activate_carrier_member` — declared at `crates/verter_type_runtime/src/traits.rs:387`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:387` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1298` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:937` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:841` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:141` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3248` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:1123` | `impl TypeProvider for MockTypeProvider` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsserver/project_router.rs:968` | `fn activate_carrier_members<'a>( &'a self, members: &'a [CarrierActivation], ) -> ProviderFuture<'a, ()>` | `self.` |
| `crates/verter_type_runtime/src/resilient.rs:955` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1305` | `str, project_file_name: &str, script_kind: verter_type_runtime::CarrierScriptKind, ) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:952` | `tr, companion_path: &str, project_file_name: &str, script_kind: CarrierScriptKind, ) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:868` | `str, project_file_name: &str, script_kind: verter_type_runtime::CarrierScriptKind, ) -> ProviderFuture<'_, ()>` | `.` |

**`call-trait-default`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:408` | `fn activate_carrier_members<'a>( &'a self, members: &'a [CarrierActivation], ) -> ProviderFuture<'a, ()>` | `self.` |

**`call-test`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/resilient_tests.rs:998` | `pub(crate) async fn register_carriers(&self)` | `.` |
| `crates/verter_type_runtime/src/tsserver/ipc_tests.rs:3718` | `async fn register_carriers(&self)` | `.` |

### `activate_carrier_members` — declared at `crates/verter_type_runtime/src/traits.rs:402`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:402` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1313` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:962` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:878` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:165` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:3297` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:1143` | `impl TypeProvider for MockTypeProvider` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/external_ts/membership_reconciler.rs:780` | `rces( &self, sources: &[CanonicalSource], ) -> Result<usize, verter_type_runtime::protocol::TypeProviderError>` | `provider.` |
| `crates/verter_type_runtime/src/resilient.rs:964` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `provider.` |

**`call-forwarding`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1317` | `rier_members<'a>( &'a self, members: &'a [verter_type_runtime::CarrierActivation], ) -> ProviderFuture<'a, ()>` | `self.managed.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:898` | `rier_members<'a>( &'a self, members: &'a [verter_type_runtime::CarrierActivation], ) -> ProviderFuture<'a, ()>` | `provider.` |

### `resync_open_files` — declared at `crates/verter_type_runtime/src/traits.rs:421`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:421` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:1389` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1320` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:980` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:902` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:410` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:621` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4843` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/resync_singleflight.rs:145` | `lesced( coordinator: &Arc<ResyncCoordinator>, provider: Arc<dyn crate::type_provider::traits::TypeProvider>, )` | `let _ = provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:302` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |

**`call-forwarding`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1321` | `fn resync_open_files(&self) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:984` | `fn resync_open_files(&self) -> ProviderFuture<'_, ()>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:909` | `fn resync_open_files(&self) -> ProviderFuture<'_, ()>` | `provider.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:417` | `fn resync_open_files(&self) -> ProviderFuture<'_, ()>` | `Ok(provider) => provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:622` | `fn resync_open_files(&self) -> ProviderFuture<'_, ()>` | `self.lsp.` |

**`call-test`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:1204` | `#[tokio::test] async fn resync_rebinds_a_file_opened_before_the_ownership_authority_landed()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1257` | `#[tokio::test] async fn resync_closes_a_file_no_configured_project_owns_instead_of_rebinding_it()` | `.` |

**`ref-production`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:62` | `` | `` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:301` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `if desired.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:905` | `fn resync_open_files(&self) -> ProviderFuture<'_, ()>` | `self.desired.lock().unwrap().` |

**`doc-comment`** (10)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/background_init.rs:68` | `` | `/// Project-level coalescing singleflight for '` |
| `crates/verter_lsp/src/extension_provider_binding.rs:88` | `` | `t-resort answer — superseded by ['TypeProvider::` |
| `crates/verter_lsp/src/resync_singleflight.rs:3` | `` | `//! '` |
| `crates/verter_lsp/src/resync_singleflight.rs:27` | `` | `/// Coalescing singleflight gate for '` |
| `crates/verter_lsp/src/resync_singleflight.rs:136` | `` | `/// Convenience: run a provider's '` |
| `crates/verter_lsp/src/resync_singleflight.rs:244` | `` | `his is the ordering the gate changed: a direct '` |
| `crates/verter_lsp/src/server/mod.rs:490` | `` | `/// Project-level coalescing singleflight for '` |
| `crates/verter_lsp/src/server_tests.rs:3915` | `` | `e the stored authority or perform production's '` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:4692` | `` | `/// restart re-establishes it — tsgo's '` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:5135` | `` | `/// The '` |

**`comment`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:1122` | `` | `// 'background_init' calls '` |
| `crates/verter_lsp/src/type_provider/mock.rs:1600` | `ject_ownership( &self, authority: std::sync::Arc<dyn verter_type_runtime::traits::ConfiguredOwnerAuthority>, )` | `// '` |

### `update_workspace_folders` — declared at `crates/verter_type_runtime/src/traits.rs:426`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:426` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:1331` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |
| `crates/verter_lsp/src/tsgo/composite.rs:1324` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:990` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:913` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:434` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4087` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:625` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4807` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`impl-test`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:902` | `impl TypeProvider for FailingTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/mock.rs:1581` | `impl TypeProvider for MockTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient_tests.rs:311` | `impl TypeProvider for MockProvider` | `fn` |

**`call-production`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/background_init.rs:283` | `pub(super) async fn background_init(args: BackgroundInitArgs) -> Result<()>` | `let _ = tp.` |
| `crates/verter_lsp/src/server/lifecycle.rs:415` | `pub(super) async fn handle_initialized(server: &VerterLanguageServer, _params: InitializedParams)` | `let _ = tp.` |
| `crates/verter_lsp/src/server/lifecycle.rs:1137` | `handle_did_change_workspace_folders( server: &VerterLanguageServer, params: DidChangeWorkspaceFoldersParams, )` | `let _ = tp.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:249` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:426` | ` Vec<serde_json::Value>, removed: Vec<serde_json::Value>, background: bool, ) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_type_runtime/src/resilient.rs:825` | `async fn replay_into<P: TypeProvider>(&self, provider: &P, log_name: &str)` | `.` |
| `crates/verter_type_runtime/src/resilient.rs:923` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1329` | `ce_folders( &self, added: Vec<serde_json::Value>, removed: Vec<serde_json::Value>, ) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:999` | `ce_folders( &self, added: Vec<serde_json::Value>, removed: Vec<serde_json::Value>, ) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:630` | `ce_folders( &self, added: Vec<serde_json::Value>, removed: Vec<serde_json::Value>, ) -> ProviderFuture<'_, ()>` | `self.lsp.` |

**`call-trait-default`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:492` | `background( &self, added: Vec<serde_json::Value>, removed: Vec<serde_json::Value>, ) -> ProviderFuture<'_, ()>` | `self.` |

**`call-test`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:773` | `transport: ScriptedTsQueryTransport, nested_config: &str, ) -> ExtensionTypeProvider<ScriptedTsQueryTransport>` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:892` | `#[tokio::test] async fn without_an_ownership_authority_the_workspace_folder_is_the_last_resort()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1093` | `#[tokio::test] async fn without_an_ownership_authority_no_config_is_invented()` | `.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1172` | ` bootstrap_provider( transport: ScriptedTsQueryTransport, ) -> ExtensionTypeProvider<ScriptedTsQueryTransport>` | `.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:181` | `#[tokio::test] async fn update_workspace_folders_is_cached_while_restarting()` | `.` |
| `crates/verter_lsp/src/resilient_provider_tests.rs:230` | `#[tokio::test] async fn restart_replays_cached_state_without_downgrading_loaded_files()` | `.` |
| `crates/verter_lsp/src/test_harness.rs:589` | `pub(crate) async fn build(self) -> Option<RealProviderTestSession>` | `let _ = provider.` |
| `crates/verter_type_runtime/src/resilient_tests.rs:1426` | `#[tokio::test(start_paused = true)] async fn restart_replays_state_without_downgrading_loaded_files()` | `.` |

**`string-literal`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs:1617` | `#[test] fn no_fallback_to_inferred_anywhere_self_test_discriminates()` | `fn` |

### `set_project_ownership` — declared at `crates/verter_type_runtime/src/traits.rs:450`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:450` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider.rs:1364` | `impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T>` | `fn` |

**`impl-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:1593` | `impl TypeProvider for MockTypeProvider` | `fn` |

**`call-production`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/background_init.rs:271` | `pub(super) async fn background_init(args: BackgroundInitArgs) -> Result<()>` | `tp.` |

**`call-test`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/extension_provider_tests.rs:796` | `transport: ScriptedTsQueryTransport, nested_config: &str, ) -> ExtensionTypeProvider<ScriptedTsQueryTransport>` | `provider.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1202` | `#[tokio::test] async fn resync_rebinds_a_file_opened_before_the_ownership_authority_landed()` | `provider.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1255` | `#[tokio::test] async fn resync_closes_a_file_no_configured_project_owns_instead_of_rebinding_it()` | `provider.` |
| `crates/verter_lsp/src/extension_provider_tests.rs:1282` | `#[tokio::test] async fn an_authoritatively_unowned_file_fails_closed_rather_than_binding_an_invented_project()` | `provider.` |

### `child_pid` — declared at `crates/verter_type_runtime/src/traits.rs:453`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:453` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1341` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1015` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:929` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:448` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4108` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:633` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |
| `crates/verter_type_runtime/src/tsserver/ipc.rs:4803` | `impl TypeProvider for TsserverTypeProvider` | `fn` |

**`call-production`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/main.rs:1225` | `l<tower_lsp_server::Client>>, provider: &Arc<dyn TypeProvider>, tsgo_reason: &str, advisory: Option<String>, )` | `if let Some(pid) = provider.` |
| `crates/verter_lsp/src/server/lifecycle.rs:303` | `pub(super) async fn handle_initialized(server: &VerterLanguageServer, _params: InitializedParams)` | `if let Some(pid) = tp.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:332` | `fn announce_started(&self, provider: &Arc<dyn TypeProvider>)` | `let Some(pid) = provider.` |
| `crates/verter_type_runtime/src/resilient.rs:1075` | `tate<P, B>>, crash_notify: Arc<Notify>) where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `let child_pid = provider.` |

**`call-forwarding`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1342` | `fn child_pid(&self) -> Option<u32>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1018` | `fn child_pid(&self) -> Option<u32>` | `.find_map(\|provider\| provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:930` | `fn child_pid(&self) -> Option<u32>` | `self.current().and_then(\|provider\| provider.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:453` | `fn child_pid(&self) -> Option<u32>` | `rd\| guard.as_ref().and_then(\|provider\| provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:634` | `fn child_pid(&self) -> Option<u32>` | `self.lsp.` |

**`call-test`** (8)

| site | context | snippet |
|---|---|---|
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1666` | `(flavor = , worker_threads = 4)] async fn composite_successful_shared_route_never_activates_managed_fallback()` | `assert_eq!(h.composite.` |
| `crates/verter_relay_shim/tests/cases/shared_provider_live.rs:1708` | `(flavor = , worker_threads = 4)] async fn composite_successful_shared_route_never_activates_managed_fallback()` | `assert_eq!(h.composite.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:89` | `#[tokio::test] async fn initialized_non_owning_transport_serves_hover_without_initialize_or_child()` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3380` | `#[tokio::test] async fn test_child_pid_returns_id()` | `let pid = provider.` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3690` | `#[tokio::test] async fn managed_shutdown_kills_and_reaps_unresponsive_owned_child()` | `provider.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:110` | `(flavor = , worker_threads = 2)] async fn owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process()` | `let pid = provider.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:279` | `#[tokio::test(flavor = , worker_threads = 2)] async fn owned_provider_is_one_process_no_second_spawn()` | `let inner_pid = inner.` |
| `crates/verter_type_runtime/tests/cases/owned_provider_live.rs:288` | `#[tokio::test(flavor = , worker_threads = 2)] async fn owned_provider_is_one_process_no_second_spawn()` | `provider.` |

**`ref-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/resilient.rs:1075` | `tate<P, B>>, crash_notify: Arc<Notify>) where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `let` |
| `crates/verter_type_runtime/src/resilient.rs:1105` | `tate<P, B>>, crash_notify: Arc<Notify>) where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `state.notifier.provider_started(` |

**`doc-comment`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_relay_shim/src/main.rs:422` | `` | `/// 'killpg(` |
| `crates/verter_relay_shim/src/main_tests.rs:166` | `` | `/// makes a later 'killpg(` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3349` | `` | `/// Verify` |

**`comment`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_relay_shim/src/main.rs:435` | `#[cfg(unix)] fn contain_child_unix(parent_pid: u32) -> std::io::Result<()>` | `// group of its own, so teardown's 'killpg(` |
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3379` | `#[tokio::test] async fn test_child_pid_returns_id()` | `// The` |

**`string-literal`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:3385` | `#[tokio::test] async fn test_child_pid_returns_id()` | `"` |

### `open_file_background` — declared at `crates/verter_type_runtime/src/traits.rs:459`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:459` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:910` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1021` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:634` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:458` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4120` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:461` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`impl-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:978` | `impl TypeProvider for MockTypeProvider` | `fn` |

**`call-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:280` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:349` | `tent: String, access: Option<FileAccess>, lane: PriorityLane, update: bool, ) -> Result<(), TypeProviderError>` | `FileAccess::Open => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:548` | `pub async fn open_dts_background( &self, dts_path: &str, dts_content: &str, ) -> Result<(), TypeProviderError>` | `.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:143` | `elf, path: &str, content: &str, lane: ProviderLane, verb: ProviderFileVerb, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/resilient.rs:887` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Background => provider.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:914` | `fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1027` | `fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:462` | `fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

**`doc-comment`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/workspace_scanner.rs:1725` | `` | `/// Discriminating: 'MockTypeProvider' records '` |

**`string-literal`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/mock.rs:987` | `fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `Box::pin(async move { fail_or_ok(fail, "` |

### `load_file_background` — declared at `crates/verter_type_runtime/src/traits.rs:463`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:463` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:920` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1032` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:644` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:569` | `impl ProjectSync` | `pub async fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:467` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4124` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:465` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`call-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:289` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:350` | `tent: String, access: Option<FileAccess>, lane: PriorityLane, update: bool, ) -> Result<(), TypeProviderError>` | `FileAccess::Loaded => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:535` | `pub async fn load_dts_background( &self, dts_path: &str, dts_content: &str, ) -> Result<(), TypeProviderError>` | `.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:140` | `elf, path: &str, content: &str, lane: ProviderLane, verb: ProviderFileVerb, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/resilient.rs:892` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Background => provider.` |

**`call-forwarding`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:924` | `fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1038` | `fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:574` | `pub async fn load_file_background( &self, path: &str, content: &str, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:466` | `fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

### `update_file_background` — declared at `crates/verter_type_runtime/src/traits.rs:467`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:467` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:930` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1043` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:654` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:476` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4129` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:469` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`call-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:338` | `tent: String, access: Option<FileAccess>, lane: PriorityLane, update: bool, ) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:561` | `pub async fn sync_dts_background( &self, dts_path: &str, dts_content: &str, ) -> Result<(), TypeProviderError>` | `.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:582` | `pub async fn sync_file_background( &self, path: &str, content: &str, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:146` | `elf, path: &str, content: &str, lane: ProviderLane, verb: ProviderFileVerb, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/resilient.rs:897` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Background => provider.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:934` | `fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1049` | `fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:470` | `fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

### `close_file_background` — declared at `crates/verter_type_runtime/src/traits.rs:471`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:471` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (7)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:940` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1054` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:664` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:585` | `impl ProjectSync` | `pub async fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:485` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4133` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:473` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`call-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:368` | `async fn record_close( &self, path: String, lane: PriorityLane, ) -> Result<(), TypeProviderError>` | `PriorityLane::Background => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:566` | `pub async fn close_dts_background(&self, dts_path: &str) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:225` | `per) async fn close_tsx_in_lane( &self, tsx_path: &str, lane: ProviderLane, ) -> Result<(), TypeProviderError>` | `ProviderLane::Background => self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:257` | `c fn close_virtual_verter_types( &self, tsx_path: &str, lane: ProviderLane, ) -> Result<(), TypeProviderError>` | `ProviderLane::Background => self.provider.` |
| `crates/verter_type_runtime/src/resilient.rs:902` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Background => provider.` |

**`call-forwarding`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:943` | `fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1059` | `fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:586` | `pub async fn close_file_background(&self, path: &str) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:474` | `fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

### `get_diagnostics_background` — declared at `crates/verter_type_runtime/src/traits.rs:475`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:475` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:997` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1064` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:668` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:493` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4137` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:513` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`impl-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:163` | `impl TypeProvider for MarkerOwned` | `fn` |

**`call-production`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:821` | ` managed_diagnostics( &self, path: &str, background: bool, ) -> Result<Vec<TypeDiagnostic>, TypeProviderError>` | `self.managed.` |

**`call-forwarding`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsserver/project_router.rs:1069` | `fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:673` | `fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `.` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:498` | `fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `provider.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:514` | `fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>` | `self.lsp.` |

**`call-test`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:536` | `#[tokio::test] async fn gate_bound_carrier_delegates_to_owned_background()` | `.` |
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:555` | `#[tokio::test] async fn gate_non_bound_carrier_fails_closed_to_empty_background()` | `.` |

**`comment`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/tests/cases/owned_binding_gate.rs:550` | `#[tokio::test] async fn gate_non_bound_carrier_fails_closed_to_empty_background()` | `// an impl that delegated '` |

### `configure_paths_background` — declared at `crates/verter_type_runtime/src/traits.rs:479`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:479` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1271` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:716` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:504` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:384` | `paths( &self, base_url: String, paths: serde_json::Value, background: bool, ) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_type_runtime/src/resilient.rs:908` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `.` |

**`call-forwarding`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1276` | `fn configure_paths_background( &self, base_url: &str, paths: serde_json::Value, ) -> ProviderFuture<'_, ()>` | `self.managed.` |

**`call-test`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/tsgo/ipc_tests.rs:5046` | `#[tokio::test] async fn tsgo_sends_no_workspace_configuration_notification()` | `.` |

### `update_workspace_folders_background` — declared at `crates/verter_type_runtime/src/traits.rs:487`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:487` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1332` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1074` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:921` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:519` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4180` | `impl TypeProvider for TsgoTypeProvider` | `fn` |

**`call-production`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:423` | ` Vec<serde_json::Value>, removed: Vec<serde_json::Value>, background: bool, ) -> Result<(), TypeProviderError>` | `.` |
| `crates/verter_type_runtime/src/resilient.rs:918` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `.` |

**`call-forwarding`** (2)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:1338` | `background( &self, added: Vec<serde_json::Value>, removed: Vec<serde_json::Value>, ) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1083` | `background( &self, added: Vec<serde_json::Value>, removed: Vec<serde_json::Value>, ) -> ProviderFuture<'_, ()>` | `.` |

### `open_file_normal` — declared at `crates/verter_type_runtime/src/traits.rs:497`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:497` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:949` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1090` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:678` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:535` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4201` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:477` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`call-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:277` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:345` | `tent: String, access: Option<FileAccess>, lane: PriorityLane, update: bool, ) -> Result<(), TypeProviderError>` | `FileAccess::Open => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:656` | `pub async fn open_dts_normal( &self, dts_path: &str, dts_content: &str, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:152` | `elf, path: &str, content: &str, lane: ProviderLane, verb: ProviderFileVerb, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/resilient.rs:888` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Normal => provider.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:953` | `fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1096` | `fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:478` | `fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

### `load_file_normal` — declared at `crates/verter_type_runtime/src/traits.rs:501`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:501` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:959` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1101` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:688` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:544` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4205` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:481` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`call-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:286` | `async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError>` | `provider.` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:346` | `tent: String, access: Option<FileAccess>, lane: PriorityLane, update: bool, ) -> Result<(), TypeProviderError>` | `FileAccess::Loaded => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:645` | `pub async fn load_dts_normal( &self, dts_path: &str, dts_content: &str, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:149` | `elf, path: &str, content: &str, lane: ProviderLane, verb: ProviderFileVerb, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/resilient.rs:893` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Normal => provider.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:963` | `fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1107` | `fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:482` | `fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

### `update_file_normal` — declared at `crates/verter_type_runtime/src/traits.rs:505`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:505` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:969` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1112` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:698` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:553` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4209` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:485` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`call-production`** (4)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:336` | `tent: String, access: Option<FileAccess>, lane: PriorityLane, update: bool, ) -> Result<(), TypeProviderError>` | `(true, PriorityLane::Normal) => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:668` | `pub async fn sync_dts_normal( &self, dts_path: &str, dts_content: &str, ) -> Result<(), TypeProviderError>` | `.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:155` | `elf, path: &str, content: &str, lane: ProviderLane, verb: ProviderFileVerb, ) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_type_runtime/src/resilient.rs:898` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Normal => provider.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:973` | `fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1118` | `fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:486` | `fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

### `close_file_normal` — declared at `crates/verter_type_runtime/src/traits.rs:509`


**`trait-declaration`** (1)

| site | context | snippet |
|---|---|---|
| `crates/verter_type_runtime/src/traits.rs:509` | `pub trait TypeProvider: Send + Sync` | `fn` |

**`impl-production`** (6)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:979` | `impl TypeProvider for TsgoCompositeProvider` | `fn` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1123` | `impl TypeProvider for ProjectTsserverProvider` | `fn` |
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:708` | `impl TypeProvider for LazyManagedTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/resilient/forwarding.rs:562` | `ypeProvider for ResilientProvider<P, B> where P: TypeProvider + Send + Sync + 'static, B: ResilientBackend<P>,` | `fn` |
| `crates/verter_type_runtime/src/tsgo/ipc.rs:4213` | `impl TypeProvider for TsgoTypeProvider` | `fn` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:489` | `impl TypeProvider for TsgoOwnedProvider` | `fn` |

**`call-production`** (5)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/type_provider/lazy_managed.rs:367` | `async fn record_close( &self, path: String, lane: PriorityLane, ) -> Result<(), TypeProviderError>` | `PriorityLane::Normal => provider.` |
| `crates/verter_lsp/src/type_provider/project_sync.rs:673` | `pub async fn close_dts_normal(&self, dts_path: &str) -> Result<(), TypeProviderError>` | `self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:226` | `per) async fn close_tsx_in_lane( &self, tsx_path: &str, lane: ProviderLane, ) -> Result<(), TypeProviderError>` | `ProviderLane::Normal => self.provider.` |
| `crates/verter_lsp/src/type_provider/project_sync/virtual_types.rs:258` | `c fn close_virtual_verter_types( &self, tsx_path: &str, lane: ProviderLane, ) -> Result<(), TypeProviderError>` | `ProviderLane::Normal => self.provider.` |
| `crates/verter_type_runtime/src/resilient.rs:903` | `ard<P: TypeProvider>( provider: &P, mutation: &DesiredMutation, lane: Lane, ) -> Result<(), TypeProviderError>` | `Lane::Normal => provider.` |

**`call-forwarding`** (3)

| site | context | snippet |
|---|---|---|
| `crates/verter_lsp/src/tsgo/composite.rs:982` | `fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()>` | `self.managed.` |
| `crates/verter_lsp/src/tsserver/project_router.rs:1128` | `fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()>` | `.` |
| `crates/verter_type_runtime/src/tsgo/owned.rs:490` | `fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()>` | `self.lsp.` |

