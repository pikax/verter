import { mergeProps as _mergeProps, createElementVNode as _createElementVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

const _hoisted_1 = ["onClick", "title"]

import { ref } from 'vue'; 
export default {
  __name: 'App',
  setup(__props) {
const attrs = ref({ id: 'a' }); const label = ref('x'); function setBox(el) {} function pick(item) {}
return (_ctx, _cache) => {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(({ item }) => [
      _createElementVNode("button", _mergeProps(attrs.value, {
        onClick: $event => (pick(item)),
        title: label.value
      }), "x", 16 /* FULL_PROPS */, _hoisted_1),
      _createElementVNode("p", _mergeProps(attrs.value, { ref: setBox }), "y", 16 /* FULL_PROPS */)
    ]),
    _: 1 /* STABLE */
  }))
}
}

}
