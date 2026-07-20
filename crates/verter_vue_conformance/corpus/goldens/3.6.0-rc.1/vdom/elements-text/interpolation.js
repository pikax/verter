import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { computed, ref } from "vue"


const _sfc_main = {
  __name: 'interpolation',
  setup(__props, { expose: __expose }) {
  __expose();

const count = ref(1)
const doubled = computed(() => count.value * 2)

const __returned__ = { count, doubled, computed, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("p", null, " Count: " + _toDisplayString($setup.count) + " / Doubled: " + _toDisplayString($setup.doubled) + " / Upper: " + _toDisplayString("hi".toUpperCase()) + " / Sign: " + _toDisplayString($setup.count > 1 ? "many" : "one"), 1 /* TEXT */))
}
_sfc_main.render = render
export default _sfc_main
