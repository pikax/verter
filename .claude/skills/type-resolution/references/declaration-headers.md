## IndexedReady Target Contract

**Engine-facing input records.** The source backend retains `IndexedReady`,
`ShallowFileState` and their lazy workers. `IndexedInputs` returns cached owned
`IndexedInputRecord`, `ShallowInputRecord` and `PreparedInputRecord` projections
with header facts, locators and exact content identity. Those records expose no
body memo, prepared cache, macro cells or workspace. Body/dependency/preparation
demands go through `OwnedLowering`; source endpoints reuse the same retained
parse and return owned products. Private request retention keeps the exact
observed source alive, including fenced artifacts, without promoting admission
or changing completion state. New raw-artifact observations and source edits
retain their own identity; cloning a pure projection preserves its observation.
A missing exact lease returns the existing typed miss.
`PreparedDeclBundle` and its owned input record share one immutable `Arc` of
owner scopes. Declaration-scope payloads read those same name sets and binding
maps; selecting a port record does not copy a scope collection.

Architectural target for the project-global cache cutover:

- `IndexedReady` is the canonical post-parse per-file artifact: a shallow declaration INDEX plus body locators, NOT a body store.
- Scheduler remains the sole source and parse authority. `IndexedReady` is built from scheduler-owned parsed snapshots.
- `IndexedReady` stores canonical imports and exports for the file.
- `IndexedReady` eagerly owns the declaration inventory: top-level symbol names, kinds, declaration/name spans, source-order contributor grouping (statement locators), type-parameter names, syntactic member headers, enum member headers, and the augmentation-scope inventory — all safe for host-owned `Send + Sync` caches. It must NOT store per-symbol lowered `TypeExpr` bodies, per-symbol body dependency vectors, member deps, `typeof` roots, a whole-file `EvalEnv`, or body semantic hashes.
- Declaration BODIES lower lazily on first semantic demand through the shared lazy body service: the content-addressed `DeclBodyMemo` (a file-artifact child keyed by the owning artifact's `(canonical, whole_hash, parse_env_hash)`) asks the scheduler-side `DeclLoweringService` to borrow the worker-retained parse snapshot, lower exactly the demanded symbol's contributing statements, and return owned typed IR. Publishing an artifact lowers ZERO declaration bodies. First-touch singleflights per `(space, scope, name)` entry; sibling backfill is coverage-gated (only symbols whose FULL header contributor set was lowered populate). A content edit produces a fresh memo — superseded bodies can never serve a new-content demand; overlay artifacts own their own memo, never the base's.
- Parse once per content generation through the lowering service's retained snapshot. Retention is LEASE-PINNED, not LRU/budget-evicted: the cold-index parse acquires ONE `SnapshotLease` for the artifact's `(canonical, whole_hash, parse_env_hash)` key and hands it to the artifact's `DeclBodyMemo`, so the header-index parse and every later body / whole-env / raw-surface demand reuse that ONE parse for the artifact's whole life — a live artifact never silently re-parses. The lease drops with the memo (hence the artifact), releasing the retained `Rc<ParsedEvalProgram>`. The temporary OXC parse arena stays on the worker (native) / the wasm thread-local shard, is per-file and per-version only, never crosses a thread boundary, and must not leak into long-lived `Send + Sync` shared caches; jobs return owned typed IR. A content edit produces a new key (fresh memo, fresh lease), so a superseded snapshot can never answer a new-content demand.
- `IndexedReady` is authoritative for declaration STRUCTURE and locators (import edges, export edges, headers); lazily lowered bodies are authoritative only after materialization and are keyed by the observed file content/version.
- `AnalysisReady` is an additive layer built from `IndexedReady`; it must not rediscover the same file structure — or body structure — through a second path.
- If analysis or component-meta expands a shallow symbol, both paths must populate and reuse the same host-owned route, prepared-declaration, owner-import, and projection caches.
- Shared symbol expansion helpers are the default. Do not add consumer-specific shallow expansion paths when the existing shared resolver can serve the work.
- New work moves toward `SourceReady -> IndexedReady -> optional higher layers`, not further toward request-local or duplicate parser/resolver paths.

### whole_env() consumer graph-native readers (Stage 6-prep readiness)

The `DeclBodyMemo::whole_env()` whole-file env product has exactly four consumers, all reaching it through `VerterHost::base_eval_env_arc`. Each now has a NON-BREAKING, bounded, graph-native per-symbol reader sitting BESIDE the legacy whole-env path; the legacy `whole_env()` path is retained in production as the equivalence ORACLE. The LANDED Stage 6 Option-B flip mints `HotTypeRef` handles at the dispatch boundary (the `decl_body_hot_ref` accessor) over the `Instantiate` query result — the consumer-visible `SemanticNodeId` the graph-bearing producer drives via the RESOLVING lowerer — and does NOT remove `EvalEnv` / `whole_env()` (the oracle is retained as the parity rail); `EvalEnv` / `whole_env()` removal remains a LATER stage (the oracle-deletion + Stage 7+ work), not landed. The readers route through `ShallowFileState::{type_decl, value_decl, header_index}` and never materialise `whole_env()` — including any DEPENDENCY whole env (C3's export-target + alias peel routes through `resolve_value_export_target_graph_native` → `peel_value_decl_alias_graph_native`, never the legacy `resolve_value_export_target` whose peel materialises the dependency's `base_eval_env_arc`). A non-test debug cross-check on the C1/C2/C4 consumers exercises each graph-native reader against the oracle (release builds skip it): C1 and C4 run on every real host call; C2 runs on every non-rune-module call (the Svelte rune-ambient-env modules are gated out because their per-symbol reader does not replay the rune ambient overlay). C1/C2/C4 assert presence/terminal/field equivalence against the oracle. C3 carries NO in-production cross-check: its equivalence is proved OFFLINE on full `(source_canonical, source_name)` pairs by `c3_fallthrough_runtime_value_deps_graph_native_equals_materializer_touched_full_pairs` (subset/equality on the touched-pair SET, never a name-count proxy — legal double-alias-onto-one-source hydrates two bindings from a single dep pair, so any `deps >= added` count bound is unsound). The only faithful in-production touched-pair recompute would route through the legacy `resolve_value_export_target` whole-env peel — the exact dependency-whole-env cost the readiness work removes — so the offline pair-equality test is the authoritative C3 equivalence rail. The residual inventory scanner `whole_env_consumer_graph_native_inventory.rs` is retired. That the consumer SET is exactly these four (no fifth) is established by the exhaustive `whole_env()` consumer enumeration + the per-consumer oracle-equivalence tests + review. Post-SIMP5 capability enforcement is compile-time witnesses in `crates/verter_source_policy_gate/tests/cases/semantic_capability_witnesses.rs` plus the trybuild fixtures run by `scripts/compile-contracts.mjs`.

- **C1 `local_type_declaration_id`** → `local_type_declaration_id_graph_native`: both paths select the sole authored owner from the cached declaration-header inventory, then perform the exact-owner type-header lookup. Import bindings are not declaration headers and cannot mask a same-named local declaration in another SFC owner; an import-only name has no candidate, while same-name declarations in multiple lexical owners are genuinely ambiguous at this owner-agnostic API and fail closed. The oracle's `DeclarationId` is the 1-based ordinal in the INTERLEAVED type+value `add_type`/`add_value` registration order of `build_eval_env` (single shared `next_declaration_id` counter), NOT recoverable from the unordered, kind-split `DeclHeaderIndex` without replaying the registration walk. The id is an OPAQUE in-process token — it never crosses the FFI/wire surface (`FfiResolvedTypeDeclaration` carries no `declaration_id`), is never compared cross-file, and no production reader branches on its value. C1's contract is therefore STABLE-AND-UNIQUE, NOT EQUAL-TO-ORACLE; the reader returns a stable per-owner header-name ordinal id and the oracle stays authoritative for the value. The equivalence test pins presence (`Some`/`None`); the value-id derivation stays oracle-owned.
- **C2 `peel_value_decl_alias`** → `peel_value_decl_alias_graph_native`: same single-segment `typeof` alias chain, but each hop reads the one demanded value symbol via `value_decl(name)` and resolves the membership check through `header_index().value_header(next).is_some()` (presence, no lowering).
- **C3 `build_fallthrough_eval_env_lightweight`** → `fallthrough_runtime_value_deps_graph_native`: the whole-env CLONE this consumer takes of the OWNER env as its mutable base is NOT eliminated here (the LANDED Stage 6 Option-B flip mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) and retains `EvalEnv`/`whole_env()` as the parity-rail oracle, so that collapse is a LATER stage — the oracle-deletion + Stage 7+ work, not landed). The readiness deliverable is the graph-native runtime-value DEP SET — the `(source_canonical, source_name)` pairs the materializer touches, enumerated via the per-import route + export resolution (through `resolve_value_export_target_graph_native`, so NO DEPENDENCY whole env is materialised) WITHOUT a whole-env clone, proven equal on FULL pairs to the materializer-touched set (a re-export/aliased fixture where `source_canonical != dep_canonical` and `source_name != binding.name` pins source identity, not a name collapse).
- **C4 `dependency_eval_env`** → `dependency_value_symbol_graph_native`: the consumer's sole whole-env use is `source_env.value_symbols.get(name).primary().clone()` after a `prepared_value_decl` miss. The per-name reader reproduces that read via `value_decl(name)` (declaration_id 0, matching the prepared/alias hydration path) without the dependency's whole env.

**Known oracle/per-symbol divergence (scoped).** `whole_env()` post-build applies `apply_sfc_script_setup_type_params` and `apply_svelte_rune_ambient_env`; per-symbol `type_decl`/`value_decl` do not. SFC `<script setup generic="T">` params land in `env.type_bindings` (a SEPARATE namespace), NOT `env.type_symbols`, so `T` is NOT a `type_declaration_id` in EITHER path — C1/C2 over real symbols are unaffected. Svelte rune modules inject ambient `$`-rune VALUE symbols into the oracle env that the per-symbol header index lacks; a `typeof`-hop targeting a `$`-rune ambient name would terminate one hop earlier in the C2 graph-native peeler than in the oracle — a non-real-export corner the oracle covers in production. Because that divergence is REAL for a `$`-rune-targeting alias in a rune module, the C2 oracle's debug cross-check is GATED on `!is_svelte_rune_module(canonical)` (it would otherwise debug-panic on the documented divergence); the cross-check still runs for every plain `.ts`/`.svelte`/SFC file. These are why the oracle is retained.

## Declaration symbol inventories

`verter_semantic::analysis::type_eval` contains the shallow per-file
declaration inventory. `EvalEnv` stores content-free facts and authored-body
locators for ordered type/value contributor groups and augmentation scopes. It
stores no `TypeExpr` and performs no evaluation. Authored bodies are lowered
on demand by the shared semantic dispatch.

The parser's `route_inventory` module owns authored syntax routes only. The
semantic `ScriptShallowIndex` joins those routes with declaration headers, and
semantic `decl_dependencies` owns structural dependency names. These layers
perform no type evaluation and expose no resolved-element carrier. Query-time
finite types are governed by the shared semantic-dispatch contract above.

## Member header key index

`TypeDeclHeader.member_headers` and `ValueDeclHeader.object_member_headers`
are `MemberHeaderList`s (`verter_session_query::declarations::header_index`):
the members in exact source order plus a key → position index. The index is
required state, built by the shallow header walk and retained with the
header; `MemberHeaderList::get(key)` reads a member without scanning.

Key identity is the authored key's exact equality, as before the index:
`1` and `0x1` are the same numeric key, but `1` and `"1"` stay distinct
keys (a non-integer number such as `1.5` becomes its string spelling), and
symbol keys (`[sym]`, `[Symbol.iterator]`) are computed keys compared by their
exact typed child.

Order rules, unchanged by the index:

- interface members, type-literal members (descending intersection and
  parenthesized arms) and merged contributors are first-wins: a repeated
  key keeps its first header and position;
- an object literal is last-wins: a repeated key takes its last header at
  its last position (`{ a, b, a }` is `b, a`);
- object-literal and class-static headers merge across contributors
  first-wins.

Each offered member costs at most two index operations, so a wide header
costs linear key work. The `key_probes()` work counter is optional
observation, compiled only under `cfg(any(test, feature = "test-support",
feature = "semantic-observe"))`; its inventory row is
`crates/verter_semantic/observe-inventory/SKR-P-HEADERS.md`.
