<script setup lang="ts">
import { ref } from "vue";

interface Person {
  name: string;
  age: number;
}

const person = ref<Person | null>(null);
const flag = ref(true);
const items = ref<Array<string | number>>(["a", 1]);

function setPerson() {
  person.value = { name: "Ada", age: 36 };
}
</script>

<template>
  <div>
    <button @click="setPerson">set</button>
    <p v-if="person">{{ person.name }} — {{ person.age }}</p>
    <p v-else>no person</p>
    <p v-if="flag">flag-on</p>
    <span v-for="item in items" :key="String(item)">{{ item }}</span>
  </div>
</template>
