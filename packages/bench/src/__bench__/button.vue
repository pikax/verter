<script setup lang="ts">
import { computed } from "vue";
import type { RouteLocationRaw, RouteLocationNormalized } from "vue-router";
import HIcon from "./icon.vue";

const props = defineProps<{
  label?: string;
  // TODO add the correct action type
  action?: {
    to?: RouteLocationNormalized["name"];
    label?: string;
    loading?: boolean;
    disabled?: string | boolean;
  };
  to?: string | RouteLocationRaw;
  secondary?: boolean;
  color?: "secondary" | "default" | "danger" | string;
  icon?: string;
  loading?: boolean;
}>();
const label = computed(() => {
  return props.label || props.action?.label;
});

const disabled = computed(
  () => props.loading || props.action?.loading || props.action?.disabled
);

const color = computed(() => {
  switch (props.secondary ? "secondary" : props.color) {
    case "default":
    default: {
      return "bg-blue-500 text-white hover:bg-blue-700 disabled:bg-blue-300 disabled:border-blue-900";
    }
    case "secondary": {
      return "text-gray-700 hover:bg-gray-100 hover:text-gray-900 disabled:bg-gray-300 disabled:border-gray-900";
    }
    case "danger": {
      return "bg-red-600 text-white focus:ring-2 focus:ring-offset-2 focus:ring-red-500 disabled:bg-red-300 disabled:border-red-900";
    }
  }
});
</script>

<template>
  <component
    :is="action?.to || to ? 'router-link' : 'button'"
    v-bind="action"
    :to="to || action?.to"
    :disabled="disabled"
    class="relative inline-flex cursor-pointer justify-center rounded px-4 focus:outline-none focus:ring"
    :class="[color, { 'py-2 font-bold': label }]"
  >
    <slot name="icon">
      <HIcon
        v-if="icon"
        :icon="icon"
        class="flex h-full items-center text-xl max-sm:text-base"
        :class="
          !label
            ? 'w-full justify-center'
            : 'absolute inset-y-0 left-0 w-10 pl-2'
        "
      />
    </slot>
    <span
      class="pointer-events-none flex h-fit self-center max-sm:text-sm"
      :class="{ 'ml-4': icon && label }"
      data-test-id="label-container"
    >
      <slot>
        {{ label }}
      </slot>
    </span>
  </component>
</template>
