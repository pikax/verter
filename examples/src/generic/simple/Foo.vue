<script setup lang="ts">
import Child from "./Child.vue";
</script>

<template>
  <Child :item="{ foo: 1 }" />
</template>
