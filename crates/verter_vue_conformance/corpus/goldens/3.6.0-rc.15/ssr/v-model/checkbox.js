import { ref } from "vue"


const _sfc_main = {
  __name: 'checkbox',
  setup(__props, { expose: __expose }) {
  __expose();

const agreed = ref(false)

const __returned__ = { agreed, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrLooseContain as _ssrLooseContain, ssrIncludeBooleanAttr as _ssrIncludeBooleanAttr, ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<label${
    _ssrRenderAttrs(_attrs)
  }><input type="checkbox"${
    (_ssrIncludeBooleanAttr((Array.isArray($setup.agreed))
      ? _ssrLooseContain($setup.agreed, null)
      : $setup.agreed)) ? " checked" : ""
  }> Agree</label>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
