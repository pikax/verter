import { child as _child, setDynamicProps as _setDynamicProps, setElementText as _setElementText, setDOMProp as _setDOMProp, setAttr as _setAttr, setHtml as _setHtml, setValue as _setValue, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<svg>", 0, 1)
const t1 = _template("<svg><text>", 0, 1)
const t2 = _template("<symbol>", 0, 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  const n2 = t1()
  const n1 = _child(n2)
  const n3 = t2()
  _renderEffect(() => {
    _setDynamicProps(n0, [_ctx.attrs], null, true)
    _setDynamicProps(n2, [{ [_ctx.name || ""]: _ctx.value }], null, true)
    _setElementText(n1, _ctx.label)
    _setDOMProp(n1, "ariaLabel", _ctx.hint)
    _setAttr(n1, "aria-label", _ctx.note, true)
    _setHtml(n3, _ctx.icon)
    _setValue(n3, _ctx.level)
  })
  return [n0, n2, n3]
}
