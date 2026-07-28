<script setup>
import { ref } from 'vue'

const count = ref(0)

function bump(step) {
  count.value += step
}

defineExpose({
  bump,
  count,
  // A METHOD SHORTHAND: its shape is authored on the macro property itself,
  // not on a setup declaration the property names.
  focus(target) {
    return target
  },
  // A DEFAULTED parameter followed by a REQUIRED one. Legal JavaScript — a
  // caller reaches `mode` by passing `undefined` for `region` — but
  // `(region?: any, mode: any)` is TS1016 in a declaration, so the generated
  // surface must render `region` required rather than emit something that does
  // not compile.
  scrollTo(region = 'top', mode) {
    return [region, mode]
  },
  // The expressible case, kept beside it: a trailing run of defaults DOES
  // render `?`, so the repair above is a placement rule and not a blanket
  // "defaults are ignored".
  resize(width, height = 0) {
    return width + height
  },
})
</script>
<template>
  <button @click="bump(1)">{{ count }}</button>
</template>
