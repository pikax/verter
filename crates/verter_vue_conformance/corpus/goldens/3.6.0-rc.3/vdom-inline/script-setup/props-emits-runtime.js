import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"


const _sfc_main = {
  __name: 'props-emits-runtime',
  props: {
  msg: { type: String, default: "hi" },
  level: { type: Number, required: true },
},
  emits: ["save", "cancel"],
  setup(__props, { emit: __emit }) {

const props = __props
const emit = __emit

function onSave() {
  emit("save", props.msg)
}

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("button", { onClick: onSave }, _toDisplayString(__props.msg) + ":" + _toDisplayString(__props.level), 1 /* TEXT */))
}
}

}
export default _sfc_main
