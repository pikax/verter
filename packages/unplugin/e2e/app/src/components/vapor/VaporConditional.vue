<script setup lang="ts">
import { ref } from "vue";

const show = ref(true);

function toggle() {
  show.value = !show.value;
}
</script>

<template vapor>
  <div data-testid="vapor-conditional">
    <button data-testid="vapor-toggle" @click="toggle">Toggle</button>
    <span v-if="show" data-testid="vapor-visible">Visible</span>
    <span v-else data-testid="vapor-hidden">Hidden</span>
  </div>
</template>
