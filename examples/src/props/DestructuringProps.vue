<script setup lang="ts">
// Vue 3.5+ destructuring props with defaults

// Basic destructuring
const { name, age } = defineProps<{
  name: string;
  age: number;
}>();

// With defaults using destructuring assignment defaults
const { 
  title = "Default Title",
  count = 0,
  items = [] as string[],
} = defineProps<{
  title?: string;
  count?: number;
  items?: string[];
}>();

// Complex defaults
const {
  config = { theme: "light", lang: "en" },
  callback = () => console.log("default"),
} = defineProps<{
  config?: { theme: string; lang: string };
  callback?: () => void;
}>();

// Renamed props with defaults
const {
  value: inputValue = "",
  onChange: handleChange = (val: string) => {},
} = defineProps<{
  value?: string;
  onChange?: (val: string) => void;
}>();

// Mixed required and optional with defaults
const {
  id,  // required, no default
  label = "Label",  // optional with default
  disabled = false,  // optional with default
} = defineProps<{
  id: number;
  label?: string;
  disabled?: boolean;
}>();
</script>

<template>
  <div>
    <p>Name: {{ name }}, Age: {{ age }}</p>
    <p>Title: {{ title }}, Count: {{ count }}</p>
    <p>ID: {{ id }}, Label: {{ label }}</p>
    <p>Config: {{ config.theme }}, {{ config.lang }}</p>
  </div>
</template>
