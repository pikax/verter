<script setup lang="ts">
import Comp from "./Comp.vue";

const c = new Comp();
// c.$slots
</script>

<template>
  <Comp name="foo">
    <template #header="{ foo }">
      <h1>{{ foo }}</h1>
      <span>{{ new Comp().$slots.header }} </span>
    </template>
  </Comp>
</template>
