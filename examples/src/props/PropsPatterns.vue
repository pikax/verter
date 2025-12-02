<script setup lang="ts">
// Various defineProps patterns for parser testing

// 1. Type-only props (most common)
const typeOnlyProps = defineProps<{
  name: string;
  age: number;
  active?: boolean;
}>();

// 2. Runtime props
const runtimeProps = defineProps({
  title: String,
  count: { type: Number, required: true },
  items: { type: Array as () => string[], default: () => [] },
});

// 3. Mixed type inference with PropType
import type { PropType } from "vue";

interface User {
  id: number;
  name: string;
}

const propTypeProps = defineProps({
  user: {
    type: Object as PropType<User>,
    required: true,
  },
  status: {
    type: String as PropType<"active" | "inactive" | "pending">,
    default: "pending",
  },
  tags: {
    type: Array as PropType<string[]>,
    default: () => [],
  },
});

// 4. Boolean casting behavior
const booleanProps = defineProps({
  // Boolean-only prop: <Comp disabled /> -> disabled = true
  disabled: Boolean,
  // Boolean with default
  visible: { type: Boolean, default: true },
  // String | Boolean (String takes precedence)
  value: [String, Boolean],
  // Boolean | String (Boolean takes precedence)  
  flag: [Boolean, String],
});

// 5. Validator function
const validatedProps = defineProps({
  percentage: {
    type: Number,
    required: true,
    validator: (value: number) => value >= 0 && value <= 100,
  },
  email: {
    type: String,
    validator: (value: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value),
  },
});

// 6. Object/Array defaults (must use factory functions)
const complexDefaultProps = defineProps({
  config: {
    type: Object as PropType<{ a: number; b: string }>,
    default: () => ({ a: 1, b: "default" }),
  },
  list: {
    type: Array as PropType<number[]>,
    default: () => [1, 2, 3],
  },
});

// 7. Function props
const functionProps = defineProps({
  onClick: Function as PropType<(event: MouseEvent) => void>,
  formatter: {
    type: Function as PropType<(value: number) => string>,
    default: (v: number) => v.toString(),
  },
});
</script>

<template>
  <div>
    <p>Type-only: {{ typeOnlyProps.name }}</p>
    <p>Runtime: {{ runtimeProps.title }}</p>
    <p>PropType: {{ propTypeProps.user.name }}</p>
    <p>Boolean: {{ booleanProps.disabled }}</p>
    <p>Validated: {{ validatedProps.percentage }}%</p>
  </div>
</template>
