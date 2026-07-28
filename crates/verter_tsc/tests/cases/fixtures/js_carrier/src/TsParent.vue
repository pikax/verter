<script setup lang="ts">
import JsExpose from './JsExpose.vue'

// The point of the exposed surface: a TypeScript PARENT consumes it. A member
// typed `unknown` cannot be called at all (TS2571/TS18046), so a JavaScript
// child whose expose entries degrade to `unknown` moves the false diagnostic
// from the child to every consumer.
function drive(child: InstanceType<typeof JsExpose>) {
  // A `function` declaration named by a shorthand property.
  child.bump(1)
  // A METHOD SHORTHAND, whose shape is authored on the macro property.
  child.focus('main')
  // A defaulted parameter followed by a required one: `region` cannot be
  // spelled `?` without TS1016, so it renders required and BOTH arguments are
  // passed here.
  child.scrollTo('top', 'smooth')
  // A trailing default IS expressible, so `height` really is optional.
  child.resize(10)
  child.resize(10, 20)
}

defineExpose({ drive })
</script>
<template>
  <JsExpose />
</template>
