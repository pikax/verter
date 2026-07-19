import { child as _child, next as _next, on as _on, withModifiers as _withModifiers, withVaporModifiers as _withModifiers1, delegateEvents as _delegateEvents, template as _template } from 'vue';

const _sfc_main = {
  __name: 'modifiers',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

function save() {}
function del() {}
function open() {}

const __returned__ = { save, del, open }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<div><button>Delete</button><a href=/x>Open", 1)
_delegateEvents("click")

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n2 = t0()
  const n0 = _child(n2)
  const n1 = _next(n0, 1)
  _on(n0, "click", _withModifiers(_ctx.del, ["stop","prevent"]))
  _on(n1, "click", _ctx.open, {
    once: true,
    capture: true
  })
  n2.$evtclick = _withModifiers1(_ctx.save, ["self"])
  return n2
}
_sfc_main.render = render
export default _sfc_main
