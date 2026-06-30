<script setup lang="ts">
// withDefaults default values with intentional type mismatches.
interface Props {
  count: number
  label: string
}

// TS error — default `count` is a string (expected number), default `label`
// is a number (expected string). Exercises withDefaults default-value typing.
const props = withDefaults(defineProps<Props>(), {
  count: "not-a-number",
  label: 123,
})
</script>

<template>
  <div>{{ props.count }} {{ props.label }}</div>
</template>
