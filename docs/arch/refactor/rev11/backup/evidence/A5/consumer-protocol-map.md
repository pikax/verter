# A5 — `TypeExpr` / component-meta / graph / wire consumer map

The seed inventory `E1` turns into an exact producer/consumer/protocol/lifetime map
(`program.md`: `E1` **Predecessors: `A5`, `C1`, `D2`**). A5's obligation is **enumeration**: after
this file, no later block can discover a public or wire consumer by omission. Lifetime modelling
and the exact migration order are `E1`'s.

Counts are from this checkout. Reproduce any row with the command in its section.

---

## 1. `TypeExpr` — the internal typed IR

Owner crate: `crates/verter_type_expr/`. Sixteen workspace crates declare a dependency on it.
Reference density (`grep -rn --include="*.rs" "TypeExpr" crates/*/src`):

| crate | refs | role |
|---|---|---|
| `verter_session` | 4,280 | the dominant consumer: dispatch, projection, materialisation, caches |
| `verter_semantic` | 1,421 | producer — analysis lowering emits it |
| `verter_type_expr` | 626 | the owner |
| `verter_protocol` | 217 | wire projection (`TypeExpr` → graph nodes) |
| `verter_type_expr_oxc` | 127 | OXC → `TypeExpr` lowering front-end |
| `verter_no_typeexpr` (+`_derive`) | 137 | the **structural rail**: a marker crate proving absence, not a consumer |
| `verter_ffi` | 61 | FFI conversion |
| `verter_lsp` | 46 | IDE surfaces |
| `verter_macro_dto` | 45 | the dependency-neutral macro DTO |
| `verter_napi` | 24 | native binding |
| `verter_wasm` | 17 | WASM binding |
| `verter_parser` | 16 | parse-side types |
| `verter_session_query` | 6 | query keys |
| `verter_no_storedspan` (+`_derive`) | 6 | second structural rail |
| `verter_session_oracle_macro` | 1 | test macro |

Two observations that bear on `E2` ("eliminate internal general `TypeExpr` transit"):

- The distribution is not flat. `verter_session` holds 65% of all references; `E2`'s real
  blast radius is one crate, not sixteen. The FFI/binding tail (`ffi` + `napi` + `wasm` + `lsp` =
  148 refs) is the *public* half and is what `E3` replaces with operation DTOs.
- `verter_no_typeexpr` / `verter_no_storedspan` are **not** consumers to migrate. They are marker
  crates (`NoTypeExpr` / `NoStoredSpan` derives) whose whole purpose is to prove, structurally,
  that a type contains no `TypeExpr`. They are `E2`'s *instrument*, and they are the model the
  program should extend rather than replace — a compiler-enforced absence proof, not a scanner.

Per `architecture.md` §8.2 and maintainer ruling R-3, the plan supersedes `CLAUDE.md`'s
Typed-IR-Only rule **for the end state**: the final architecture contains no general recursive
owned `TypeExpr` as generic semantic transit IR, final cache value, compile projection contract,
or public result. `CLAUDE.md` remains accurate about today.

### TypeScript counterpart

`@verter/type-ir` (`packages/type-ir/`, PUBLISHED) owns `TypeDescriptor`, the TS mirror.
Consumers: `@verter/component-meta`, `@verter/typeinfo`, `@verter/playground`,
`@verter/benchmark`. `CLAUDE.md` requires every semantic decision in the compat layer to read
`prop.type` (`TypeDescriptor`), never `prop.rawType`.

## 2. Wire surfaces — protobuf

`crates/verter_protocol/proto/verter/v1/`:

Counts are **all declarations, nested included** — one method for all three rows, stated because
the rows are otherwise not comparable:

| proto | messages | enums | compatibility posture |
|---|---|---|---|
| `typeinfo.proto` | 195 | 36 | **closed contract** — `CLAUDE.md` → Typeinfo Wire Contract (CRITICAL); `TYPEINFO_GRAPH_SCHEMA_VERSION = 7` |
| `component_meta.proto` | 142 | 45 | `COMPONENT_META_SCHEMA_VERSION = 10` |
| `selective_component_meta.proto` | 25 | 4 | selective surface / expansion API |

```sh
cd crates/verter_protocol/proto/verter/v1
for f in *.proto; do
  printf '%s msg=%s enum=%s\n' "$f" \
    "$(grep -cE '^[[:space:]]*message ' $f)" "$(grep -cE '^[[:space:]]*enum ' $f)"
done
```

Only one nested declaration exists across the three files: `BatchExpandError.Reason`
(`selective_component_meta.proto:263`). `typeinfo.proto` and `component_meta.proto` have zero
nested messages and zero nested enums, so their 36 and 45 are unchanged by the choice of method —
which is exactly why a top-level-only count of `selective_component_meta.proto` (3) silently used a
different rule from its siblings and undercounted it.

Generated bindings, both directions:

- Rust: `crates/verter_protocol/build.rs` → `crates/verter_protocol/src/{typeinfo,component_meta,graph}/`
- TypeScript: `packages/proto/src/gen/verter/v1/{typeinfo_pb.ts, component_meta_pb.ts, selective_component_meta_pb.ts}` — **byte-pinned** by
  `crates/verter_protocol/tests/cases/typeinfo_proto_ts_freshness.rs`, which regenerates through
  the workspace `buf` + `oxfmt` binaries and byte-compares.

Wire guards in force (all named in `CLAUDE.md`): `typeinfo_graph_taxonomy`,
`typeinfo_proto_ts_freshness`, `request_kind_payload_parity`, `typeinfo_request_validation`,
`typeinfo_wire_surface_guards`, `typeinfo_graph_contract_guards`,
`typeinfo_request_contract_guards`, `typeinfo_audit_contract_guards`.

### The one provisional wire

`FrameworkSurfacePayload` (the `GRAPH_OPERATION_FRAMEWORK_SURFACES` operation) rides the existing
typeinfo graph envelope with an embedded `SemanticTypeGraph`. `CLAUDE.md` states this shape is
**PROVISIONAL** — the retag to a `TypeInfoGraphPayload` carrier, the schema-version bump, and
reserving the old field are still owed. See row 17 of [`owner-rows.md`](owner-rows.md); Revision 11
owner is `E3`.

## 3. Rust binding/adapter consumers of the host

Every crate that reaches `VerterHost` and can therefore observe a `TypeExpr`-derived or
component-meta result:

| crate | surface | published to crates.io |
|---|---|---|
| `verter_napi` | `NapiVerterHost`, `NapiMetaSession` (`meta.rs`, 30+ `#[napi]` methods), `typeinfo.rs`, `audit.rs`, `memory_audit.rs` | yes |
| `verter_wasm` | `lib.rs`, `typeinfo.rs`, `audit.rs` | yes |
| `verter_ffi` | shared conversion layer (`convert/typeinfo.rs`, `convert/input.rs`) | no (`publish = false`) |
| `verter_lsp` | LSP server binary + custom methods | no |
| `verter_mcp` / `verter_mcp_server` | MCP tools | no |
| `verter_tsc` | batch typecheck CLI | yes |
| `verter_bench`, `verter_dx_baseline`, `verter_vue_conformance`, `verter_svelte_conformance` | harnesses | no |

Eleven of 39 crates are crates.io-publishable (no `publish = false`): `verter-editor-client`,
`verter_actions`, `verter_analysis_inputs`, `verter_compiler`, `verter_diagnostics`,
`verter_language`, `verter_napi`, `verter_session_oracle_macro`, `verter_span`, `verter_tsc`,
`verter_wasm`. All at workspace version `0.0.1-beta.3`.

## 4. TypeScript package consumers

Sixteen published npm packages, all at `0.0.1-beta.3` (pre-1.0). Dependency edges relevant to the
`E`-track cutover:

| package | published | consumes |
|---|---|---|
| `@verter/component-meta` | yes | `@verter/native`, `@verter/wasm`, `@verter/proto`, `@verter/type-ir`, `@verter/typeinfo`, `@verter/types` |
| `@verter/typeinfo` | yes | `@verter/native`, `@verter/proto`, `@verter/type-ir`, `@verter/types` |
| `@verter/type-ir` | yes | — (leaf) |
| `@verter/proto` | yes | — (generated bindings) |
| `@verter/types` | yes | — (leaf; 12 packages depend on it) |
| `@verter/native` | yes | the NAPI artifact |
| `@verter/wasm` | yes | the WASM artifact |
| `@verter/unplugin` | yes | `@verter/native` |
| `@verter/nuxt` | yes | `@verter/unplugin` |
| `@verter/typescript-plugin` | yes | `@verter/native`, `@verter/types` |
| `@verter/language-shared` | yes | `@verter/types` |
| `@verter/svelte-jsx`, `@verter/binary-launcher`, `verter-lsp`, `verter-mcp`, `verter-tsc` | yes | packaging/runtime shims |
| `@verter/playground`, `@verter/benchmark`, `@verter/dx-harness`, `@verter/lsp-test-client`, `verter-vscode`, `example`, `@verter/svelte-runtime-tests`, `@verter/vue-conformance-oracle` | **private** | not a compatibility obligation |

The private/published split is the compatibility line: `E1`–`E3` owe migration to the 16
published packages and owe nothing to the 8 private ones beyond keeping the repo green.

## 5. Committed TS mirrors of Rust-owned schemas

Four generated TS files are committed and must move in lockstep with their Rust owner. A later
block that changes the owner and not the mirror produces a silent drift, so they are enumerated:

| file | owner | freshness enforcement |
|---|---|---|
| `packages/proto/src/gen/verter/v1/*_pb.ts` | the three `.proto` files | `typeinfo_proto_ts_freshness` (byte-pin via `buf` + `oxfmt`) |
| `packages/types/audit.generated.ts` | `verter_audit` record schema | `request_kind_payload_parity` |
| `packages/language-shared/src/virtual-file-naming.generated.ts` | `VirtualFileNaming` descriptor column | `virtual_file_naming_ts_freshness` |
| `packages/language-shared/src/client-framework-manifest.generated.ts` | the framework registry, via `verter_session::framework::client_framework_manifest_ts::render_client_framework_manifest_ts` | `client_framework_manifest_ts_freshness` (`crates/verter_session/tests/cases/client_framework_manifest_ts_freshness.rs:43`) — renders and byte-compares |

All four mirrors are guarded. The enumeration is recorded because the *guard* is what a later
block must keep alive when it changes an owner: a renderer change that also regenerates the
committed file passes trivially, so each of these guards is only as good as its rendering path
being the production one. That property holds today for all four — each guard calls the
production renderer, not a test-local copy.

## 6. What this map deliberately does not contain

Per `E1`'s charter, the *exact* map adds lifetime, cache route, and per-consumer compatibility
obligation for each row. A5 stops at enumeration plus the compatibility posture (published vs
private, closed-contract vs provisional). Extending further would pre-empt `E1` with decisions
made without `C1`'s and `D2`'s results, which is the failure mode the program's predecessor edges
exist to prevent.
