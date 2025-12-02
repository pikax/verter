<script setup lang="ts">
import { useAttrs, useSlots } from "vue";

// Disable automatic attribute inheritance
defineOptions({
  inheritAttrs: false,
});

// Access attributes via useAttrs
const attrs = useAttrs();

// Access slots via useSlots
const slots = useSlots();

// Props that we handle explicitly
defineProps<{
  label: string;
}>();

// Type-safe attribute access
const className = attrs.class as string | undefined;
const style = attrs.style as string | Record<string, string> | undefined;
const dataTestId = attrs["data-testid"] as string | undefined;
</script>

<template>
  <div class="wrapper">
    <label>{{ label }}</label>
    <!-- Forward all attrs to inner input -->
    <input v-bind="attrs" />

    <!-- Display attrs for debugging -->
    <pre>Attrs: {{ JSON.stringify(attrs, null, 2) }}</pre>

    <!-- Check slot existence -->
    <div v-if="slots.default">
      <slot />
    </div>
    <div v-if="slots.helper">
      <slot name="helper" />
    </div>
  </div>
</template>
