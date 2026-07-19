import { withVaporKeys as _withKeys, delegateEvents as _delegateEvents, template as _template } from 'vue';

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
const t0 = _template("<input>", 1)
_delegateEvents("keyup", "keydown")

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  n0.$evtkeyup = _withKeys(_ctx.onEnter, ["enter"])
  n0.$evtkeydown = _withKeys(_ctx.onEsc, ["esc"])
  return n0
}
_sfc_main.render = render
export default _sfc_main
