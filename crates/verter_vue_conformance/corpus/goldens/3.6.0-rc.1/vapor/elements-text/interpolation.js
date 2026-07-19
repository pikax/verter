import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<p> ", 1)
import { computed, ref } from "vue"


export default {
  __name: 'interpolation',
  __vapor: true,
  setup(__props) {

const count = ref(1)
const doubled = computed(() => count.value * 2)


  const n0 = t0()
  const x0 = _txt(n0)
  _renderEffect(() => {
    const _count = count.value
    _setText(x0, " Count: " + _toDisplayString(_count) + " / Doubled: " + _toDisplayString(doubled.value) + " / Upper: " + _toDisplayString("hi".toUpperCase()) + " / Sign: " + _toDisplayString(_count > 1 ? "many" : "one"))
  })
  return n0

}

}
