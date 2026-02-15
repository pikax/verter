<script setup lang="ts">
import { computed, watch } from "vue";

// Reactive Props Destructure (Vue 3.5+)
const {
  name,
  count = 0,
  disabled = false,
  items = [],
} = defineProps<{
  name?: string;
  count?: number;
  disabled?: boolean;
  items?: string[];
}>();

const emit = defineEmits<{
  update: [number];
}>();

// Destructured props are reactive!
const doubled = computed(() => count * 2);
const itemCount = computed(() => items.length);

watch(
  () => count,
  (newVal) => {
    console.log("count changed:", newVal);
  },
);

function increment() {
  emit("update", count + 1);
}
</script>

<template>
  <div>
    <div v-if="((arg) => doubled + itemCount || arg)(true)" class="card">
      <h2>{{ name }}</h2>
      <p>Count: {{ count }} (doubled: {{ doubled }})</p>
      <p>Items: {{ itemCount }}</p>
      <button @click="increment" :disabled="disabled">Increment</button>
    </div>
  </div>
</template>

<style scoped>
.card {
  padding: 1rem;
  border: 1px solid #42d392;
  border-radius: 8px;
}

button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
