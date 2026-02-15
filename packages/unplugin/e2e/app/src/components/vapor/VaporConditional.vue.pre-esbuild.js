import {defineComponent as _defineComponent} from 'vue';
import { ref } from 'vue'
import {template as _template,delegateEvents as _delegateEvents,createIf as _createIf,createInvoker as _createInvoker,child as _child,setInsertionState as _setInsertionState} from 'vue';



const _sfc_main = /*@__PURE__*/_defineComponent({
__name: 'VaporConditional',__vapor: true,setup(__props){


const show = ref(true)

function toggle() {
  show.value = !show.value
}






return { show, toggle }
}});


const t0 = _template("<span data-testid=\"vapor-visible\">Visible")
const t1 = _template("<span data-testid=\"vapor-hidden\">Hidden")
const t2 = _template("")
_delegateEvents("click")

function render(_ctx) {

    const n0 = t2()
  _setInsertionState(n0, null, 1, true)
  const n2 = _createIf(() => (_ctx.show), () => {
    const n3 = t0()
  const n1 = _child(n0)
    n1.$evtclick = _createInvoker(e => _ctx.toggle(e))
    return n3
  }, () => {
    const n5 = t1()
    return n5
  }, null, 0)

  return n0
}

_sfc_main.render = render

export default _sfc_main