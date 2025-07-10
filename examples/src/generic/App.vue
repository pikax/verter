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


const c = new Comp<'foo'>().$props['___VERTER___v-slot'];
}
</script>

<template>
  <Comp name="foo">
    <template  #foo="{ name }"> {{ ___VERTER___slotInstance.$props }}</template>
    <template #header></template>
  </Comp>

  <span>{{ $data.bar }}</span>
</template>
