<script setup lang="ts">
import JsExpose from './JsExpose.vue'

// The other half of "callable": the exposed method's ARITY is still checked.
// `focus(target)` takes one parameter, so this call is an error (TS2554) — and
// it is the assertion that a permissive `(...args: any[]) => any` fallback
// would silently accept.
function misuse(child: InstanceType<typeof JsExpose>) {
  child.focus('main', 'extra')
  // `scrollTo(region = 'top', mode)` renders BOTH parameters required — a
  // default before a required parameter cannot be spelled `?` — so one
  // argument is short.
  child.scrollTo('top')
}

defineExpose({ misuse })
</script>
<template>
  <JsExpose />
</template>
