<script setup lang="ts">
import { ref, computed, onMounted } from "vue";

const count = ref(0);
const doubled = computed(() => count.value * 2);

onMounted(() => {
  console.log("mounted");
});

function increment() {
  count.value++;
}
</script>
<template>
  <div>
    <p>{{ count }} x 2 = {{ doubled }}</p>
    <button @click="increment">+</button>
  </div>
</template>
