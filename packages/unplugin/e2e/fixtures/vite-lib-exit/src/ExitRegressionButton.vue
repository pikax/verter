<script setup lang="ts">
const props = withDefaults(defineProps<{
  label?: string;
}>(), {
  label: "Exit regression fixture",
});

defineEmits<{
  click: [];
}>();
</script>

<template>
  <button class="exit-regression-button">
    {{ props.label }}
  </button>
</template>

<style scoped lang="scss">
$border-hover: #555;

.exit-regression-button {
  border: 1px solid #222;
  padding: 0.5rem 0.75rem;

  &:hover {
    border-color: $border-hover;
  }
}
</style>
