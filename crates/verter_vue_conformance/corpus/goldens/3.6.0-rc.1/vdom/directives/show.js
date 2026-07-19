import { vShow as _vShow, withDirectives as _withDirectives, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { ref } from "vue"


export default {
  __name: 'show',
  setup(__props) {

const visible = ref(true)

return (_ctx, _cache) => {
  return _withDirectives((_openBlock(), _createElementBlock("p", null, "Peekaboo", 512 /* NEED_PATCH */)), [
    [_vShow, visible.value]
  ])
}
}

}
