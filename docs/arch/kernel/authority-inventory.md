# Kernel authority inventory

This decision records what the universal kernel reuses, amends, replaces or
deletes. Today, framework-shaped host/session registries and untagged public
boundaries own these concerns. The final and sole owner is the typed immutable
universal catalog and the demand-selected kernel services.

It describes the repository at `docs(arch): publish the docs-only rule for the
kernel decision tier`, 2026-10-07. It follows the docs-only rule in
[README.md](README.md): it changes no production route and adds no check.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/UAK0/products/`:

| File | Holds |
| ---- | ----- |
| `owner-consumer-inventory.v1.json` | Named boundaries `B01`–`B06`, outcomes `O01`–`O13`, consumers `C01`–`C17`, retained seams `S01`–`S13`, the superseded-proposal map, the zero-work baseline cells `Z01`–`Z06` and the transferred obligations |
| `deletion-retag-ledger.v1.json` | Displaced routes `D01`–`D19` with category, symbols, paths, disposition, one deletion owner, successor path and receiving acceptance ID, plus the three empty populations |

Every implementation and deletion owner in both files is an existing plan node; retained seams are owned by their current crate. `consumes` names boundaries (`B..`); `consumesOutcomes` names outcomes (`O..`). Each deletion owner updates the repository skill documenting the route it removes in the same change. Every `successorPath`
starts at UAK0, and each step follows a predecessor edge recorded in the
receiving charter. UAM0 (`UAM0-AC-R1`) owns the validator that checks this
data and runs its negative controls. This node ships no validator.

## Method

1. Enumerated every production site in `crates/verter_language/src`,
   `crates/verter_session/src`, `crates/verter_protocol/src` and
   `crates/verter_identity/src`, then followed each producer to its consumers
   in the compiler, LSP, MCP, NAPI, WASM, FFI and the TypeScript packages.
   Paths follow producer→consumer edges, not name matches.
2. Classified every route against the three charter categories: central
   framework switch; untagged coordinate or public identity; duplicate
   component-information authority.
3. Chose each deletion owner from the descendants whose charter allows
   production work and names that route's family. The family locks (UAI0,
   UAO0, UAP0, UAM0) and the verifiers (ENCT0, ENC1, PER0) delete nothing, so
   none of them owns a deletion.

## Named boundaries at the described head

| Boundary | State today | Disposition | Contract → implementation owner |
| -------- | ----------- | ----------- | ------------------------------- |
| `CarrierProfileId` | absent; carried by `FileLanguage::Framework { adapter_id, language_id }` | replaced | VID0 → VID0T (`VID0T-AC1`) |
| `FrameworkProfileId` | absent; carried by `FrameworkAdapterId(Arc<str>)` and the open proto `framework_adapter_id` string | replaced | VID0 → VID0T (`VID0T-AC1`) |
| `ProjectProfileId` | absent; carried by the LSP `ProjectRegistry` | replaced | VID0 → VID0T (`VID0T-AC1`) |
| `CatalogSnapshot` | absent; split over `LanguageRegistry`, `CarrierGrammarAuthority`, `ImmutableCapabilityCatalog`, `FrameworkAdapterRegistry` and two generated TS mirrors | replaced | CAT0 → CPF1 (`CPF1-AC2`) |
| `DemandPlan` | absent; selection is one framework per file by extension, plus the process-wide `--frameworks` admission | replaced | DEM0 → COX0 (`COX0-AC2`) |
| `TypeInfoRequest` | an alias: `pub use graph::TypeInfoGraphRequest as TypeInfoRequest` | amended | TIF0 → TIF1 (`TIF1-AC2`) |

Five of the six names do not exist in code yet. That does not disprove the
boundary, because the charter names them as the successor's identities. The
current carriers are the owners the inventory displaces.

## Displacement summary

| Category | Routes | Deletion owner |
| -------- | ------ | -------------- |
| Central framework switch | `D01`–`D07` (grammar enum, installed-capability enums, request enums, parse-identity switches, core predicate branches, hard-coded registrations, extension table) | CPF1 (`CPF1-AC1`) |
| Central framework switch | `D10` LSP/editor branches, `D11` MCP branches | COX0 (`COX0-AC1`) |
| Untagged identity | `D08` conflated `FileLanguage` and adapter/language ids | CPF1 (`CPF1-AC1`) |
| Untagged identity | `D09` capability ids and global framework gates | COX0 (`COX0-AC1`) |
| Untagged identity | `D19` `ProjectRegistry` as project authority | PM1 (`PM1-AC1`; PM4 proves) |
| Untagged coordinate | `D16` NAPI/WASM/FFI/MCP/proto positions | ENCF0 (`ENCF0-AC1`) |
| Untagged coordinate | `D17` fixed-UTF-16 editor contracts | ENCL0 (`ENCL0-AC1`) |
| Duplicate component information | `D12` component-meta resolver/cache/schema, `D13` public-API projection, `D14` off-store surface caches, `D15` legacy serde TypeInfo DTOs | TIF1 (`TIF1-AC1`) |
| Duplicate component information | `D18` per-request component scan | IDX0 (`IDX0-AC1`) |

Empty populations at this head:

- **The combined `CarrierCompiler` trait.** It and the dynamic compiler
  registry are already gone. Capabilities are separate traits in
  `framework_common/capability.rs`, so CPF1 inherits only `D02`.
- **A shared Web Component schema or registry.** None exists, so CEC0 receives
  nothing to delete.
- **A framework-specific index.** None exists apart from `D18`.

## Findings recorded for the receiving owners

- Three component-information authorities coexist, sharing only the macro-DTO
  layer:
  - component-meta (`ComponentMetaResultDb`, schema 12);
  - the framework-surface executor (`FrameworkSurfaceStore`);
  - the macro-only `get_public_api` (`ComponentPublicContract`).

  TIF1 owns collapsing the first and third into the TypeInfo view. The second
  survives only as its TypeInfo root, and its off-store cache is deleted (`D14`).
- Public positions are untagged. NAPI/WASM `getCodeActions` take UTF-16
  offsets, while MCP `QuickFixParams.offset` is a byte offset. Two surfaces
  therefore disagree on the same concept today (`D16`).
- The `verter.enable` client setting is declared but never read, and the
  client sends a `frameworks` init option that the server ignores (`D09`).
- `StoreBackedCarrierRegistry` (`crates/verter_lsp/src/carrier_registry.rs`)
  is reached only from tests. It belongs to the project-bound external-TS
  contract, not to the three categories here, so it is reported and not
  assigned.

## No parallel authority

For each successor identity or service, exactly one row names its
implementation owner (`O01`–`O13`), and every current carrier of that
authority is a displaced route with one deletion owner. No route appears under
two owners, and no retained seam (`S01`–`S13`) duplicates a successor
authority. The seams are:

- the engine crates, kept as kernel services under their existing owners;
- the typed digest identities in `verter_identity`;
- the certified engine binding;
- `CodeTransform` mapping;
- the TypeScript-provider UTF-16 conversion, the one correct boundary
  conversion, verified by ENCT0.

The engine inventory (`S01`–`S06`) keeps the ownership fixed by the
port/test splits: `verter_type_engine`, `verter_session_query` and
`verter_execution` stay parser-free, and `verter_semantic_source` is the only
parser-bearing semantic layer. Changes L4 makes to these owners are reconciled
by the family locks UAI0, UAO0 and UAP0. This decision is not re-dispatched.

## Acceptance evidence

Evidence selection: the change adds contract data only, so existing coverage
and bounded inspection discriminate it. The diff adds no test.

- **AC1 — ownership contract.** The two product files bind every outcome,
  consumer and displaced route to one existing node, a successor path from
  UAK0 and a receiving acceptance ID. The executable validator and its
  negative controls are UAM0's (`UAM0-AC-R1`).
- **AC2 — positive contract.** The current identity, provenance and ordering
  behaviour of the named boundaries is already pinned:
  - `crates/verter_language/tests/cases/` (`parse_identity`,
    `registered_authorities`, `sealed_block_identity`, `diagnostic_ordering`)
    and its compile-fail fixtures;
  - the `crates/verter_identity` compile-fail fixtures (no
    `SourceUnitId::from_canonical`);
  - `typeinfo_proto_ts_contract`, `typeinfo_proto_roundtrip` and
    `typeinfo_graph_export` in `crates/verter_protocol/tests/cases/`;
  - `framework_registry_complete`, `client_framework_manifest_ts_freshness`,
    `virtual_file_naming_ts_freshness`,
    `framework_surface_wire_executor_validates_first` and
    `typeinfo_request_validation` in `crates/verter_session`.

  New tests belong to UAM0.
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no production
  byte changes.
- **AC4 — bounded work: not applicable.** No hot path changes. The baseline
  cells this decision names (`Z01`–`Z06`) are confirmed by PER0E
  (`PER0E-AC1`) against L4's published bench-m3 run summaries.
