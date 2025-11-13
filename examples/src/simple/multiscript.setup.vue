<script>
let a = 0;
/* FOO */
</script>
<script setup>
import { computed } from "vue";
const foo = computed(() => 1);
</script>
<template>
  <div>
    {{ foo }}
  </div>
</template>
