# Participation presets, effective modes and capability ownership

Verter can stand down for one profile, document scope and capability without
withdrawing unrelated features or silently processing the withdrawn work.
Explicitly demanded workspace semantics remain available in workspace mode.
The current authority is the combination of global settings, static LSP
advertisement, provider availability and framework-shaped client branches.
The final and sole participation authority is COX0's per-profile,
per-capability participation service. Semantic producers retain their owners.

This decision describes the repository at `docs(arch): define atomic
contribution and candidate contracts (#879)`, 2026-10-13. That landing title
is descriptive context, never proof. Following the [docs-only rule](README.md),
this file supplies the contract, inventory and case table. It changes no
production route, dependency or test, adds no validator or CI check, and
deletes nothing. COX0 implements the masks, host observation, withdrawal,
cancellation and counter tests. UAP0 validates the ownership inventory under
`UAP0-AC-R1`.

## Imported boundaries and intent

Participation consumes the [catalog](catalog.md), [typed identities](identities.md),
[effective configuration](configuration.md), [demand plan](demand-activation.md)
and [atomic contributions](idx0d-contributions.md). It enters DEM0's `P4`
`CapabilityDemandPlan`, after selection and semantic claims. It does not select
a carrier, activate a framework, change project membership, resolve competing
framework claims, or change a parse/transform generation. Ambiguous, dormant,
unproven or withdrawn claims remain ineligible even under `full`.

Planning stages declare dependencies, not unconditional execution. Reading a
captured mask requests no producer facts. With no admitted demand, the host
does not run activation/projection/claim prerequisites merely to reach `P4`.
Standing down leaves existing activation and parse identities intact without
causing a cold open or edit to compute artifacts that no admitted demand needs.

DX0's `FeatureExposureContract`, `HostExecutionClass`, `ProductReceiptBasis`
and rails policy in `tests/product-experience/DX0/products/` remain binding.
Capability withdrawal is a participation decision, not removal of a shipped
feature from the catalog or relaxation of its `RequiredCurrent` obligation.
A browser host cannot turn a NativeOnly operation into Portable execution by
changing its mask. One selected TypeScript authority supplies strict projection
checks, typed hover and completion. Unavailable TypeScript stays unavailable;
native inspection never substitutes for it. An explicitly selected comparison
returns two separately labelled results with their own authorities and bases.

The logical fields below describe a contract, not a new wire schema or a
second activation/configuration store. PUB0 owns public result encoding;
VIM1 consumes the capability vocabulary in generated descriptor data.

## Presets and precedence

The public preset vocabulary is exactly `auto | disabled | workspace | full`.
It is separate from TypeScript provider selection (`auto`, `tsserver`, etc.).
COX0 evaluates UI aliases; this decision introduces none.

First resolve the preset for a profile and document scope through the existing
configuration authority's layer and scope precedence. Omitted participation
uses `auto`; invalid values are validation errors, not `auto`. Contradictory
unresolved scope declarations cannot be broken by arrival or registration order.
An explicit effective `full` wins over competing-tool observations and automatic
stand-down recommendations. It does not override a more specific effective
configuration value, unsupported catalog capability, unavailable provider,
absent client support or an explicitly disabled optional visual layer.

| Effective preset          | Effective mode per matching capability cell             | Admission and publication                                                                                                                          |
| ------------------------- | ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `disabled`                | `Disabled`                                              | No capability execution, interactive result, diagnostic, action or workspace contribution in this scope.                                           |
| `workspace`               | `WorkspaceOnly`                                         | Only explicitly demanded bounded workspace semantics; no interactive presentation, diagnostic publication or actions/edits.                        |
| `full`                    | `Full`                                                  | All requested, supported capabilities are eligible irrespective of competing ownership. Selection, demand, provider and client checks still apply. |
| `auto`, no proven overlap | `Full`                                                  | Ordinary demanded behavior. Merely installing another tool does not withdraw anything.                                                             |
| `auto`, proven overlap    | `WorkspaceOnly` for overlapping cells; `Full` elsewhere | Stand down interactively for the exact overlapping profile, selector and capability; keep separately demanded workspace facts eligible.            |

Effective modes are exactly `Disabled | WorkspaceOnly | Full`. They belong to
cells, not to a process-wide boolean. An `auto` profile may have Full hover and
WorkspaceOnly diagnostic publication simultaneously. A UI summary of that
profile cannot widen its cells. `Full` is permission to do demanded work, never
a command to precompute every capability.

`WorkspaceOnly` admits `IndexContribution` and explicit read-only workspace
queries with declared roots, facts and budgets. An explicitly requested
`BatchCheck` may return its typed check result to that caller; it cannot publish
editor diagnostics or turn findings into actions. Workspace symbol presentation
and workspace facts are distinct demands. `Interactive` and
`DiagnosticsPublication` are not admitted in this mode. Compile, inspection or
comparison requests retain their own operation descriptors and permissions;
being named `Explain` or using a custom method cannot bypass a WorkspaceOnly
cell's prohibition on interactive presentation or edits. Such independent
operations require their own admitted cells. A Disabled cell admits none.

An explicit CLI, browser or agent invocation has its own captured participation
scope. Disabling an editor scope cannot silently start those invocations, and
does not globally disable an independently requested operation in another scope.

## Abstract CapabilityOwnershipMask

The mask is immutable captured data, indexed by
`(profile-qualified CapabilityId, document selector scope)` within one host
participation scope. Profiles use catalog identities, including carrier or
project profiles where applicable; editor and browser product surfaces are not
framework identities. Region selectors retain their typed source/region
identity, so a conflict in a stylesheet region cannot withdraw template hover.

Each cell retains these independent facts:

| Field                 | Closed meaning                                                                                                                                                         |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Preset and mode       | The resolved preset and `Disabled`, `WorkspaceOnly` or `Full`.                                                                                                         |
| Interactive ownership | `Verter` or `OtherTool` with host-supplied, identity-free evidence; `full` selects Verter despite observed overlap. This selects participation, not a semantic engine. |
| Provider state        | `NotRequired`, `Available` or `ProviderUnavailable`; the producer declares the requirement.                                                                            |
| Client support        | `Supported` or `UnsupportedClientFeature` for this operation and transport. A transport without LSP uses its own declared operation support.                           |
| Optional presentation | `NotOptional`, `Enabled` or `DisabledOptionalVisualLayer`; disabling one presentation does not disable its semantic facts.                                             |
| Evidence basis        | Captured configuration value, normalized selector, catalog capability, client-support and provider-state identities, plus normalized ownership evidence.               |

These states are not synonyms. A withdrawn capability is not an empty semantic
answer; provider absence is not tool ownership; unsupported client support is
not disabled configuration. Return a typed non-execution reason through PUB0,
preserving the independent states for explanation. If several restrictions
apply, the primary reason order is `Disabled`, purpose forbidden by
`WorkspaceOnly`, unsupported capability, `UnsupportedClientFeature`,
`DisabledOptionalVisualLayer`, then `ProviderUnavailable`. No rejected demand
starts its producer to discover that it should have been rejected.

Hosts observe competing tools through generated descriptors. COXD1 validates
declarations against the catalog and this vocabulary; COXD2 may supply mask
inputs. A declaration identifies overlapping profiles, selectors and capability
cells. Installation, display names, file suffixes and discovery order alone
are not ownership proof. Unknown or malformed declarations establish no
overlap, and are reported as invalid evidence without broad stand-down.

Editor extension IDs and editor-specific discovery APIs stay in the host.
Before crossing into Rust, the host resolves selectors to typed scopes and
translates observations into validated ownership cells. Rust receives modes,
capability masks and normalized evidence, never extension IDs, extension-name
lists, editor detection logic or file-extension ownership heuristics. A
non-VS-Code host can declare exactly the same cells without simulating VS Code.

Canonical mask encoding orders cells by their catalog profile/capability and
typed selector identities. Duplicate identical cells coalesce; contradictory
cells are refused before publication. Equivalent host observations produce
the same semantic mask identity regardless of tool enumeration order. Its
identity includes mode, ownership and the three availability dimensions; raw
editor identity and observation timestamps do not enter it. Catalog support
and activation claims are still checked by their respective owners.

## Withdrawable capability inventory (COX0D-AC1)

The table inventories the current shipped boundary, not a claim that dynamic
withdrawal exists today. Each method listed in a grouped row is a separately
maskable cell. Semantic/provider, syntax, CSS and inspection facets of a
method retain separate catalog cells when their producers differ. COX0 owns
participation admission and replacement of the current gates in every row;
it does not replace the listed semantic producers.

Source abbreviations: **CAP** = `crates/verter_lsp/src/capabilities.rs`;
**NAV** = `crates/verter_lsp/src/server/nav_features.rs` and its navigation/
completion-resolve modules; **AUX** = `crates/verter_lsp/src/server/aux_features.rs`;
**CLIENT** = `packages/vue-vscode/src/extension.ts`; **CUSTOM** =
`crates/verter_lsp/src/main.rs` and `crates/verter_lsp/src/server/custom_methods/`.

| Capability cells / current surface                                                                                                                                | Current gate or branch                                                                                                                                                                  | Later participation owner; retained producer                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| Hover, including CSS and separately labelled inspection supplements                                                                                               | CAP static hover; NAV provider and native-hover options; CLIENT carrier/CSS middleware                                                                                                  | COX0; selected TypeScript provider, CSS and inspection producers                                                                         |
| Completion; completion-item resolve and auto-import edits                                                                                                         | CAP static completion with provider-dependent resolve flag; NAV repaired provider context; CLIENT carrier/CSS merge                                                                     | COX0; provider, syntax/CSS producers and owning edit operations; resolve cannot survive withdrawal of the item basis                     |
| Definition, type definition, references                                                                                                                           | CAP static registration; NAV provider-context and framework contract availability                                                                                                       | COX0; symbol/provider owners                                                                                                             |
| Prepare rename and rename                                                                                                                                         | CAP static prepare/rename; navigation module validates provider edits                                                                                                                   | COX0; symbol and edit owners; workspace mode returns no rename edit                                                                      |
| Document highlights and signature help                                                                                                                            | CAP static registration; AUX provider/syntax branches; CLIENT CSS highlights                                                                                                            | COX0; provider and syntax/CSS owners                                                                                                     |
| Document symbols, folding ranges, selection ranges                                                                                                                | CAP static registration; AUX document/analysis presence                                                                                                                                 | COX0; shallow syntax inventory and symbol owners                                                                                         |
| Linked editing and document links                                                                                                                                 | CAP static registration and no link resolve; AUX carrier/block handlers                                                                                                                 | COX0; syntax and PM link owners                                                                                                          |
| Call hierarchy prepare, incoming calls, outgoing calls                                                                                                            | CAP static hierarchy registration; AUX hierarchy/provider routes                                                                                                                        | COX0; symbol/call owners                                                                                                                 |
| Workspace symbol presentation                                                                                                                                     | CAP static workspace symbols; AUX workspace query                                                                                                                                       | COX0; bounded IDX0 discovery and owning symbol validation                                                                                |
| TypeScript diagnostic publication                                                                                                                                 | Push-only diagnostics; selected provider snapshots and sync/publication pipeline                                                                                                        | COX0; selected TypeScript authority and diagnostic mapper                                                                                |
| Compiler/projection-limit, component-usage and optional lint diagnostic publication                                                                               | `compute_verter_diagnostics_for_with_views`; lint options and eager component-meta read; no per-capability ownership mask                                                               | COX0; compiler, component and lint producers; no duplicate inspection-as-TypeScript answer                                               |
| Code actions: quick fixes, organize imports, refactors/extract; custom document-drop edits                                                                        | CAP static action kinds; AUX provider/native actions; CUSTOM document-drop handler                                                                                                      | COX0; action and edit owners; every kind separately withdrawable                                                                         |
| Document formatting; on-type tag closing                                                                                                                          | CAP static format/on-type registration; AUX carrier-kind and formatter branches                                                                                                         | COX0; formatter and syntax owners; each has its own cell                                                                                 |
| Document colors and color presentations                                                                                                                           | CAP color registration; AUX CSS route; CLIENT carrier/CSS service                                                                                                                       | COX0; CSS producer, including client-side work                                                                                           |
| Semantic tokens full                                                                                                                                              | CAP static legend/full registration; AUX provider merge and `decorations_must_wait`                                                                                                     | COX0; token mapping and provider/syntax owners                                                                                           |
| Inlay hints                                                                                                                                                       | CAP static registration; init-only `inlayHints.enabled`, AUX provider/native lanes                                                                                                      | COX0; provider and inspection producers                                                                                                  |
| Code lenses                                                                                                                                                       | CAP static registration without resolve; AUX document/analysis availability                                                                                                             | COX0; code-lens producer                                                                                                                 |
| API-call, binding-color and prop-constness decorations                                                                                                            | `VueApiDecorationProvider`, `BindingColorDecorationProvider`, `PropConstnessDecorationProvider`; individual `verter.decorations.*` booleans, including the API-call Vue language branch | COX0; individual visual producers; disabling a visual withdraws its presentation-only demand                                             |
| Analysis and binding-type inspection; document structure                                                                                                          | CUSTOM methods; `analysis.enabled`/`lint.enabled` semantic-enrichment gate and CLIENT Analysis UI context                                                                               | COX0; labelled native inspection and shallow structure producers                                                                         |
| Component metadata, selective surface and type-handle expansion                                                                                                   | CUSTOM component-meta methods with source/snapshot validation                                                                                                                           | COX0; TIF1D view/TIF1 facet owners, never a local metadata copy                                                                          |
| Project overview, component parents and route tree                                                                                                                | CUSTOM workspace methods; existing workspace scans and framework-shaped routes                                                                                                          | COX0; bounded IDX0 candidates, PM and owning vertical validation                                                                         |
| Compiled-code, virtual-file and source-map inspection                                                                                                             | CUSTOM compiled/virtual-file methods and CLIENT source-map commands                                                                                                                     | COX0; compiler artifact and coordinate owners; demand remains explicit                                                                   |
| Workspace semantic contribution and bounded read/query facets                                                                                                     | Existing component/index scans and document-triggered enrichment; no WorkspaceOnly mask                                                                                                 | COX0; IDX0 atomic contributions and semantic producers under `IndexContribution` demand                                                  |
| Browser hover, completion, diagnostics, definition, references, highlights, rename, code actions, symbols, lenses, hints and optional analysis/inspection display | `packages/playground/src/editor/lspProviders.ts`, browser TS worker and per-view registration, including Vue-language provider branches                                                 | COX0 vocabulary; Portable browser producers, distinct from NativeOnly LSP producers                                                      |
| Explicit project check, compile, inspection and MCP query facets                                                                                                  | `verter_tsc`, compiler APIs, `verter_mcp` tools (`is_vue()` branches) and `verter.mcp.enabled` transport switch                                                                         | COX0 demand masks in the invocation's own scope; existing check/compiler/inspection producers; transport enablement grants no capability |

Text synchronization, workspace-folder/file notifications and watchers are
input/control channels, not automatically admitted semantic capabilities.
They may retain current bytes and fences needed by another admitted demand;
they may not schedule withdrawn projection, enrichment or provider work.
Initialize/shutdown, participation changes, statistics and audit-record reads
remain control-plane operations. Reading an existing record does not create
semantic work, and observability capture off does not change admission.
TextMate grammar display is host-owned static presentation, not kernel
semantic-token execution. No kernel ownership decision needs editor identity.

### Current limitations and deletion assignments

| Typed exception / displaced route | Observed limitation                                                                                                                          | COX0 obligation                                                                                                                                                                                                 |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `DeclaredGateWithoutConsumer`     | `verter.enable` is contributed in the client manifest, but client startup does not consume it as a runtime participation gate.               | Replace the advertised global switch with truthful preset configuration and migrate its public setting; never use it as evidence that disabled is already zero-work.                                            |
| `StaticRegistration`              | CAP advertises capabilities at initialize; current dynamic registration in lifecycle is for watchers, not a per-cell withdrawal mechanism.   | Replace static blanket ownership with mask-derived registration where supported, and server admission checks everywhere.                                                                                        |
| `RestartOnlySetting`              | `languageServerConfig.ts` restarts for analysis, inlay, provider and other initialization settings.                                          | Move participation/visual changes to captured demand-epoch updates; retain provider lifecycle ownership for actual engine changes.                                                                              |
| `ClientLocalProducer`             | CSS middleware and decoration providers can perform work before or independently of a Rust request.                                          | Gate client-local producer demand with the same cells; clearing Rust results alone is insufficient.                                                                                                             |
| `UnsupportedDynamicRegistration`  | A client may have only static registration or no refresh protocol.                                                                           | Enforce masks server-side, clear/retire owned display state through supported channels, and report transport limits truthfully. A statically advertised method is never permission to execute a withdrawn cell. |
| `EagerDocumentWork`               | Open/change hooks can schedule enrichment, imported-carrier prewarming and diagnostic producers outside request-level masks.                 | Replace those admission paths with declared demand, including DEM0's `DEM-D01` and `DEM-D06`.                                                                                                                   |
| `FrameworkClientBranch`           | CLIENT carrier CSS routes, API-call Vue-only decoration, AUX carrier mapping and MCP `is_vue()` gates have no universal participation cells. | Replace participation decisions with catalog descriptor/mask consumption, including UAK0 `D10`/`D11`. Retain genuinely profile-specific semantic implementations with their existing owners.                    |

These are typed implementation obligations, not waivers or claims of a
completed repair. COX0 reconciles this source inventory on its implementation
candidate. Its production cutover removes the old parallel participation
decisions, global on/off authority and framework client ownership branches in
the same change. Provider selection, optional visual preferences, transport
launch settings and semantic implementations remain their own facts; they
feed the single service instead of becoming alternative participation owners.
The case table below tests client limits without treating static advertisement
as proof of dynamic support. UAP0 reconciles truthfulness and validates coverage.

## Demand epochs, withdrawal and equivalence (COX0D-AC3)

DEM0 `DR21` owns epoch composition: the activation epoch plus participation
mode and mask identity forms the demand epoch. A participation, ownership,
client-support, optional-visual or provider-availability change creates a new
demand epoch when its effective mask changes. It leaves the activation epoch
and source parse/transform generation unchanged. No-op equivalent observations
mint neither epoch. Actual source/configuration/activation changes still follow
their owning rules.

Content-equivalent masks may share a semantic identity across hosts or after a
toggle, but an observed transition also advances the host's retirement fence.
Returning to the original mask never makes an earlier request current again.
The fence is a lifecycle generation, not an editor ID or a new semantic cache
identity; reusable facts still require current admission and source bases.

COX0 publishes a new mask and demand epoch atomically before admitting new
work. It fences results at admission, response return, warm-cache publication,
diagnostic/action publication and client-local presentation. Pending old work
is never admitted. Running old work is cancelled or completes internally as
`ReturnOnly`; it cannot be served after the newer epoch is observed, even if
the final preset equals the original preset. Old completion resolves, action
payloads and type handles must revalidate their basis, never resurrect an old
result because the cell was re-enabled.

Withdraw diagnostic ownership by clearing only Verter's affected publications;
withdraw tokens, lenses, hints and decorations by retiring affected owned state.
No clear removes another tool's results or an unaffected profile/cell. In-flight
old publications cannot refill a cleared display. Unsupported transport refresh
is explicit and still cannot authorize stale answers. COX0 implements the
registration/refresh mechanics rather than this document prescribing a second
client state owner.

IDX0 owns fenced contribution replacement/removal and snapshot-qualified reads.
A transition to WorkspaceOnly may preserve still-valid admitted workspace facts;
it cannot add unsolicited contributions. Disabled/superseded eligibility retires
the affected profile's current contribution authority through IDX0's delta and
membership contract. Immutable old snapshots may retain data, but no current
query can serve that old eligibility. Shared carrier facts needed by other
admitted cells remain usable with their proper bases.

Given the same current source, PM/catalog/configuration snapshots, selected
provider, client facts and final mask, an incremental transition must yield the
same admitted cells, answer classes, labelled provenance and current
contribution membership as a fresh host. Cache reuse may reduce work, never
change eligibility or answers. Repeated disable/re-enable and out-of-order
provider completion are required discriminators, not just a single toggle.

## Bounded work counters (COX0D-AC4)

Measure deltas per profile, selector, capability, purpose and demand epoch.
Count producer admission and execution, framework projection, script-fact
capture, TypeInfo execution, provider dispatch, index production/commit,
diagnostic publication, action/edit production, client-local computation,
presentation publication and stale-result serving separately. Shared dependency
work is attributed to its admitted demand, not to a withdrawn cell. Existing
work may drain after a fence; report that drain separately from new-epoch work.

| Condition                                                                 | Required work delta                                                                                                                                                                   |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Disabled scope, no independent admitted demand                            | All semantic/capability counters above are zero, including parse/compile, enrichment, filesystem discovery, provider startup/sync and background timers/tasks created for that scope. |
| WorkspaceOnly, no explicit workspace demand                               | Zero producer admission/execution, projection, script-fact, TypeInfo, provider dispatch and index production/commit. No hidden discovery or enrichment.                               |
| WorkspaceOnly, explicit bounded demand                                    | Only declared dependency work; interactive presentation, diagnostic publication and action/edit production are zero.                                                                  |
| Auto, overlapping interactive cell                                        | Zero producer/client-local work and result publication for that cell; explicit separately admitted workspace demand may compute its declared facts.                                   |
| Full or Auto, selected but undemanded cell                                | Zero capability work. Shared parse occurs only for a different admitted demand.                                                                                                       |
| ProviderUnavailable, UnsupportedClientFeature or disabled optional visual | Zero rejected producer work. A disabled visual's presentation-only computation is zero; unrelated admitted semantic work may continue.                                                |
| Superseded demand epoch                                                   | Zero new admission and zero response/stale serving, warm publication, contribution commit or presentation publication from superseded work.                                           |

One mask/configuration update and necessary cancellation, state clearing and
contribution retirement are control-plane work, accounted separately from
semantic counters. They do not license repeated background processing. Required
plan state survives audit capture off; capture on follows DEM0's one record per
plan, rather than one per cell or transition. Limits come from the operation's
declared roots/fact closure and budget, not elapsed-time guesses. Same-worker
work counts and answer equivalence are acceptance evidence; timing/allocation/
memory samples are advisory under the standing measurement rule.

## Executable case obligations (COX0D-AC2 through AC4)

COX0 implements each case through real request admission/publication and
client-local producer paths. UAP0 validates the inventory and case ownership.
These are specified tests, not tests executed by this docs-only change.

| Case   | Inputs / schedule                                                                                                              | Required and forbidden result                                                                                                                                                                                                |
| ------ | ------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PC01` | Auto; test tool proves overlap only for one profile's diagnostic publication on one selector                                   | Only that cell becomes WorkspaceOnly. Hover, completion, navigation and formatting remain Full and execute when requested. Other profiles/selectors retain diagnostics. Withdrawn-cell interactive work/publication is zero. |
| `PC02` | Same evidence, explicit effective full                                                                                         | Requested supported diagnostic cell stays Full. Tool order does not alter the mask. Full performs no undemanded work.                                                                                                        |
| `PC03` | Workspace; no demand, then one bounded IndexContribution/read query                                                            | Idle work is zero; the query contributes/reads only its declared facts. No hover/diagnostic/action publication or edit. An explicit BatchCheck result is caller-only.                                                        |
| `PC04` | Disabled; cold open, edits, watcher events and attempted feature/custom requests                                               | No hidden parse/projection/enrichment, dependency crawl, provider startup/dispatch/sync, index or client-local producer work. Requests explain Disabled; only required control state changes.                                |
| `PC05` | Full, admitted provider-dependent request; provider unavailable                                                                | ProviderUnavailable remains distinct from withdrawal. Native syntax/CSS capabilities still work if demanded; native inspection never supplies the missing strict TypeScript answer.                                          |
| `PC06` | Full; client lacks inlay support, or disables one optional visual layer                                                        | UnsupportedClientFeature and DisabledOptionalVisualLayer remain distinct; rejected-cell work is zero. Hover and other supported semantic work remain available.                                                              |
| `PC07` | Hold old diagnostics/completion/resolve and client-local visual work; full → workspace → disabled → full; release out of order | Old work is cancelled/ReturnOnly, never served/warmed/published. Cleared displays stay clear until a current admitted result. Resolves/handles cannot reuse old epochs.                                                      |
| `PC08` | Compare PC07's final state to a fresh host with identical current inputs                                                       | Same answer classes, admitted cells and current contribution membership. Old immutable snapshots grant no current eligibility.                                                                                               |
| `PC09` | Overlap one region/profile; simultaneous unrelated request and contribution                                                    | Only overlapping cells stand down; unaffected controls keep answers and own publications. No global diagnostic clear or parse-generation change.                                                                             |
| `PC10` | Equivalent competing declarations in reversed order, then identical re-observation                                             | Same mask identity and answer order; no-op observation mints no new epoch and schedules no semantic work. Contradictory/invalid declarations cannot create broad withdrawal.                                                 |
| `PC11` | Desktop, browser and non-VS-Code hosts supply equivalent normalized cells                                                      | Same participation decisions; no editor IDs reach Rust. Browser NativeOnly requests stay unavailable; local-native and Portable receipts remain distinct.                                                                    |
| `PC12` | Static-only client receives a withdrawn request; late old push/display update                                                  | Server rejects execution despite static advertisement, reports refresh limits and prevents stale publication. No fictitious successful dynamic unregister.                                                                   |
| `PC13` | Toggle only optional visual/client-support facts under a stable source and activation                                          | New demand epoch only; no source activation, parser generation or unrelated recomputation. Presentation-only producer counters become zero.                                                                                  |
| `PC14` | Ambiguous/unproven/dormant profile, full and explicit request                                                                  | Participation does not activate or repair a claim; zero profile projection/TypeInfo/index work under DEM0's rules.                                                                                                           |
| `PC15` | Audit on/off; same idle and bounded demanded scenarios                                                                         | Same answers, eligibility and required counters/plan state. Capture off performs no observation-only work. Capture on adds only declared audit records.                                                                      |

## Evidence selection and receiving obligations

For AC1, bounded source inspection of CAP, NAV, AUX, CUSTOM, CLIENT, the visual
providers and provider/diagnostic synchronization establishes the current gate
inventory. DEM0 assigns planning and epoch semantics to COX0; IDX0D assigns
contribution storage and retirement to IDX0; DX0 fixes exposure, execution
classes and answer rails. The current static registration and restart settings
are evidence of limitations, not acceptance evidence for future withdrawal.

AC2's executable discriminators, AC3's concurrent transition equivalence and
AC4's zero-work measurements are not applicable to a prose-only mutation that
changes no admission or producer. COX0 owns their implementation and execution
under `COX0-AC2`, `COX0-AC3` and `COX0-AC4`; its gate must exercise affected and
unaffected controls in the case table. UAP0 owns inventory validation under
`UAP0-AC-R1`. This decision adds no fake guards or prose-string tests. PUB0 and
VIM1 consume the vocabulary without claiming that it is already implemented.
