<script setup>
import { ref, watch, watchEffect } from "vue";

const count = ref(0);
const name = ref("John");

// Basic watch
watch(count, (newVal, oldVal) => {
  console.log(`Count: ${oldVal} -> ${newVal}`);
});

// Watch with options
watch(
  name,
  (newVal) => {
    console.log(`Name changed to: ${newVal}`);
  },
  { immediate: true, deep: true }
);

// Watch multiple sources
watch([count, name], ([newCount, newName], [oldCount, oldName]) => {
  console.log(`Count: ${oldCount} -> ${newCount}, Name: ${oldName} -> ${newName}`);
});

// watchEffect
watchEffect(() => {
  console.log(`Effect: count=${count.value}, name=${name.value}`);
});

function increment() {
  count.value++;
}
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <p>Name: {{ name }}</p>
    <button @click="increment">Increment</button>
    <input v-model="name" />
  </div>
</template>
