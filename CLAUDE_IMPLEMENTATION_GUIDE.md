# Implementation Guide

## AST-Based Compilation Pipeline

The `verter_core` crate compiles Vue SFCs through a linear 5-phase pipeline orchestrated by `compile()` in `compile.rs`.

### Pipeline Overview

```
Vue SFC Source
    ↓
[Tokenizer]   byte-level SFC tokenization (tokenizer/byte.rs)
    ↓
[Parser]      event-driven state machine → arena-based template AST (parser/mod.rs)
    ↓
[Style]       v-bind() scan + CSS processing (style/ + css/)
    ↓
[Script]      macro expansion, binding extraction (script/)
    ↓
[Template]    OXC expression parsing + render function codegen (template/)
```

### Arena-Based Template AST

The parser builds a flat `Vec<AstNode>` arena with O(1) navigation:

```rust
pub struct TemplateAst {
    nodes: Vec<AstNode>,        // flat arena
    root: RootNodeTemplate,
}

pub struct AstNode {
    kind: AstNodeKind,          // Element | Text | Comment | Interpolation
    parent: Option<NodeId>,     // O(1) parent lookup
    index_in_parent: usize,     // O(1) sibling lookup
}
```

`ElementNode` pre-computes metadata during parsing to avoid re-scanning in codegen:
- `tag_type`: Element / Component / SlotOutlet / Template
- `prop_flag`: Bitset of prop characteristics (has class, style, spread, etc.)
- `children_flag`: Bitset of children characteristics (has text, elements, v-if, etc.)
- `children_mode`: Enum for codegen branching (Empty, TextOnly, SingleElement, Mixed, etc.)
- Cached directives: `v_condition`, `v_for`, `v_slot`, `v_once`, `v_ref`

### CodeTransform (Deferred Mutations)

All codegen phases use `CodeTransform` — a chunk-based deferred mutation engine:

```rust
let mut ct = CodeTransform::new(input, &allocator);
ct.overwrite(start, end, replacement);  // deferred
ct.prepend_left(pos, content);          // deferred
let output = ct.build_string();         // single-pass concatenation
```

Key features:
- `cursor_hint`: Accelerates forward-progressing access patterns to amortized O(1)
- `output_delta`: Incremental length tracking avoids full scan
- Pre-allocated chunk capacity: `source_len / 13` (empirically tuned)

### Binding Metadata Flow

1. `script/process.rs` parses `<script setup>` → walks AST → classifies bindings as `BindingType` (SetupConst, SetupRef, Props, etc.)
2. Bindings passed to `template/code_gen/` via `generate_template()` parameter
3. `BindingResolver` determines correct accessor prefix (`_ctx.`, `$setup.`, `__props.`) and suffix (`.value` for refs)
4. Binding patches accumulated in `CodeGenOutput`, batch-applied to `CodeTransform`

### Template Codegen Backends

Three backends implement the `TemplateCodeGen` trait, called by `walker::walk_template()` in DFS order:

- **VDOM** (`vdom/`): In-place source overwrites producing `_createElementVNode()` calls
- **Vapor** (`vapor/`): Replaces entire template block with direct DOM manipulation code
- **Vapor2** (`vapor2/`): Experimental alternative Vapor approach (kept for comparison)

### CSS Processing Pipeline

```
Style block content
    ↓ style/v_bind.rs     — scan v-bind() expressions
    ↓ css/prepass.rs       — replace Vue syntax with CSS markers
    ↓ lightningcss         — parse + normalize CSS
    ↓ css/modules.rs       — hash class names (CSS Modules)
    ↓ css/scoped.rs        — insert [data-v-xxx] attribute selectors
```

### Cross-File Type Resolution

External types for macros like `defineProps<ExternalType>()` are pre-resolved by the host:
1. Host detects type dependencies from imports
2. Host resolves types from its file store
3. Host passes resolved types via `VerterCompileOptions::external_types`
4. `script/process.rs` merges external types with companion `<script>` types

The Rust compiler never does file I/O — all external resolution is the host's responsibility.

### TDD Workflow

1. Write failing tests first
2. Implement the minimum code to pass
3. Run `cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings && cargo fmt --all`
4. Run `cargo test --package verter_core --lib`

### Test Validation Pattern

All codegen tests must validate generated JS syntax:

```rust
let result = compile_sfc(source);
let tpl = result.template.unwrap();
// Parse generated code with OXC to verify valid JS
let parsed = oxc_parser::Parser::new(&alloc, &tpl.code, source_type).parse();
assert!(parsed.errors.is_empty(), "JS parse error: {:?}\n{}", parsed.errors, tpl.code);
```
