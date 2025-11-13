<script lang="ts">
let a = 0;
let b = <string>0;
/* FOO */
</script>
<script setup lang="ts">
import { computed } from "vue";
const foo = computed(() => 1);
// @ts-expect-error
let c = <string>0;
let b = "";
</script>
<template>
  <div>
    {{ foo }}
  </div>
</template>
