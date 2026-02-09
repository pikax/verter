# Cross-File TypeScript Type Resolution for Vue defineProps

## Context

When a Vue SFC uses `defineProps<T>()` where `T` is a type reference (local interface, type alias, imported type, or script generic param), the Rust compiler currently cannot resolve it:

- `extract_type_params()` at [setup.rs:680](crates/verter_core/src/utils/oxc/vue/script/setup.rs#L680) calls `resolve_type_elements()` — the context-free variant that silently drops all `TSTypeReference` at [resolve_type.rs:447-451](crates/verter_core/src/utils/oxc/vue/script/resolve_type.rs#L447).
- The context-aware `resolve_type_elements_with_ctx_ref()` exists and works (tested) but is never invoked from macro parsing.
- Result: `type_params.resolved.props` is empty → fallback to bare `__props;` in [props.rs:125-138](crates/verter_core/src/codegen/vue/macros/props.rs#L125).

**Goal**: (A) Wire up local type resolution (interfaces, aliases, script generics), (B) add host protocol for imported types.

---

## Phase 1: Fix Local Type Resolution

### Span/source correctness

`TypeResolutionContext.source` must contain the bytes that spans index into. Existing `build_type_context()` adds `base_offset` to spans (making them SFC-file-relative). So `source` must be the **full SFC bytes** — not the script content slice.

- From `pre_scan_script_setup_bindings()`: `source`/`bytes` params are already the full SFC. Pass `bytes` to `build_type_context`.
- From Analysis plugin: `ctx.input` / `ctx.bytes` is the full SFC. Pass `ctx.bytes`.
- From unit tests: `base_offset=0`, so `source.as_bytes()` works directly.

No String allocations. Names are looked up via `&self.source[span.start..span.end]` (existing pattern) and compared against `&[u8]` from `get_type_reference_name()` which we change to return `&str` instead of `String`.

### Step 1.1: Change `get_type_reference_name` to return `&str`

**File**: [resolve_type.rs:979](crates/verter_core/src/utils/oxc/vue/script/resolve_type.rs#L979)

```rust
fn get_type_reference_name<'a>(type_name: &'a TSTypeName<'a>) -> &'a str {
    match type_name {
        TSTypeName::IdentifierReference(id) => id.name.as_str(),
        TSTypeName::QualifiedName(q) => q.right.name.as_str(),
        TSTypeName::ThisExpression(_) => "this",
    }
}
```

Update all 3 call sites (lines ~497, ~579, ~615) to use `.as_bytes()` for the comparison. This eliminates the `String` allocation per type reference.

### Step 1.2: Add `sfc_source` and `generic_params` to `parse_script()`

**File**: [mod.rs:101](crates/verter_core/src/utils/oxc/vue/script/mod.rs#L101)

```rust
pub fn parse_script<'a>(
    program: &Program<'a>,
    mode: ScriptMode,
    base_offset: u32,
    source: &'a str,
    // NEW: Full SFC bytes for type context span lookups. None = use source.as_bytes()
    sfc_source: Option<&'a [u8]>,
    // NEW: Script generic type params from <script setup generic="T extends ...">
    generic_type_params: Option<&'a TSTypeParameterDeclaration<'a>>,
) -> ScriptParseResult<'a> {
```

Build `TypeResolutionContext` before `process_setup_statements`:

```rust
let type_context = if mode == ScriptMode::Setup {
    let type_source = sfc_source.unwrap_or(source.as_bytes());
    let mut ctx = build_type_context(program, type_source, base_offset);

    // Add script generic type params (e.g., <script setup generic="T extends { foo: string }">)
    if let Some(generics) = generic_type_params {
        for param in &generics.params {
            let name_span = Span {
                start: param.name.span.start,  // These are wrapper-relative
                end: param.name.span.end,       // Need adjustment — see below
            };
            // Generic params use a different allocator context;
            // store constraint as type_param in context
            ctx.type_params.push((name_span, param.constraint.as_deref()));
        }
    }

    Some(ctx)
} else {
    None
};

// Pass type_context to process_setup_statements
match mode {
    ScriptMode::Setup => {
        let mut setup_ctx = SetupContext::new();
        process_setup_statements(
            &program.body, &ctx, &mut setup_ctx,
            &mut items, &mut errors,
            type_context.as_ref(),
        );
        // ...
    }
    // ...
}
```

**Generic param spans**: The `GenericParseResult.type_parameters()` returns params with AST spans relative to the wrapped arrow function (offset by `ast_offset`). But `find_type_param()` compares against `source[span.start..span.end]`. For generics, we need the name bytes to match. Since generic param names like "T" are simple identifiers, we can store the span pointing into the generic attribute text in the SFC. The `GenericParseResult.position` gives the file-relative offset. Each param's `name_span` is relative to the generic string start. So: `file_span = position.start + name_span.start`.

```rust
if let Some(generics_result) = generic_parse_result {
    if let Some(type_params_decl) = generics_result.type_parameters() {
        for (i, param) in type_params_decl.params.iter().enumerate() {
            if let Some(gp) = generics_result.params.get(i) {
                // File-relative name span
                let name_span = Span {
                    start: generics_result.position.start + gp.name_span.start,
                    end: generics_result.position.start + gp.name_span.end,
                };
                ctx.type_params.push((name_span, param.constraint.as_deref()));
            }
        }
    }
}
```

But `param.constraint` is `&TSType` from the generic's parse — its AST is in the pipeline allocator. The constraint would be resolved recursively, which works since `resolve_type_elements_inner_with_ctx_ref` takes `&'a TSType<'a>` and the allocator is shared.

### Step 1.3: Update call sites of `parse_script()`

**File**: [codegen.rs:417](crates/verter_core/src/builder/codegen.rs#L417) (pre_scan):

```rust
let parsed = parse_script(
    &ret.program,
    ScriptMode::Setup,
    content_start as u32,
    script_content,
    Some(bytes),    // full SFC bytes
    None,           // no generic params available in pre_scan
);
```

**File**: [analysis.rs:508](crates/verter_core/src/syntax/plugins/analysis/analysis.rs#L508) (Analysis plugin):

```rust
let generic_type_params = e.generic.as_ref().and_then(|g| g.type_parameters());
let result = parse_script(
    &e.program,
    mode,
    e.content_start,
    ctx.input,
    Some(ctx.bytes),          // full SFC bytes
    generic_type_params,      // script generic params
);
```

**Test files** (mod.rs tests): Add `None, None` for the two new params. All existing tests pass unchanged since they use `base_offset=0`.

### Step 1.4: Thread `type_ctx` through setup.rs call chain

**File**: [setup.rs](crates/verter_core/src/utils/oxc/vue/script/setup.rs)

Add `type_ctx: Option<&TypeResolutionContext<'a>>` parameter to:

| Function                          | Line | Passes to                                                      |
| --------------------------------- | ---- | -------------------------------------------------------------- |
| `process_setup_statements`        | 82   | `process_setup_statement`                                      |
| `process_setup_statement`         | 95   | `process_variable_declaration`, `process_expression_statement` |
| `process_variable_declaration`    | ~330 | `try_parse_macro_from_expression`                              |
| `process_expression_statement`    | ~395 | `try_parse_macro_from_expression`                              |
| `try_parse_macro_from_expression` | 519  | `parse_macro_call`                                             |
| `parse_macro_call`                | 531  | `extract_type_params`                                          |

Recursive calls at lines 139, 207, 212, 218 also pass `type_ctx` through.

### Step 1.5: Use context in `extract_type_params()`

**File**: [setup.rs:661](crates/verter_core/src/utils/oxc/vue/script/setup.rs#L661)

```rust
fn extract_type_params<'a>(
    tp: &'a TSTypeParameterInstantiation<'a>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: Option<&TypeResolutionContext<'a>>,
) -> MacroTypeParams {
    // ... lt_span, gt_span, type_span unchanged ...

    let resolved = tp.params.first().map(|ts_type| {
        if let Some(tctx) = type_ctx {
            resolve_type_elements_with_ctx_ref(ts_type, ctx.base_offset, tctx)
        } else {
            resolve_type_elements(ts_type, ctx.base_offset)
        }
    }).unwrap_or_default();

    // runtime_types: infer_runtime_type doesn't need context
    let runtime_types = tp.params.first()
        .map(|ts_type| infer_runtime_type(ts_type))
        .unwrap_or_default();

    MacroTypeParams { lt_span, type_span, gt_span, resolved, runtime_types }
}
```

Also update the second call at line ~634 (inside `withDefaults` parsing).

### Step 1.6: Tests

**File**: [codegen.rs](crates/verter_core/src/builder/codegen.rs) (E2E tests with `gen_and_validate`):

```
1. interface Props { title: string; count?: number } + defineProps<Props>()
   → output: title: { type: String, required: true }, count: { type: Number, required: false }
2. type Props = { foo: string } + defineProps<Props>()  → correct runtime props
3. defineProps<Base & Extra>() with two local interfaces → merged props
4. withDefaults(defineProps<Props>(), { count: 0 }) + local interface → defaults applied
5. defineProps<{ msg: string }>() → regression test (inline literal still works)
```

**File**: [mod.rs](crates/verter_core/src/utils/oxc/vue/script/mod.rs) (unit tests):

```
6. parse_script with interface + defineProps<Props>() → MacroTypeParams.resolved.props is non-empty
7. parse_script with type alias + defineProps → resolved correctly
8. Script generic: generic="T extends { items: string[] }" + defineProps<T>() → resolved from constraint
```

---

## Phase 2: Cross-File Host Protocol

### Step 2.1: CachedTypeScope — allocator-independent type info

**New file**: `crates/verter_core/src/type_host/mod.rs`

```rust
pub mod cache;

use rustc_hash::FxHashMap;
use crate::utils::oxc::vue::script::resolve_type::ResolvedElements;

/// Allocator-independent type scope for a cached file.
#[derive(Debug, Clone, Default)]
pub struct CachedTypeScope {
    pub types: FxHashMap<String, ResolvedElements>,  // name → resolved
    pub imports: Vec<CachedImport>,
}

#[derive(Debug, Clone)]
pub struct CachedImport {
    pub local_name: String,
    pub original_name: String,
    pub source_specifier: String,
}
```

Owned `String`s are only used here (cache layer, not hot path). During compilation the hot path uses `&[u8]` span lookups. `String`s are only created when ingesting a file (rare event after warm-up).

### Step 2.2: Global file cache with LRU

**New file**: `crates/verter_core/src/type_host/cache.rs`

```rust
use lazy_static::lazy_static;
use std::sync::Mutex;

pub struct TypeFileCache {
    files: FxHashMap<String, FileCacheEntry>,
    resolutions: FxHashMap<(String, String), String>,  // (specifier, from) → resolved_id
    max_entries: usize,
    access_order: Vec<String>,
}

struct FileCacheEntry {
    version: String,
    scope: CachedTypeScope,
}

lazy_static! {
    pub(crate) static ref GLOBAL_TYPE_CACHE: Mutex<TypeFileCache> =
        Mutex::new(TypeFileCache::new(512));
}
```

**`ingest()`** method:

1. Create temporary `Allocator`
2. Parse content as TS with `oxc_parser::Parser`
3. Call `build_type_context(&program, content.as_bytes(), 0)` — base_offset=0 since standalone file
4. For each type alias: resolve via `resolve_type_elements_with_ctx_ref()` → clone `ResolvedElements`
5. For each interface: resolve members → clone `ResolvedElements`
6. Store in `CachedTypeScope.types`
7. Collect import declarations → `CachedImport` (String allocations are fine here)
8. Drop allocator

### Step 2.3: Add `imported_types` to `TypeResolutionContext`

**File**: [resolve_type.rs](crates/verter_core/src/utils/oxc/vue/script/resolve_type.rs)

```rust
pub struct ImportedType<'a> {
    pub local_name_span: Span,       // file-relative span of the local name
    pub original_name: &'a str,      // from OXC AST (no allocation)
    pub source_specifier: &'a str,   // from OXC AST (no allocation)
}

pub struct TypeResolutionContext<'a> {
    pub source: &'a [u8],
    pub type_aliases: Vec<(Span, &'a TSType<'a>)>,
    pub interfaces: Vec<(Span, &'a oxc_allocator::Vec<'a, TSSignature<'a>>)>,
    pub type_params: Vec<(Span, Option<&'a TSType<'a>>)>,
    pub diagnostics: Vec<ResolutionDiagnostic>,
    // NEW:
    pub imported_types: Vec<ImportedType<'a>>,
    pub from_file: &'a str,  // canonical path of the current .vue file
}
```

Extend `build_type_context` to collect import declarations:

```rust
Statement::ImportDeclaration(import) => {
    if let Some(specifiers) = &import.specifiers {
        for spec in specifiers {
            if let ImportDeclarationSpecifier::ImportSpecifier(s) = spec {
                ctx.imported_types.push(ImportedType {
                    local_name_span: Span {
                        start: s.local.span.start + base_offset,
                        end: s.local.span.end + base_offset,
                    },
                    original_name: s.imported.name(),
                    source_specifier: import.source.value.as_str(),
                });
            }
        }
    }
}
```

### Step 2.4: NeedFileRequest and cross-file lookup

**File**: [resolve_type.rs](crates/verter_core/src/utils/oxc/vue/script/resolve_type.rs)

```rust
#[derive(Debug, Clone)]
pub struct NeedFileRequest {
    pub specifier: String,    // owned — leaves the allocator scope
    pub from_file: String,
    pub type_name: String,
}
```

In the `TSTypeReference` branch of `resolve_type_elements_inner_with_ctx[_ref]`, after local resolution fails (steps 1-3):

```rust
// 4. Check imported types
let type_name_bytes = type_name.as_bytes();
if let Some(import) = ctx.imported_types.iter().find(|i|
    &ctx.source[i.local_name_span.start as usize..i.local_name_span.end as usize] == type_name_bytes
) {
    let cache = GLOBAL_TYPE_CACHE.lock().unwrap();
    if let Some(resolved_id) = cache.resolve_specifier(import.source_specifier, ctx.from_file) {
        if let Some(elements) = cache.lookup_type(&resolved_id, import.original_name) {
            result.props.extend(elements.props.iter().cloned());
            result.emits.extend(elements.emits.iter().cloned());
            result.has_call_signature |= elements.has_call_signature;
            drop(cache);
            return;
        }
    }
    drop(cache);
    // Cache miss — record NeedFiles request
    // (only in mutable ctx variant; ref variant adds diagnostic instead)
    ctx.need_files.push(NeedFileRequest {
        specifier: import.source_specifier.to_string(),
        from_file: ctx.from_file.to_string(),
        type_name: import.original_name.to_string(),
    });
    return;
}
// 5. Unresolved — diagnostic (existing code)
```

**Note**: `need_files` field only on the mutable `TypeResolutionContext` variant (used with `resolve_type_elements_with_ctx`). The immutable `_ref` variant emits a diagnostic instead. Pre_scan and Analysis both use the `_ref` variant for now; `need_files` collection is done via a new mutable context that wraps the compile call in `generate_for_vite`.

### Step 2.5: Propagate `need_files` to `ViteCodegenResult`

Add `need_files: Vec<NeedFileRequest>` and `deps: Vec<String>` fields to:

| Struct                              | File                            |
| ----------------------------------- | ------------------------------- |
| `ScriptParseResult`                 | `utils/oxc/vue/script/types.rs` |
| `AnalysisScriptInfo` (pass through) | `syntax/types.rs`               |
| `TemplateCodegenState`              | `codegen/vue/template/types.rs` |
| `ViteCodegenResult` (core)          | `builder/codegen.rs`            |
| `ViteCodegenResult` (NAPI)          | `verter_napi/src/lib.rs`        |

In `generate_for_vite()`: after pipeline runs, read `need_files` and `deps` from the plugin state and include in result.

### Step 2.6: NAPI functions

**File**: [verter_napi/src/lib.rs](crates/verter_napi/src/lib.rs)

```rust
#[napi(object)]
pub struct JsNeedFile {
    pub specifier: String,
    pub from_file: String,
    pub type_name: String,
}

// Extend ViteCodegenResult:
pub struct ViteCodegenResult {
    // ... existing ...
    pub need_files: Vec<JsNeedFile>,
    pub deps: Vec<String>,
}

#[napi]
pub fn ingest(id: String, version: String, content: Buffer) -> Result<()>

#[napi]
pub fn register_resolution(specifier: String, from_file: String, resolved_id: String) -> Result<()>

#[napi]
pub fn invalidate_file(id: String) -> Result<()>

#[napi]
pub fn clear_file_cache() -> Result<()>
```

### Step 2.7: JS type declarations and exports

**File**: [packages/native/index.ts](packages/native/index.ts) — add TS types for new NAPI functions and extended `ViteCodegenResult`.

**File**: [packages/native/index.js](packages/native/index.js) — re-export 4 new functions.

---

## Phase 3: Vite Plugin Iterative Loop

### Step 3.1: Utility additions

**File**: [packages/vite-plugin/src/utils.ts](packages/vite-plugin/src/utils.ts)

- `canonicalizePath(id)`: strip `?` query, `path.resolve()`, `\` → `/`
- `computeFileVersion(path)`: `statSync().mtimeMs + ':' + size`
- Reverse dep map: `Map<string, Set<string>>` — dep → parent .vue files
- Update `setDescriptor`/`deleteDescriptor` to maintain reverse deps
- Update cache value to `{ result, deps }`

### Step 3.2: Host compilation loop

**New file**: `packages/vite-plugin/src/host.ts`

```typescript
const MAX_ITERATIONS = 8;

export async function compileWithHost(pluginCtx, compiler, code, options, filename) {
  let iteration = 0;
  while (iteration < MAX_ITERATIONS) {
    const result = compiler.compileForVite(code, options);
    if (!result.needFiles?.length) {
      for (const dep of result.deps ?? []) pluginCtx.addWatchFile(dep);
      return { result, deps: result.deps ?? [] };
    }
    for (const need of result.needFiles) {
      const resolved = await pluginCtx.resolve(need.specifier, need.fromFile, { skipSelf: true });
      if (!resolved || resolved.external) continue;
      const rp = canonicalizePath(resolved.id);
      const ver = computeFileVersion(rp);
      if (!ver) continue;
      compiler.ingest(rp, ver, readFileSync(rp));
      compiler.registerResolution(need.specifier, need.fromFile, rp);
      pluginCtx.addWatchFile(rp);
    }
    iteration++;
  }
  throw new Error(`Verter: ${filename} exceeded ${MAX_ITERATIONS} resolution iterations`);
}
```

### Step 3.3: Transform hook

**File**: [packages/vite-plugin/src/index.ts](packages/vite-plugin/src/index.ts)

Replace direct `compileForVite` call with `compileWithHost`. Feature-detect: `typeof compiler.ingest === 'function'` → use host loop, else fallback.

### Step 3.4: HMR

**File**: [packages/vite-plugin/src/index.ts](packages/vite-plugin/src/index.ts) — `handleHotUpdate`

When `.ts`/`.d.ts` changes:

1. `compiler.invalidateFile(canonical)`
2. Find parent `.vue` files via reverse dep map
3. `deleteDescriptor(vueFile)` for each parent
4. `server.moduleGraph.invalidateModule()` + `full-reload`

---

## Phase 4: Docs & Tests

### Docs

**New file**: `docs/compiler-host-protocol.md` — protocol types, flow, canonicalization, HMR, iteration cap.

### Tests

**Rust E2E** (codegen.rs):

1. Local interface → runtime props
2. Local type alias → runtime props
3. Intersection types
4. withDefaults + interface
5. Inline literal (regression)
6. Script generic `T extends { ... }` → props from constraint
7. Cross-file: pre-populated cache → correct props
8. Cross-file: empty cache → need_files returned

**Rust unit** (resolve_type.rs, cache.rs):

- TypeResolutionContext with imports
- Cache ingest/lookup/LRU/invalidation

**JS** (host.spec.ts, utils.spec.ts):

- Host loop mock: NeedFiles → Ok
- Iteration cap
- canonicalizePath
- Reverse dep tracking

---

## Files Modified

### Rust

| File                                                   | Change                                                                                                                                                       |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `verter_core/src/utils/oxc/vue/script/resolve_type.rs` | `get_type_reference_name` → `&str`; add `ImportedType`, `NeedFileRequest`; cross-file lookup in TSTypeReference; add `imported_types`/`from_file` to context |
| `verter_core/src/utils/oxc/vue/script/mod.rs`          | Build context in `parse_script()`; add `sfc_source`, `generic_type_params` params; pass context down                                                         |
| `verter_core/src/utils/oxc/vue/script/setup.rs`        | Thread `type_ctx` through 6+ functions                                                                                                                       |
| `verter_core/src/utils/oxc/vue/script/types.rs`        | Add `need_files` to `ScriptParseResult`                                                                                                                      |
| `verter_core/src/type_host/mod.rs`                     | **NEW**: `CachedTypeScope`, `CachedImport`                                                                                                                   |
| `verter_core/src/type_host/cache.rs`                   | **NEW**: `TypeFileCache`, LRU, `lazy_static`                                                                                                                 |
| `verter_core/src/lib.rs`                               | Add `pub mod type_host`                                                                                                                                      |
| `verter_core/src/builder/codegen.rs`                   | Pass SFC bytes + `None` generic to pre_scan `parse_script`; extend `ViteCodegenResult`                                                                       |
| `verter_core/src/syntax/plugins/analysis/analysis.rs`  | Pass SFC bytes + generic params to `parse_script`                                                                                                            |
| `verter_core/src/codegen/vue/plugin.rs`                | Collect `need_files` from AnalysedScript                                                                                                                     |
| `verter_core/src/codegen/vue/template/types.rs`        | Add `need_files`/`deps` to `TemplateCodegenState`                                                                                                            |
| `verter_napi/src/lib.rs`                               | Add `ingest`, `register_resolution`, `invalidate_file`, `clear_file_cache`; extend result                                                                    |

### JS

| File                                | Change                                                 |
| ----------------------------------- | ------------------------------------------------------ |
| `packages/native/index.ts`          | Type declarations for new functions + extended result  |
| `packages/native/index.js`          | Re-export 4 new functions                              |
| `packages/vite-plugin/src/utils.ts` | `canonicalizePath`, `computeFileVersion`, reverse deps |
| `packages/vite-plugin/src/host.ts`  | **NEW**: iterative compilation loop                    |
| `packages/vite-plugin/src/index.ts` | Host loop in transform, enhanced HMR                   |

### Docs

| File                             | Change  |
| -------------------------------- | ------- |
| `docs/compiler-host-protocol.md` | **NEW** |

---

## Implementation Order

1. **Phase 1** (Rust only) — local resolution + script generics
2. **Phase 2** (Rust + NAPI) — cache, cross-file lookup, NAPI functions
3. **Phase 3** (JS) — Vite plugin host loop, HMR
4. **Phase 4** — docs, integration tests

## Verification

1. `cargo test --package verter_core` — Phase 1 tests
2. `cargo test --workspace` — Phase 2 tests
3. `pnpm run build:native && pnpm run build:ts` — build
4. Manual: example project with imported types, verify correct runtime props + HMR
