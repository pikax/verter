---
ruling_id: "STEERING-TCM-CONTENT-MAPPERS"
type: "maintainer-directive"
date: "2026-08-22"
date_source: "stated"
binds: ["TCM0", "TCM1", "TCM2", "TCM3", "TCM4"]
source_file: "MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md"
summary: "The maintainer's original steering directive for the TypeScript content-mapper architecture change: requires TCM0 to certify one current TypeScript contract and ship one codec, complete a feature-ownership ledger over every TypeProvider method, name the exact deletion closure (not deferred to TCM4), and benchmark process topologies; mandates TCM1 build compact typed mapping products inside CodeTransform; TCM2 build the content-mapper projection plane and TCM3 the semantic-capability closure, both dormant until TCM4; and TCM4 perform atomic activation and deletion across configured/inferred projects, trust boundaries, and the required conformance/performance/security coverage."
supersedes: []
superseded_by: []
contradicts: []
notes: "Landed as the primary source per its own 'Authority order' section below: where a derived artifact (the DISC amendment, ADR-021, a TCM charter, a TCM0 evidence file) disagrees with this text, this document wins. Copied byte-for-byte from ~/.claude/briefs/rev11/TCM-STEERING.md (1549 lines) on 2026-08-23, with only the in-tree repository wrapper (and this frontmatter) prepended; no line of the maintainer's original text altered."
---

# MAINTAINER STEERING — TypeScript Content-Mappers and Semantic Integration

**Status:** RATIFIED by the maintainer, 2026-08-22. This is the maintainer's original steering
directive that discovered the TypeScript content-mapper architecture change and mandated the
`TCM0-TCM4` amendment train. It is landed here verbatim, as a citable, digest-bound authority
document, because it was previously held only outside the repository
(`~/.claude/briefs/rev11/TCM-STEERING.md`) and referenced informally by
`amendments/DISC-2026-08-22-TYPESCRIPT-CONTENT-MAPPERS-amendment.md`,
`decisions/ADR-021-typescript-content-mapper-dual-plane.md`, `charters/TCM0.md`-`TCM4.md`, and the
`evidence/TCM0/*` artifacts, without itself being a ratified in-tree artifact those documents could
cite by digest.

**Authority order:** this document is the primary source. Where any derived artifact (the DISC
amendment, ADR-021, a TCM charter, a TCM0 evidence file) appears to disagree with the text below,
this document wins and the derived artifact is corrected — see
`MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md` and the TCM1-TCM4 charter rewrites for the
specific reconciliations this landing performs.

**Provenance:** copied byte-for-byte from `~/.claude/briefs/rev11/TCM-STEERING.md` (1549 lines) on
2026-08-23, with only this header prepended. No line of the maintainer's original text below has been
altered.

---

# Rev11 Steering — TypeScript Content-Mappers and Semantic Integration

You are the live program orchestrator for `pikax/verter` Rev11 on `program/architecture-lock`.

You have not previously been steered on this discovery. Treat this document as the complete initial direction.

## Immediate program disposition

TypeScript has introduced content mappers, materially changing the correct architecture for Verter’s TypeScript integration.

Immediately register this as an external architecture discovery with the program’s existing discovery classifications equivalent to:

- `DISC-ARCH`
- `DISC-INVESTIGATE`

Do not interrupt an atomic block that is already executing. Let that block reach its normal acceptance or rejection checkpoint unless it is currently changing the exact TypeScript/provider/mapping authority touched by this discovery and the external ledger permits an amendment window.

After the current legal checkpoint:

1. Amend Rev11 on `program/architecture-lock`.
2. Do not open a competing implementation against `main`.
3. Obtain the candidate base, predecessor relationships, block identifiers, branch names, and worktrees from the authoritative external program ledger.
4. Do not use a previously observed branch SHA, branch-ahead count, or issue comment as execution authority.
5. Do not begin production implementation until the investigation and architecture-lock block described below has been ratified.

Breaking changes are allowed. Preserve performance and correctness rather than retaining obsolete integration paths.

---

## Verified architectural context

The current content-mapper protocol is a **projection callback protocol**. TypeScript calls the mapper through operations equivalent to `initialize`, `openProject`, `transform`, and `closeProject`. It lets TypeScript obtain generated outputs, mappings, mapper diagnostics, directives, watched-file/config identities, and supplemental outputs. It does not provide a reverse semantic-query interface by which Verter can ask TypeScript for hover, completion, rename, checker types, or similar data.

Verter’s current `TypeProvider` surface is much broader than projection transport. It participates in file/project lifecycle and provides completions, completion resolution, hover, diagnostics, definitions, type definitions, references, rename, signature help, code actions, semantic tokens, highlights, inlay hints, and related semantic operations. Deleting this plane without a feature-by-feature replacement would cause capability loss.

The current TypeScript tree also contains a separate native semantic API/session model with snapshots, projects, source files, checker operations, symbols, types, references, completions, and diagnostics. It must be evaluated as the only permitted TypeScript-side semantic-query mechanism for Verter-owned features that still require TypeScript facts. Do not assume the exact published package exposes every repository-main API or that it covers the complete existing `TypeProvider` surface.

There are known recent semantic-API lifecycle and initialization issues, including stale snapshot/source-file behavior and an API-session initialization hang in earlier development builds. Reproduce these against the exact candidate package before certifying it; do not hide them behind the old relay.

`microsoft/typescript-go` is staging history whose development has moved into `microsoft/TypeScript`. Use the current `microsoft/TypeScript` implementation and published package as production authority. Do not ship the old TypeScript-Go protocol as a second runtime profile.

Rev11 intentionally defines `PlacementMap`, `SourceProjectionMap`, `RuntimeSourceMapData`, and `EncodedSourceMap` as distinct products. They may share low-level packed primitives, but they must not be collapsed into a universal mapping object.

The current branch still exposes source projection at parts of the assembly boundary as `Option<String>`, making a typed semantic `SourceProjectionMap` a necessary migration before a correct TypeScript span-map adapter can exist.

Rev11’s current no-compatibility ruling forbids retaining dual production routes, compatibility shims, deferred removal, or old APIs without a surviving owner. The final cutover must not leave the carrier/plugin/relay path as fallback.

Treat `typescript@7.1.0-dev.20260822.1` as the first package candidate to investigate, not as automatically certified merely from its version or publication date.

---

# Required target architecture

The accepted architecture must be **one current TypeScript integration with explicitly separated directional planes**:

```text
                    ONE CERTIFIED TYPESCRIPT CONTRACT
                                  │
             ┌────────────────────┴────────────────────┐
             │                                         │
     PROJECTION PLANE                         SEMANTIC CAPABILITY PLANE
     TypeScript → Verter                      Verter-side feature execution
             │                                         │
     Content-mapper protocol            ┌──────────────┼──────────────┐
             │                          │              │              │
     Verter compiler             TypeScript LSP   Verter + current   Verter
             │                       direct        official TS API    native
     generated output +
     SourceProjectionMap
```

These are complementary capabilities, not old and new integration routes.

## Projection plane

TypeScript calls Verter’s content mapper to obtain:

- generated TypeScript/TSX;
- exact semantic span mappings;
- mapper diagnostics;
- diagnostic directives;
- primary and supplemental outputs;
- configuration and watched-file identities.

This plane replaces generated-companion ingestion, carrier injection, TypeScript plugins used for carrier publication, and the current relay transport.

## Semantic capability plane

Every user-visible feature must be assigned exactly one primary execution model:

```text
TypeScriptLspDirect
VerterWithTypeSemanticOracle
VerterNative
DisabledByExplicitApprovedContract
```

`VerterWithTypeSemanticOracle` may use only the **current official TypeScript semantic API** certified for the selected package.

There is no legal owner named:

```text
LegacyProvider
CarrierProvider
TsserverFallback
RelayFallback
CompatibilityProvider
```

The old provider/query plane may not be deleted until every existing capability has a proven new owner. It also may not survive once every capability row has closed.

## Critical acyclic dependency

The content-mapper callback must never query the TypeScript semantic API or send requests to the TypeScript LSP.

The only legal order is:

```text
TypeScript requests transform
→ Verter compiles and returns generated output plus mappings
→ TypeScript commits or updates its project snapshot
→ Verter may acquire that committed snapshot
→ Verter-owned semantic operations may query it
```

This prevents:

```text
TypeScript
→ content mapper
→ Verter compiler
→ TypeScript semantic API
→ TypeScript project update
→ content mapper
```

Add a discriminating deadlock/reentrancy test proving that this cycle is impossible.

---

# Required amendment train

Create five bounded blocks with fresh, collision-free program IDs chosen from the external ledger.

The following names are logical names, not pre-authorized ledger IDs:

| Logical block | Responsibility |
|---|---|
| **TCM0 — Current TypeScript contract and dual-plane architecture lock** | Exact upstream/package evidence, feature ownership, topology decisions, mapping contract, lifecycle, security, cache, external-source policy, deletion closure, and performance baselines. |
| **TCM1 — Compact mapping products inside `CodeTransform`** | Typed compact `SourceProjectionMap`, static no-projection mode, direct typed composition, lazy encoders, and hot-path proof. |
| **TCM2 — Content-mapper projection plane** | Current mapper executable/package, one codec, project lifecycle, span encoding, directives, diagnostics, supplemental outputs, and conformance. Dormant. |
| **TCM3 — TypeScript semantic capability closure** | Direct-LSP ownership, Verter-native ownership, and the narrow official semantic API adapter required for remaining Verter-owned features. Dormant. |
| **TCM4 — Atomic activation and deletion** | Editor/CLI activation, recommendations and documentation, activation attestation, removal of the old transport/query architecture, and final performance acceptance. |

TCM2 and TCM3 may proceed in parallel only if TCM0 and the external DAG permit it. Both depend on the relevant accepted TCM1 mapping contract. TCM4 depends on all preceding blocks.

TCM2 and TCM3 must be inaccessible from production routing:

- no user-facing experimental feature flag;
- no environment-variable back door;
- no second provider selection;
- no hidden fallback;
- no extension activation before TCM4.

---

# Global architecture locks

## 1. Certify one current contract and ship one codec

Production must ship exactly one current content-mapper codec.

Do not ship:

- both versioned and versionless initialization codecs;
- a `V1 | V2` runtime enum;
- a TypeScript-Go historical protocol;
- semver-selected wire formats;
- a fallback codec;
- a compatibility parser accepting several protocol generations.

Use `microsoft/typescript-go#4712` and its tests/comments as architectural evidence, but use the current `microsoft/TypeScript` package implementation as production authority.

TCM0 must inspect the exact package tarball and binary corresponding to the candidate `typescript@7.1.0-dev.20260822.1` and establish:

- package digest;
- source-commit provenance;
- exact content-mapper request and response shapes;
- exact mapper manifest shape;
- exact configured/inferred project behavior;
- exact semantic API availability;
- exact LSP API-session behavior;
- trust and external-code behavior;
- declaration/build/watch/incremental behavior;
- known defects affecting production use.

Do not assume that a package published after a merged PR contains every repository-main change.

If the candidate package does not contain the required contract or fails correctness probes, select the first later current package that does. Do not restore an older protocol or carrier fallback to support the deficient candidate.

“One current codec” does not require one forever-frozen binary. Multiple TypeScript builds may be certified only when they pass the exact same locked contract and conformance suite.

Unknown future contracts fail closed until Verter is updated. When upstream changes the contract, replace the superseded codec in the same Verter release rather than accumulating protocol generations.

## 2. Version is candidate discovery, not activation authority

Do not implement activation as a plain `semver >= 7.1.0`.

A preview such as `7.1.0-dev.20260822.1` sorts differently from stable `7.1.0`, and a future-looking version string does not prove that the expected content-mapper or semantic API is present.

Use a contract equivalent to:

```text
candidate engine =
    stable TypeScript with base version >= 7.1.0
    OR a recognized TypeScript preview with base version >= 7.1.0

certified engine =
    candidate engine
    AND executable/package identity accepted
    AND current content-mapper conformance probe succeeds
    AND required semantic capability probe succeeds
    AND trust/external-code requirements are satisfied

active project =
    certified engine
    AND mapper configuration is valid
    AND exact mapper activation is attested
    AND required project/session identities agree
```

Semver may decide whether to inspect and recommend. It may not independently select the production route.

There is no legacy route for engines that fail certification. Verter may continue framework functionality that does not require TypeScript, but TypeScript-backed features must be explicitly unavailable rather than silently answered through the old carrier architecture.

## 3. Complete feature-ownership ledger

TCM0 must inventory every method, call site, capability, and background consumer of the current `TypeProvider`.

At minimum cover:

- file and project lifecycle;
- completions;
- completion details and resolution;
- auto-imports;
- hover;
- diagnostics;
- definition;
- type definition;
- implementation;
- references;
- rename and rename preparation;
- signature help;
- code actions;
- formatting;
- semantic tokens;
- document highlights;
- inlay hints;
- call hierarchy;
- code lens;
- folding;
- selection ranges;
- document symbols;
- component surface resolution;
- template expression typing;
- props;
- events;
- slots and snippets;
- directives;
- framework macros;
- background semantic analysis;
- all provider configuration and cache methods;
- all carrier lifecycle methods.

For each row, record:

```text
current implementation
current callers
framework and source region
new primary owner
required TypeScript capability
required mapping class/mask
diagnostic behavior
failure behavior
conformance test
performance cell
old code deleted by TCM4
```

Primary ownership must be one of:

### `TypeScriptLspDirect`

TypeScript answers the editor directly using content mappings.

Requirements:

- Verter emits no duplicate result.
- Verter does not proxy or re-remap the response.
- TypeScript feature masks enable only the ratified source projections.
- Document-wide operations have a single named owner.

### `VerterWithTypeSemanticOracle`

Verter owns the user-visible framework feature but obtains structured compiler facts through the certified official TypeScript API.

Requirements:

- TypeScript direct ownership is excluded for conflicting projections.
- Queries are snapshot-bound.
- Queries are structured and preferably batched.
- No markdown parsing or generic LSP-response reconstruction.
- No private carrier protocol.
- No retained remote compiler handles after snapshot release.

### `VerterNative`

Verter implements the feature without querying TypeScript.

Requirements:

- Conflicting TypeScript projections are disabled where possible.
- Native behavior has its own correctness and performance tests.

### `DisabledByExplicitApprovedContract`

Use only when intentional product removal is explicitly approved through Rev11 governance.

Do not use this category to hide an unimplemented migration.

TCM0 cannot be accepted until every existing `TypeProvider` capability has a complete row.

## 4. Diagnostics require their own ownership model

Do not treat diagnostics as an ordinary single-owner LSP feature.

Distinguish:

- TypeScript compiler diagnostics;
- mapper parse/configuration diagnostics;
- mapper diagnostic directives;
- Verter framework diagnostics;
- duplicate diagnostic classes;
- generated-region diagnostics;
- diagnostics attributable to external source units.

TypeScript span feature masks do not provide a general switch for disabling diagnostics. Define deterministic attribution, suppression, precedence, and deduplication rules.

Generated diagnostics without a valid authored projection must remain visible with an honest generated/mapper attribution. Do not map them to a convenient but false authored position.

## 5. Preserve distinct Rev11 mapping products

Retain distinct identities and owners for:

```text
PlacementMap
SourceProjectionMap
RuntimeSourceMapData
EncodedSourceMap
```

Their responsibilities must remain:

- `PlacementMap`: source-unit placement and composition.
- `SourceProjectionMap`: authored-to-generated semantic/IDE projection.
- TypeScript `SpanMapping`: terminal view of `SourceProjectionMap`.
- Verter IDE mapping: consumer of `SourceProjectionMap`.
- `RuntimeSourceMapData`: runtime/build mapping geometry.
- Runtime V3 source maps: terminal view of `RuntimeSourceMapData`.
- `EncodedSourceMap`: terminal external serialization with separate identity from compiler semantics.

They may share:

- packed offset/range primitives;
- boundary-validation utilities;
- deterministic sorting utilities;
- coordinate-conversion infrastructure.

Do not introduce:

```text
UniversalProjectionMap
OneMapForAllConsumers
MapperOwnedSourceMap
```

Do not derive semantic TypeScript mappings by decoding or interpreting an encoded V3 source map.

## 6. `CodeTransform` remains the byte and geometry authority

Do not introduce a parallel emitter that independently writes generated bytes or owns mapping geometry.

Integrate static mapping modes into the existing `CodeTransform` authority, equivalent to:

```rust
CodeTransform<NoProjection>
CodeTransform<ProjectionRecorder>
```

or another sealed, statically dispatched design that preserves the same properties.

Projection facts must originate from the same write/edit/chunk operations that produce generated bytes.

Forbidden:

- rescanning generated output;
- decoding V3 mappings;
- identifying aliases from generated identifier spelling;
- inferring semantic classes from equal lengths after generation;
- mapper-specific duplicate code generation;
- a second writer that can disagree with `CodeTransform`.

## 7. Zero projection-product work when unrequested

The ordinary no-projection compiler route must have:

- no per-write dynamic `maps_enabled` branch;
- no `SourceProjectionMap` allocation;
- no projection side-table allocation;
- no UTF-16 conversion;
- no TypeScript feature-policy work;
- no V3 serialization;
- no mapper JSON construction;
- no content-mapper process;
- no semantic API process;
- no JSON-RPC or local-pipe traffic.

Normal `CodeTransform` state required for transformations regardless of mapping is not counted as projection overhead.

Prove zero additional projection work through:

- allocation counters;
- compiler invocation counters;
- generated-code or optimized-IR inspection;
- structure-size measurements;
- equivalent-work performance comparisons;
- tests that fail if projection recording is invoked under `NoProjection`.

## 8. Compact typed `SourceProjectionMap`

Use packed contiguous records with sparse side tables.

A provisional shape is:

```rust
struct SourceProjectionSegment {
    generated_start: u32,
    generated_len: u32,
    original_start: u32,
    original_len: u32,
    tag: u32,
}
```

The final shape, tag layout, size budget, and indexing strategy must be selected through TCM1 measurements.

The generic geometric relations are:

```text
ExactCopy
Atom
IdentityAlias
```

The TypeScript terminal adapter maps these to the current TypeScript relations equivalent to `Verbatim`, `Atom`, and `Alias`. Current TypeScript span geometry uses those three relations and signed 32-bit text positions.

There is no fourth `Anchor` relation.

A synthesized definition target is represented as:

```text
relation = Atom
original_len = 0
projection_class = DefinitionAnchor
```

Generated scaffolding normally has no source segment and is represented by an unmapped generated gap.

Required invariants:

- generated spans are ordered;
- generated spans do not overlap;
- overlapping original spans are permitted;
- one original range may project to several generated ranges;
- exact-copy ranges are textually and length identical;
- edit-producing operations require exact length-preserving `ExactCopy`;
- zero-length original ranges require a named semantic class;
- generated gaps are not assigned fake authored ownership;
- all range additions are checked before narrowing.

Do not place any of the following on every segment:

- `String`;
- `Arc`;
- `Vec`;
- `Box`;
- `HashMap`;
- a repeated feature bitset;
- a repeated provenance structure;
- an allocated semantic object.

Use compact sorted side tables for uncommon:

- policy overrides;
- diagnostic directives;
- provenance runs;
- source-unit identities;
- exceptional semantic metadata.

Use local ownership such as `String` or `Box<str>` during one-shot compilation. Introduce `Arc` only at a proven shared publication/cache boundary.

## 9. Producer classes and terminal feature policy

Compiler producers must not emit TypeScript-specific `SpanMapFeature` values.

They emit:

- exact geometry;
- generic relation;
- a compact consumer-neutral `ProjectionClass`;
- rare explicit semantic overrides where relation and class are insufficient.

Possible classes may include concepts equivalent to:

```text
ScriptExpression
ScriptIdentifier
TemplateExpressionRead
TemplateSymbolIdentity
DiagnosticCorrespondence
DefinitionAnchor
NoLanguageFeatures
```

TCM0/TCM1 must ratify the minimal final class set.

The current project/session owns an immutable terminal policy which derives TypeScript feature masks from:

```text
ProjectionClass
× relation
× framework/source region
× primary feature owner
× certified TypeScript capability set
```

Every TypeScript wire span must receive an explicit feature mask. Never omit it and unintentionally receive the upstream default of all features.

The terminal policy identity belongs to terminal serialization, not compiler semantics.

## 10. Correct cache boundaries

Do not claim one impossible global in-memory cache shared by unrelated processes.

Require one cache implementation, identity model, and invalidation law per host process.

Within a process, one prepared-artifact authority owns generated bytes and requested compiler mapping products.

A prepared compiler-artifact key may include:

- exact source/content identity;
- framework and language mode;
- code-generation/compiler options;
- source-unit revisions;
- compiler/product profile;
- semantic projection schema/classification identity;
- compiler ABI/version.

It must not include:

- TypeScript feature-mask policy;
- `projection_policy_id`;
- UTF-8 versus UTF-16;
- TypeScript JSON/wire representation;
- V3 encoding options.

A derived serialization key may include:

- prepared-artifact identity;
- terminal encoder identity;
- terminal projection-policy identity;
- current wire-contract identity;
- requested position encoding.

Changing feature ownership, terminal masks, or position encoding must not recompile the component.

Semantic state is separately snapshot-scoped. Remote TypeScript nodes, symbols, types, source files, or related handles must never survive their snapshot.

Permitted adapter state:

- bounded singleflight;
- bounded project-handle table;
- bounded derived-serialization cache;
- bounded snapshot-local semantic results where measurements justify them.

Forbidden:

- an independently invalidated mapper correctness cache;
- a second generated-code authority;
- a second mapping authority;
- reuse of remote semantic handles across snapshots;
- recompilation caused only by position encoding or terminal feature policy.

## 11. Single-input TypeScript projection

Verter may internally represent several source units. A TypeScript content-mapper transform maps an output back to one exact transform input.

Introduce a validated terminal view equivalent to:

```rust
TypeScriptSingleInputProjection<'a>
```

Its constructor must prove that every serialized original range belongs to the exact input file and revision supplied by TypeScript.

Do not:

- encode source-unit IDs into TypeScript offsets;
- map offsets from an external file onto the component file;
- silently select the first source unit;
- blindly discard foreign-origin spans;
- serialize `PlacementMap` as a TypeScript span map.

TCM0 must explicitly disposition:

- inline `<script>`;
- inline template content;
- inline styles;
- Vue custom blocks;
- Svelte script/template/style regions;
- `<script src>`;
- `<template src>`;
- external styles;
- imported Svelte assets;
- generated supplemental outputs;
- helpers derived from several source units.

For each external unit, select one proven model:

1. TypeScript owns it as a normal project file.
2. It is independently content-mapped under a proven project/context contract.
3. Verter owns its features and diagnostics.
4. The project shape is explicitly unsupported and activation fails closed.

Foreign-origin segments may be excluded from TypeScript output only when the ownership matrix proves that all affected features and diagnostics are correctly owned elsewhere.

## 12. Offset and encoding safety

Use canonical UTF-8 byte offsets internally unless TCM1 evidence identifies a better common internal coordinate system.

Validate:

- `usize` to `u32`;
- internal `u32` limits;
- UTF-8 character boundaries;
- generated and original range ordering;
- additions before narrowing;
- generated non-overlap;
- product-specific original-overlap rules;
- UTF-16 conversion;
- diagnostic-directive ranges;
- supplemental-output ranges;
- TypeScript’s signed `int32` wire range.

Every TypeScript wire offset and end must be in:

```text
0..=i32::MAX
```

Overflow returns a typed mapper error before serialization.

Never wrap, saturate, truncate, or reinterpret an unsigned offset as signed.

Position encoding remains terminal state and never enters the compiler-artifact cache key.

## 13. Distributed lifecycle ownership

Do not create one Rust epoch object that claims ownership over state in the TypeScript process, mapper process, editor extension, and Verter LSP.

Use local lifecycle owners equivalent to:

```text
MapperProcessProjectState
EditorRegistrationState
TypeScriptApiSessionState
VerterSemanticClientState
```

Coordinate them through an immutable serializable capability descriptor containing identities and attestations, but owning no remote resource lifetime.

The descriptor should include:

- certified engine/package/binary identity;
- project/config identity;
- mapper package/config identity;
- mapper activation attestation;
- semantic API capability/session attestation where required;
- source/project generation;
- feature-ownership table identity;
- terminal projection-policy identity;
- trust/external-code state.

Identity or generation disagreement fails closed.

Recovery may restart the same certified mechanism. It must not switch to the carrier route or silently change feature ownership.

---

# TCM0 — Current TypeScript contract and dual-plane architecture lock

TCM0 is read-only with respect to production routing.

It must produce implementation-ready decisions, not a broad research memo.

## Exact upstream/package lock

Inspect:

- `microsoft/typescript-go#4712`;
- `microsoft/TypeScript#63936`;
- `microsoft/TypeScript#63800`;
- all relevant implementation code, tests, follow-up commits, and review comments;
- the exact candidate npm package and shipped binaries;
- current content-mapper implementation;
- current semantic API implementation;
- current TypeScript extension registration path.

Record:

- exact package identity and provenance;
- one current content-mapper wire contract;
- mapper lifecycle;
- position-encoding behavior;
- diagnostic directives;
- mapper diagnostics;
- supplemental-output behavior;
- configured-project behavior;
- inferred-project contribution behavior;
- trust requirements;
- `--runExternalCode` behavior;
- build/watch/incremental behavior;
- declaration and declaration-map behavior;
- process consolidation behavior;
- current failure and cleanup semantics.

## Semantic API certification

Inspect and probe the exact current equivalents of:

- LSP/API-session initialization;
- local pipe/session discovery;
- JavaScript API attachment where applicable;
- snapshot acquisition and update;
- temporary snapshots;
- project lookup;
- source-file lookup;
- `Program` and `TypeChecker` operations;
- bulk symbols/types/references;
- completions;
- diagnostics;
- snapshot disposal and handle invalidation;
- cancellation;
- pipe/process failure;
- workspace trust and external-code behavior.

Reproduce known recent failure modes against the exact candidate package, including:

- stale source-file or snapshot behavior after disposal/update;
- API-session startup or stdio/local-pipe hangs.

If the exact package fails a required correctness probe:

- do not certify it;
- do not add a hidden workaround using the old relay;
- select a fixed current package or keep TCM4 blocked.

Do not assume the semantic API exposes every current `TypeProvider` operation.

## Complete repository closure

Inspect at minimum:

- `crates/verter_type_runtime`;
- the complete `TypeProvider` trait;
- every implementation and adapter of that trait;
- every LSP handler and analysis pipeline that calls it;
- provider hub, provider selection, synchronization, and publication;
- external TypeScript synchronization;
- carrier stores and generated companions;
- `crates/verter_tsgo_api`;
- `@verter/typescript-plugin`;
- VS Code extension activation;
- Native Preview relay and `tsdk` staging;
- compiler mapping producers;
- `CodeTransform`;
- assembly, publication, and composition;
- `PlacementMap`;
- `SourceProjectionMap`;
- `RuntimeSourceMapData`;
- `EncodedSourceMap`;
- CLI/build/watch/declaration flows;
- package manifests, release packaging, tests, documentation, and gates.

Produce an owner/reader/writer/lifetime/identity inventory.

## Feature replacement ledger

Complete the feature-by-feature ownership ledger described above.

TCM0 cannot be accepted with:

- “semantic mechanism TBD”;
- “retain provider temporarily”;
- “use the old provider when required”;
- unclassified `TypeProvider` methods;
- a feature claimed by both TypeScript and Verter;
- an intentional capability removal lacking explicit governance approval.

## Process topology benchmarks

Benchmark serious projection-plane topologies:

1. Native content-mapper executable with in-process Verter compiler/cache.
2. Thin mapper executable backed by a shared native Verter daemon.
3. Node/N-API topology only if it remains competitive after initial evidence.
4. Another topology only when it has a concrete architectural advantage.

Measure:

- cold startup;
- first transform;
- warm transform;
- unchanged transform;
- rapid edit sequence;
- CPU;
- allocations;
- RSS and peak RSS;
- process count;
- IPC and serialization bytes;
- project open/close;
- multi-project consolidation;
- crash isolation;
- cleanup;
- packaging complexity;
- security boundaries.

Benchmark serious semantic-plane topologies:

1. Attachment to the editor-owned TypeScript API session, returning compact structured batches.
2. Direct native/Rust client to the current official API transport.
3. A managed TypeScript semantic process for non-editor hosts.
4. A thin JavaScript bridge only when it provides a measured advantage.

Avoid starting a second TypeScript project graph when an editor-owned current graph can safely and correctly be reused.

Select the non-dominated topology based on evidence, not implementation convenience.

## Multi-source and external-source decision

Produce the complete external-source table required by the single-input rule.

TCM0 is blocked until every supported external-source shape has correct ownership and diagnostics.

## Cache and lifecycle contracts

Lock:

- cache keys;
- invalidation identities;
- per-process ownership;
- singleflight boundaries;
- remote snapshot lifetimes;
- project-handle lifetimes;
- control-plane attestations;
- restart behavior;
- bounded state;
- cleanup requirements.

## Deletion closure

Name every old mechanism that TCM4 must delete and every generic facility that survives with a proven owner.

Do not defer this inventory to TCM4.

## Performance baselines

Lock equivalent-work baselines for:

- direct no-projection compilation;
- projection-enabled compilation;
- mapper cold transform;
- mapper warm transform;
- TypeScript project open;
- direct TypeScript LSP features;
- semantic snapshot update;
- semantic batch queries;
- edit-to-hover;
- edit-to-completion;
- edit-to-definition;
- edit-to-diagnostic;
- build;
- incremental build;
- watch;
- declaration and declaration-map emit;
- CPU;
- allocations;
- RSS and peak RSS;
- process count;
- IPC bytes;
- retained state after close.

Do not choose acceptance thresholds after viewing implementation results.

## TCM0 required artifacts

Produce:

- architecture amendment ADR;
- current-upstream contract lock;
- amended program/DAG rows;
- TCM1–TCM4 charters;
- feature-ownership ledger;
- diagnostic-ownership matrix;
- projection-class contract;
- distributed-lifecycle contract;
- cache/invalidation contract;
- external-source decision table;
- process-topology evidence;
- exact deletion matrix;
- conformance fixture plan;
- performance baseline/gate plan;
- abort and rescope conditions.

---

# TCM1 — Compact mapping products inside `CodeTransform`

TCM1 must:

- preserve `PlacementMap`, `SourceProjectionMap`, `RuntimeSourceMapData`, and `EncodedSourceMap` as distinct types and identities;
- replace string-encoded semantic projection ownership with a compact typed `SourceProjectionMap`;
- integrate projection recording into `CodeTransform`;
- provide statically eliminated no-projection operation;
- record projection facts in the same operations that write or mutate bytes;
- compose typed semantic projections directly;
- stop decoding and re-encoding V3 data for semantic composition;
- retain independent runtime source-map production;
- implement the final generic relation and projection-class model;
- preserve generated gaps;
- support one-to-many projections;
- support permitted overlapping original ranges;
- implement zero-length `Atom` definition anchors;
- provide lazy Verter/internal and TypeScript terminal projection views;
- keep runtime V3 encoding on the runtime mapping product;
- preserve atomic publication of bytes and required mapping products;
- validate UTF boundaries, integer limits, and product invariants;
- add property, differential, mutation, composition, and concurrency tests;
- prove that no second semantic geometry authority remains;
- prove zero additional projection-product allocation/work when unrequested.

TCM1 must not contain:

- a content-mapper process;
- TypeScript JSON-RPC types in compiler core;
- a TypeScript semantic API client;
- TypeScript feature-mask constants in framework code generation;
- product activation.

---

# TCM2 — Content-mapper projection plane

Create a separately versioned package, provisionally:

```text
@verter/typescript-content-mapper
```

Do not overload `@verter/typescript-plugin`.

Use the one exact current codec selected by TCM0.

The mapper must:

- use the measured topology;
- speak the current JSON-RPC protocol over standard input/output;
- write only protocol frames to stdout;
- send logs and telemetry only through a safe non-protocol channel;
- call the canonical Verter compiler/cache implementation for its process;
- never call TypeScript semantic APIs;
- never send TypeScript LSP requests;
- isolate state using TypeScript’s opaque project handle;
- serve several project handles in arbitrary order;
- release project state on `closeProject`;
- enforce bounded message sizes, queues, outstanding work, handles, and caches;
- support cancellation where the current protocol permits it;
- implement stable configuration and watched-file identities;
- support primary and supplemental outputs;
- validate `TypeScriptSingleInputProjection`;
- emit explicit TypeScript feature masks;
- implement mapper diagnostics and diagnostic directives;
- support the certified position encoding;
- validate signed `int32` bounds before serialization;
- preserve exact project/compiler options supplied through the current contract;
- conform to project references, monorepos, build, watch, incremental operation, declarations, and declaration maps;
- avoid treating TypeScript-created virtual filenames as stable Verter identities;
- expose complete timing, cache, allocation, IPC, memory, and lifecycle telemetry.

The mapper must be pure with respect to TypeScript semantic state.

Add a hard test that fails if any transform path can call:

- `TypeSemanticOracle`;
- TypeScript LSP;
- a TypeScript API-session bridge;
- a TypeScript project-snapshot wait;
- the old provider/relay.

TCM2 remains unregistered and unreachable until TCM4.

---

# TCM3 — TypeScript semantic capability closure

TCM3 implements the feature-ownership decisions ratified by TCM0.

The preferred order is:

1. Let TypeScript LSP directly own a feature when content mappings can express it correctly and no richer Verter ownership is required.
2. Implement the feature in Verter when framework-native analysis is authoritative.
3. Use a narrow official TypeScript semantic API only for Verter-owned features that truly require checker/project facts.

Do not route all existing `TypeProvider` methods through a new IPC layer merely to preserve its old shape.

## Narrow semantic oracle

When required, replace the broad LSP-shaped provider abstraction with a narrow, snapshot-bound API equivalent to:

```rust
trait TypeSemanticOracle: Send + Sync {
    fn acquire_snapshot(
        &self,
        project: ProjectIdentity,
        expected_generation: ProjectGeneration,
    ) -> SemanticFuture<SemanticSnapshot>;

    fn query_batch(
        &self,
        snapshot: SemanticSnapshotId,
        batch: SemanticQueryBatch,
    ) -> SemanticFuture<SemanticQueryResults>;

    fn release_snapshot(
        &self,
        snapshot: SemanticSnapshotId,
    ) -> SemanticFuture<()>;
}
```

The final interface may differ, but it must remain:

- structured;
- snapshot-scoped;
- batch-oriented;
- cancellation-aware;
- generation-validated;
- bounded;
- independent from mapper-process state;
- incapable of retaining remote compiler objects after snapshot release.

Prefer bulk operations such as:

- symbols at several positions;
- types at several positions;
- reference sets;
- component/member surface extraction;
- batched checker facts needed for one template operation.

Do not mirror TypeScript’s entire remote object graph into Verter unless the TCM0 topology benchmark and maintainability review prove that to be superior.

Do not create another generic LSP relay.

For editor-attached operation, use the exact certified TypeScript API-session mechanism associated with the active TypeScript project rather than creating a second project graph.

For non-editor operation, use the same certified TypeScript engine contract and a topology selected by TCM0.

## Snapshot correctness

Require:

- exact project/session identity;
- exact source/project generation;
- immutable snapshot scope;
- explicit snapshot release;
- no reuse of remote handles;
- stale-response rejection;
- cancellation under rapid edits;
- bounded concurrent queries;
- honest failure states;
- deterministic recovery using the same selected mechanism.

A semantic-plane failure may leave independently owned direct TypeScript LSP features operational. It must not fabricate empty successful results for Verter features that require the failed semantic capability.

## Capability closure

For every old `TypeProvider` capability, tests must prove one of:

- TypeScript directly provides the correct editor result.
- Verter provides it natively.
- Verter provides it using the certified semantic oracle.
- Its intentional removal has explicit governance approval.

If the official current TypeScript API lacks a required capability, the legal outcomes are:

1. Reassign the feature to direct TypeScript LSP ownership.
2. Implement it natively in Verter.
3. Require and certify an upstream API addition/fix.
4. Keep TCM4 blocked.

Do not retain a private legacy query protocol.

TCM3 remains unregistered and unreachable until TCM4.

---

# TCM4 — Atomic activation and deletion

TCM4 activates the accepted projection and semantic capabilities together and deletes the superseded architecture in the same accepted transition.

There is no intermediate production state in which both paths operate.

## Configured projects

Configured projects must use their `tsconfig`-declared `contentMappers`.

Extension registration may assist discovery but must not pretend to inject mapper configuration into a configured project.

Before publishing TypeScript-backed Verter capabilities, attest that:

- the exact certified TypeScript engine is active;
- the expected project/config is active;
- the Verter mapper is loaded;
- the mapper configuration identity matches;
- the content-mapped source is present in the current TypeScript project;
- required semantic API capabilities are attached where needed;
- project/session generations agree.

When the mapper is absent, use an explicit state equivalent to:

```text
NeedsMapperConfiguration
```

In that state:

- do not run the old carrier path;
- do not publish Verter features requiring unattested TypeScript data;
- retain only independently sound framework-native features;
- issue one actionable project-scoped recommendation.

## Inferred projects

Use the current inferred-project contribution mechanism.

Resolution order should be:

1. Trusted compatible workspace-installed mapper.
2. Exact extension-bundled mapper.

Record the selected package, path, manifest, and contract identity.

## Trust and external code

Respect:

- workspace trust;
- TypeScript external-code execution requirements;
- package execution boundaries;
- local-pipe permissions;
- mapper executable resolution;
- project configuration ownership.

Do not bypass a TypeScript trust refusal through a private Verter channel.

Do not automatically enable arbitrary third-party external code.

## Recommendation and documentation

Reserve and publish the canonical documentation location:

`https://verterjs.dev/typescript/content-mappers`

Document:

- certified TypeScript versions/builds;
- content-mapper installation;
- configured versus inferred projects;
- trusted workspaces;
- external-code requirements;
- Vue projects;
- Svelte projects;
- mixed projects;
- monorepos and project references;
- mapper options;
- external source limitations;
- semantic feature ownership;
- build/watch/declaration behavior;
- conflicting content mappers;
- diagnostics;
- troubleshooting and trace collection;
- migration from older Verter releases.

Canonical configuration shape:

```json
{
  "contentMappers": [
    {
      "package": "@verter/typescript-content-mapper",
      "extensions": [".vue", ".svelte"]
    }
  ]
}
```

Use only the extensions actually owned by the project.

When a candidate TypeScript 7.1+ configured project lacks the mapper, issue one deduplicated recommendation keyed by:

```text
project configuration identity
+ certified engine identity
+ mapper configuration identity
```

The recommendation should communicate:

> This TypeScript version supports Verter’s current content-mapper integration, but this configured project has not declared the Verter content mapper. Add the mapper to enable TypeScript project integration and TypeScript-backed Verter features.

Provide actions equivalent to:

- Open Verter content-mapper documentation
- Copy configuration
- Apply reviewed JSONC edit
- Dismiss for this configuration identity

Never mutate `tsconfig` silently.

A JSONC edit must:

- preserve comments;
- preserve formatting;
- preserve `extends`;
- preserve existing mapper entries;
- avoid duplicates;
- refuse overlapping `.vue` or `.svelte` ownership by another mapper;
- show the exact edit before applying it.

## Required deletion

TCM0 must determine the exact closure, but TCM4 must delete, where applicable:

- `@verter/typescript-plugin`;
- carrier injection into TypeScript;
- carrier-only generated-file stores;
- carrier-only external synchronization;
- provider-only `.verter.ts` import projection;
- Native Preview relay interception;
- temporary global `tsdk` staging;
- relay advertisement and carrier attestation;
- relay taint filtering and synthesized neutral responses;
- duplicate generated/provider/original TypeScript remapping;
- duplicate companion compilation used only for TypeScript ingestion;
- the old TypeScript version-selection policy;
- old tsserver and TSGO carrier providers;
- private TypeScript semantic-query protocols;
- carrier lifecycle methods on `TypeProvider`;
- the broad `TypeProvider` abstraction when no surviving caller requires it;
- old APIs and DTOs whose only owner was the removed route;
- historical content-mapper codecs;
- compatibility feature flags and fallback branches.

Do not delete a neutral compiler/query facility that has a demonstrated surviving owner. TCM0 must distinguish shared substrate from transport-specific machinery.

Do not delete the old query plane before its capability ledger is green.

Do not retain the old query plane after its capability ledger is green.

Activation and deletion are one atomic accepted transition.

---

# Required conformance coverage

At minimum test:

- Vue and Svelte;
- JavaScript, JSX, TypeScript, and TSX script modes;
- configured and inferred projects;
- trusted and untrusted workspaces;
- missing mapper;
- malformed mapper;
- duplicate mapper;
- unavailable mapper executable;
- conflicting extension ownership;
- candidate, certified, uncertified, malformed, and future TypeScript versions;
- multiple TypeScript installations in one monorepo;
- project references;
- solution builds;
- multi-root workspaces;
- symlinks;
- realpath/case behavior;
- import resolution;
- auto-imports;
- primary and supplemental outputs;
- one mapper process serving several projects;
- project open and close;
- dynamic configuration;
- watched-file invalidation;
- build;
- incremental build;
- watch;
- declaration emit;
- declaration maps;
- mapper diagnostics;
- mapper option diagnostics;
- diagnostic directives;
- unused expected-diagnostic directives;
- generated-gap diagnostics;
- every supported TypeScript feature mask;
- explicit no-feature masks;
- exact-copy edit permission;
- edit rejection through `Atom` and `IdentityAlias`;
- one-to-many mappings;
- overlapping original mappings;
- zero-length definition anchors;
- UTF-8;
- UTF-16;
- non-BMP characters;
- combining characters;
- CRLF and LF;
- signed `int32` limits;
- rapid edit/config/restart races;
- snapshot acquisition and disposal;
- stale semantic-handle rejection;
- mapper crash;
- semantic API crash;
- cancellation;
- cleanup after project close;
- cleanup after process shutdown;
- no duplicate TypeScript/Verter result;
- no cross-plane callback;
- external source-unit handling;
- framework feature parity with the current accepted surface.

Add differential/property tests showing that the same typed compiler facts produce correct:

- Verter IDE semantic projections;
- TypeScript span mappings;
- runtime mappings from their separate runtime product;
- encoded terminal maps.

Do not create independent expected geometry implementations that can agree with the same bug.

---

# Performance and memory acceptance

Use the existing Rev11 equivalent-work methodology and locked baseline policy.

Measure at minimum:

- direct no-projection compiler path;
- projection-enabled compiler path;
- `CodeTransform` allocation counts;
- compact segment bytes per generated kilobyte;
- mapper process startup;
- first transform;
- warm transform;
- unchanged transform;
- rapid incremental transform;
- project discovery/open;
- TypeScript edit-to-diagnostic;
- edit-to-hover;
- edit-to-completion;
- edit-to-definition;
- semantic snapshot update;
- semantic batch-query latency;
- build;
- incremental build;
- watch;
- declaration emit;
- declaration-map emit;
- CPU;
- allocations;
- RSS and peak RSS;
- process count;
- JSON/IPC bytes;
- cache and singleflight hit rates;
- project-handle retention;
- snapshot retention;
- derived-cache retention after pressure and close.

Acceptance requires:

- no measurable regression in direct no-projection compilation;
- zero projection-product allocations in the no-projection path;
- no compiler invocation caused only by terminal feature policy or position encoding;
- no duplicate TypeScript compilation;
- no unbounded state;
- no hidden second project graph in editor-attached operation unless TCM0 explicitly proves it is necessary and superior;
- removed relay/plugin/carrier work is absent, not merely bypassed after initialization;
- no weakening of existing performance or correctness gates.

A performance miss blocks acceptance. Do not waive it because the upstream API is new.

---

# Security requirements

Review:

- workspace trust;
- `--runExternalCode`;
- executable/package resolution;
- package substitution;
- local pipe permissions;
- cross-workspace process reuse;
- JSON-RPC message bounds;
- malformed input;
- project-handle isolation;
- path traversal;
- symlink behavior;
- cancellation and resource exhaustion;
- diagnostic text size;
- supplemental-output count and size;
- process crash containment;
- stdout protocol integrity;
- log redaction;
- mapper and semantic-session identity attestation.

Do not trade away TypeScript’s trust model to simplify activation.

---

# Independent reviews

Require separate independent reviews for:

1. Current TypeScript content-mapper contract.
2. Current TypeScript semantic API contract and known lifecycle defects.
3. Feature ownership and complete capability parity.
4. Compact mapping products and `CodeTransform` hot path.
5. External/multi-source correctness.
6. Distributed project, snapshot, and process lifecycle.
7. Trust, external-code, executable, and local-IPC security.
8. Cache identities and invalidation.
9. Unicode, signed-offset, diagnostic, and edit safety.
10. End-to-end performance and memory.
11. TCM4 deletion closure and absence of legacy production behavior.

The implementor of a block must not be the sole acceptance authority for its critical invariants.

---

# Acceptance invariants

The amendment is not complete unless evidence proves:

- every current `TypeProvider` capability has a ratified new owner;
- no capability silently disappears;
- no feature has two primary owners;
- no duplicate TypeScript and Verter result is emitted;
- diagnostics have deterministic attribution and deduplication;
- content-mapper transforms never call back into TypeScript;
- no old plugin, carrier, or relay starts;
- no `.verter.ts` transport rewrite remains;
- no global `tsdk` mutation remains;
- no private legacy TypeScript query protocol remains;
- no historical content-mapper codec remains;
- no silent fallback exists;
- no mapper failure changes the selected architecture;
- no stale semantic snapshot or remote handle is used;
- no object pretends to own lifetimes in another process;
- no foreign source range is serialized as the primary TypeScript input;
- no signed-offset truncation occurs;
- position encoding does not cause recompilation;
- terminal projection policy does not cause recompilation;
- no second mapping authority survives;
- no second correctness-cache authority survives;
- no projection-product work occurs when unrequested;
- project, snapshot, singleflight, and derived-cache state are bounded and released;
- direct compilation, editor interaction, build, watch, declarations, memory, and startup meet locked gates;
- old transport/query code is absent from the accepted final tree unless a surviving non-TypeScript owner is explicitly proven.

---

# Abort or rescope conditions

Stop acceptance and rescope if:

- content mapping is treated as semantic querying;
- `TypeProvider` is deleted before its capability ledger closes;
- the old provider remains after its capability ledger closes;
- a private carrier/query protocol remains;
- a required feature has no direct-LSP, semantic-oracle, native, or approved-disabled owner;
- the selected TypeScript package fails required semantic API correctness;
- generated output must be rescanned to reconstruct semantic mappings;
- semantic mappings are reconstructed from V3 JSON;
- mapping products are collapsed into a universal map;
- `CodeTransform` ceases to be the byte/geometry authority;
- TypeScript terminal policy enters the compiler cache key;
- position encoding enters the compiler cache key;
- a mapper transform can re-enter TypeScript;
- normal Verter compilation crosses JSON-RPC;
- a multi-source projection is falsely encoded as single-source;
- remote compiler handles outlive their snapshot;
- old and new routes coexist;
- deletion is deferred to another release;
- correctness, memory, or performance gates would need weakening.

---

# Required orchestrator response

Do not respond merely with “discovery recorded” or begin coding immediately.

Return:

1. The discovery records and their exact program disposition.
2. Whether the currently executing block is affected and why it can legally continue or must enter an amendment window.
3. The accepted checkpoint from which this amendment train will branch.
4. Fresh block IDs and exact DAG/predecessor relationships for TCM0–TCM4.
5. Complete block charters with scope, non-scope, owners, reviewers, inputs, outputs, gates, and abort conditions.
6. The architecture ADR plan.
7. The feature-ownership-ledger plan.
8. The exact upstream/package evidence plan.
9. The mapping-product migration plan.
10. The semantic API certification plan.
11. The external-source decision plan.
12. The topology and performance benchmark plan.
13. The cache/lifecycle/security contracts to be ratified.
14. The exact deletion-closure process.
15. The next legal program action.

The target end state is:

> One certified current TypeScript contract; one current content-mapper codec; one TypeScript-to-Verter projection plane; direct TypeScript LSP ownership where correct; a narrow official semantic API plane only where Verter-owned framework features require it; Verter-native ownership elsewhere; distinct compact Rev11 mapping products; projection recording inside `CodeTransform`; zero projection-product work when unrequested; one coherent cache and invalidation model per host process; and no carrier, plugin, relay, private query protocol, compatibility codec, or capability regression.