# syntax_kai — Event-driven Vue SFC Pipeline

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

Metadata Events (code_gen_script output):
  ScriptBindings (BindingMetadata)
```

## Plugin Pipeline Order

### Script pipeline
`element_compiler → oxc_parser → code_gen_script`

### Template pipeline
`element_compiler → css_style → oxc_parser → code_gen_template`

Where `code_gen_template` is one of:
- `code_gen_template` (VDOM mode)
- `code_gen_template_vapor` (Vapor mode)
- `code_gen_tsx` (TSX for type checking)

## Binding Resolution Order

1. Check scope stack (v-for locals, slot params) — bare identifier
2. Check `BindingMetadata` — setup/props/data/options prefix
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
| `syntax.rs` | Tokenizer → Event conversion |
| `plugin.rs` | SyntaxPlugin trait and SyntaxResult |
| `binding_types.rs` | BindingType, ReactivityLevel, BindingMetadata |
| `plugins/element_compiler/` | Raw events → Compiled events |
| `plugins/oxc_parser/` | Compiled events → OXC-parsed events |
| `plugins/code_gen_script/` | OxcScript → BindingMetadata extraction |
| `plugins/css_style/` | Scoped CSS, v-bind(), CSS Modules |
| `plugins/code_gen_template/` | VDOM render function codegen |
| `plugins/code_gen_template_vapor/` | Vapor mode codegen |
| `plugins/code_gen_tsx/` | TSX codegen for type checking |
