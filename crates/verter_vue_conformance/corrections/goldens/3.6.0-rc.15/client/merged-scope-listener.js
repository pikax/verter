import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString, createElementVNode as _createElementVNode } from "vue"

const _hoisted_1 = ["onClick"]

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.rows, (row) => {
      return (_openBlock(), _createElementBlock("div", {
        key: row.id
      }, [
        _createElementVNode("button", {
          onClick: [
            _cache[0] || (_cache[0] = $event => ($setup.pick(0))),
            $event => ($setup.pick(row))
          ]
        }, _toDisplayString(row.label), 9 /* TEXT, PROPS */, _hoisted_1)
      ]))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
