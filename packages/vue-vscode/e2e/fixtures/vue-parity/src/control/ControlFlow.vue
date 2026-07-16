<script setup lang="ts">
import { ref } from "vue";

interface User {
  name: string;
  email: string;
}

const users = ref<User[]>([
  { name: "Ada", email: "ada@example.com" },
  { name: "Grace", email: "grace@example.com" },
]);
const selected = ref<User | null>(null);

function select(user: User) {
  selected.value = user;
}
</script>

<template>
  <div>
    <button v-for="user in users" :key="user.email" @click="select(user)">
      {{ user.name }}
    </button>
    <p v-if="selected">{{ selected.name }} &lt;{{ selected.email }}&gt;</p>
    <p v-else>none</p>
  </div>
</template>
