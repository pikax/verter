import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createIf as _createIf, template as _template } from 'vue';
const t0 = _template("<p>Loading", 3)
const t1 = _template("<p>Failed", 3)
const t2 = _template("<p> ", 1)
import { ref } from "vue"


export default {
  __name: 'if-else-if-else',
  __vapor: true,
  setup(__props) {

const status = ref("loading")


  const n0 = _createIf(() => (status.value === 'loading'), () => {
    const n2 = t0()
    return n2
  }, () => _createIf(() => (status.value === 'error'), () => {
    const n4 = t1()
    return n4
  }, () => {
    const n7 = t2()
    const x7 = _txt(n7)
    _renderEffect(() => _setText(x7, "Done: " + _toDisplayString(status.value)))
    return n7
  }, 549 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, TRUE_NO_SCOPE, KEYED_INDEX_1 */), 293 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, TRUE_NO_SCOPE, KEYED_INDEX_0 */)
  return n0

}

}
