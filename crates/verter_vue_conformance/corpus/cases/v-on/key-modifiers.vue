<script setup>
function onEnter() {}
function onEsc() {}
</script>

<template>
  <input @keyup.enter="onEnter" @keydown.esc="onEsc" />
</template>
