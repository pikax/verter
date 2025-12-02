<script lang="tsx" setup>
// Script setup with TSX lang
import { ref, type VNode } from "vue";

const count = ref(0);

// TSX expression in script - wrapped properly
const element: VNode = (<div>TSX in script</div>);
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
  </div>
</template>
