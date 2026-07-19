import { setInsertionState as _setInsertionState, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createFor as _createFor, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'array',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const items = ref(["a", "b", "c"])

const __returned__ = { items, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<li> ")
const t1 = _template("<ul>", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n3 = t1()
  _setInsertionState(n3, null, 0)
  const n0 = _createFor(() => (_ctx.items), (_for_item0) => {
    const n2 = t0()
    const x2 = _txt(n2)
    _renderEffect(() => _setText(x2, _toDisplayString(_for_item0.value)))
    return n2
  }, (item) => (item), 9 /* FAST_REMOVE, IS_SINGLE_NODE */)
  return n3
}
_sfc_main.render = render
export default _sfc_main
