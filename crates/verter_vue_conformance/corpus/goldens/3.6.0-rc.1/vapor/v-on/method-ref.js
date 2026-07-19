import { on as _on, template as _template } from 'vue';
const t0 = _template("<form><button>Send", 1)

export default {
  __name: 'method-ref',
  __vapor: true,
  setup(__props) {

function onSubmit() {}


  const n0 = t0()
  _on(n0, "submit", onSubmit)
  return n0

}

}
