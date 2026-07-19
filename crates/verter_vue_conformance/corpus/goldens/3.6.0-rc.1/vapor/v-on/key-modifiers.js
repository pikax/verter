import { withVaporKeys as _withKeys, delegateEvents as _delegateEvents, template as _template } from 'vue';
const t0 = _template("<input>", 1)
_delegateEvents("keyup", "keydown")

export default {
  __name: 'key-modifiers',
  __vapor: true,
  setup(__props) {

function onEnter() {}
function onEsc() {}


  const n0 = t0()
  n0.$evtkeyup = _withKeys(onEnter, ["enter"])
  n0.$evtkeydown = _withKeys(onEsc, ["esc"])
  return n0

}

}
