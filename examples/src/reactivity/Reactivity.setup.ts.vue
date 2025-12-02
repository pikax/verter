<script setup lang="ts">
import { ref, computed } from "vue";

const count = ref(0);
const name = ref("John");

const doubleCount = computed(() => count.value * 2);
const greeting = computed(() => `Hello, ${name.value}!`);

function increment() {
  count.value++;
}

function updateName(newName: string) {
  name.value = newName;
}
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <p>Double: {{ doubleCount }}</p>
    <p>{{ greeting }}</p>
    <button @click="increment">Increment</button>
    <input :value="name" @input="(e) => updateName((e.target as HTMLInputElement).value)" />
  </div>
</template>
