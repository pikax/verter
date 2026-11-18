import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, createElementVNode as _createElementVNode, vModelText as _vModelText, withDirectives as _withDirectives } from "vue"

const _hoisted_1 = ["onClick"]
const _hoisted_2 = ["onUpdate:modelValue"]

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.rows, (row) => {
      return (_openBlock(), _createElementBlock("div", { key: row }, [
        _createElementVNode("button", {
          onClick: $event => ($setup.pick(row))
        }, "x", 8 /* PROPS */, _hoisted_1),
        _createElementVNode("button", {
          onClick: _cache[0] || (_cache[0] = $event => ($setup.pick(0)))
        }, "y"),
        _withDirectives(_createElementVNode("input", {
          "onUpdate:modelValue": $event => (($setup.draft[row]) = $event)
        }, null, 8 /* PROPS */, _hoisted_2), [
          [_vModelText, $setup.draft[row]]
        ])
      ]))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
