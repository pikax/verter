# H2 current-source charter review

Inspection of the tree as it stands against H2 (project-scoped ProviderHub bindings). This is a source review of behavior and callers. It does not certify suite execution, and it does not use commit identity or ancestry as proof.

## Authority

`ProviderHub` is the serving lifecycle owner. `establish`, `recover`, epoch minting, desired-state replay, and applied receipts live on that type (`crates/verter_type_runtime/src/provider_hub/mod.rs`). Adapters spawn processes and report death through the crash signal the hub hands them. `recover` is inert unless the named epoch is the one still serving, so a late signal does not settle a replacement.

Production routes construct that owner rather than a second lifecycle:

- tsserver and tsgo resilient adapters return a `ProviderHub` (`crates/verter_lsp/src/tsserver/resilient.rs`, `crates/verter_lsp/src/tsgo/resilient.rs`).
- The managed fallback is `ProviderHub<dyn TypeProvider>` established on demand (`crates/verter_lsp/src/type_provider/lazy_managed.rs`).
- `ProjectRouter` stores one hub per project engine and admits through `bind_project` / `admit_request_with` (`crates/verter_lsp/src/tsserver/project_router.rs`).
- The shared tsgo composite synchronizes and admits through the same hub API (`crates/verter_lsp/src/tsgo/composite.rs`).

`ClientHandle::restart` in `crates/verter_tsgo_api/src/actor/mod.rs` shuts the current single-flight actor down so the caller can spawn a new transport. It does not mint a serving epoch, replay desired state, or accept a retired engine's result. That remains hub authority. `restart_stops_the_actor` covers the transport teardown.

## Acceptance, inspected against current code

- **H2-AC1.** One serving cell, written by the hub actor. Queries and mutations read that cell. Displaced route owners are the hub-typed adapters above, not a parallel readiness service. Protocol framing stays in the tsgo actor; workspace membership proof stays outside the hub and does not by itself authorize a write (`admit_request` still requires a current generated-unit proof).
- **H2-AC2.** `bind_project`, `admit_request`, `establish`, `synchronize`, `apply_overlay`, and `recover` are the hub surface. Admission binds the witness project, snapshot identity, and the exact unit set (`crates/verter_type_runtime/src/provider_hub/admission.rs`). Receipts are the bytes `note_file_receipt` records only after the wrapped engine reports them applied. Direct overlay withdrawal now keeps the serving read guard from the epoch check through `applied_map` removal, so a retired close cannot delete another incarnation's receipt or report that close as current. The oracle is `withdrawal_receipt_release_holds_the_serving_fence`: it parks the removal on the applied-map lock and requires `serving.try_write()` to fail. A pause before the epoch check does not reproduce the race; a replacement installed in that earlier window makes the check itself stale.
- **H2-AC3.** Replay installs only an engine that accepted the current desired state (`an_engine_that_rejects_replay_is_torn_down_and_never_serves`). Cancelled queued work does not become applied state (`cancelled_queued_overlay_never_reaches_the_engine_or_replay`). A withdrawal the issuer abandons still settles on the detached task (`cancelling_the_issuer_of_an_inflight_withdrawal_still_drops_its_receipt`). Failed compensation retires the incarnation instead of publishing a healthy drift (`first_overlay_failed_compensation_retires_only_its_incarnation`).
- **H2-AC4.** The withdrawal change adds no parse, resolve, plan, emit, or second replay. It holds the existing serving read across the existing receipt removal. This review did not run soak or benchmark lanes and does not claim a performance measurement.
- **H2-AC-ADMISSION.** `admit_request` refuses a missing proof, an excluded unit, the wrong project, a stale snapshot, or an incomplete unit set before any write. Router coverage includes `authored_owner_without_generated_membership_never_reaches_a_project_engine`. Composite coverage includes `unadmitted_carrier_is_refused_before_the_transport_is_touched` and `managed_generated_write_requires_membership_before_activation`.
- **H2-AC-RECOVERY.** Crash control enters `ProviderHub::recover`. A stalled mutation is interrupted and retained desired state replays (`crash_interrupts_a_stalled_mutation_and_replays_retained_state`). An unsaved overlay survives background load and replay (`background_load_never_replaces_an_unsaved_overlay_live_or_replayed`). Old-epoch settlement cannot remove a replacement receipt while the serving read is held across removal (H2-AC2).
- **H2-AC-BATCH.** `carrier_batches_preserve_each_provider_order_without_scalar_dispatch` and `two_provider_aba_aliases_share_a_group_and_withdrawal_blocks_stale_delivery` in `project_router_tests`.
- **H2-AC-ISOLATION.** `a_held_project_engine_does_not_block_an_independent_hub_group` and `a_held_provider_batch_does_not_delay_an_independently_admitted_provider`. The hub module states that each instance has its own actor.

Deadlines are the submitter's ambient deadline captured once and re-opened on the actor (`a_forwarded_mutation_runs_under_the_submitters_absolute_deadline`). Neighboring scopes (engine acquisition policy, public readiness publication, mapper activation) are not implemented here.

## What this review does not do

It does not treat a git ancestor relationship as acceptance. It does not mark H2 implemented. Running the named tests is a separate check from this inspection.
