<script setup lang="ts">
import { ref } from "vue";

// CTRL+CLICK target: `content` in `v-text="content"` must navigate here.
const content = ref("plain text");
</script>
<template>
  <div v-text="content" />
</template>
