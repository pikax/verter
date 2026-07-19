import { txt as _txt, createInvoker as _createInvoker, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, delegateEvents as _delegateEvents, template as _template } from 'vue';
const t0 = _template("<button> ", 1)
_delegateEvents("click")

export default {
  __name: 'props-emits-runtime',
  props: {
  msg: { type: String, default: "hi" },
  level: { type: Number, required: true },
},
  emits: ["save", "cancel"],
  __vapor: true,
  setup(__props, { emit: __emit }) {

const props = __props
const emit = __emit

function onSave() {
  emit("save", props.msg)
}


  const n0 = t0()
  const x0 = _txt(n0)
  n0.$evtclick = _createInvoker(onSave)
  _renderEffect(() => _setText(x0, _toDisplayString(__props.msg) + ":" + _toDisplayString(__props.level)))
  return n0

}

}
