import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList([...$setup.points].reverse(), (p) => {
      return (_openBlock(), _createElementBlock("i", { key: p }, _toDisplayString(p), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.points.filter(n => $setup.seen.has(n) || n > $setup.limit), (n) => {
      return (_openBlock(), _createElementBlock("i", { key: n }, _toDisplayString(n), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList([...new Set([...$setup.book.current.map(row => row.model)])], (m) => {
      return (_openBlock(), _createElementBlock("i", { key: m }, _toDisplayString(m), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.points.filter(limit => limit !== $props.owner), (o) => {
      return (_openBlock(), _createElementBlock("i", { key: o }, _toDisplayString(o), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList((function () { return arguments[0] })($setup.points), (f) => {
      return (_openBlock(), _createElementBlock("i", { key: f }, _toDisplayString(f), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.points, (e) => {
      return (_openBlock(), _createElementBlock("i", { key: e }, _toDisplayString(e), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.groups, (row, k) => {
      return (_openBlock(), _createElementBlock("p", { key: k }, [
        (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(row.filter(x => x !== row.length && x < $setup.limit), (c) => {
          return (_openBlock(), _createElementBlock("b", { key: c }, _toDisplayString(c), 1 /* TEXT */))
        }), 128 /* KEYED_FRAGMENT */))
      ]))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
