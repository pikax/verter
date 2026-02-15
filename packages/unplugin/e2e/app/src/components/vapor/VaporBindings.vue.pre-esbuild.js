import {defineComponent as _defineComponent} from 'vue';
import { ref, computed } from 'vue'
import {template as _template,setText as _setText,setStyle as _setStyle,delegateEvents as _delegateEvents,renderEffect as _renderEffect,toDisplayString as _toDisplayString,txt as _txt,createInvoker as _createInvoker,child as _child,next as _next} from 'vue';



const _sfc_main = /*@__PURE__*/_defineComponent({
__name: 'VaporBindings',__vapor: true,setup(__props){


const color = ref('blue')
const fontSize = ref(16)
const styleObj = computed(() => ({
  color: color.value,
  fontSize: fontSize.value + 'px',
}))

function toggleColor() {
  color.value = color.value === 'blue' ? 'red' : 'blue'
}

function bigger() {
  fontSize.value += 2
}






return { styleObj, color, fontSize, toggleColor, bigger }
}});


const t0 = _template("<div data-testid=\"vapor-bindings\"><span data-testid=\"vapor-styled\">Styled text</span><button data-testid=\"vapor-color\">Toggle Color</button><button data-testid=\"vapor-bigger\">Bigger</button><span data-testid=\"vapor-color-val\"> </span><span data-testid=\"vapor-size-val\"> ")
_delegateEvents("click")

function render(_ctx) {

    const n0 = t0()
  const n1 = _child(n0)
  const n2 = _next(n1, 1)
  const n3 = _next(n2, 2)
  const n4 = _next(n3, 3)
  const n5 = _next(n4, 4)
  const x0 = _txt(n4)
  const x1 = _txt(n5)
  _renderEffect(() => {
    _setStyle(n1, _ctx.styleObj)
    _setText(x0, _toDisplayString(_ctx.color))
    _setText(x1, _toDisplayString(_ctx.fontSize))
  })
  n2.$evtclick = _createInvoker(e => _ctx.toggleColor(e))
  n3.$evtclick = _createInvoker(e => _ctx.bigger(e))

  return n0
}

_sfc_main.render = render

export default _sfc_main