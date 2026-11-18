import { ref } from "vue"


const _sfc_main = {
  __name: 'object',
  setup(__props, { expose: __expose }) {
  __expose();

const scores = ref({ alice: 3, bob: 5 })

const __returned__ = { scores, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate, ssrRenderList as _ssrRenderList } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<ul${_ssrRenderAttrs(_attrs)}><!--[-->`)
  _ssrRenderList($setup.scores, (score, name) => {
    _push(`<li>${
      _ssrInterpolate(name)
    }: ${
      _ssrInterpolate(score)
    }</li>`)
  })
  _push(`<!--]--></ul>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
