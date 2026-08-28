# TCM0 §13 — Distributed lifecycle ownership

Scope: steering §13 ("Distributed lifecycle ownership") in
`docs/arch/refactor/rev11/rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md`. This artifact documents
that required contract for TCM-plan integration; it does not itself assert a separate settlement act.

## The rule

Do not create one Rust epoch object that claims ownership over state in the TypeScript process, mapper
process, editor extension, and Verter LSP. Each process/actor's local state has its OWN local owner;
these owners coordinate through an immutable, serializable capability descriptor that carries identities
and attestations but owns no remote resource lifetime.

## The four local owners

| Owner | Lives in | Owns | Does NOT own |
|---|---|---|---|
| `MapperProcessProjectState` | TCM2's content-mapper process | per-project-handle state for the projection plane: TypeScript's opaque project handle, bounded message/queue/handle/cache state, the mapper's own `closeProject`-triggered release (`evidence/TCM0/deletion-closure.md`'s "survives" row for the content-mapper process reasoning) | the TypeScript semantic session; any editor-extension state; any Verter LSP process state |
| `EditorRegistrationState` | the editor extension (VS Code today) | mapper discovery/registration bookkeeping — which mapper package/config the extension has offered the configured/inferred project, the `NeedsMapperConfiguration` recommendation dedup key (TCM4 §"Recommendation and documentation") | the mapper process's own lifecycle; the TypeScript session; the Verter LSP's own state |
| `TypeScriptApiSessionState` | the process that holds the TypeScript semantic-API client (`API`/`Snapshot` per `package-lock-and-semantic-api.md` §4) — editor-attached or Verter-managed per TCM0's topology certification | snapshot acquisition/update/disposal, project/source-file lookup, the `Program`/`Checker` handle discipline TCM3 must enforce (never outlive the owning `Snapshot`) | the mapper process; the editor extension's registration bookkeeping |
| `VerterSemanticClientState` | Verter LSP / the process running `VerterWithTypeSemanticOracle` queries | the narrow `TypeSemanticOracle` client state (bounded singleflight, bounded project-handle table, bounded derived-serialization cache — steering §10's "Permitted adapter state" list) | any remote TypeScript object graph past its snapshot; the mapper process; the editor's registration state |

Each owner is local: it has full authority over its own process's state and MUST NOT reach into another
owner's process to mutate or extend that owner's lifetime. Recovery restarts the SAME certified mechanism
locally — it does not switch to the carrier route (per steering §13: "Recovery may restart the same
certified mechanism. It must not switch to the carrier route or silently change feature ownership.").

## The coordinating capability descriptor

An immutable, serializable value — never a live handle, never something that owns a remote resource —
carrying:

- certified engine/package/binary identity (settled per
  `rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md`: `typescript@7.1.0-dev.20260822.1` or
  a later package certified against the same locked contract);
- project/config identity;
- mapper package/config identity;
- mapper activation attestation (TCM4's "Before publishing... attest" list);
- semantic API capability/session attestation where required;
- source/project generation;
- feature-ownership table identity (`feature-ownership-ledger.md`'s row set, versioned);
- terminal projection-policy identity (`projection-class-contract.md`'s class/mask policy, versioned);
- trust/external-code state.

**Identity or generation disagreement fails closed** — any owner that observes a descriptor whose
identity/generation does not match its own local state's expectation refuses to serve the affected
capability rather than guessing or silently reusing stale state. This is the same fail-closed discipline
CLAUDE.md's Project-Bound External-TS Contract already requires for `NotReady`/terminal no-serve states,
applied to this new descriptor.

## What this forbids

- A single Rust type (e.g. a `TcmSessionEpoch` god-object) held by one owner but read/mutated by another
  process's code — this is exactly "one Rust epoch object that claims ownership over state in the
  TypeScript process, mapper process, editor extension, and Verter LSP" the steering names as forbidden.
- The capability descriptor acquiring mutable remote-resource-owning fields (a live `Snapshot` handle, a
  live project handle) — it stays a value type; only the four local owners hold live handles, each
  scoped to its own process.
- Any owner extending another owner's handle lifetime by retaining a copy past that owner's release
  (e.g. `VerterSemanticClientState` retaining a `Snapshot` reference past `TypeScriptApiSessionState`'s
  own disposal) — this is the general form of the §4c stale-`Program` defect
  (`package-lock-and-semantic-api.md`), now stated as a cross-owner rule, not only a TCM3-local one.

## Who implements what

- TCM1: no lifecycle-owner code (compiler-core scope only); this contract is forward reference material.
- TCM2 implements `MapperProcessProjectState` and the mapper-side half of `EditorRegistrationState`'s
  descriptor fields.
- TCM3 implements `TypeScriptApiSessionState` and `VerterSemanticClientState`, including the
  never-outlive-snapshot discipline this contract states as a cross-owner rule.
- TCM4 implements `EditorRegistrationState`'s editor-side half (activation attestation, the
  `NeedsMapperConfiguration` recommendation flow) and assembles the capability descriptor's full field
  set from the three upstream owners' identities.
