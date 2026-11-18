import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString, unref as _unref } from "vue"

import { ref } from 'vue'

export default {
  __name: 'App',
  props: { owner: Number },
  setup(__props) {


const points = ref([3, 1, 2])
const seen = new Set([1])
const book = { current: [{ model: 'a' }] }
const limit = ref(2)
const groups = ref([[1, 2]])

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList([...points.value].reverse(), (p) => {
      return (_openBlock(), _createElementBlock("i", { key: p }, _toDisplayString(p), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(points.value.filter(n => _unref(seen).has(n) || n > limit.value), (n) => {
      return (_openBlock(), _createElementBlock("i", { key: n }, _toDisplayString(n), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList([...new Set([...book.current.map(row => row.model)])], (m) => {
      return (_openBlock(), _createElementBlock("i", { key: m }, _toDisplayString(m), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(points.value.filter(limit => limit !== __props.owner), (o) => {
      return (_openBlock(), _createElementBlock("i", { key: o }, _toDisplayString(o), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList((function () { return arguments[0] })(points.value), (f) => {
      return (_openBlock(), _createElementBlock("i", { key: f }, _toDisplayString(f), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(points.value, (e) => {
      return (_openBlock(), _createElementBlock("i", { key: e }, _toDisplayString(e), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */)),
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(groups.value, (row, k) => {
      return (_openBlock(), _createElementBlock("p", { key: k }, [
        (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(row.filter(x => x !== row.length && x < limit.value), (c) => {
          return (_openBlock(), _createElementBlock("b", { key: c }, _toDisplayString(c), 1 /* TEXT */))
        }), 128 /* KEYED_FRAGMENT */))
      ]))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
}

}
