<script setup>
import { onMounted, onUnmounted, onBeforeMount, onBeforeUnmount, onUpdated, onBeforeUpdate } from "vue";
import { ref } from "vue";

const count = ref(0);

onBeforeMount(() => {
  console.log("beforeMount");
});

onMounted(() => {
  console.log("mounted");
});

onBeforeUpdate(() => {
  console.log("beforeUpdate");
});

onUpdated(() => {
  console.log("updated");
});

onBeforeUnmount(() => {
  console.log("beforeUnmount");
});

onUnmounted(() => {
  console.log("unmounted");
});

function increment() {
  count.value++;
}
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <button @click="increment">Increment</button>
  </div>
</template>
