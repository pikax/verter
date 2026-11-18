<script setup lang="ts">
import Child from './Child.vue';
</script>
<template>
  <Child><template v-if="$slots.actions" #actions><slot name="actions" /></template></Child>
</template>
