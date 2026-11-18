import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString, createElementVNode as _createElementVNode } from "vue"


export default {
  __name: 'App',
  setup(__props) {
const lines = ['a', 'b'];
return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("pre", null, [
    (_openBlock(), _createElementBlock(_Fragment, null, _renderList(lines, (line) => {
      return _createElementVNode("span", { key: line }, _toDisplayString(line) + "\n", 1 /* TEXT */)
    }), 64 /* STABLE_FRAGMENT */))
  ]))
}
}

}
