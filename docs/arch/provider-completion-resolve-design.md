# Provider-neutral completion-resolve / auto-import

Auto-import-on-completion-accept is a provider-NEUTRAL capability. Every type
provider — TSGO, tsserver, and the in-process extension provider — is a
first-class producer of `completionItem/resolve` auto-import edits. This document
is the as-implemented record of that contract; it mirrors the codex-locked
binding design for Issue #1.

## Problem (root cause)

Auto-import-on-accept worked only under the `tsgo` provider. The chain of three
independent defects:

1. **tsserver discarded the resolve handle.** `parse_tsserver_completion`
   hard-coded `data: None`, throwing away the entry's `source`/`data` — the exact
   fields a `completionEntryDetails` request keys on. TSGO's
   `parse_completion_item` preserved its `data`.
2. **No tsserver/extension resolve.** Neither tsserver nor the extension provider
   implemented `resolve_completion`; both inherited the trait default `Ok(None)`,
   so neither ever returned `additionalTextEdits`.
3. **The LSP gated dispatch on a provider-baked marker.** `merge_completions`
   tagged items with `{ "tsgo": true, "original_data", "tsx_path" }` and
   `handle_completion_resolve` only fired the auto-import branch inside
   `if data.get("tsgo") == Some(true)` — so a tsserver completion could never
   reach resolve.

The carrier re-anchor machinery (`auto_import.rs`,
`ProviderPositionMapper::helper_preamble_end`) was already provider-neutral but
unreachable for non-tsgo providers.

## Contract (`verter_type_runtime`)

### `Completion.data`: a typed, provider-pure resolve key

`Completion.data` is `Option<CompletionResolveData>` — the provider's OWN lazy
resolve handle, NOT an LSP routing payload. It carries no carrier path and no
provider id (those live in the LSP envelope).

```rust
pub enum CompletionResolveData {
    /// Upstream-LSP handle (TSGO): the item's label + opaque `data`, replayed
    /// verbatim into `completionItem/resolve`.
    Lsp { label: String, data: serde_json::Value },
    /// tsserver-family handle: the entry name + optional source/data the
    /// `completionEntryDetails` request keys on, plus the completion-site
    /// `offset` (a position in the provider's OWN generated file — provider
    /// domain, not LSP routing) so resolve re-issues at the right position.
    TsserverEntry {
        name: String,
        source: Option<String>,
        data: Option<serde_json::Value>,
        offset: u32,
    },
}
```

`CompletionResolveResult` additionally carries optional `detail` / `documentation`
so a lazy resolve can enrich the item's hover text as well as its edits.

### Trait

`TypeProvider` gains two methods and changes one:

- `fn provider_id(&self) -> &'static str` (REQUIRED) — `"tsgo" | "tsserver" |
  "extension"`. Stamped on the resolve envelope and validated on the way back.
- `fn supports_completion_resolve(&self) -> bool` (default `false`) — drives the
  HONEST `resolve_provider` server capability; overridden to `true` by providers
  that implement resolve.
- `fn resolve_completion(&self, path, data: CompletionResolveData)` — now takes
  the typed key, never arbitrary JSON.

### Shared tsserver-family mapping (lowest reusable owner)

`verter_type_runtime::tsserver::ipc` owns the `completionEntryDetails → byte
edit` mapping used by BOTH the out-of-process tsserver provider and the
in-process extension provider:

- `completion_entry_details_to_resolve_result(detail, target_file, cache)` folds
  a detail's auto-import `codeActions[].changes[].textChanges` that target the
  generated file into `ResolvedTextEdit`s (reusing `parse_tsserver_code_action`),
  plus `displayParts`→detail and combined docs. Cross-file edits are dropped —
  the LSP carrier re-anchor owns the generated-TSX → `.vue` mapping.
- `stamp_tsserver_completion_offset(item, request_offset)` stamps the
  completion-site offset onto a freshly-parsed entry's handle (the offset is
  identical for every entry in one `completionInfo` request, so `get_completions`
  applies it).
- `enrich_completion_with_entry_details(item, detail)` — shared
  `get_completion_details` enrichment that preserves the resolve handle.

The LSP owns ONLY envelope dispatch + the carrier re-anchor.

## Provider implementations

- **TSGO** (`tsgo/ipc.rs`): `parse_completion_item` wraps the upstream `data` as
  `CompletionResolveData::Lsp { label, data }`; `resolve_completion` accepts the
  `Lsp` variant and replays `completionItem/resolve`. Behavior-preserving.
- **tsserver** (`tsserver/ipc.rs`): `parse_tsserver_completion` preserves the
  entry's `name`/`source`/`data` as `TsserverEntry` (the `data: None` discard is
  gone); `get_completions` stamps the request offset; `resolve_completion`
  re-issues `completionEntryDetails` at the stored offset and maps via the shared
  helper.
- **extension** (`extension_provider.rs`): `resolve_completion` +
  `get_completion_details` via the SAME shared helper, transported over
  `$/verter/tsQuery` (`query()`), not the tsserver child transport. The
  `extensionTsService.ts` host answers a `completionEntryDetails` command and
  enriches `completionInfo` entries with the `source`/`data` resolve key.

### Actionability contract: `source`/`data` only (not `hasAction`)

The auto-import resolve handle is actionable iff it carries `source` and/or
`data` — `hasAction` is deliberately NOT part of the contract. An auto-import
(module-export) entry ALWAYS carries `source` (the module specifier), and
tsserver's `getCompletionEntryDetails` keys the auto-import `codeActions`
lookup on `(name, source, data)` — `hasAction` is purely an output hint, never
an input to that lookup. The remaining tsserver entries that set
`hasAction: true` with NO `source`/`data` — class-member snippet completions,
object-literal missing-comma insertion, and type-only-alias wrappers — are a
DIFFERENT code-action class. This block resolves auto-imports only; routing a
bare-`hasAction` entry through the auto-import envelope would mis-key its
resolve (no `source` to look up) and yield no edit, so those entries correctly
earn no envelope. The extension shim therefore does NOT forward `hasAction`
(it was dead wire data — no consumer), and `CompletionResolveData::is_actionable`
checks `source`/`data` only. A future block that supports the non-import action
class adds its own handle variant rather than overloading the auto-import rail.

## LSP dispatch (`verter_lsp`)

### Envelope (provider-neutral)

`merge_completions(…, provider_id, …)` stamps resolve-bearing items with:

```json
{
  "verter_resolve": {
    "kind": "type_provider",
    "provider_id": "<active provider id>",
    "provider_path": "<generated-TSX path>",
    "provider_data": <serialized CompletionResolveData>
  }
}
```

The old `{ "tsgo": true, "original_data", "tsx_path" }` keys are DELETED. The
`verter_resolve` envelope is namespaced SEPARATELY from the top-level
workspace-component `auto_import` data shape (`features/completion.rs`); the two
never overload one key.

### Dispatch + fail-closed validation

`handle_completion_resolve` (`nav_features.rs`):

1. reads `data.verter_resolve` and requires `kind == "type_provider"`;
2. validates `provider_id == tp.provider_id()` — a mismatch (mid-session provider
   swap) FAILS CLOSED: the item is returned unchanged, `resolve_completion` is
   never called against a foreign provider's item;
3. deserializes `provider_data` back into `CompletionResolveData` (a
   malformed/foreign key fails closed);
4. calls `tp.resolve_completion(provider_path, data)` and routes any non-empty
   edits through `resolve_provider_auto_import_edits` (renamed from
   `resolve_tsgo_auto_import_edits`) — the provider-neutral carrier re-anchor.

### Honest capability

`server_capabilities(encoding, resolve_provider)` ties the advertised
`completion_provider.resolve_provider` to whether the ACTIVE provider implements
resolve (`tp.supports_completion_resolve()`), computed in the initialize handler.
A session with no provider, or a provider without resolve support, advertises
`resolve_provider: false` rather than a dishonest `true`.

## Tests

The coverage is layered. Pure unit / mock tests run hermetically on every gate;
the REAL provider-parity proof runs a live tsgo AND tsserver. This section is
accurate to what actually lands — it deliberately does NOT claim a synthetic test
proves a real provider behavior.

**Pure unit (hermetic, always run):**

- **`verter_type_runtime`** (`protocol.rs`, `tsserver/completion_resolve_tests.rs`):
  `CompletionResolveData::is_actionable` (auto-import handle vs local); the
  `CompletionResolveData` wire shape is pinned; `parse_tsserver_completion`
  preserves the external-module resolve handle; the offset is stamped;
  `completion_entry_details_to_resolve_result` maps same-file auto-import code
  actions to edits and drops cross-file edits; **the stale-offset fragility is
  characterized** (`stamped_offset_drifts_when_buffer_changes_before_resolve`).
- **`verter_lsp`** (`server_tests.rs`, `tsgo/merge.rs` tests, mock provider):
  dispatch reaches resolve for the neutral envelope; a tsgo-kind (`Lsp` key) AND a
  tsserver-kind (`TsserverEntry` key) mock both resolve through the same envelope
  with their REAL per-provider key shapes; a provider-id mismatch fails closed;
  the envelope is minted ONLY for an actionable handle (a local item carries
  none); a label-dedupe collision adopts the import-capable handle onto the
  retained item; the resolved `detail`/`documentation` are applied onto the item;
  `merge_completions` emits the neutral envelope and deletes the old keys; the
  `resolve_provider` capability is honest.
- **dx-harness verifier-unit** (`differentialAutoImport.test.ts`,
  `collectorAutoImport.test.ts`): exercise the `verifyAutoImport` /
  `findCompletionItem` VERIFIER over hand-built items (and over the exact resolved
  edit text the real providers emit), classifying a correct import as `applied`
  and a no-edit one as a divergence. These pin the verifier; they do NOT spawn a
  provider.

**Real-provider parity (live spawn — the headline guarantee):**

- **`packages/dx-harness/test/providerResolveParity.integration.test.ts`** spawns
  a REAL tsgo AND a REAL tsserver through the `verter-dx-baseline` bridge's
  `resolveCompletion` route: it runs a `completion` query for an unimported symbol,
  picks the item carrying the actionable `resolveData` handle, re-issues resolve,
  and asserts BOTH providers return the SAME auto-import `additionalTextEdits`
  (`import { myHelper } from "./helper";`). Gated on `DX_BASELINE_BIN`; **require
  mode `DX_REQUIRE_PROVIDERS=1`** makes a provider skip (binary absent / spawn
  failed) a HARD FAILURE so it can never vacuously pass. Discriminating: reverting
  `parse_tsserver_completion` to `data: None` drops tsserver's `resolveData` and
  the test goes RED while tsgo stays green.
- **VS Code E2E** (`packages/vue-vscode/e2e/suite/completion.test.ts`, the
  `auto-import: accepting an unimported symbol resolves an import edit` test over
  the `single-project` fixture `src/AutoImportCase.vue`): drives the real
  `verter-lsp` over a real `.vue` SFC — a `<script setup>` that references an
  UNIMPORTED `computed` from `vue` — forces VS Code to resolve the `computed`
  completion (`itemResolveCount`), and asserts the resolve produces
  `additionalTextEdits` that import `computed` and land in the `<script setup>`
  region (not the template). This is the end-to-end proof of the **CARRIER**
  completion path (`merge_completions` — a real `.vue` URI maps the provider's
  generated-TSX edits back to `<script setup>` source), through the shipped
  server, across whichever provider the suite runs (`TYPE_PROVIDER`). It
  self-skips when no type provider is configured. NOTE: opening a real `.vue`
  URI routes through the carrier path, NOT the `verter-virtual://` branch — so
  this E2E test does NOT exercise the F1 virtual-file routing (see the next
  item for the F1 discriminator).

**F1 virtual-file routing (Rust server-level discriminator):**

- **`virtual_file_completion_routes_actionable_handle_through_envelope`**
  (`crates/verter_lsp/src/server_tests.rs`) drives `handle_completion` over a
  `verter-virtual://...?sourceUri=<vue-uri>` URI with a tsserver-kind mock
  provider returning an ACTIONABLE `TsserverEntry` (a `source`-bearing
  auto-import handle), and asserts the emitted LSP item carries the
  provider-neutral `verter_resolve` envelope routed back to the carrier TSX
  path — and that a non-actionable local handle does NOT. This is the
  discriminating proof of the F1 fix on the virtual-file branch
  (`nav_features.rs` → `merge::provider_completion_to_lsp_item`): reverting that
  branch to the pre-fix `data`-stripping form makes this test RED (envelope
  absent) while the rest of the corpus stays green. The VS Code E2E test above
  does NOT discriminate F1 because it exercises the carrier path.

**Extension provider auto-import (how it is regression-guarded):**

The in-process **extension** provider (`provider_id = "extension"`) is a third
provider mode whose auto-import-on-accept is guarded in TWO layers, because it
cannot be driven through the DX real-spawn parity bridge (that bridge spawns
child-process providers; the extension provider answers `$/verter/tsQuery` over
a live extension-host LSP `Client`) and the VS Code E2E job is deliberately
disabled (flaky):

1. **Shared Rust resolve path** — the extension provider's `get_completions` /
   `get_completion_details` / `resolve_completion` reuse the EXACT same
   tsserver-family helpers as the tsserver provider (`parse_tsserver_completion`,
   `stamp_tsserver_completion_offset`, `build_completion_entry_details_request`,
   `build_entry_names_entry`, `completion_entry_details_to_resolve_result`). The
   real-tsserver parity gate therefore already proves the extension provider's
   Rust-side parse / offset-stamp / envelope-mint / resolve-mapping mechanics.
2. **Extension-specific TS shaping** —
   `packages/vue-vscode/src/extensionTsService.autoimport.spec.ts` drives
   `ExtensionTsService.handleQuery` headlessly (no VS Code, no LSP) over the same
   two-file unimported-symbol workspace the parity gate uses, asserting
   `completionInfo` surfaces the export with a `source` resolve key and
   `completionEntryDetails` returns the auto-import `codeActions` that insert the
   import. This runs in the main-CI `js` job by explicit path. It is
   discriminating: it caught (and the fix here closed) a real defect where the
   shim passed `undefined` format options to `getCompletionEntryDetails`,
   crashing the import code-action builder in TypeScript 6.x — the extension
   provider could never have resolved an auto-import in production.

FOLLOW-UP (`TODO`): the Rust extension TRANSPORT seam — the `$/verter/tsQuery`
command names + arg-envelope shapes emitted by `ExtensionTypeProvider` (the
static `completionInfo`/`completionEntryDetails` arg keys, beyond the shared
`entryNames` builder) — is not yet covered by a Rust-side test, because
`ExtensionTypeProvider::query` is bound to a concrete `tower_lsp_server::Client`
and a mock would require introducing a transport-trait seam. That refactor is a
separate scoped change; the shaping it would assert is simple non-branching code
and the shared `entryNames` builder it calls IS unit-tested.

The CI gate runs the Rust unit layer (including the F1 virtual-file
discriminator above) on every push. The real-provider parity integration test
runs in require-mode (`DX_REQUIRE_PROVIDERS=1`) in the `dx-harness-hermetic` CI
job once the baseline binary is built (it spawns the vendored tsgo + the pinned
tsserver). The extension-provider auto-import guard runs headlessly in the `js`
job. The VS Code E2E auto-import test runs in the extension E2E job (when
re-enabled) under whichever provider that job pins. See `.github/workflows`.

## Excluded (other blocks)

- Moving the `verter_lsp/src/tsgo/` neutral module tree to `type_provider/` (the
  `ipc` + `resilient` modules stay TSGO-specific) — a separate topology block
  (Block 2).
- P1/P2 polish: TSGO `get_completion_details`; tsserver/extension syntactic +
  suggestion diagnostics; enriching the `codeActions` field on
  `get_completion_details` (Block 3).

## Known limitation: lazy-resolve stale offset

The `TsserverEntry.offset` is captured at completion-LIST time and re-converted to
a tsserver `(line, offset)` against the buffer the provider holds at RESOLVE time.
If the open buffer changed between the list request and the accept (text inserted
before the offset), the stored byte offset re-converts to a different position and
tsserver may resolve no edits. This is fail-closed (a drifted offset yields NO
auto-import, never a WRONG import) and acceptable because resolve fires immediately
on accept; the version-anchored fix is deferred to Block 3. Characterized by
`stamped_offset_drifts_when_buffer_changes_before_resolve`.

## Deferred test ledger (`#[ignore]` tracking)

These tests are `#[ignore]`d for an environmental reason unrelated to the
completion-resolve feature, and are tracked here so the ignore is durable, not
silent. Each `#[ignore]` reason in
`crates/verter_lsp/tests/tsserver_e2e_generated_outputs.rs` points back to this
ledger row.

| Test | Characterizes | Blocked on | Un-ignore when |
| ---- | ------------- | ---------- | -------------- |
| `test_e2e_tsserver_scoped_slot_types_with_in_memory_child_api` | Scoped-slot member completion on a child `.vue.ts` public-API file opened in-memory, queried from the PARENT IDE that imports it (cross-file scoped-slot type flow). | A cold inferred-project tsserver returns `"No content available."` for the in-memory child-API file even after reopen+retry — a pre-existing tsserver multi-file-sync fragility, NOT a completion-resolve defect. (Dead `__lsp_tests` code that never ran.) | The in-memory child-API project setup is made deterministic (the child API file is reliably part of the tsserver project before the parent query). |
| `test_e2e_tsserver_scoped_slot_types_with_plugin_and_open_child_ide` | The same scoped-slot member-completion flow via the tsserver plugin + an explicitly OPEN child IDE document (rather than in-memory). | Same pre-existing tsserver multi-file-sync fragility (`"No content available."` against a cold inferred project). | Same condition: the child-IDE project setup is deterministic. |

Both are scoped-slot/multi-file-sync scenarios; neither exercises the
auto-import completion-resolve path this block owns. They remain `#[ignore]`d
(not deleted) because the scenarios are real and worth restoring once the
tsserver project-sync setup is deterministic.
