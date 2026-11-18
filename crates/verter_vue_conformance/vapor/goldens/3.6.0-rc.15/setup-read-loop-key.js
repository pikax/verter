import { setInsertionState as _setInsertionState, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createFor as _createFor, template as _template } from 'vue';
const t0 = _template("<p> ")
const t1 = _template("<i> ")
const t2 = _template("<div>")

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = _createFor(() => (_ctx.groups), (_for_item0) => {
    const n8 = t2()
    _setInsertionState(n8)
    const n2 = _createFor(() => (_for_item0.value.items), (_for_item1) => {
      const n4 = t0()
      const x4 = _txt(n4)
      _renderEffect(() => _setText(x4, _toDisplayString(_for_item1.value.id)))
      return n4
    }, (item) => (`${_for_item0.value.id}/${item.id}/${_ctx.seen[item.id] ?? 0}`), 8 /* IS_SINGLE_NODE */)
    _setInsertionState(n8, 1)
    const n5 = _createFor(() => (_for_item0.value.children), (_for_item1) => {
      const n7 = t1()
      const x7 = _txt(n7)
      _renderEffect(() => _setText(x7, _toDisplayString(_for_item1.value.id)))
      return n7
    }, (group) => (group.id + _ctx.seen.base), 8 /* IS_SINGLE_NODE */)
    return n8
  }, (group) => (_ctx.keyOf(group)), 8 /* IS_SINGLE_NODE */)
  return n0
}
