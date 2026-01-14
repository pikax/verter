<script setup lang="ts">
import Comp from "./Comp.vue";

defineOptions({
  data() {
    return {
      bar: "foo",
    };
  },
});

function onChange(e) {
  e;
}
function onClick(e) {
  e;
}
const foo = {} as InstanceType<typeof Comp>;

</script>

<template>
  <Comp name="foo" @change.once="onChange">
    <!-- <template #foo="{ test }">
      {{ ___VERTER___slotInstance.$slots.header }}</template
    >
    <template #header></template> -->
  </Comp>
  <div @click.prevent="onClick"></div>

  <!-- <span>{{ $data.bar + b }}</span> -->
</template>
