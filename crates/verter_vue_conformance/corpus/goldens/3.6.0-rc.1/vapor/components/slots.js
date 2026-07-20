import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createComponent as _createComponent, template as _template } from 'vue';
import ChildComp from "./child-comp.vue"

const _sfc_main = {
  __name: 'slots',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();


const __returned__ = { ChildComp }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<h1>Title", 2)
const t1 = _template("<p> ")
const t2 = _template("<span> ")

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n6 = _createComponent(_ctx.ChildComp, { label: "Slotted" }, {
    "header": () => {
      const n0 = t0()
      return n0
    },
    "default": (_slotProps0) => {
      const n2 = t1()
      const x2 = _txt(n2)
      _renderEffect(() => _setText(x2, _toDisplayString(_slotProps0.rows.length) + " rows"))
      return n2
    },
    "footer": (_slotProps0) => {
      const n4 = t2()
      const x4 = _txt(n4)
      _renderEffect(() => _setText(x4, "Total " + _toDisplayString(_slotProps0.total)))
      return n4
    }
  }, true)
  return n6
}
_sfc_main.render = render
export default _sfc_main
