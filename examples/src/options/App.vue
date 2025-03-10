<script setup lang="ts">
import Comp from "./Comp.vue";
import CompSetup from "./Comp.setup.vue";
const c = new Comp();

c.$data.foo;
</script>

<template>
  <Comp bar="foo" />
  <Comp :bar="1" />

  <CompSetup />

  {{ $data }}
</template>
