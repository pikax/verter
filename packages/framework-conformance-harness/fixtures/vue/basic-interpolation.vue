<script setup>
import { ref } from "vue";

const count = ref(0);
const items = ["a", "b", "c"];
</script>

<template>
  <div class="root">
    <p v-if="count > 0">{{ count }}</p>
    <p v-else>zero</p>
    <ul>
      <li v-for="item in items" :key="item">{{ item }}</li>
    </ul>
  </div>
</template>
