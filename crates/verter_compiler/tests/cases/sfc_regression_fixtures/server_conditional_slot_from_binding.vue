<script setup lang="ts">
import Child from './Child.vue';
const show = true;
</script>
<template>
  <Child><template v-if="show" #actions>x</template></Child>
</template>
