import { createAssetComponent as _createAssetComponent } from 'vue';

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const _on_b = () => (_ctx.save())
  const _on_c = () => (_ctx.count = 1)
  const _on_d = (v) => _ctx.count = v
  const _on_e = function (v) { _ctx.count = v }
  const _on_f = () => {_ctx.save(); _ctx.save()}
  const _on_g = $event => (_ctx.count = $event)
  const _on_i = () => (_ctx.store.reset())
  const n0 = _createAssetComponent("Panel", {
    onA: () => _ctx.save,
    onB: () => _on_b,
    onC: () => _on_c,
    onD: () => _on_d,
    onE: () => _on_e,
    onF: () => _on_f,
    onG: () => _on_g,
    onH: () => _ctx.store.reset,
    onI: () => _on_i
  }, null, true)
  return n0
}
