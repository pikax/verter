import { child as _child, next as _next, on as _on, withModifiers as _withModifiers, withVaporModifiers as _withModifiers1, delegateEvents as _delegateEvents, template as _template } from 'vue';
const t0 = _template("<div><button>Delete</button><a href=/x>Open", 1)
_delegateEvents("click")

export default {
  __name: 'modifiers',
  __vapor: true,
  setup(__props) {

function save() {}
function del() {}
function open() {}


  const n2 = t0()
  const n0 = _child(n2)
  const n1 = _next(n0, 1)
  _on(n0, "click", _withModifiers(del, ["stop","prevent"]))
  _on(n1, "click", open, {
    once: true,
    capture: true
  })
  n2.$evtclick = _withModifiers1(save, ["self"])
  return n2

}

}
