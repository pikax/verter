<script setup lang="ts">
import Comp from "./Comp.vue";

const foo = {} as InstanceType<typeof Comp>


  defineOptions({
    data() {
      return {
        bar: 'foo'
      }
    }
  })

</script>

<template>
  <Comp name="Foo">
    <template #foo="{ name }"></template>
  </Comp>

  <span>{{ $data.bar }}</span>
</template>
