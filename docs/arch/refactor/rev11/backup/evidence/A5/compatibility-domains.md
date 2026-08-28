# A5 — Versions, compatibility domains, and cache dimensions

Classifies every version-like value and every cache-identity dimension in the tree against
[`ADR-002 — Compatibility Domains Are Explicit and Monotonic`](../../decisions/ADR-002-compatibility-domains.md)
and [`contracts/identity-encoding.md`](../../contracts/identity-encoding.md).

ADR-002's test for each value: *is there a real compatibility domain?* If yes it has exactly one
owner and a monotonic epoch. If no, it is an ordinary versionless in-memory value and the counter
should not exist.

---

## 1. Real compatibility domains (a boundary exists; the value is load-bearing)

| domain | owner (source) | current epoch | boundary it protects |
|---|---|---|---|
| TypeInfo graph wire | `crates/verter_protocol/src/typeinfo/graph.rs` — `TYPEINFO_GRAPH_SCHEMA_VERSION` | 7 | the closed-enum protobuf graph contract; off-tree clients |
| component-meta payload | `crates/verter_protocol/src/component_meta.rs` — `COMPONENT_META_SCHEMA_VERSION` | 10 | the published `@verter/component-meta` payload |
| tsgo control protocol | `crates/verter_tsgo_api/src/control/messages.rs` — `PROTOCOL_VERSION` | 2 | the out-of-process tsgo control channel |
| tsgo advertisement | `crates/verter_tsgo_api/src/control/advertisement.rs` — `ADVERTISEMENT_VERSION` | 1 | capability advertisement handshake |
| editor/tsserver attestation | `crates/verter_lsp/src/editor_tsserver.rs` — `EDITOR_TSSERVER_ATTESTATION_VERSION` | 1 | the editor-neutral attestation record |
| Svelte conformance manifest | `crates/verter_svelte_conformance/src/manifest.rs` — `SCHEMA_VERSION` | 4 | a committed on-disk manifest |
| analysis-projects config | `crates/verter_analysis_inputs/src/config.rs` — `ANALYSIS_PROJECTS_SCHEMA` (`"verter.analysis-projects.v1"`) | v1 | a user-authored config file; **namespaced string, not an integer epoch** |

The last row is the only value in the tree that follows ADR-002's *namespace* form (an
incompatible clean replacement creating a new domain) rather than the epoch form. It is
consistent with the ADR; noted because a later block converging "all versions to integers" would
regress it.

## 2. Disposable-cache invalidation namespaces (not compatibility domains)

ADR-002: "disposable private caches may be invalidated by a new namespace/build fingerprint".
These are correctly *not* compatibility epochs even though they are spelled like versions.

| value | owner | current | note |
|---|---|---|---|
| `CACHE_CLUSTER_SCHEMA_VERSION` | `crates/verter_session/src/cache_schema.rs` | 10 | invalidates the project-global cache cluster |
| `CURRENT_PARSER_VERSION` | `crates/verter_session/src/file_artifact_store.rs:312` | 5 | keyed into `FileArtifactKey` |
| `LEGACY_PARSER_VERSION` | `crates/verter_session/src/file_artifact_store.rs:338` | 6 | the canonical-keyed surface's independent sequence |
| `ROUTE_DB_RESOLVER_VERSION` | `crates/verter_session/src/resolver_core/route_db.rs` | 2 | route-cache invalidation |
| `RESOLVED_IMPORT_FACTS_RESOLVER_VERSION` | `crates/verter_session/src/resolved_import_facts.rs` | 1 | import-facts invalidation |
| `VUE_CARRIER_PARSER_VERSION` / `SVELTE_CARRIER_PARSER_VERSION` | `crates/verter_compiler/src/framework_common/vue_bridge.rs`, `svelte/carrier.rs` | 6 / 2 | per-carrier parse artifact identity |
| `CARRIER_*_SCHEMA_VERSION`, `FRAMEWORK_PARSE_ARTIFACT_SCHEMA_VERSION` | `crates/verter_language/src/carrier_versions.rs`, `carrier_grammar.rs` | typed newtypes | carrier artifact identity |
| `RUNE_AMBIENT_PRELUDE_VERSION` | `crates/verter_compiler/src/svelte/ide/prelude.rs` | 1 | generated prelude identity |
| `framework_facts::svelte::VERSION` | `crates/verter_semantic/src/analysis/framework_facts/svelte.rs` | 12 | script-fact capture identity |

### Finding D-1 — `CURRENT_PARSER_VERSION = 5` and `LEGACY_PARSER_VERSION = 6`

Two independent monotonic sequences whose names imply one ordering and whose values invert it: the
"legacy" number is *higher* than the "current" number. Reading the source confirms they are
genuinely independent (one keys `FileArtifactKey`, the other keys the canonical-keyed surface that
builds its key inline), so the values are correct and the **names** are the defect. ADR-002's
"duplicate counters that must stay equal are collapsed or separated into genuinely independent
domains" is satisfied on substance and violated on legibility. Disposition: rename when `B2` or
`G4` touches the artifact key — no behavioural change, so this is not a blocker for any block.

## 3. Not compatibility domains at all

| value | why not |
|---|---|
| workspace `version = "0.0.1-beta.3"` (`Cargo.toml`), all 16 published npm package versions | ADR-002: "package semver … not compatibility epochs" |
| `BUNDLED_TSGO_VERSION` (`TsgoVersion::new(7, 0, 2)`) | ADR-002: "external tool versions are not compatibility epochs" |
| `host_manage.rs` `SCHEMA = 1` | module-local constant, no boundary |

## 4. The consumer-compatibility manifest, and a contradiction with ADR-002

`crates/verter_protocol/src/consumer_compatibility_manifest.json` assembles ten version fields
plus a generated-binding hash:

```json
{ "block_content_artifact_schema_version": 1, "qualified_source_map_schema_version": 1,
  "cache_cluster_schema_version": 10, "component_meta_schema_version": 10,
  "structure_protocol_version": 1, "provider_protocol_version": 12,
  "napi_schema_version": 1, "wasm_schema_version": 1,
  "native_api_version": 1, "unplugin_api_version": 1,
  "generated_binding_manifest_hash": "sha256:d0884f30…" }
```

Its own doc comment is correct about ownership — "each nominal field remains owned and bumped by
its downstream surface; this module only assembles the closed generated row" — so it is an
*assembly*, not an eleventh owner. Two findings:

### Finding D-2 — `nonzero_version!` forbids epoch zero, contradicting ADR-002

`consumer_compatibility_manifest.rs:9-29` generates every version newtype through a
`nonzero_version!` macro whose `new` returns `None` for `0`. ADR-002 states the opposite as a
decision: **"zero is a valid first epoch and never an uninitialized sentinel"**, and "an
incompatible clean replacement creates a new domain/namespace **whose first epoch may be zero**".

So the current type system makes the ADR's prescribed clean-replacement move unrepresentable in
this manifest. This is a live constraint on `E3` and on any block that creates a new
compatibility domain: it will either bump from 1, or change this macro. A5 records it rather than
resolving it — the fix is a source change with a public-boundary consequence, which belongs to the
block that first needs epoch zero.

### Finding D-3 — `provider_protocol_version = 12` is hand-pinned in the assembler, not sourced from a provider owner

The producer is in the assembler itself:

```rust
// crates/verter_protocol/src/consumer_compatibility_manifest.rs:75
const PROVIDER_PROTOCOL_VERSION: ProviderProtocolVersion = ProviderProtocolVersion(12);
```

consumed at `:109` by `current_consumer_compatibility_manifest()`. The committed
`consumer_compatibility_manifest.json` is a **generated mirror** of that function, byte-pinned by
`generated_consumer_manifest_is_fresh` (`:124`) and value-pinned by
`public_hash_grammar_and_version_domains_are_closed` (`:134`, which asserts
`manifest.provider_protocol_version.get() == 12` at `:141`). The JSON is therefore not an
independent source, and there is no "missing producer".

The finding is the *shape* of that producer, not its absence. Of the ten version fields the module
assembles, exactly one is sourced from the surface that owns it —

```rust
// :105-108
component_meta_schema_version: ComponentMetaSchemaVersion::new(
    crate::component_meta::COMPONENT_META_SCHEMA_VERSION,
)
```

— and the other nine, `PROVIDER_PROTOCOL_VERSION` among them, are literals hand-maintained at
`:69-78`. The module's own doc comment claims "each nominal field remains owned and bumped by its
downstream surface"; for nine of ten fields that ownership is a convention held by review, not a
reference the compiler checks. So the assembler's `12` cannot drift *from the JSON* (two tests
prevent that), but it can drift from whatever provider surface it is supposed to track, silently.

The two open questions this leaves, both `H2`'s:

1. **Does `12` duplicate a compatibility domain owned elsewhere?** The provider-shaped constants
   that exist under `crates/verter_tsgo_api/src` are `control::messages::PROTOCOL_VERSION` and
   `ADVERTISEMENT_VERSION`, neither at `12`. If `provider_protocol_version` is meant to track a
   provider surface's own epoch, ADR-002's "duplicate counter that must stay equal" prohibition
   applies and the field should read from that owner. If it is a *distinct* domain — the
   consumer-visible provider contract, versioned independently of any tsgo wire — then it is an
   owner in its own right and the module's doc comment is wrong about it.
2. **Why is it hand-pinned when `component_meta_schema_version` is not?** The sourced form exists
   in the same function, three lines away. Whichever answer question 1 takes, the asymmetry is an
   unrecorded decision.

A5 does not answer either; both are provider-compatibility questions, and `H2` (project-scoped
provider bindings) cannot ratify a provider compatibility story without resolving them.

---

## 5. Cache-identity dimensions (a different axis, deliberately separated)

These are not compatibility domains — nothing serialized crosses a boundary — but they *are* the
tree's identity/domain classification and A5 must record them so no later block collapses them.
Producer: `crates/verter_workspace/src/env_hash.rs`, the single place a dimension may be defined.
Each dimension is domain-separated by per-dimension salt bytes so five hashes derived from one
baseline cannot collide (`identity-encoding.md` §1's domain-separation requirement, satisfied).

| dimension | inputs it folds (read from source) | inputs it must NOT fold |
|---|---|---|
| `parse_env_hash` | `parser_flags` only, today | project root, tsconfig paths, alias maps, TS semantic options, lib data |
| `resolve_env_hash` | `base_url`, `paths`, workspace aliases, project references, `resolve_extensions`, `module_resolution_mode`, `export_conditions` | lib data (R21) |
| `type_env_hash` | `type_strict`, `type_no_implicit_any` | — |
| `lib_env_hash` | `lib_names`, `type_roots`, `ambient_corpus_fingerprint` | — |
| `project_identity` | project identity basis | — |

R21 scoping rule: a cache layer keys only on the dimensions its value depends on; a single bundled
`project_config_hash` is forbidden. `ResolvedImportFacts` must not key on `lib_env_hash`;
`RouteDb`, typed-IR resolve, `MaterializeStructureDb`, `SemanticGraphStore`,
`ComponentMetaResultDb` must.

### Finding D-4 — the semantic and lib dimensions have no production ingress

`EnvHashInputs` (`crates/verter_workspace/src/env_hash.rs:68-108`) exposes nine input fields. It
is constructed at exactly **three** non-test sites, all in
`crates/verter_workspace/src/engine.rs` — `compose_env_hash_tables` (4471),
`compose_env_hash_tables_from_configs` (4513), and `compute_workspace_default_env_hash_array`
(4645) — and all three hardcode:

```rust
type_strict: false,
type_no_implicit_any: false,
lib_names: &[],
type_roots: &[],
module_resolution_mode: ModuleResolutionMode::default(),
```

So `type_env_hash` and `lib_env_hash` are **constant across every project in production today**.
The five-dimension split is structurally real (distinct salts, distinct key participation, R21
scoping enforced) but two of the five dimensions carry no project-derived input: no tsconfig
value reaches them.

The strict-family semantics themselves are not absent — they exist as
`StrictFamilyConfig`, driven by `RelationHostKnobs::strict_family_relax_bits`
(`crates/verter_session/src/host_construction.rs:40-52`), a `pub(crate)` **test-injection**
`AtomicU8`: bit 0 relaxes `strictNullChecks`, bit 1 `strictFunctionTypes`, bit 2 enables
`exactOptionalPropertyTypes`. It is read at
`project_semantic_dispatch/relation.rs:668` and folds into the relation key's `type_env_hash`,
so a relaxed judgement never warm-hits a strict one. Its only writers are test files; production
is pinned to the zero regime (TS-strict, exact optionality disabled).

Stated precisely, because the distinction matters for whether this is a bug or a gap:

- It is **not** a live cache-correctness defect. Nothing varies, so nothing collides.
- It **is** a missing ingress: a user's real `strictNullChecks: false` or
  `exactOptionalPropertyTypes: true` changes neither the semantics nor any cache key. The
  relaxation exists but only tests can reach it.
- `contracts/semantic-profile.md` §1 defines `TypeScriptSemanticProfileId` as covering exactly
  this set ("strictness, nullability, exact optional property behavior, module/resolution
  semantics, JSX/type-language rules"), and §2 requires every behaviour-affecting option to be
  classified. The distance between three test-only bits and that definition is the concrete size
  of `B1`'s profile-schema work.

The consequence for sequencing is the part a later charter would otherwise miss: whichever block
first threads real tsconfig values into `type_env_hash` **changes cache identity for every
existing project**, and does so at the moment the values stop being constant. That is a `B1`
obligation with a `G4` blast radius, and it must not be discovered mid-cutover. Recorded in
[`option-classification.tsv`](option-classification.tsv); owner `B1`.
