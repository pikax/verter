import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createIf as _createIf, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'if-else-if-else',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const status = ref("loading")

const __returned__ = { status, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<p>Loading", 3)
const t1 = _template("<p>Failed", 3)
const t2 = _template("<p> ", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = _createIf(() => (_ctx.status === 'loading'), () => {
    const n2 = t0()
    return n2
  }, () => _createIf(() => (_ctx.status === 'error'), () => {
    const n4 = t1()
    return n4
  }, () => {
    const n7 = t2()
    const x7 = _txt(n7)
    _renderEffect(() => _setText(x7, "Done: " + _toDisplayString(_ctx.status)))
    return n7
  }, 549 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, TRUE_NO_SCOPE, KEYED_INDEX_1 */), 293 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, TRUE_NO_SCOPE, KEYED_INDEX_0 */)
  return n0
}
_sfc_main.render = render
export default _sfc_main
