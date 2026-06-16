<template>
  <button @click="onClick">btn</button>
  <input @keydown="onKeydown" />
</template>

<script setup lang="ts">
function onClick(event: MouseEvent) {
  const clickEvent = event; // @dx-anchor click.event
  void clickEvent;
}

function onKeydown(event: KeyboardEvent) {
  const keyEvent = event; // @dx-anchor keydown.event
  void keyEvent;
}
</script>
