<script setup lang="ts">
import Comp from "./Comp.vue";

const foo = {} as InstanceType<typeof Comp>;

foo?.$props.

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
</script>

<template>
  <Comp name="foo" @change="onChange" >
    <!-- <template #foo="{ test }">
      {{ ___VERTER___slotInstance.$slots.header }}</template
    >
    <template #header></template> -->
  </Comp>
  <div @click="onClick"></div>

  <!-- <span>{{ $data.bar + b }}</span> -->
</template>
