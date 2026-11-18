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
import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<p${
    _ssrRenderAttrs(_attrs)
  }> Count: ${
    _ssrInterpolate($setup.count)
  } / Doubled: ${
    _ssrInterpolate($setup.doubled)
  } / Upper: ${
    _ssrInterpolate("hi".toUpperCase())
  } / Sign: ${
    _ssrInterpolate($setup.count > 1 ? "many" : "one")
  }</p>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
