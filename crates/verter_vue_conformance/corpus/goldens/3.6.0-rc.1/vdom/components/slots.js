import { createElementVNode as _createElementVNode, toDisplayString as _toDisplayString, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"
import ChildComp from "./child-comp.vue"

const _sfc_main = {
  __name: 'slots',
  setup(__props, { expose: __expose }) {
  __expose();


const __returned__ = { ChildComp }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createBlock($setup["ChildComp"], { label: "Slotted" }, {
    header: _withCtx(() => [...(_cache[0] || (_cache[0] = [
      _createElementVNode("h1", null, "Title", -1 /* CACHED */)
    ]))]),
    default: _withCtx(({ rows }) => [
      _createElementVNode("p", null, _toDisplayString(rows.length) + " rows", 1 /* TEXT */)
    ]),
    footer: _withCtx(({ total }) => [
      _createElementVNode("span", null, "Total " + _toDisplayString(total), 1 /* TEXT */)
    ]),
    _: 1 /* STABLE */
  }))
}
_sfc_main.render = render
export default _sfc_main
