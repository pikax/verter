<template>
  <button @click="send">go</button>
</template>

<script setup lang="ts">
const emit = defineEmits<{ submit: [value: string]; close: [] }>();

const trigger = emit; // @dx-anchor emit

function send() {
  trigger("submit", "ok");
}
</script>
