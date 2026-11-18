import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createFor as _createFor, template as _template } from 'vue';
const t0 = _template("<div> ")

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = _createFor(() => (_ctx.rows.map(({ [_ctx.field]: value }) => value)), (_for_item0) => {
    const n2 = t0()
    const x2 = _txt(n2)
    _renderEffect(() => _setText(x2, _toDisplayString(_for_item0.value)))
    return n2
  }, undefined, 8 /* IS_SINGLE_NODE */)
  const n3 = _createFor(() => ((() => { const { [_ctx.field]: entries } = _ctx.table; return entries })()), (_for_item0) => {
    const n5 = t0()
    const x5 = _txt(n5)
    _renderEffect(() => _setText(x5, _toDisplayString(_for_item0.value)))
    return n5
  }, undefined, 8 /* IS_SINGLE_NODE */)
  return [n0, n3]
}
