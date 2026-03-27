<script setup lang="ts">
import { ref } from "vue";

const count = ref(0);

function increment() {
  count.value++;
}
</script>

<template vapor>
  <div data-testid="vapor-counter">
    <span data-testid="vapor-count">{{ count }}</span>
    <button data-testid="vapor-increment" @click="increment">+1</button>
  </div>
</template>
