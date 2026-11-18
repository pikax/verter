import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.groups, (row, i) => {
      return (_openBlock(), _createElementBlock("div", { key: i }, [
        (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(row, (cell) => {
          return (_openBlock(), _createElementBlock("span", { key: cell }, _toDisplayString(cell), 1 /* TEXT */))
        }), 128 /* KEYED_FRAGMENT */))
      ]))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
