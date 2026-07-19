import { createElementVNode as _createElementVNode, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"


export default {
  __name: 'method-ref',
  setup(__props) {

function onSubmit() {}

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("form", { onSubmit: onSubmit }, [...(_cache[0] || (_cache[0] = [
    _createElementVNode("button", null, "Send", -1 /* CACHED */)
  ]))], 32 /* NEED_HYDRATION */))
}
}

}
