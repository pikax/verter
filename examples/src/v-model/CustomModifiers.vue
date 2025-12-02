<script setup lang="ts">
// Component that accepts custom v-model with modifiers
const modelValue = defineModel<string>({ default: "" });
const title = defineModel<string>("title", { default: "" });

// Custom modifier handling via modelModifiers
const modelModifiers = defineProps<{
  modelModifiers?: { capitalize?: boolean; uppercase?: boolean };
  titleModifiers?: { reverse?: boolean };
}>();

// Emit for custom handling
const emit = defineEmits<{
  "update:modelValue": [value: string];
  "update:title": [value: string];
}>();

function handleInput(event: Event) {
  let value = (event.target as HTMLInputElement).value;

  if (modelModifiers.modelModifiers?.capitalize) {
    value = value.charAt(0).toUpperCase() + value.slice(1);
  }
  if (modelModifiers.modelModifiers?.uppercase) {
    value = value.toUpperCase();
  }

  emit("update:modelValue", value);
}

function handleTitleInput(event: Event) {
  let value = (event.target as HTMLInputElement).value;

  if (modelModifiers.titleModifiers?.reverse) {
    value = value.split("").reverse().join("");
  }

  emit("update:title", value);
}
</script>

<template>
  <div>
    <input :value="modelValue" @input="handleInput" placeholder="Custom input" />
    <input :value="title" @input="handleTitleInput" placeholder="Title input" />
  </div>
</template>
