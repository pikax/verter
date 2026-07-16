<script setup lang="ts">
import StrictLeaf from "./StrictLeaf.vue";
</script>

<template>
  <!-- totallyFakeProp is NOT declared and is NOT a native fallthrough attr → must error -->
  <StrictLeaf label="ok" totallyFakeProp="nope" />
</template>
