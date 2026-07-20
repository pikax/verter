import { renderSlot as _renderSlot, resolveDynamicComponent as _resolveDynamicComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock, createElementVNode as _createElementVNode, Fragment as _Fragment, createElementBlock as _createElementBlock } from "vue"
const _sfc_main = {}
function render(_ctx, _cache) {
  return (_openBlock(), _createElementBlock(_Fragment, null, [
    (_openBlock(), _createBlock(_resolveDynamicComponent(_ctx.current), { class: "control" }, {
      default: _withCtx(() => [
        _renderSlot(_ctx.$slots, "default")
      ]),
      _: 3 /* FORWARDED */
    })),
    _cache[0] || (_cache[0] = _createElementVNode("input", { type: "checkbox" }, null, -1 /* CACHED */))
  ], 64 /* STABLE_FRAGMENT */))
}
_sfc_main.render = render
export default _sfc_main
