# Required amendments to existing Rev11 and expansion nodes

The three successor trains depend on contracts that already exist. Those contracts should be strengthened rather than copied into successor-private variants. The amendment below is normative intent for the canonical authority workflow; exact source atoms, line references, conflict-domain leases, and generated charter digests must be produced by `programctl`.

## B2 — Framework parsing, recovery diagnostics, and stable identities

Add the following obligations:

1. A recoverable parser diagnostic and an unusable IDE/semantic surface are separate outcomes.
2. Recoverable syntax errors must be stored as authored native diagnostics with stable parser/source/recovery identity.
3. Recovery preserves authored token identity and mapping wherever possible; synthetic repairs are minimal, unmapped, and capability-tagged.
4. `has_errors` or an equivalent aggregate flag may not automatically clear all semantic diagnostics or block all region processing.
5. Recovery incompleteness must fail open for usage/liveness and fail closed for cache admission.
6. Vue and Svelte broken-carrier behavior must be characterized through the same shared outcome vocabulary.

Acceptance additions:

- `b2_recoverable_error_does_not_clear_stable_semantics`
- `b2_native_syntax_diagnostics_survive_provider_absence`
- `b2_recovery_preserves_authored_identifier_occurrences`
- `b2_synthetic_repairs_are_unmapped_and_capability_tagged`

## PAR0 — Parser ownership, reuse, and lineage contract

Add:

- `RecoverySnapshot` and per-region `RecoveryParticipation` are parser/lowering products, not resolver state.
- executable-region discovery is performed during the one parse/shallow pass per content hash;
- no checker or language-service operation may reparse source to recover semantic facts;
- JSDoc is the only approved dedicated type-text parse path, scoped to its parser owner;
- parser errors carry exact extracted-region-to-authored-source lineage.

## TCM3 — TypeScript semantic capability closure

Add the single-spec certification model:

- external TypeScript is a pinned observation/oracle and residual runtime owner, not a native-query dependency;
- native semantic output has one correctness behavior;
- TypeScript bugs are represented by a review-gated correction overlay in test/certification data;
- no user-facing compatibility mode, query-key dimension, or alternate resolver branch exists;
- every provider observation names exact engine artifact, provider epoch, project/profile/source basis, mapper snapshot, and capability;
- normalized diagnostic/navigation/completion observations must preserve stable identity and provenance rather than only rendered text/ranges.

Acceptance additions:

- `tcm3_no_runtime_compatibility_mode_or_spec_key_dimension`
- `tcm3_oracle_and_correction_overlay_are_data_not_resolver_behavior`
- `tcm3_observation_identity_is_exact_across_provider_restart_and_mapping_change`

## TCM4 — Atomic activation and deletion

Add:

- generated/provider ranges are consumable only when their mapper snapshot exactly matches the provider observation snapshot;
- provider handles and resolve keys are epoch-scoped and fail closed after swap;
- activation completion means successfully applied healthy binding, not discovered path or spawned process;
- EPR4/EPR5 may replace discovery/activation mechanics, but TCM4 remains the semantic observation/certification boundary.

## IDX0 — Atomic semantic contributions and workspace index

Add:

- index entries may store target/contribution/occurrence candidates, typed memberships, dependency read sets, and authored source bases;
- indexes may not store checker verdicts, final navigation targets, rename plans, or public operation answers;
- incomplete/budget-exhausted enumeration cannot admit a negative complete result;
- framework global registrations, component links, aliases/reexports, and project memberships are set-valued, profile-qualified, and atomically versioned;
- target and occurrence planners must validate candidates downstream against the semantic owner.

Acceptance additions:

- `idx0_candidates_are_not_authoritative_targets_or_diagnostics`
- `idx0_partial_enumeration_never_negative_admits`
- `idx0_profile_qualified_registrations_do_not_alias`

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

## VIM0 / VIM1 — Vertical conformance manifest and generator

Extend the manifest generator with two new row families:

1. `native_checker_slice`
2. `language_service_operation`

A checker row includes:

- family/slice/profile/applicability;
- semantic input and expected authored diagnostics;
- oracle/correction-overlay identity;
- exact fact/proof demand;
- incremental/cancellation/admission expectations;
- authority promotion and provider-zero-work expectations;
- equivalent-work/allocation/latency/RSS thresholds.

A language-service row includes:

- operation/profile/provider/recovery/coexistence/encoding/consumer applicability;
- expected target, occurrence, presentation fragment, rename intent, or transaction identity;
- exact authored anchors and provenance;
- typed outcome/completeness;
- zero-work requirements for disabled/inapplicable topologies.

The generator must produce:

- DAG nodes and charters where requested;
- hermetic test cases;
- gated provider/platform canaries;
- capability/maturity inputs;
- receipt validators;
- stable generated indexes.

## PER0 — Cache/backend identity, cancellation, budgets, and zero work

Add work counters for:

- diagnostic query/rule/fact/proof reads;
- oracle comparison and correction-overlay certification (offline/test only);
- target nodes/edges, index candidates, occurrence validation, provider calls, mappings;
- rename conflict checks and edit intents;
- edit mappings/anchors/conflicts/staged bytes/files;
- engine source adapters, stats/hashes/network bytes, validation, selection comparisons;
- spawn/handshake/restart/swap/health/orphan processes;
- retained regions/results/targets/cursors/resolve keys/transactions/provider epochs.

Required scenarios:

- cold, first warm, repeated warm, incremental edit, edit-revert, provider/profile/config change;
- cancellation before and during production;
- project open/close and long churn;
- disabled/inapplicable/unopened channel zero-work;
- equivalent fresh-versus-incremental result and work evidence.

Do not impose blanket “0.0% wall-time regression” where the successor adds new required work. Use ratified equivalent-work thresholds and explicit replacement SLOs.

## H2 / H3 — Provider lifecycle and publication

Add:

- provider handles, observations, completion resolve keys, and mapped results are scoped to `ProviderEpoch`;
- ProviderHub binding becomes visible only after exact activation/handshake receipt;
- engine swap is old-or-new atomic and invalidates old epoch handles;
- latest-basis publication joins source/profile/config/authority/provider epochs;
- mixed-epoch and stale results cannot publish as complete;
- dynamic capability withdrawal cancels work and clears only results owned by the withdrawn authority.

## COX0 — Per-profile editor participation and coexistence

Add generated operation/family capability masks for:

- native semantic diagnostic families;
- navigation, references/hierarchy, rename, completion/resolve, hover/signature/inlay, edits;
- provider-backed residual operation families;
- engine availability/source status.

`WorkspaceOnly` must not publish interactive diagnostics/actions or run editor-only leaf features. `Disabled` performs zero hidden work. `auto` withdraws only overlapping capabilities.

## CLI2 — Verter-native typecheck

When opened after NCK7:

- consume `DiagnosticService` rather than constructing a checker/provider/project plan;
- select project/profile scope explicitly;
- write nothing;
- return exact provenance/completeness/NeedInputs;
- exclude lint and formatting unless the command explicitly composes them at a higher application-service layer;
- avoid aliasing `tsc --noEmit`.

Before NCK7, CLI2 may use external/native existing owners according to its current contract, but must not pre-empt NCK authority.

## CLI4 — type-info, lsp, and mcp adapters

When opened:

- expose thin adapters to shared diagnostic, language-service, and engine status services;
- preserve core request/result identity and outcomes;
- do not add command-local provider discovery, capability, mapping, or semantic DTOs;
- engine acquisition commands, if ever exposed, must be explicit EPR2 side-effect requests and never ordinary `lsp` startup behavior.
