import { setInsertionState as _setInsertionState, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createFor as _createFor, template as _template } from 'vue';
const t0 = _template("<li> ")
const t1 = _template("<ul>", 1)
import { ref } from "vue"


export default {
  __name: 'object',
  __vapor: true,
  setup(__props) {

const scores = ref({ alice: 3, bob: 5 })


  const n3 = t1()
  _setInsertionState(n3, null, 0)
  const n0 = _createFor(() => (scores.value), (_for_item0, _for_key0) => {
    const n2 = t0()
    const x2 = _txt(n2)
    _renderEffect(() => _setText(x2, _toDisplayString(_for_key0.value) + ": " + _toDisplayString(_for_item0.value)))
    return n2
  }, (score, name) => (name), 9 /* FAST_REMOVE, IS_SINGLE_NODE */)
  return n3

}

}
