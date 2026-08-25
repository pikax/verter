# TCM0 §9 — Deletion closure

Scope: charter item 9. Name every mechanism TCM4 deletes and every generic facility that survives with a
proven owner. The steering states this plainly ("Do not defer this inventory to TCM4",
`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §"Deletion closure"), and `TCM0.md:92` repeats it —
17 of the 19 steering items are named by mechanism below, with no exception.

Items 17 (old APIs/DTOs orphaned by the deleted route) and 18 (historical content-mapper codecs) are the
two the steering's own literal rule cannot be satisfied for today: neither has anything to name yet,
because no TCM1-TCM3 implementation exists to produce the DTOs/codecs in question. The
accumulation-at-creation mechanism worked out below ("Closure, 2026-08-23: items 17-18 resolved") is
retained as evidence — TCM1, TCM2 and TCM3 each recording what they introduce or orphan, so TCM4 verifies
a handed-over list instead of re-deriving one.

**Its "therefore CLOSED" verdict is WITHDRAWN.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 returns this block's round-3 candidate as wrongly scoped, lands its work as a NON-ACCEPTANCE evidence
package, and hands the incomplete contract remainder to a **successor block with fresh verification**. A
closure that needs fresh verification is not a closure, so `G-DELETION-CLOSURE-ITEMS-17-18` is OPEN with
the successor as its owner (`OPEN-GAPS.md`; scope: `successor-block-scope.md`). The mechanism below is
the proposal the successor must independently verify and have adopted, not a settled resolution. See the
"Items 1-11 and 13..." paragraph below for the full accounting of which items had a located mechanism at
TCM0 time versus which close only once TCM1-TCM3 land.

## Deleted (named now, executed only in TCM4)

| Mechanism | Location | Why it's deleted | Superseded by |
|---|---|---|---|
| The relay half of `open_file`/`load_file`/`update_file`/`close_file` for `TypeScriptLspDirect`-owned features | `crates/verter_type_runtime/src/tsgo/ipc.rs`, `tsserver/ipc.rs` (the LSP-relay call paths) | Once TypeScript talks to the editor directly through the content mapper, Verter's role as a relay proxying these calls to answer plain-TS questions has no reason to exist | Editor's own `didOpen`/`didChange`/`didClose` driving the mapper's `transform()` directly (`feature-ownership-ledger.md` rows #2-5) |
| The TS-diagnostic merge half of `type_provider/merge/diagnostics.rs` (`merge_diagnostics`, `same_mapped_diagnostic` for the compiler-diagnostic class specifically) | `crates/verter_lsp/src/type_provider/merge/diagnostics.rs` | TypeScript reports compiler diagnostics against the mapped file directly, position-mapped via its own `SpanMap`/`DiagnosticDirectives` — no Verter-side merge step for THIS class | `diagnostic-ownership-matrix.md`'s compiler/checker row |
| The relay round-trip for every `TypeScriptLspDirect`-owned interactive feature (hover/completion/definition/type-definition/references/signature-help/document-highlights/inlay-hints, plain-TS subset of code-actions/semantic-tokens) | `crates/verter_lsp/src/server/nav_features*.rs`, `aux_features.rs` (the relay-calling halves only — the merge/attribution logic for Verter-owned overlays survives) | Same reasoning — TypeScript answers these directly against the mapped file | `feature-ownership-ledger.md` rows #6,9,12-14,16,18-20 |
| The `verter-tsc` CLI's independent compiler-diagnostic implementation, once TCM3's convergence makes this path redundant (`diagnostic-ownership-matrix.md`) | `crates/verter_tsc/src/api_check.rs` | Two independent implementations of "get TS diagnostics" is exactly the kind of duplicate-engine outcome the Shared Optimized Codebase rule forbids, once one path can serve both. Owner settled by `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q6: **TCM3 already owns the convergence** through its `TypeSemanticOracle` / `VerterWithTypeSemanticOracle` diagnostic contract; no new block is authorized, and the CLI/oracle divergence stands disclosed until TCM3 lands | whichever single path TCM3's diagnostic contract lands on |
| The current string-encoded `source_projection_map()`/`PositionMapper::from_json` double-parse path | `crates/verter_compiler/src/code_transform/source_map.rs` (the `_json` variants as the ONLY typed-to-string exit), `crates/verter_lsp/src/documents/position_map.rs:142,200-215` (`PreambleEnvelope` struct + the `from_json` double-parse that consumes it) | Superseded by the typed `SourceProjectionMap`, per `mapping-products-string-surface.md` | TCM1's typed product |
| `get_diagnostics_background`, its forwarding implementations, and `feature-ownership-ledger.md` row 31 — **ruled for deletion; the executing owner is TCM4** | `crates/verter_type_runtime/src/traits.rs:475-477`, plus six implementations and one shared private helper — enumerated in "Executing owner and corrected scope" below | Confirmed to have **no production root**: every named reference in `crates/` is a declaration, an intra-cycle hop, or a test, and a search by name also covers `dyn TypeProvider` dynamic dispatch. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q5 rules it deleted outright and rules that **dead API surface has no capability owner and must not be labelled `DisabledByExplicitApprovedContract`** — a contract disabling something would imply something could have called it, and nothing outside the cycle ever could. This block edits no source; see below for the executing owner, the corrected scope, and the guard proviso | n/a — nothing to supersede, it answers no live consumer today |


## Executing owner and corrected scope (`get_diagnostics_background`)

**TCM4 is the executing owner, by pre-existing assignment rather than nomination.** `charters/TCM4.md:9`
adopts this manifest; `:25` requires every item of its cross-check to reach its recorded disposition;
`:78` mandates deletion exactly per this manifest; and `:116` makes this table authoritative, with TCM4
forbidden from second-guessing it. No new block, amendment, or authorising act is required — the ordinary
digest-bound TCM4 dispatch authorization, once its predecessors are accepted, is sufficient.

**The corrected scope, recorded so TCM4 inherits the real shape rather than rediscovering it.** The
surface is dead because it has **no production root**, NOT because every implementation is a trivial
forwarder. Two sites raise the deletion's COST without creating reachability:

- `crates/verter_type_runtime/src/tsgo/ipc.rs:4137` is a **substantive implementation** — URI conversion,
  transport handle, contents and diagnostics caches, `request_with_priority` with
  `ProviderPriority::Background`. Deleting it removes real behaviour, not a forwarding shim.
- `crates/verter_lsp/src/tsgo/composite.rs:815` `managed_diagnostics(path, background)` is a **private
  helper shared with the foreground path**. The `background` boolean is threaded into it from
  `diagnostics_gated(path, background)`, which is entered at `:994` with `false` from `get_diagnostics`
  and at `:999` with `true` from `get_diagnostics_background`. Removing the background entry point leaves
  the parameter single-valued through two helpers on live foreground code.

So the work is a **parameter collapse through two helpers plus guard-fixture repair**, not a six-line
removal. The remaining implementations are ordinary forwards:
`crates/verter_type_runtime/src/tsgo/owned.rs:513`, `resilient/forwarding.rs:493`,
`crates/verter_lsp/src/tsserver/project_router.rs:1064`, `type_provider/lazy_managed.rs:668`, and
`tsgo/composite.rs:997`. Two gating tests call the method directly and are rewritten or deleted with it:
`crates/verter_lsp/tests/cases/owned_binding_gate.rs:536` and `:555`.

**Guard proviso — the binding constraint on that repair.** The parameter collapse lands on
`crates/verter_session/tests/g_extts/no_fallback_to_inferred_anywhere.rs`, a guard of the
Project-Bound External-TS Contract. It anchors by name (`const OWNED_GATE_FN: &str = "diagnostics_gated"`)
and its fixtures embed the exact signature text **including `background: bool`**. Collapsing the parameter
and updating those synthetic fixtures is routine deletion closure that TCM4 may perform without separate
authority — **provided the guard retains its brace-scoped witnesses, its decoys, and its discrimination
proof. The fixtures may change; the guard's ability to discriminate may not.**

**Authority and a provenance note.** Ruling: lane `architecture-q5-slice-owner`, RESULT PASS, findings
none. Its receipt records `REVIEWED: f7b86376563ceef269493fa05e791f65ded78b8e` — a commit that was the
working branch's tip when the seat read the tree and **is no longer in that branch's history**, having
been reset away to hold the branch still for a landing. The object still resolves but is not an ancestor
of the branch. **The ruling is unaffected**: it rests on charter and manifest text whose bytes are
identical either side of that commit. This note exists so a reader does not hunt a sha that resolves
nowhere and conclude the citation is broken.

## Survives, with a proven owner (not silently kept alive by omission)

| Facility | Owner | Why it survives |
|---|---|---|
| `VerterWithTypeSemanticOracle`-owned methods (component-meta type queries, cross-region rename, carrier-lifecycle notifications, workspace-folder sync) | `feature-ownership-ledger.md` rows #3,7,15,24,27,28 | These need Verter-specific knowledge the mapped file alone cannot express — the content-mapper protocol has no field for them, so the oracle-session client (the SAME `API`/`Snapshot` class inspected live in this investigation) remains the correct mechanism, just no longer wrapped in a relay for the OTHER (TypeScript-native) features |
| `VerterNative`-owned methods and all lint/parse/CSS/directive-syntax diagnostics | `feature-ownership-ledger.md` rows #1,8,10,22,29,30; `diagnostic-ownership-matrix.md`'s directive/framework/custom-block/style rows | Never touched TypeScript; nothing about this program changes that |
| The `Fragment`/`SourceUnit` intra-file assembly system | `crates/verter_compiler/src/assembly/{fragment,source_unit}.rs` | This is the machinery that PRODUCES the content mapper's `transform()` output — it becomes MORE load-bearing, not less |
| `PlacementMap`/`RuntimeSourceMapData`/`EncodedSourceMap` as distinct concepts | TCM1 | Kept distinct per the amendment's explicit rule; only their representation (string → typed) changes |
| `ProjectTypeStore` and the rest of Verter's own type-resolution engine | unchanged | Disjoint concern from the TypeScript-contract program — this investigation finds no reason for TCM1-TCM4 to touch it |
| The External-Source Decision Table's `ExternalSourceRequest`/`ExternalBlockKind` dependency-tracking mechanism | `crates/verter_session/src/types.rs:1643-1678` | Still needed for dependency-edge bookkeeping even where content itself becomes TS-owned (`external-source-decision-table.md` row #6) |

## Correction, 2026-08-23: cross-check against the steering's own 19-item deletion checklist

TCM4's charter ("Required deletion") lists 19 items verbatim from the maintainer's steering
(`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §"Required deletion"). The tables above name 6
concrete mechanisms this investigation located and traced to a file/path. This section walks all 19
steering items explicitly, so none is silently left as a category description for TCM4 to re-derive at
execution time (the charter's own rule: "Do not defer this inventory to TCM4").

| # | Steering item | Located mechanism | Disposition |
|---|---|---|---|
| 1 | `@verter/typescript-plugin` | `packages/typescript-plugin/` | **Deleted.** Its carrier-publication role is superseded by TCM2's content mapper; any surviving IDE-only helper (e.g. cursor-geometry utilities with no carrier-publication role) must show a proven non-TypeScript-contract owner before TCM4 may keep it — default is deletion. |
| 2 | Carrier injection into TypeScript | `crates/verter_lsp/src/carrier_registry.rs`, `carrier_provider_projection.rs` | **OPEN — disposition withdrawn 2026-08-23**, was "Deleted, the injection half only". The stated reasoning (TypeScript no longer needs Verter to inject carrier identity once the mapper's own `virtualFileName`/`canonicalSourceFileName` fields carry it) is the SAME reasoning as ledger rows #25-26, and that reasoning rests on a premise now proved inverted: the carrier-registration path is tsserver-family and hydrates a content cache plus a carrier-to-project map, not merely identity strings (`crates/verter_type_runtime/src/tsserver/ipc.rs:3126`). Identity fields do not obviously subsume a content cache. That ruling was taken: `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q4 RETAINS rows #25-26 under `VerterWithTypeSemanticOracle`, and this item's injection half is gated on the same condition Q4 sets for them — **TCM4 may remove the tsserver-specific methods only after TCM3 supplies and tests equivalent semantics**. Nothing here is waiting on a ruling to be re-requested. See `OPEN-GAPS.md` `G-TCM0-ACCEPTANCE-ROWS-25-26` and this file's "Both rows are RETAINED" section below. |
| 3 | Carrier-only generated-file stores | provider-side generated-companion storage feeding the deleted relay path (row 1/2 of the "Deleted" table above) | **Deleted** — already named in row 1/2 above; listed here to confirm it is the steering's item 3, not a separate undiscovered mechanism. |
| 4 | Carrier-only external synchronization | `crates/verter_lsp/src/external_ts_sync.rs`'s carrier-specific half (its non-carrier dependency-edge bookkeeping survives per `external-source-decision-table.md` row #6) | **Deleted, carrier-specific half only** — the `ExternalSourceRequest`/`ExternalBlockKind` dependency-tracking mechanism itself survives (already named in the "Survives" table above); only its carrier-file-sync consumer goes. |
| 5 | Provider-only `.verter.ts` import projection | `crates/verter_lsp/src/sync_coordinator.rs`, `workspace_scanner.rs`, `vue_assets.rs`, `configured_owner.rs`, `carrier_registry.rs`, `background_drain.rs`, `nav_features_navigation.rs` (the `.verter.ts` transport-rewrite convention across these) | **Deleted** — this is the acceptance invariant's own named item ("no `.verter.ts` transport rewrite remains"); every one of these call sites is a candidate the relevant TCM1-TCM3 rewrite must confirm has no residual dependency before TCM4 deletes it. |
| 6 | Native Preview relay interception | `crates/verter_relay_shim/` (the whole crate), plus the Native-Preview-aware halves of `crates/verter_lsp/src/main.rs` and `crates/verter_tsgo_api/src/relay.rs` | **Deleted** — `verter_relay_shim` has no owner once the content mapper is the sole TypeScript-facing transport; confirmed as a whole-crate deletion target, not a partial one, since its only role is relay interception. |
| 7 | Temporary global `tsdk` staging | `crates/verter_lsp/src/main.rs`, `tsserver/project_router.rs`, `crates/verter_type_runtime/src/discovery.rs`, `crates/verter_type_runtime/src/tsserver/ipc.rs`, `crates/verter-editor-client/src/args.rs` (the `tsdk` staging/discovery paths in each) | **Deleted, tsdk-staging half only** — package/engine discovery for the CERTIFIED content-mapper package (a different, TCM0-owned discovery concern — `package-lock-and-semantic-api.md` §1) survives; only the old global-`tsdk`-mutation convention goes, per the acceptance invariant "no global `tsdk` mutation remains." |
| 8 | Relay advertisement and carrier attestation | the relay-side halves of `crates/verter_lsp/src/carrier_registry.rs` and `verter_tsgo_api/src/relay.rs` that advertise/attest carrier identity to the relay-managed engine | **Deleted** — superseded by TCM4's own attestation list (engine/project/mapper/config/generation agreement), a materially different attestation object with a different owner (`TypeScriptApiSessionState`/`MapperProcessProjectState` per `distributed-lifecycle-contract.md`), not a renamed version of this one. |
| 9 | Relay taint filtering and synthesized neutral responses | `crates/verter_lsp/src/features/diagnostics_bridge.rs`, `features/hover.rs`, `type_provider/merge/position.rs`, `tsgo/shared.rs` (the taint-filter / neutral-response halves specific to the relay's fabricated-response fallback) | **Deleted, relay-taint half only.** Diagnostic dedup/precedence for the SURVIVING diagnostic classes is unaffected — `diagnostic-ownership-matrix.md`'s own precedence rules replace this specific mechanism, they do not depend on it. |
| 10 | Duplicate generated/provider/original TypeScript remapping | the merge-time remapping halves of `type_provider/merge/position.rs` and `type_provider/merge/diagnostics.rs` not already covered by the "Deleted" table's row 2 | **Deleted** — TypeScript answers directly against the mapper's own `SpanMap` for `TypeScriptLspDirect`-owned features (feature-ownership-ledger.md sub-rows `a`), so Verter's own duplicate remap step for those classes has no consumer left. |
| 11 | Duplicate companion compilation used only for TypeScript ingestion | any second compile pass whose sole purpose is producing a TypeScript-ingestible companion, distinct from the content-mapper's own `transform()` output | **Deleted** — the content mapper's `transform()` output IS the one compilation TypeScript ingests (steering §6 "Fragment/SourceUnit assembly... feeds the mapper", `external-source-decision-table.md` row #11); a second compile pass producing a companion file for TypeScript to read from disk is exactly what the mapper protocol replaces. |
| 12 | The old TypeScript version-selection policy | the pre-TCM0 semver-only activation policy this program's steering §2 explicitly forbids ("Do not implement activation as a plain `semver >= 7.1.0`") | **Deleted** — superseded by TCM4's `candidate engine` / `certified engine` / `active project` three-tier contract (steering §2, TCM4's rewritten charter). |
| 13 | Old tsserver and TSGO carrier providers | `crates/verter_type_runtime/src/tsgo/` and `tsserver/` provider implementations whose ONLY role is `TypeScriptLspDirect`/`VerterWithTypeSemanticOracle` capability now served by the mapper + oracle | **Deleted, once feature-ownership-ledger.md's per-row TCM4-deletes column confirms every capability they served has a live TCM1-TCM3 replacement** — this is the broad form of the "Deleted" table's row 1/3, stated as its own steering-checklist item because the steering names the provider TYPES (`TsgoTypeProvider`/`TsserverTypeProvider`), not only the relay call paths. |
| 14 | Private TypeScript semantic-query protocols | any bespoke JSON-RPC/IPC shape Verter invented to talk to tsgo/tsserver outside the certified content-mapper + official semantic-API contracts | **Deleted** — TCM3's charter forbids retaining one ("Do not retain a private legacy query protocol"); this item names the deletion consequence of that forbidding rule. |
| 15 | Carrier lifecycle methods on `TypeProvider` | feature-ownership-ledger.md rows #23 (`configure_paths`), #24 (`notify_carrier_changed`, partially — its oracle-session half SURVIVES), #25-26 (gated, see below) | **Deleted where the row's TCM4-deletes column says so; rows #25-26 are RETAINED by Q4, and TCM4 may remove their tsserver-specific methods only after TCM3 supplies and tests equivalent semantics** (feature-ownership-ledger.md's correction section, and "Both rows are RETAINED" below) — not a blanket deletion, since some carrier-lifecycle methods have a surviving oracle-session owner. |
| 16 | The broad `TypeProvider` abstraction when no surviving caller requires it | `crates/verter_type_runtime/src/traits.rs:130-512` (the trait itself) | **Deleted only once every one of the 31 ledger rows has zero remaining caller of the OLD trait shape** — the trait is not deleted until its capability ledger is fully green, per the charter's own "Do not delete the old query plane before its capability ledger is green" rule; this is the terminal item, not an early one. |
| 17 | Old APIs and DTOs whose only owner was the removed route | any DTO type whose sole producer/consumer pair is entirely inside the deleted rows 1-16 above | **OPEN — owned by the successor block. The "Closure, 2026-08-23: items 17-18 resolved" section below is the PROPOSAL the successor must independently verify and have adopted, not a settled resolution; its "therefore CLOSED" verdict is withdrawn per this file's header. See `OPEN-GAPS.md`'s `G-DELETION-CLOSURE-ITEMS-17-18` row.** Not independently enumerable here without re-deriving TCM1-TCM3's own implementation, which has not happened yet — the PROPOSED mechanism is ACCUMULATION AT CREATION: TCM1, TCM2 and TCM3 each record, as an added exit criterion, every DTO/API type they introduce or orphan whose sole producer/consumer pair lies inside the deleted set, appended to this file as each block lands, and TCM4 verifies rather than re-derives that list. |
| 18 | Historical content-mapper codecs | any interim/versioned codec a TCM2 implementation might have carried during its own development before converging on the ONE certified codec | **OPEN — owned by the successor block. The "Closure, 2026-08-23: items 17-18 resolved" section below is the PROPOSAL the successor must independently verify and have adopted, not a settled resolution; its "therefore CLOSED" verdict is withdrawn per this file's header. See `OPEN-GAPS.md`'s `G-DELETION-CLOSURE-ITEMS-17-18` row.** No codec exists yet at TCM0 time, so the PROPOSED closure is an **empty list, established by a negative check**: TCM2's added exit criterion proves it ships exactly one codec and never carried an interim versioned one into its landed tree; if that check ever fails, the interim codec is named at that moment and enters item 17's accumulated list instead. |
| 19 | Compatibility feature flags and fallback branches | any `if legacy_route_enabled` / env-var-gated fallback a TCM1-TCM4 implementation might be tempted to add during migration | **Forbidden from ever landing, not merely deleted after the fact** — each TCM1-TCM4 charter's Forbidden section names this explicitly; TCM4's acceptance invariant "no silent fallback exists" is the terminal check. |

Items 1-11 and 13 have a located, named mechanism today. Items 12, 15-16, 19 are policy/structural items
whose "location" is a rule about the FINAL state, not a single file. Items 17-18 genuinely cannot be
enumerated before TCM1-TCM3 exist (there is no DTO or codec yet to name); the accumulation-at-creation
mechanism proposed below ("Closure, 2026-08-23: items 17-18 resolved") is TCM0's answer to that, and its
closure verdict is withdrawn to the successor block per `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1. See `OPEN-GAPS.md`'s `G-DELETION-CLOSURE-ITEMS-17-18` row.

## RETAINED by ruling (ledger rows #25-26)

`register_carrier_member`/`register_carrier_metadata`/`activate_carrier_member(s)`
(`feature-ownership-ledger.md` rows #25-26) were named here as deletion candidates on the argument that
the content-mapper protocol's own `virtualFileName`/`canonicalSourceFileName`/
`supplementalSourceFileNames` fields already carry this identity on the wire. TCM0's own investigation
then found that argument rests on an inverted premise — the substantive implementation is tsserver-family
and hydrates a `contents` cache and a carrier→project map, not a tsgo relay artifact
(`feature-ownership-ledger.md`, "six factual attribution errors").

**Both rows are RETAINED.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q4: retain rows 25 and 26 under `VerterWithTypeSemanticOracle` — row 25 preserves local content/position
conversion and carrier-to-project routing, row 26 preserves oracle working-set activation. **TCM4 may
remove the tsserver-specific methods only after TCM3 supplies and tests equivalent semantics.** That is
the deletion gate; it is no longer an open governance question and no longer gates TCM0.

## Closure, 2026-08-23: items 17-18 resolved, and why only one resolution is legal (`G-DELETION-CLOSURE-ITEMS-17-18`)

`OPEN-GAPS.md`'s `G-DELETION-CLOSURE-ITEMS-17-18` row states that TCM0's acceptance must decide between
two resolutions before TCM0 leaves LOCKED:

> (a) items 17-18 stay genuinely unenumerable until TCM1-TCM3 exist, and TCM0's acceptance record
> explicitly ratifies a per-type execution-time discovery method as the closure mechanism for exactly
> these two items; or (b) TCM0 is held in LOCKED until TCM1-TCM3 produce enough of the DTO/codec surface
> that items 17-18 can be enumerated by name before TCM4 dispatch.

**Resolution (b) is unsatisfiable, and provably so.** `program-dag.toml` gives
`TCM1.predecessors = ["TCM0"]`, `TCM2.predecessors = ["TCM0", "TCM1"]` and
`TCM3.predecessors = ["TCM0", "TCM1"]`. None of the three may be dispatched until TCM0 is ACCEPTED. So
holding TCM0 in LOCKED until TCM1-TCM3 have produced the DTOs and codecs requires those blocks to produce
output before they may start — the identical unsatisfiable shape already identified and rejected for the
rows #25-26 gate (`feature-ownership-ledger.md`'s "Correction, 2026-08-23") and for `G-TOPOLOGY`
(`topology-benchmark-plan.md`'s addendum). Resolution (b) is therefore struck, not merely
un-preferred.

That leaves (a) — but (a) as the row phrases it, "a per-type execution-time discovery method", is exactly
what round-2 review already rejected as an unassigned exception, and ratifying it verbatim would re-adopt
the thing the gap was opened to prevent. TCM0 ratifies a strengthened form instead.

### Ratified: items 17-18 close by ACCUMULATION AT CREATION, not by discovery at deletion

The steering's rule is *"Do not defer this inventory to TCM4"*. That rule is about who must hand TCM4 a
named list — not about when the names have to exist. The names cannot exist at TCM0 time because the
things they name do not exist. They can, however, be recorded the moment they come into existence, by the
block that creates them, so that TCM4 still receives a **named inventory it does not re-derive**:

**Item 17 — old APIs and DTOs whose only owner was the removed route.**
Each of TCM1, TCM2 and TCM3 records, as part of its own evidence, every DTO or API type it introduces
whose sole producer/consumer pair lies entirely inside the deleted set (rows 1-16 of the table above), and
every pre-existing type its work orphans. The list is appended to this file as each block lands. TCM4's
exit criterion 5 then verifies that accumulated list is complete — for each named type, that nothing
outside the deleted set constructs or consumes it — and deletes it. TCM4 performs a **verification** of a
list it was handed, not a discovery of a list nobody wrote.

**Item 18 — historical content-mapper codecs.**
Only TCM2 can create one, and the certified design has exactly one codec. Item 18's correct end state is
therefore an **empty list, established by a negative check rather than a deletion sweep**: TCM2 proves it
ships exactly one codec and never carried an interim versioned one into its landed tree. If that check
passes, item 18 has nothing to delete and closes as "empty by construction". If it fails, the interim
codec TCM2 carried is named by TCM2 at that moment and enters item 17's accumulated list. Either way TCM4
receives names, never a search.

### Why this is a closure and not a rephrased deferral

The distinction is who holds the obligation and when it is discharged. Execution-time discovery leaves the
obligation with TCM4 and discharges it by search, which is what the steering forbids and what
`TCM4.md`'s own owned-scope item 9 contradicted by simultaneously claiming TCM4 *"does not re-derive the
deletion list at execution time"*. Accumulation-at-creation moves the obligation to the three blocks that
can actually discharge it, at the only moment they can — and leaves TCM4 doing exactly what its charter
already says it does. `TCM4.md`'s required-outcomes item 3, owned-scope item 9 and exit criterion 5 are
already written to defer to whichever resolution TCM0 ratifies, so this resolution needs no amendment to
TCM4's charter and resolves its internal contradiction rather than preserving it.

### The amendments this would require before dispatch

The accumulation obligation is new work for TCM1, TCM2 and TCM3, and their charters are ratified,
digest-pinned documents (`authority-registry.toml`: `TCM1-CHARTER`, `TCM2-CHARTER`, `TCM3-CHARTER`). **This
evidence pass does not edit them and does not re-pin their digests.** These amendment texts are carried
forward as proposals, not as a settled mandate: because the closure verdict above is withdrawn to the
successor block, the amendments derived from it transfer with it (`OPEN-GAPS.md`
§`G-CHARTER-AMENDMENTS`). They are named here and in `OPEN-GAPS.md`:

- **TCM1, TCM2, TCM3** — one added exit criterion each: record every DTO/API type the block introduces or
  orphans whose sole producer/consumer pair lies inside the deleted set, appended to
  `deletion-closure.md`'s item-17 list. For TCM1 this folds into the charter amendment
  `G-STRING-SURFACE-CITATIONS` already requires, so TCM1 needs one amendment act, not two.
- **TCM2** — one added exit criterion: prove exactly one content-mapper codec ships, with no interim
  versioned codec in the landed tree (item 18's negative check).
- **TCM4** — none. Its existing deferral wording consumes this resolution as written.

Items 17 and 18 therefore have a **worked-out closure mechanism with its obligations mapped to named
blocks and its residue reduced to a list of charter amendments** — which is what TCM0 produced and what
lands here as evidence. What TCM0 does NOT have is an accepted closure: per
`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 the verdict is withdrawn and `G-DELETION-CLOSURE-ITEMS-17-18` passes to the successor block for fresh,
independently checkable verification.
