# LSP component-meta custom-method binding spec

Per migration plan Tier 0 Step 0.7 (D113 + D96 + D100 corrected).

## Methods (all NEW; verified absent at validation SHA `60b1295a`)

The LSP server registers three custom methods via the `.custom_method(...)`
builder chain at `crates/verter_lsp/src/main.rs:118..160`:

| Custom method | Purpose | Tier 5b registration target |
|---|---|---|
| `$/verter/getComponentMeta` | Full Volar-shape `ComponentMetaPayload` for backward-compatible consumers (Volar checker, benchmarks, IDE display). Bridge over selective primitives per D90 + D98. | NEW handler in `custom_methods/component_meta.rs::get_component_meta`. |
| `$/verter/getComponentMetaSurface` | Selective `ComponentMetaSurface` envelope per D32 + D99. 23 `ComponentMetaAnalysis` fields projected: 14 eager scalar fields verbatim, 9 lazy fields as `NamedTypeHandle` references. | NEW handler in `custom_methods/component_meta.rs::get_component_meta_surface`. |
| `$/verter/getComponentMetaTypeExpansion` | Selective `TypeExpansion` for one `TypeHandle` per D32 + D104. Used by IDE consumers that need progressive type drill-down (hover, completion, peek-definition). | NEW handler in `custom_methods/component_meta.rs::get_component_meta_type_expansion`. |

## D113 verification record

`grep -n 'getComponentMeta' crates/verter_lsp/src/main.rs` returns 0 matches at
SHA `60b1295a`. The existing chain registers `getAnalysis`, `getProjectOverview`,
`getBindingTypes`, `getComponentParents`, `documentDropEdit`,
`applyStyleOverrides`, `getRouteTree`. None of the three component-meta
methods exist; all three are new additions.

## Wire schema (D100 corrected)

The wire schema for the three methods comes from
`crates/verter_protocol/proto/verter/v1/`:

- `component_meta.proto` — existing types (`ComponentMetaPayload`,
  `ComponentMetaBody`, `ComponentFlags`, `RootReachability`, etc.) used by
  `$/verter/getComponentMeta`.
- `selective_component_meta.proto` (NEW per Tier 0 Step 0.7) — selective API
  types (`TypeHandle`, `TypeQueryPath`, `ChildKind`, `NamedTypeHandle`,
  `ComponentMetaSurface`, `FallthroughSurfaceLazy`, `TypeExpansion`,
  `ShapeOutline`, `BatchExpandError`, `BridgeError`, `TypeHandleError`).

`build.rs` invokes `prost-build` over both files; the generated module is
included via `pub mod verter::v1` in `crates/verter_protocol/src/lib.rs:8-12`.

**Plan-deviation note**: revision 8's D100 claimed the repo uses Rust-side
`prost::Message` derive without `.proto` codegen. The actual repo state at
SHA `60b1295a` is `.proto` codegen via `prost-build` (see `build.rs`:
`config.compile_protos(&["proto/verter/v1/component_meta.proto"], …)`). Step
0.7 follows the actual repo pattern (extending the `.proto` schema) rather
than the plan's claim about it.

## D96 module conversion (Tier 5b)

`crates/verter_lsp/src/server/custom_methods.rs` (file, 27754 bytes at SHA
`60b1295a`) becomes a directory at Tier 5b:

```
crates/verter_lsp/src/server/custom_methods/
├── mod.rs               // existing handler implementations migrate here
└── component_meta.rs    // NEW — three component-meta handlers
```

The conversion lands in Tier 5b, NOT Tier 0. Tier 0 only documents the
plan; the actual `mod.rs` / `component_meta.rs` files do not exist yet at
the close of Tier 0.

## Handler signatures (Tier 5b implementation)

```rust
// crates/verter_lsp/src/server/custom_methods/component_meta.rs (NEW; Tier 5b)

impl VerterLanguageServer {
    pub async fn get_component_meta(
        &self,
        params: GetComponentMetaParams,
    ) -> Result<Option<Vec<u8>>, ResponseError> {
        // Delegates to MetaSession::get_component_meta_payload (D90 BFS bridge).
        // Returns protobuf-encoded ComponentMetaPayload, or magic-byte-prefixed
        // BridgeError envelope (D114) on bridge failure.
    }

    pub async fn get_component_meta_surface(
        &self,
        params: GetComponentMetaSurfaceParams,
    ) -> Result<Option<Vec<u8>>, ResponseError> {
        // Delegates to MetaSession::get_component_meta_surface (D32).
        // Returns protobuf-encoded ComponentMetaSurface envelope.
    }

    pub async fn get_component_meta_type_expansion(
        &self,
        params: GetComponentMetaTypeExpansionParams,
    ) -> Result<Vec<u8>, ResponseError> {
        // Delegates to MetaSession::get_component_meta_type_expansion (D32).
        // Returns protobuf-encoded TypeExpansion, or magic-byte-prefixed
        // TypeHandleError envelope on resolution failure.
    }
}

#[derive(Deserialize)]
pub struct GetComponentMetaParams {
    pub canonical_or_alias: String,
}

#[derive(Deserialize)]
pub struct GetComponentMetaSurfaceParams {
    pub canonical_or_alias: String,
}

#[derive(Deserialize)]
pub struct GetComponentMetaTypeExpansionParams {
    pub handle_buf: Vec<u8>,    // protobuf-encoded TypeHandle
    pub depth: Option<u32>,
}
```

## Builder-chain registration (Tier 5b commit)

```rust
// crates/verter_lsp/src/main.rs (extend chain at lines 118..160)

LspService::build(VerterLanguageServer::new)
    // ... existing custom methods ...
    .custom_method(
        "$/verter/getComponentMeta",
        VerterLanguageServer::get_component_meta,
    )
    .custom_method(
        "$/verter/getComponentMetaSurface",
        VerterLanguageServer::get_component_meta_surface,
    )
    .custom_method(
        "$/verter/getComponentMetaTypeExpansion",
        VerterLanguageServer::get_component_meta_type_expansion,
    )
    .finish();
```

## MCP exposure (D95 — out of scope)

`verter_mcp` does NOT expose a component-meta tool at SHA `60b1295a`. Per
D95, the MCP component-meta tool is OUT OF SCOPE for this migration. If a
future MCP tool is added, it consumes the same `MetaSession` API surface as
the LSP — the schema in `selective_component_meta.proto` is the wire
contract for both surfaces.

## Tier 0 acceptance for this doc

Tier 0 Step 0.7 closes when:

1. `selective_component_meta.proto` committed and compiles via `cargo check
   -p verter_protocol`. ✓
2. `build.rs` updated to compile both `.proto` files. ✓
3. This document committed at `docs/arch/debt-closure/15-lsp-component-meta-method.md`. ✓

The discriminating tests for this step (`lsp_custom_request_method_binding_doc_present`,
`lsp_method_binding_names_three_methods`,
`mcp_component_meta_tool_binding_documented`,
`selective_api_proto_definitions_present_with_required_fields`) are scaffolded
into `crates/verter_session/tests/hermetic_checkout.rs` and
`crates/verter_protocol/tests/proto_audit.rs` per §2.2 of the plan; their
post-state assertion checks for the existence of this doc + the schema file
+ the three method names listed above.
