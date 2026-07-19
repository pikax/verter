<script setup lang="ts">
const props = defineProps<{ liveProp: string; deadProp: number }>();
const emit = defineEmits<{ liveEvent: []; deadEvent: [] }>();
defineSlots<{ liveSlot(): unknown; deadSlot(): unknown }>();

function fire() {
  emit("liveEvent");
}
</script>

<template>
  <button @click="fire">{{ props.liveProp }}</button>
  <slot name="liveSlot" />
</template>
