import { createElementVNode as _createElementVNode, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, createCommentVNode as _createCommentVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, createBlock as _createBlock } from "vue"

const visible = true;
export default {
  __name: 'App',
  setup(__props) {

return (_ctx, _cache) => {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      visible
        ? (_openBlock(), _createElementBlock(_Fragment, { key: 0 }, [
            _cache[0] || (_cache[0] = _createElementVNode("b", null, null, -1 /* CACHED */)),
            _cache[1] || (_cache[1] = _createElementVNode("i", null, null, -1 /* CACHED */))
          ], 64 /* STABLE_FRAGMENT */))
        : _createCommentVNode("v-if", true)
    ]),
    _: 1 /* STABLE */
  }))
}
}

}
