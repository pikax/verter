<script setup lang="ts">
import { ref } from "vue";

const typeCount = ref(0);
function bumpType() {
  typeCount.value += 1;
}
</script>

<template>
  <div>
    <button type="button" @click="bumpType">{{ typeCount }}</button>
    <!-- type-complete-here --><
  </div>
</template>
