# Type display inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

The display repairs change how a type expression is spelled, not what is
retained: the renderer (`verter_type_expr::display`) and the component-meta
fallback printers keep no field, store, hook, counter or trace beyond the
text they return. The print-cost evidence reads the existing counting
allocator of the `verter_session` allocation canaries, which is test-only
and adds nothing to production.

## verter_type_expr

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Renderer work stack (`Frame` worklist, `in_extends`, `bound` and `declaring` binder scopes) | `render_type_expr_display`: the text every hover, metadata display and observation print reads | REQUIRED | One render call | `src/display.rs` | always |
