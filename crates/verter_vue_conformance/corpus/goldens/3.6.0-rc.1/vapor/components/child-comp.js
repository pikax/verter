import { setInsertionState as _setInsertionState, createSlot as _createSlot, child as _child, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createInvoker as _createInvoker, delegateEvents as _delegateEvents, template as _template } from 'vue';
const t0 = _template("<button class=child-comp> ", 1)
_delegateEvents("click")

export default {
  __name: 'child-comp',
  props: {
  label: { type: String, required: true },
  count: { type: Number, default: 0 },
},
  emits: ["select"],
  __vapor: true,
  setup(__props, { emit: __emit }) {


const emit = __emit

function onClick() {
  emit("select")
}


  const n4 = t0()
  const n1 = _child(n4, 1)
  _setInsertionState(n4, 0)
  const n0 = _createSlot("header")
  _renderEffect(() => _setText(n1, " " + _toDisplayString(__props.label) + " (" + _toDisplayString(__props.count) + ") "))
  _setInsertionState(n4, null, 2)
  const n2 = _createSlot()
  _setInsertionState(n4, null, 3)
  const n3 = _createSlot("footer", { total: () => (__props.count) })
  n4.$evtclick = _createInvoker(onClick)
  return n4

}

}
