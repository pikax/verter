import { txt as _txt, createInvoker as _createInvoker, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, delegateEvents as _delegateEvents, template as _template } from 'vue';
const t0 = _template("<button> ", 1)
_delegateEvents("click")
import { ref } from "vue"


export default {
  __name: 'inline',
  __vapor: true,
  setup(__props) {

const count = ref(0)


  const n0 = t0()
  const x0 = _txt(n0)
  n0.$evtclick = _createInvoker(() => (count.value++))
  _renderEffect(() => _setText(x0, "Count: " + _toDisplayString(count.value)))
  return n0

}

}
