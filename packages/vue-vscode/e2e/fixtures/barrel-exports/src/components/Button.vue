<script setup lang="ts">
defineProps<{
  label: string;
  disabled?: boolean;
  size?: "small" | "medium" | "large";
}>();
defineEmits<{ click: [event: MouseEvent] }>();
</script>
<template>
  <button :disabled="disabled">{{ label }}</button>
</template>
