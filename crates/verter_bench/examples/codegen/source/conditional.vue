<template>
  <div>
    <span v-if="show">Visible</span>
    <span v-else>Hidden</span>
  </div>
</template>

<script setup>
import { ref } from "vue";
const show = ref(true);
</script>
