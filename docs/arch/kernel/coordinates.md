# UTF-8 internal coordinate constitution

This decision fixes how Verter represents positions. Rust core facts are UTF-8
byte offsets in one named coordinate domain, and a requester's encoding exists
only at the boundary that speaks it. Today, framework-shaped host/session
registries and untagged public boundaries own these concerns. The final and
sole owner is the typed immutable universal catalog and the demand-selected
kernel services.

It describes the repository at `fix(ci): retry marketplace extension installs
in the VS Code E2E (#798)`, 2026-10-09.

Six current wires do not let a consumer identify their unit today (see
[Wires with no identifiable unit today](#wires-with-no-identifiable-unit-today)).
This decision fixes the target contract for each of them and binds each to the
owner that repairs it; it does not repair them.

It follows the docs-only rule in [README.md](README.md): it changes no production route and adds no check. It
builds on the [authority inventory](authority-inventory.md) and the
[constitution](constitution.md) and does not re-own anything they assign. UAK0
routes `D16` (untagged non-editor public positions, ENCF0) and `D17`
(fixed-UTF-16 editor contracts, ENCL0) keep their owners; this decision splits
them into the concrete boundaries below and adds the coordinate routes UAK0 did
not list.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/ENC0/products/`:

| File | Holds |
| ---- | ----- |
| `coordinate-inventory.v1.json` | The law `R01`–`R10`, coordinate domains `CD1`–`CD6`, line-index ownership `LI1`–`LI7`, the re-derived accepted Rev11 owners `T01`–`T06`, outcomes `E-O1`–`E-O7`, consumers `E-C01`–`E-C06` and transferred obligations |
| `boundary-route-ledger.v1.json` | Every position-bearing boundary at the described head (`P01`–`P32`): direction, encoding at the head, whether a consumer can identify it, whether it is tagged, whether the head produces it (`emission`), the intended contract (domain, encoding, base, terminators, refusal), its parent UAK0 route, one owner, successor path and receiving acceptance ID; plus the completeness audit |
| `case-table.v1.json` | The checked-arithmetic, invalid-boundary, overflow, line-terminator, Unicode-property and baseline-cost cases `K01`–`K28` that ENC0T implements |

Every owner exists in the controller-owned plan. Every `successorPath` starts at
ENC0 (or at UAK0 for an inherited route) and follows predecessor edges. The
executable validator that rejects missing members, unknown or pathless owners
and conflicting assignments is acceptance of UAI0; this node ships none.

## The law

- **R01 — one internal unit.** Every position a Rust core fact stores, caches,
  keys or compares is a UTF-8 byte offset (or a half-open byte range) into one
  named coordinate domain. There is no "UTF-8 unless evidence suggests
  otherwise": a value whose domain or unit the code cannot name is a defect.
- **R02 — named domains, no mixing.** The domains are disjoint (see below). An
  offset in one domain never flows into another by arithmetic, comparison or
  assignment; it crosses only through the owner of the map between them.
- **R03 — requester encodings live at the boundary.** UTF-16 code units,
  UTF-32 code points and line/column forms exist only inside the adapter that
  speaks them (LSP, NAPI, WASM, FFI, MCP, CLI, the TypeScript provider wire,
  the V3 source-map format). Each crossing converts exactly once, in the
  adapter, against the exact content snapshot the position indexes.
- **R04 — every wire names its encoding.** A wire position carries its
  encoding and line/column base, either as a tag on the value or as a fixed,
  versioned property of the wire contract (the LSP negotiated
  `positionEncoding`, the V3 source-map UTF-16 column rule, the TypeScript
  UTF-16 position rule). A doc comment alone is not a contract.
- **R05 — no cached mirrors.** No layer retains a copy of a document's
  coordinates re-expressed in a requester encoding (a UTF-16 text copy, or a
  per-version UTF-16 offset table kept to answer editor requests). A boundary
  index built for one content snapshot and dropped with it is not a mirror; it
  holds byte line starts and computes requester columns on demand. The
  TypeScript provider's own UTF-16 line table (`LI2`) is the provider wire's
  index, not a mirror of the internal domain.
- **R06 — exact or refused.** A position that does not land on a valid
  boundary of its domain (inside a UTF-8 sequence, inside a surrogate pair,
  past the end, inverted, overflowing) is a typed failure. Nothing clamps,
  saturates or snaps to the nearest character or token. The only exception is
  a rule the wire itself defines (LSP 3.17: a `character` past the line length
  defaults to the line length), which the boundary adapter applies for
  read-only queries and records as the wire's rule; edits and secondary links
  use the checked form and refuse.
- **R07 — no ASCII shortcut as a contract.** Generated sources (TSX, compiled
  JS, companions) carry authored identifiers and literals, so they are not
  ASCII. An ASCII fast path is an optimisation whose answer equals the general
  path; it is never an assumption that lets byte and UTF-16 units be swapped.
- **R08 — encoding never enters identity.** A requester encoding never enters
  a prepared-artifact key, a semantic-flight key, a cache key or a stored
  fact. Two sessions that negotiated different encodings share every cache
  entry.
- **R09 — map revision is the coordinate basis.** A position mapped between
  domains is valid only against the exact content and map revision it was
  computed on (`ContentId`, `MapRevision`, the `content_hash`/`map_hash` pair
  of the certified engine query). A changed basis rejects the position; it is
  never re-mapped approximately. This is the input PER0D consumes for
  invalidation.
- **R10 — line terminators belong to the wire.** A line/column form is
  defined by the wire that uses it, including its terminator set: LSP 3.17 uses
  `\n`, `\r\n` and lone `\r`; TypeScript positions also break at U+2028 and
  U+2029; the V3 source-map line split is the compiler's emitted `\n`. Two
  line/column forms are never converted into each other directly; both go
  through byte offsets of one content snapshot.

The rules restate and extend the accepted Rev11 TCM law (exact correspondence,
refusal instead of clamping, the content/map basis) to every successor product.
Accepted TCM code needs no migration.

## Coordinate domains

| Domain | Unit and base | Type at the described head | Successor type (ENC0T) | Mapping owner |
| ------ | ------------- | -------------------------- | ---------------------- | ------------- |
| `CD1` source | UTF-8 bytes from the start of an authored file (carrier or plain module) | `Span` (serde, used for authored positions), `SourceByteOffset`/`SourceByteRange`, `verter_language::SourceSpan` (tagged with a `SourceSpaceId`; `SourceEncoding::Utf8` is its only variant) | `SourceByteOffset`/`SourceByteRange` | carrier parser (geometry), TCM1 maps |
| `CD2` source-relative | UTF-8 bytes from a declared base inside one authored file (block content, expression start) | `RelativeSpan`, OXC spans before rebasing | stays `RelativeSpan`; `to_absolute(base)` is the only exit | owning parser/scanner |
| `CD3` generated | UTF-8 bytes from the start of one generated surface (IDE TSX, compiled output, companion) | `GeneratedByteOffset`/`GeneratedByteRange`/`GeneratedByteLen`, `MappingSpan`, `PartialGeneratedSpan` | `GeneratedByteOffset`/`GeneratedByteRange` | `CodeTransform` mapping products (TCM1) |
| `CD4` embedded | UTF-8 bytes into the cooked or derived text of a region whose bytes are not its authored bytes | no cooked literal is positioned at the head; preprocessor output spaces (`SourceSpaceIdentity::DerivedTransformOutput`) carry derived-text offsets as `SourceSpan` under their own `SourceSpaceId` | `EmbeddedByteOffset`/`EmbeddedByteRange` | EMB0 codec map chain (EMB0I) |
| `CD5` boundary line/column | `(line, character)` in a wire's encoding and base | `LspPosition`, `TsPosition`, `codec::LineColumn`, `verter_span::LineCol`, tower-lsp `Position` | not a core type; adapter-local | the boundary owner (`LI1`–`LI7`) |
| `CD6` boundary offset | UTF-16 or UTF-32 offset from file start | `SourceUtf16Offset`, `GeneratedUtf16Offset`, raw `u32` on NAPI/WASM | not a core type; adapter-local | the boundary owner |

Rulings for ENC0T:

- The successor types reuse the names that already exist in `verter_span`
  (`SourceByteOffset`, `SourceByteRange`, `GeneratedByteOffset`,
  `GeneratedByteRange`). ENC0T moves their definitions into `coord/`,
  re-exports them at their current paths, adds `EmbeddedByteOffset` and
  `EmbeddedByteRange`, and adds the checked API. It never creates a second type
  with an existing name.
- Construction from a raw integer is explicit and named; there is no `From`,
  `Into`, `Deref`, or `Add<u32>` between a domain type and an integer, and no
  arithmetic across domains. Range constructors reject `end < start` instead of
  relying on the current `saturating_sub` in `len()`.
- `CD2` source-relative spans stay `RelativeSpan`: they are source bytes with a
  base held in context. An embedded type is only for cooked text, where an
  offset is not a byte of the authored file.
- `Span` remains the serialised authored-source span (rule 1 of the span
  rules in the `position-encoding` skill). `PartialGeneratedSpan` and
  `GeneratedSpan` have no production consumer outside `verter_span` and the
  `verter_parser` re-export; they are recorded (`P31`) and not migrated. `GeneratedByteOffset`,
  `GeneratedByteRange`, `SourceUtf16Offset` and `GeneratedUtf16Offset` also
  have no consumer outside `verter_span` today; `SourceByteOffset`,
  `SourceByteRange` and `GeneratedByteLen` are used by `verter_compiler`.

## Line-index ownership

| Id | Index | Wire it serves | Terminators | Ruling |
| -- | ----- | -------------- | ----------- | ------ |
| `LI1` | `verter_type_runtime::codec` (`SourceIndex` borrowing, `LineIndex` owning, one private core) | LSP client positions in the negotiated encoding; tsgo LSP-wire responses | `\n` only today | **Canonical** byte ↔ requester line/column owner for LSP-shaped wires. `verter_lsp::documents::line_index::LineIndex` is a type adapter over it. ENCL0 owns its LSP 3.17 terminator set (lone `\r`) and the encoding enum it takes. |
| `LI2` | `verter_span::Utf16LineIndex` + `verter_span::tsgo_offset` | TypeScript positions (tsgo `--api`, `verter-tsc`): UTF-16 offsets, 1-based line/column | TypeScript `getLineStarts` set | **Canonical** TypeScript-position owner. Accepted (UAK0 seam `S12`); ENCT0 verifies it. It is not the client-LSP owner, which corrects the ENC0T charter's working assumption. |
| `LI3` | `verter_compiler::code_transform::source_map` + `verter_parser::cursor::PositionResolver` | V3 source maps (UTF-16 columns, 0-based) | `\n` | Accepted TCM1 owner of map emission. Not migrated. |
| `LI4` | `verter_ffi::convert::OffsetIndex` and its scalar helpers | NAPI/WASM/FFI offsets (UTF-16/UTF-32, no lines) | none | Canonical non-editor offset converter. ENCF0 owns its tagging and its char-boundary clamp (`P22`). |
| `LI5` | `verter_compiler` assembly map decoders (`svelte_module::{generated_line_starts, utf16_offset_to_bytes}`, shared by `standalone.rs` and `vue_module.rs`), `generated_chunk::byte_position` and its copy in `assembly/compose.rs`, `map_input::classify_column`; `verter_session::block_content::position_within_utf16_content` | decoding or validating V3 source maps | `\n` | Duplicates of `LI3`. Exact (refuse inside a pair); retained in their crates and recorded (`P30`); ENC1 confirms whether they remain duplicate conversion owners. |
| `LI6` | `packages/language-shared/src/carrier/mapper.ts`, `remap.ts`; `packages/typescript-plugin/src` line tables | TypeScript-side carrier mapping in UTF-16 | `\n` | Rev11 surfaces (`CarrierMapper` is a TCM4 displaced route). Not ENC0's to assign; ENCT0 verifies (`T05`). |
| `LI7` | `packages/vue-vscode/src/utils.ts` `utf16OffsetToPosition`, `css/styleStructure.ts` `utf8OffsetToUtf16` | VS Code client decorations from analysis spans | `\n` | ENCL0 (UAK0 `D17`). |

One index per wire contract. A request builds at most one index per (content
snapshot, wire) and converts every endpoint through it, as the `position-encoding`
skill already requires. A new line/column form needs a new row here, not a
private table.

## Accepted Rev11 owners, re-derived on the described head

"Re-hash the receipt" is done by re-deriving each accepted owner from the
current tree and naming the in-tree evidence that pins it. No digest or commit
identity is recorded.

| Id | Accepted owner | Where it lives now | Pinned by | State |
| -- | -------------- | ------------------ | --------- | ----- |
| `T01` | TCM1 compact mapping products in `CodeTransform` | `crates/verter_compiler/src/code_transform/{mapping_product,source_map,chain,segmented}.rs` | `mapping_product_tests.rs`, `source_map_tests.rs` (UTF-16 column cases), `chain_tests.rs` | present; no migration |
| `T02` | TCM2 content-mapper projection plane | `crates/verter_session/src/content_mapper.rs` (`ContentMapper`, typed `Refusal`, byte `MappingSpan`) | `crates/verter_session/tests/cases/content_mapper_projection.rs` | present; still dormant ("No production route reaches this module") |
| `T03` | TCM3 semantic capability closure | `crates/verter_session/src/semantic_capability.rs`, `external_ts/engine.rs` (`Query { carrier_offset, content_hash, map_hash, required_version }`) | `g_extts/semantic_capability_closure.rs`, `g_extts/certified_engine_seam.rs`, the `uncertified_engine_answer_is_unrepresentable` compile-fail case | present; the query offset is a `CD1` byte offset fenced by content and map hash |
| `T04` | H2 provider-wire conversion | `verter_span::{tsgo_offset, utf16_line_index}`, `verter_tsgo_api/src/offset.rs`, `verter_type_runtime/src/{tsgo,tsserver}/ipc.rs`, `contents_snapshot.rs`, `semantic_tokens.rs` | `utf16_line_index_tests.rs`, the `tsgo`/`tsserver` ipc tests | present; no migration |
| `T05` | TCM4 displaced position routes | `ProviderPositionMapper` (`crates/verter_lsp/src/documents/provider_projection.rs`), `PositionMapper`, TS `CarrierMapper` | `position_mapper_strict.rs` | **still live**: residual, see findings |
| `T06` | Client-LSP conversion | `LI1` | `codec.rs` unit tests | outside TCM; owned by ENCL0 |

## Boundary inventory

The full rows are in `boundary-route-ledger.v1.json`.

| Owner | Rows | What changes |
| ----- | ---- | ------------ |
| ENCL0 (`ENCL0-AC1`) | `P01`–`P09` | Negotiated LSP positions reach the UTF-16 source-map mapper untranslated; the editor encoding enum and UTF-16 literals used for the provider wire; the duplicate `encoded_len` counter; `LI1` lacks lone `\r`; an unknown encoding kind silently becomes UTF-16; the pre-initialize default; analysis spans converted through the FFI index; audit position records that do not carry the session's encoding; VS Code client conversions |
| ENCF0 (`ENCF0-AC1`) | `P10`–`P24`, `P32` | NAPI/WASM/FFI/MCP/CLI/proto positions without a tag; diagnostic and lint spans whose unit depends on source availability; byte and UTF-16 fields mixed in one response; zero offsets when the source is missing; the FFI char-boundary clamp; preprocessor line/column with no stated base or unit; `GraphSpanRef` with no stated unit; `verter-tsc` 1-based columns with an implicit unit; schema DTO spans with no producer |
| ENCT0 (verify only) | `P25`–`P29` | Accepted TCM1/TCM2/TCM3/H2 boundaries and the TCM4 residual; residue reopens its Rev11 owner |
| Retained / recorded | `P30`–`P31` | Duplicate source-map decoders; coordinate types with no production consumer |

Every migrated row (`P01`–`P24`, `P32`) carries an `intendedContract`: the
domain, encoding, line/column base, terminator set and refusal rule its owner
converges on. Every row carries `emission`, which separates positions the head
actually produces or reads from schema fields with no producer, ingress fields
accepted but never read, and retained values never interpreted as positions.

**Completeness audit.** Every public struct or wire message in the production
boundary crates (`verter_protocol` source and proto, `verter_ffi`,
`verter_napi`, `verter_wasm`, `verter_mcp`, `verter_tsc`, `verter_audit`
payloads, the `verter_lsp` custom protocol) with an integer field named
`span_start`/`span_end`, `start`/`end`, `offset`, `line`, `column` or
`character`, and its generated TS binding, maps to exactly one row or to the
ledger's excluded set. The audit added `P32` (the `verter_protocol::schema`
DTO spans, which nothing outside that module constructs) and the FFI
component-meta structs to `P18`.

Already tagged, no shape migration: the carrier-geometry wire `CanonicalRange`
in `component_meta.proto` carries a `PositionEncoding` enum and a source-space
token. Its only producer always writes `UTF8_BYTES`, and no consumer reads the
tag (`P18`).

## The three deletion categories, coordinate scope

- **Untagged coordinate/public identity.** Every row `P01`–`P24` and `P32` names its
  deletion owner: ENCL0 or ENCF0. UAK0 `D16` and `D17` are their parents; no
  row has a second owner.
- **Central framework switch.** No coordinate route branches on a framework.
  The population is empty here; UAK0 `D01`–`D11` keep their owners.
- **Duplicate component information authority.** No coordinate route carries
  component information. The population is empty here; UAK0 `D12`–`D15`,
  `D18` keep their owners.

## Case table for ENC0T

`case-table.v1.json` specifies the cases ENC0T turns into tests
(`ENC0T-AC1`–`ENC0T-AC3`). Groups:

- **Domain mixing** (`K01`–`K03`): cross-domain add/compare/pass and raw-integer
  conversion fail to compile.
- **Checked arithmetic** (`K04`–`K08`): overflow at `u32::MAX`, underflow on
  rebase, inverted range, empty range, rebasing with a base that overflows.
- **Boundaries** (`K09`–`K12`): end-of-file, past-end, inside a 2/3/4-byte
  sequence, empty source.
- **Line terminators** (`K13`–`K18`): `\r\n` as one terminator, a position
  between `\r` and `\n`, lone `\r`, U+2028/U+2029 under LSP versus TypeScript
  rules, terminator at EOF, mixed endings.
- **Unicode** (`K19`–`K25`): astral scalar, an offset inside a surrogate pair,
  combining marks, ZWJ sequences, BOM, non-ASCII generated source, a lone
  surrogate arriving from a JavaScript string.
- **Baseline cost** (`K26`–`K28`): `#[repr(transparent)]` size equality, no
  conversion buffer under a UTF-8 session (UAK0 `Z05`), ASCII fast path equals
  the general path.

Each failure is a typed error (`InvalidBoundary`, `Overflow`,
`NotCharBoundary`); nothing clamps.

## Wires with no identifiable unit today

These rows have `identifiable: false`: a consumer cannot tell the unit from the
wire, its type or a fixed contract. That is an observed defect and a migration
input, not a prerequisite of this decision. This node is docs-only: it fixes
the target contract below, and the named owner repairs the wire under its own
acceptance. Nothing here certifies a wire as fixed.

| Row | Emitted at the head | Observed defect | Intended contract (domain; encoding; base; refusal) | Owner, path, acceptance |
| --- | ------------------- | --------------- | --------------------------------------------------- | ----------------------- |
| `P08` audit `PositionInfo` | produced | `line`/`character` are the negotiated LSP position, but the record does not carry the session's encoding | `CD5` of the audited LSP request; the negotiated encoding recorded on the record as a closed field beside `line`/`character`; 0-based line and character, LSP 3.17 terminators; before negotiation the record carries no position (typed absence), never a UTF-16 default | ENCL0; `UAK0 → UAK1 → ENC0 → ENCL0`; `ENCL0-AC1` |
| `P11` host diagnostic spans | produced | `FfiDiagnostic`/`NapiDiagnostic`/`HostDiagnostic` `spanStart`/`spanEnd` are UTF-16 when the adapter has the source and raw UTF-8 bytes when it does not: `mandatory_utf16_offset[_with]` pass the byte offset through, and `napi_diagnostic_from_host` (so every `compileMany` entry) supplies no source | `CD1` half-open range; UTF-16 code units from file start on NAPI/WASM under a versioned encoding tag, converted once through `LI4` against the exact source the compile read; 0-based; no source is a typed `SourceUnavailable` on that diagnostic, an off-boundary or past-end endpoint is `NotCharBoundary`/`InvalidBoundary`; never a byte pass-through | ENCF0; `UAK0 → UAK1 → ENC0 → ENCF0`; `ENCF0-AC1` |
| `P12` lint spans | produced | `lint_diagnostics_to_utf16` rewrites the byte `Span` in place with UTF-16 values when a source exists and returns bytes otherwise; MCP emits bytes under the same JSON shape | `CD1`; `LintDiagnostic.span` stays UTF-8 bytes in Rust and is never rewritten; NAPI/WASM emit a separate tagged wire span in UTF-16, MCP emits bytes with the same tag field (`P21`); 0-based; missing source is a typed `SourceUnavailable`, endpoints refuse with `NotCharBoundary`/`InvalidBoundary` | ENCF0; `UAK0 → UAK1 → ENC0 → ENCF0`; `ENCF0-AC1` |
| `P19` typeinfo spans | mixed | `SymbolEntryDto` and `FrameworkSurfaceMemberDeclaration` spans are produced as UTF-8 bytes; `GraphDiagnostic` is produced as 0 with `has_span=false`; `GraphSpanRef` has no stated unit — as member/index spans it is a schema field with no producer, and as the `FlowNarrowingRequest`/`ContextualTypeRequest` span it is validated for presence and read by no executor | `CD1` of the file named by `canonical_id`; UTF-8 bytes stated in the typeinfo proto contract under its schema-version rules; 0-based half-open; on ingress an inverted, past-end or off-boundary span is a typed `TypeInfoRequestError` before execution; on egress a span is emitted only when exact, otherwise absent or `has_span=false` | ENCF0; `UAK0 → UAK1 → ENC0 → ENCF0`; `ENCF0-AC1` |
| `P20` preprocessor diagnostics | retained, never interpreted | `line`/`column` arrive from JavaScript with no stated base or column unit; the host retains them and no Rust consumer maps or re-emits the position (`style_diagnostic_of` records it absent) | `CD5` of the preprocessor's own input text; UTF-16 columns fixed by a versioned NAPI/WASM ingress contract; 1-based line and column, terminators `\n`, `\r\n`, lone `\r`; the adapter validates against the input it was given and rejects an out-of-range or in-pair position as typed (the diagnostic keeps its message without a position), never passing it through | ENCF0; `UAK0 → UAK1 → ENC0 → ENCF0`; `ENCF0-AC1` |
| `P32` schema DTO spans | schema field, no producer | `PropDto`, `EventDto`, `SlotDto`, `ModelDto`, `ExposeDto`, `BoundaryIssueDto`, `ProvenanceStepDto`, `BindingRefDto`, `ComponentRefDto` carry `spanStart`/`spanEnd` with no stated unit; nothing outside `verter_protocol::schema` constructs them, and the generated TS mirror is re-exported by `@verter/component-meta` | `CD1`; UTF-8 bytes in the tagged range shape of `P18`, or the unproduced fields are removed when generated consumers migrate; 0-based half-open; `InvalidBoundary` for inverted or past-end ranges | ENCF0; `UAK0 → UAK1 → ENC0 → ENCF0`; `ENCF0-AC1` |

The rest of the inventory has a determinate unit at the head: for example,
NAPI `getCodeActions` takes UTF-16, MCP `QuickFixParams.offset` takes bytes,
and `external_ts::Query.carrier_offset` is a `CD1` byte offset.

## Findings recorded for the receiving owners

- **Negotiated positions meet a UTF-16 mapper (`P01`).** The server prefers
  UTF-8, then UTF-32, then UTF-16. `PositionMapper` and
  `ProviderPositionMapper` work in UTF-16 source-map columns, but
  `carrier_position_to_tsx_offset` passes the negotiated `Position` straight
  in, and several provider contexts build their carrier index with the
  negotiated encoding. A UTF-8 or UTF-32 client therefore maps non-ASCII lines
  to the wrong column. VS Code only offers UTF-16, so it does not see this.
  Owner ENCL0.
- **TCM4 displaced routes are still live (`T05`, `P29`).** TCM4 is recorded as
  implemented and its charter names `ProviderPositionMapper`, `CarrierMapper`
  and the plugin's sole-mapper duty as displaced. They remain the live position
  authority, and `ContentMapper` remains dormant. ENCT0 verifies this and,
  under its charter, any residual reopens the Rev11 owner; ENC0 assigns it to
  no successor.
- **`LI1` breaks only at `\n`.** LSP 3.17 also breaks at a lone `\r`. A file
  with lone-`\r` line endings gets wrong line numbers on every LSP surface,
  and on tsgo LSP-wire responses if tsgo breaks there as TypeScript does
  (ENCT0 checks `K15`/`K16` against `P28`). Owner ENCL0 (its line-index
  subblock).
- **Two encoding enums and a third conversion.** `verter_ffi::convert::OffsetEncoding`
  and `verter_type_runtime::codec::PositionEncoding` both model the same three
  encodings, and `provider_projection.rs` `encoded_len` is a third counter. The
  LSP reaches the FFI index for analysis spans. ENCL0 and ENCF0 each converge
  their side on one enum; ENC1 confirms no duplicate owner remains.
- **The `position-encoding` skill says generated TSX is always ASCII.** That is
  the forbidden assumption (`R07`). Code does not rely on it for correctness at
  the head, but the skill text is wrong; the owner that next edits the TSGO
  section (ENCL0) corrects it.
- **`verter_span::{PartialGeneratedSpan, GeneratedSpan}` have no production
  consumer** and overload `Span` for generated positions. Recorded only.

## Acceptance evidence

Evidence selection: the change adds contract text and data only, so existing
coverage and bounded inspection discriminate it. The diff adds no test.

- **AC1 — ownership contract.** The three product files bind every coordinate
  outcome, consumer and boundary to one owner, a successor path and a
  receiving acceptance ID. UAK0's `D16`/`D17` stay the parents of the split
  rows, so no route has two owners. The executable validator is UAI0's.
  Every integer position in the inventory now has a named domain, encoding,
  base and refusal rule: the six rows with no identifiable unit today (`P08`,
  `P11`, `P12`, `P19`, `P20`, `P32`) have theirs in `intendedContract`, so
  the inventory has no unknown integer position. Making the wires conform is
  acceptance of their owners (`ENCL0-AC1`, `ENCF0-AC1`).
- **AC2 — positive contract.** Of the named boundaries, only `TypeInfoRequest`
  exists at the head and carries positions (the `GraphSpanRef` request spans
  of `P19`); its envelope identity, validation and round-trip are pinned by
  `crates/verter_protocol/tests/cases/typeinfo_proto_roundtrip.rs`,
  `crates/verter_session/tests/cases/g_type/typeinfo_request_validation.rs`
  (including the missing-span rejection), and
  `crates/verter_session/tests/cases/g_block/typeinfo_request_contract_guards.rs`.
  `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`,
  `CatalogSnapshot` and `DemandPlan` are absent at the head and carry no
  coordinate; their owners are assigned in the
  [authority inventory](authority-inventory.md). The coordinate behaviour is
  pinned by: `crates/verter_span/src/utf16_line_index_tests.rs` and the
  `tsgo_offset` unit tests; the `codec.rs` unit tests in `verter_type_runtime`
  (checked versus clamped conversion, surrogate pairs, CRLF);
  `crates/verter_compiler/src/code_transform/{source_map,segmented,mapping_product,chain}_tests.rs`
  (UTF-16 columns, mapping products);
  `crates/verter_session/tests/cases/content_mapper_projection.rs` (typed
  refusal, no clamping); `crates/verter_session/tests/g_extts/{semantic_capability_closure,certified_engine_seam}.rs`
  and the `uncertified_engine_answer_is_unrepresentable` compile-fail case
  (the content/map-hash fenced query offset);
  `crates/verter_lsp/tests/cases/position_mapper_strict.rs`; the UTF-16
  conversion tests in `crates/verter_ffi/src/convert/tests.rs`. New tests
  belong to ENC0T.
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority changes, and no production
  byte changes. `R08` and `R09` state the rule PER0D consumes.
- **AC4 — bounded work: not applicable.** No hot path changes. The UTF-8
  zero-conversion-buffer cell is UAK0 `Z05`, confirmed by PER0E.
