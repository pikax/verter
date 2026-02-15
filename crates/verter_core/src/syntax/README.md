# syntax — Event-driven Vue SFC Pipeline

## Architecture

```
Vue SFC Source
    ↓
[Tokenizer] → TokenizerEvent stream
    ↓
[Syntax] → Vec<Event> (template/style) + Vec<Event> (script)
    ↓
[Plugin Pipeline] → Processed events + codegen output
```

## Event Type Hierarchy

```
Raw Events (Syntax output):
  OpenTag, Prop, OpenTagEnd, CloseTag, Text, Interpolation, Comment
  RootOpenTagEnd, RootCloseTag

Compiled Events (element_compiler output):
  CompiledScriptStart/End, CompiledTemplateStart/End
  CompiledStyleStart/End, CompiledUnknownStart/End
  ElementStart (CompiledElementStart), ElementClosed (CompiledElementClosed)

OXC-parsed Events (oxc_parser output):
  OxcScript, OxcProp, OxcInterpolation
  OxcCompiledElementStart, OxcCompiledElementClosed

CSS Events (css_style output):
  ProcessedStyle (ProcessedStyleBlock)

Note: Script bindings are carried inside OxcScript.result.bindings
```

## Plugin Pipeline Order

### Script pipeline
`element_compiler → oxc_parser → code_gen/script`

### Template pipeline
`element_compiler → css_parser → oxc_parser → code_gen/template`

Where `code_gen/template` targets one of:
- `vdom/` (VDOM render function mode)
- `vapor/` (Vapor mode)
- TSX (for type checking — future)

## Binding Resolution Order

1. Check scope stack (v-for locals, slot params) — bare identifier
2. Check binding entries (from `OxcScript.result.bindings`) — setup/props/data/options prefix
3. Fallback: `_ctx.` prefix

## Testing Patterns

```rust
// Construct events manually
let events = vec![
    Event::ElementStart(CompiledElementStart { ... }),
    Event::Text(Text { ... }),
    Event::ElementClosed(CompiledElementClosed { ... }),
];

// Run through plugin
let output = run_pipeline(events, &mut [&mut my_plugin], &mut ctx);

// Assert output
assert!(my_plugin.take_output().contains("expected"));
```

## Files

| File | Purpose |
|------|---------|
| `types.rs` | All event and type definitions |
| `pipeline.rs` | Tokenizer → Event conversion |
| `plugin.rs` | SyntaxPlugin trait and SyntaxResult |
| `binding_types.rs` | BindingType, ReactivityLevel, binding resolution helpers |
| `plugins/element_compiler/` | Raw events → Compiled events |
| `plugins/oxc_parser/` | Compiled events → OXC-parsed events |
| `plugins/css_parser/` | Scoped CSS, v-bind(), CSS Modules |
| `plugins/code_gen/script/` | Script codegen (macros, bindings, sections) |
| `plugins/code_gen/template/vdom/` | VDOM render function codegen |
| `plugins/code_gen/template/vapor/` | Vapor mode codegen |
| `plugins/code_gen/css/` | CSS output generation |
| `plugins/code_gen/types.rs` | Shared codegen types |
