import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createComponent as _createComponent, template as _template } from 'vue';
const t0 = _template("<h1>Title", 2)
const t1 = _template("<p> ")
const t2 = _template("<span> ")
import ChildComp from "./child-comp.vue"

export default {
  __name: 'slots',
  __vapor: true,
  setup(__props) {



  const n6 = _createComponent(ChildComp, { label: "Slotted" }, {
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

}
