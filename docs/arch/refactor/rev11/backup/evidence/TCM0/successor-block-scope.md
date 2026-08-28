# Successor block — scope

> **SUPERSEDED IN WHOLE, 2026-08-25. The construct this file scopes does not exist.** That is settled by
> an act, not by this file's own say-so: `program-state.toml`'s entry at commit
> `4f0efc5e9a2050968bd61435e87709a4640623c2` cancels the successor construct, records that no successor
> block exists or is to be created, lifts the round limit, and places the remainder on this block. Until
> that act existed the cancellation had been decided in correspondence only, and this banner asserted it
> with nothing behind it. The round limit
> that made a successor necessary was lifted, and TCM0 closes its own remainder; the two obligations
> TCM0's authority cannot discharge are relocated to named existing owners by their own instrument,
> not to a successor. The continuation surface is `closure-register.md`, derived from the charter, and
> `probes/closure-validator.mjs` is the gate that reads it. This file is retained as the record of
> what was scoped and why, and **must not be used as a source of scope**. One of its claims was also
> false; see the strike below.

**This is a scoping note, not a charter.** It is not digest-pinned, not registered in
`docs/arch/architecture-lock/ledger/authority-registry.toml`, and not a node in
`docs/arch/refactor/rev11/program-dag.toml`. It carries no authority and dispatches nothing. Authorizing
the block, writing its charter, binding a digest and dispatching it are the program orchestrator's and the
maintainer's acts; this file only records what the work is, so that whoever performs those acts is not
reconstructing it from a diff.

## Why this block exists

`docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md` Q1 reviewed TCM0's
round-3 candidate and **returned it as wrongly scoped**. Its ruling was to land the accurate portion — the
source-backed inventories, the executable probes, and their transcript — as a **NON-ACCEPTANCE evidence
package**, and to make the **incomplete contract remainder a successor block with fresh verification, not
a round 4 of TCM0**.

Two consequences shape everything below.

**Fresh verification is the point, not a formality.** The five gates this block inherits were marked
CLOSED by TCM0's own closure pass. That self-certification does not stand: a closure that requires fresh
verification was not a closure. Every source-backed finding those passes produced is retained in the
evidence files and is available to this block as input — none of it is retracted. What is withdrawn is the
verdict. The successor's own closure claims must therefore be checkable by someone who did not produce
them, which is a stronger bar than "an evidence file asserts it".

**It is not TCM0 again.** TCM0's charter, its acceptance clause and its scope items are not this block's
authority. This block owns a named remainder, listed below, and nothing else.

## What this block OWNS

Six rows from `OPEN-GAPS.md`. For each: what is already established, and what specifically remains
unproven.

### 1. `G-LEDGER-SCOPE`

**Established.** The `TypeProvider` trait has 44 methods, re-enumerated from
`crates/verter_type_runtime/src/traits.rs`. Fourteen of the steering's 32 named capabilities are served by
real production code behind no `TypeProvider` method, each located to a `file:line`. `implementation` is
served by a typescript-plugin carrier-routing override, not by a `TypeProvider` method and not absent. Six
per-row citations were wrong and are corrected.

~~**Unproven.** The ledger's own disposition concedes it: TCM0 records the "uniformly `VerterNative`,
uniformly unaffected by TCM1-TCM4" characterisation as a finding and explicitly does not ratify it as an
ownership assignment for the 14 capabilities it did not individually analyse.~~

**SUPERSEDED by the capability-disposition act in `docs/arch/architecture-lock/ledger/program-state.toml`'s
TCM0 notes; the characterisation it restates is FALSE.**
`probes/capability-provider-hop-walk.mjs` walked all seventeen request paths individually instead of
characterising fourteen of them at once, and one DOES reach a provider: rename preparation calls
`TypeProvider::get_rename_locations` at `crates/verter_lsp/src/server/rename_prepare.rs:181`. The
uniform verdict is struck in `feature-ownership-ledger.md`, and this paragraph restated it after that
strike — which is the finding worth keeping: **striking a claim where it was reported does not strike
it where it is restated, and only reading sentences rather than matching the reported string finds
the restatement.** The row is closed by the ratified per-capability derivation, not by characterisation
(`docs/arch/architecture-lock/ledger/program-state.toml`, TCM0 capability-disposition act; `closure-register.md` `S3.e`).

### 2. `G-SEMANTIC-API-CERTIFICATION`

**Established.** Probes 1-8 exist, are committed, refuse to run against anything but the pinned candidate,
and assert discriminating properties; their transcript is committed. Q1 admits them as evidence. Five
constraints binding on TCM2/TCM3 fell out of them, and two of the guards are proven to discriminate by
planting the reversal.

**Current disposition.** `closure-register.md` Scope 2, rows `S2.a` through `S2.k`, owns the obligation
status and maps each clause to its probe. The earlier question here about who could close item 2 is
historical.

### 3. `G-PROJECTION-MASK-TOTALITY`

**Established.** A five-factor policy, each factor a total map from a closed domain to a 20-bit constant,
composing by AND, with the relation factor derived from the shipped source and `OWNER_WIRE_ELIGIBLE`
derived from the ownership ledger.

**Unproven.** Totality itself. The claim quantifies over all eighteen `class-domain member × relation` cells and all
twenty feature bits, and it is exactly the kind of claim that is only established by an independent
re-derivation of the table.

### 4. `G-STRING-SURFACE-CITATIONS`

**Established.** `CodeTransform` is not the single point of origin: eight in-repo producers mint map JSON,
one of them a `pub` cross-crate API. `TCM1.md`'s exit-criterion-1 deletion proof therefore covers seven
call sites in one crate and proves less than it claims. The sound instrument is a value newtype over the
encoded map, and no such newtype exists today.

**Unproven.** Exhaustiveness, by this row's own admission. The inventory is explicitly not claimed
exhaustive after two manual passes each found the prior one incomplete, and the closure text itself had to
correct an undercount (`chain_source_map`'s eighth production caller) inside the row whose subject is
undercounting.

### 5. `G-DELETION-CLOSURE-ITEMS-17-18`

**Established.** Holding TCM0 until TCM1-TCM3 produce DTOs and codecs is an unsatisfiable dependency
cycle, provable from `program-dag.toml`. Accumulation-at-creation — each of TCM1/TCM2/TCM3 recording what
it introduces or orphans, so TCM4 verifies a handed-over list — is a method that satisfies the steering's
"do not defer this inventory to TCM4" rule without requiring names that cannot exist yet.

**Current disposition.** `closure-register.md` row `S9.c` and `receiving-coverage.md` parts `B1`–`B3`
record the binding placement across TCM1–TCM4; this file's earlier unbound-proposal statement is historical.

### 6. `G-CHARTER-AMENDMENTS` — the residual TCM1/TCM2/TCM3 rows only

**Not owned:** the `TCM0.md` Scope-item-7 row. It is DISCHARGED — Q2 ratifies the topology transfer by
ruling, so no amendment effects it and nothing about it gates anything.

**Owned:** the rows that derive from rows 4 and 5 above — `TCM1.md`'s owned-scope item 1 / exit criterion
1 replacement, and the added accumulation exit criterion for `TCM1.md`/`TCM2.md`/`TCM3.md` plus `TCM2.md`'s
single-codec negative check. They travel with the closures that generate them: an amendment proposed from
a withdrawn closure has nothing under it. The successor re-establishes the findings first, then proposes.

**Also owned:** the disclosure that this gate has no mechanical rail. Nothing in
`scripts/validate-program-state.mjs` or the authority registry fails if a block is dispatched with its
charter unamended.

## What this block does NOT own

- **Q2 — the topology transfer.** Ratified. TCM0 owns candidate screening, survivor sets, metrics,
  harness, baseline method and selection rule; TCM2 and TCM3 own evidence-based projection- and
  semantic-topology selection as blocking exits of their own blocks. Not reopened, not re-argued.
- **Q3 — the performance contract.** `performance-baselines.md` requirements 6-8 are the complete Scope-10
  performance contract and no dedicated-machine absolute baseline is required. No number is owed.
- **Q4 — ledger rows 25 and 26.** Retained under `VerterWithTypeSemanticOracle`. TCM4 may remove the
  tsserver-specific methods only after TCM3 supplies and tests equivalent semantics. That gate belongs to
  TCM3 and TCM4.
- **Q5 — the dead diagnostics API.** `get_diagnostics_background`, its forwarding implementations and
  ledger row 31 are ruled deleted, and dead API surface must not be labelled
  `DisabledByExplicitApprovedContract`. **The deletion is a separate, later, code-bearing slice.** This
  block is not that slice and does not perform it.
- **Q6 — diagnostic-mapper convergence.** TCM3 already owns it; no new block is authorized. Until TCM3
  lands, severity taxonomy, canonical positioning and unpositionable-diagnostic behaviour remain divergent
  across the CLI and oracle paths — a disclosed fact, not this block's repair.
- **Q7 — transcript staleness.** Acceptable. The transcript is immutable evidence for its exact pinned
  package. TCM4 owns future-package verification at the certified-engine gate.
- **Q8 — the ledger.** `docs/arch/architecture-lock/ledger/program-state.toml` is the program
  orchestrator's to write, on trunk. No block branch edits it.
- **The four rows with owners outside TCM0** — `G-CONFORMANCE-FIXTURES-TCM2`/`-TCM3`/`-TCM4` and
  `G-TEMPLATE-SRC-PROJECT-CONTEXT-CONTRACT`. Unchanged, owned by the blocks they name.

## What EVIDENCE each gate needs

The bar is what an independent verifier would have to be shown. "The evidence file asserts it" is not that
bar — every one of these rows already had an evidence file asserting it.

| gate | what closes it |
|---|---|
| `G-LEDGER-SCOPE` | Current disposition and proof are owned by `closure-register.md` row `S3.e` and the per-capability derivation in `capability-provider-hop-walk.md`; this historical scope file does not restate them. |
| `G-SEMANTIC-API-CERTIFICATION` | A decision on record — ruling or amendment — naming who must run charter item 2's bulk probes and against what. If that is this block, then a re-execution of probes 1-8 against the pin from a clean checkout, its transcript committed, and each item-2 clause mapped to the specific assertions covering it, with the mapping checkable by re-reading the probe sources. |
| `G-PROJECTION-MASK-TOTALITY` | An independent re-derivation of the mask table: all eighteen `class-domain member × relation` cells and all twenty bits recomputed from the five factors by someone other than its author, agreeing with the committed table cell for cell, with each factor's derivation re-checked against the shipped source it cites, and the AND-only composition checked to be capable of producing every value the table shows. A mechanical derivation that a reader can re-run beats a hand table. |
| `G-STRING-SURFACE-CITATIONS` | A structural enumeration rather than a fourth manual pass: introduce the value newtype (or an equivalent compiler-enforced instrument), retype the map-carrying fields, and let the compiler produce the complete producer list. The count is then a build output, not a claim. `pub use oxc_sourcemap;` must be dispositioned in the same work, since it is the documented escape hatch around any such instrument. |
| `G-DELETION-CLOSURE-ITEMS-17-18` | The three charter amendments actually adopted and digest-re-pinned by the authority that owns them, so the accumulation obligation binds TCM1/TCM2/TCM3 — plus, if the mechanical-rail exposure is to be closed rather than only disclosed, a check that fails when a block is dispatched with its charter unamended. |
| `G-CHARTER-AMENDMENTS` (residual rows) | Discharged row by row as the amendments above are adopted. No row closes on a proposal; each closes on the ratification act and the re-pinned digest. |

## Authority

Nothing in this file authorizes anything. The block's authorization, charter text, digest binding,
`authority-registry.toml` record, `program-dag.toml` placement and dispatch are the program orchestrator's
and the maintainer's acts.
