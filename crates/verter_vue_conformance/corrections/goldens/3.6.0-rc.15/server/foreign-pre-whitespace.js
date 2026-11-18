import { ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<!--[--><svg><pre>${
    _ssrInterpolate($setup.n)
  }a b</pre></svg><math><mtext><pre>${
    _ssrInterpolate($setup.n)
  }c  d</pre></mtext><mi><mglyph><pre>${
    _ssrInterpolate($setup.n)
  }e f</pre></mglyph></mi></math><!--]-->`)
}
