<script setup lang="ts">
import { ref } from "vue";

const inputVal = ref("hello");
</script>

<template>
  <div>
    <input v-model="inputVal" />
    <input v-model.trim="inputVal" />
    <p>{{ inputVal }}</p>
  </div>
</template>
