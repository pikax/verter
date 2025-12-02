<script lang="tsx" setup>
// Script setup with TSX lang
import { ref } from "vue";

const count = ref(0);

// TSX expression in script
const element = <div>TSX in script</div>;
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
  </div>
</template>
