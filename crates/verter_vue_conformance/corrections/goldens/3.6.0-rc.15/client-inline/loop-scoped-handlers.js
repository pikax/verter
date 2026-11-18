import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, createElementVNode as _createElementVNode, vModelText as _vModelText, withDirectives as _withDirectives } from "vue"

const _hoisted_1 = ["onClick"]
const _hoisted_2 = ["onUpdate:modelValue"]

import { ref } from 'vue'; 
export default {
  __name: 'App',
  setup(__props) {
const rows = ref([1, 2]); const draft = ref({}); function pick(row) {}
return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(rows.value, (row) => {
      return (_openBlock(), _createElementBlock("div", { key: row }, [
        _createElementVNode("button", {
          onClick: $event => (pick(row))
        }, "x", 8 /* PROPS */, _hoisted_1),
        _createElementVNode("button", {
          onClick: _cache[0] || (_cache[0] = $event => (pick(0)))
        }, "y"),
        _withDirectives(_createElementVNode("input", {
          "onUpdate:modelValue": $event => ((draft.value[row]) = $event)
        }, null, 8 /* PROPS */, _hoisted_2), [
          [_vModelText, draft.value[row]]
        ])
      ]))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
}

}
