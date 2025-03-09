<script setup lang="ts">
import Comp from "./Comp.vue";
const c = new Comp();

c.$data.foo;

</script>

<template>
  <Comp />

  {{
    $data
  }}
</template>