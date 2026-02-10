# Implementation Guide

## syntax_kai Pipeline Architecture

The `syntax_kai` module provides an event-driven plugin pipeline for compiling Vue SFCs. It tokenizes Vue SFCs into raw events, then processes them through a sequence of plugins that progressively enrich and transform the data.

### Pipeline Overview

```
Tokenizer → Syntax → Raw Events
                        ↓
              [Script Pipeline]
              element_compiler → oxc_parser → code_gen_script
                        ↓
              [Template Pipeline]
              element_compiler → css_style → oxc_parser → code_gen_*
```

Two separate event Vecs are produced by `Syntax`:
- `root_script_events` — events from `<script>` blocks
- `events` — events from `<template>`, `<style>`, and unknown blocks

### Two-tier Type System

The pipeline has two compilation stages with distinct type families:

1. **Compiled\*** (element_compiler output): Props are `Vec<Prop>` — raw syntax, no expression parsing.
2. **OxcCompiled\*** (oxc_parser output): Props are `Vec<OxcProp<'alloc>>` with parsed AST expressions. Scopes extracted into ordered `Vec<ElementScope>`.

### Event Ownership Pattern

When a plugin replaces an event, the new event takes ownership of the original via an `event` field:

```rust
pub struct OxcInterpolation<'a> {
    pub expression: Option<Expression<'a>>,
    pub event: Interpolation,  // owns the original
}
```

### Plugin Trait

```rust
pub trait SyntaxPlugin<'a> {
    fn name(&self) -> &str;
    fn process_event(&mut self, event: Event<'a>, ctx: &mut SyntaxPluginContext<'a>) -> SyntaxResult<Event<'a>>;
}

pub enum SyntaxResult<E> {
    Keep(E),    // forward unchanged
    Replace(E), // forward transformed
    Drop,       // remove from pipeline
}
```

### Binding Metadata Flow

1. `code_gen_script` processes `OxcScript` → walks AST → classifies bindings → emits `Event::ScriptBindings(BindingMetadata)`
2. Builder prepends `ScriptBindings` event to template pipeline
3. Codegen plugins (VDOM/Vapor/TSX) consume `ScriptBindings` event, clone metadata into their state
4. During codegen, `resolve_binding_prefix()` / `resolve_binding_suffix()` determine correct accessor

### ReactivityLevel Abstraction

```rust
pub enum ReactivityLevel { Static, Dynamic }
```

- **VDOM**: Static → skip dynamic_props/patch flag. Dynamic → add to patch flag.
- **Vapor**: Static → one-time `_setProp()`. Dynamic → wrap in `_renderEffect()`.
- **TSX**: Static → inline literal. Dynamic → accessor with correct prefix.

### Scope Extraction Priority

When processing element props, structural directives are extracted in priority order:
1. `v-if` / `v-else-if` / `v-else` (pushed first)
2. `v-for` (pushed second)
3. `v-slot` (pushed third)

### CSS Style Plugin

Processes `CompiledStyleStart`/`CompiledStyleEnd` events:
- **Scoped CSS**: Transforms selectors with `[data-v-{scope_id}]`
- **v-bind()**: Extracts expressions, replaces with `var(--{id}-name)`
- **CSS Modules**: Hashes class names, builds runtime mappings

Emits `Event::ProcessedStyle(ProcessedStyleBlock)` containing transformed CSS, v-bind expressions, and module info.

### No String Rule

Use `&[u8]` or `Span` referencing original source throughout the pipeline. `String` is only acceptable in:
- Codegen plugins (building output text)
- Builder functions (parameters and return types)

### TDD Workflow

1. Write failing tests first (construct event Vecs, run through plugin, assert output)
2. Implement the plugin
3. Run `cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings && cargo fmt --all`
4. Run `cargo test --package verter_core --lib`

### Adding a New Codegen Plugin

1. Create `syntax_kai/plugins/my_plugin/mod.rs` and `my_plugin.rs`
2. Implement `SyntaxPlugin` trait
3. Register in `syntax_kai/plugins/mod.rs`
4. Add to builder pipeline in `builder/codegen_kai.rs`
