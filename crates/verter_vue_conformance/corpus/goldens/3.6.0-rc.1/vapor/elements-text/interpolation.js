import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, template as _template } from 'vue';
import { computed, ref } from "vue"


const _sfc_main = {
  __name: 'interpolation',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const count = ref(1)
const doubled = computed(() => count.value * 2)

const __returned__ = { count, doubled, computed, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<p> ", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  const x0 = _txt(n0)
  _renderEffect(() => {
    const _count = _ctx.count
    _setText(x0, " Count: " + _toDisplayString(_count) + " / Doubled: " + _toDisplayString(_ctx.doubled) + " / Upper: " + _toDisplayString("hi".toUpperCase()) + " / Sign: " + _toDisplayString(_count > 1 ? "many" : "one"))
  })
  return n0
}
_sfc_main.render = render
export default _sfc_main
