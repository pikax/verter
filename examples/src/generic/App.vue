<script setup lang="ts">
import Comp from "./Comp.vue";

const foo = {} as InstanceType<typeof Comp>;

defineOptions({
  data() {
    return {
      bar: "foo",
    };
  },
});

const c = new Comp<"foo">().$props["___VERTER___v-slot"];
</script>

<template>
  <Comp name="foo">
    <template #foo="{ test }"> {{ ___VERTER___slotInstance.$slots.header }}</template>
    <template #header></template>
  </Comp>

  <span>{{ $data.bar + b }}</span>
</template>
