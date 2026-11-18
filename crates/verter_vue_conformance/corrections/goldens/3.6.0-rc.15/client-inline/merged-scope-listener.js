import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString, createElementVNode as _createElementVNode } from "vue"

const _hoisted_1 = ["onClick"]

import { ref } from 'vue'; 
export default {
  __name: 'App',
  setup(__props) {
const rows = ref([{ id: 1, label: 'a' }]); function pick(row) {}
return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(rows.value, (row) => {
      return (_openBlock(), _createElementBlock("div", {
        key: row.id
      }, [
        _createElementVNode("button", {
          onClick: [
            _cache[0] || (_cache[0] = $event => (pick(0))),
            $event => (pick(row))
          ]
        }, _toDisplayString(row.label), 9 /* TEXT, PROPS */, _hoisted_1)
      ]))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
}

}
