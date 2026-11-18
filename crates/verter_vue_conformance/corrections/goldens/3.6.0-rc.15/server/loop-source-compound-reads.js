import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate, ssrRenderList as _ssrRenderList } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${_ssrRenderAttrs(_attrs)}><!--[-->`)
  _ssrRenderList([...$setup.points].reverse(), (p) => {
    _push(`<i>${_ssrInterpolate(p)}</i>`)
  })
  _push(`<!--]--><!--[-->`)
  _ssrRenderList($setup.points.filter(n => $setup.seen.has(n) || n > $setup.limit), (n) => {
    _push(`<i>${_ssrInterpolate(n)}</i>`)
  })
  _push(`<!--]--><!--[-->`)
  _ssrRenderList([...new Set([...$setup.book.current.map(row => row.model)])], (m) => {
    _push(`<i>${_ssrInterpolate(m)}</i>`)
  })
  _push(`<!--]--><!--[-->`)
  _ssrRenderList($setup.points.filter(limit => limit !== $props.owner), (o) => {
    _push(`<i>${_ssrInterpolate(o)}</i>`)
  })
  _push(`<!--]--><!--[-->`)
  _ssrRenderList((function () { return arguments[0] })($setup.points), (f) => {
    _push(`<i>${_ssrInterpolate(f)}</i>`)
  })
  _push(`<!--]--><!--[-->`)
  _ssrRenderList($setup.points, (e) => {
    _push(`<i>${_ssrInterpolate(e)}</i>`)
  })
  _push(`<!--]--><!--[-->`)
  _ssrRenderList($setup.groups, (row, k) => {
    _push(`<p><!--[-->`)
    _ssrRenderList(row.filter(x => x !== row.length && x < $setup.limit), (c) => {
      _push(`<b>${_ssrInterpolate(c)}</b>`)
    })
    _push(`<!--]--></p>`)
  })
  _push(`<!--]--></div>`)
}
