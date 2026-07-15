<template>
  <!-- @dx-anchor evt.click --><button @click="onTap">tap {{ count }}</button>
  <!-- @dx-anchor evt.touchmove --><button @touchmove.stop="onSwipe">swipe</button>
  <!-- @dx-anchor evt.ref --><input ref="field" />
</template>

<script setup lang="ts">
import { ref } from "vue";

const count = ref(0);
const field = ref<HTMLInputElement | null>(null);

// A native click handler: its parameter must stay a DOM `MouseEvent`, not a bare
// `Event`. Hovering the bound parameter surfaces the precise native type.
function onTap(event: MouseEvent) {
  const tapEvent = event; // @dx-anchor evt.handlerArg
  count.value += tapEvent.button;
}

function onSwipe(event: TouchEvent) {
  event.preventDefault();
}
</script>
