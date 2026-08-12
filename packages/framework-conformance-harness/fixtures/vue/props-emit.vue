<script setup>
const props = defineProps({
  label: { type: String, required: true },
  disabled: { type: Boolean, default: false },
});
const emit = defineEmits(["toggle"]);

function onClick() {
  emit("toggle", !props.disabled);
}
</script>

<template>
  <button :disabled="disabled" @click="onClick">{{ label }}</button>
</template>
