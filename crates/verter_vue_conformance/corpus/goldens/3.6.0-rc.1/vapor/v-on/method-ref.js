import { on as _on, template as _template } from 'vue';

const _sfc_main = {
  __name: 'method-ref',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

function onSubmit() {}

const __returned__ = { onSubmit }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<form><button>Send", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _on(n0, "submit", _ctx.onSubmit)
  return n0
}
_sfc_main.render = render
export default _sfc_main
