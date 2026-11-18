import { ref } from "vue"


const _sfc_main = {
  __name: 'select',
  setup(__props, { expose: __expose }) {
  __expose();

const picked = ref("a")

const __returned__ = { picked, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrIncludeBooleanAttr as _ssrIncludeBooleanAttr, ssrLooseContain as _ssrLooseContain, ssrLooseEqual as _ssrLooseEqual, ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<select${
    _ssrRenderAttrs(_attrs)
  }><option value="a"${
    (_ssrIncludeBooleanAttr((Array.isArray($setup.picked))
      ? _ssrLooseContain($setup.picked, "a")
      : _ssrLooseEqual($setup.picked, "a"))) ? " selected" : ""
  }>A</option><option value="b"${
    (_ssrIncludeBooleanAttr((Array.isArray($setup.picked))
      ? _ssrLooseContain($setup.picked, "b")
      : _ssrLooseEqual($setup.picked, "b"))) ? " selected" : ""
  }>B</option></select>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
