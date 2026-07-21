<template>
  <button class="base-button" :variant="variant" @click="onClick">
    <slot name="icon"></slot>
    <span class="base-label">{{ label }}</span>
  </button>
</template>

<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{ label: string; variant: string }>();
const emit = defineEmits<{ press: [id: string] }>();

const label = computed(() => props.label.trim());

function onClick(): void {
  emit("press", props.label);
}
</script>

<style scoped>
.base-button {
  color: red;
}
</style>
