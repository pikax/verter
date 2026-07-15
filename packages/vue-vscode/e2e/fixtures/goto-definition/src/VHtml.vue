<script setup lang="ts">
import { ref } from "vue";

// CTRL+CLICK target: `msg` in `v-html="msg"` must navigate here.
const msg = ref("<b>hello</b>");
</script>
<template>
  <div v-html="msg" />
</template>
