
const _sfc_main = {
  __name: 'key-modifiers',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

function onEnter() {}
function onEsc() {}

const __returned__ = { onEnter, onEsc }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { on as _on, withKeys as _withKeys, template as _template } from 'vue';
const t0 = _template("<input>", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _on(n0, "keyup", _withKeys(_ctx.onEnter, ["enter"]))
  _on(n0, "keydown", _withKeys(_ctx.onEsc, ["esc"]))
  return n0
}
_sfc_main.render = render
export default _sfc_main
