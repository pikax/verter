<script setup lang="ts" generic="T, U extends string | number = string">
import { computed } from "vue";

// Generic component with type parameters
// T is unconstrained, U has a constraint and default

interface SelectOption<V> {
  label: string;
  value: V;
  disabled?: boolean;
}

// Props using generic types
const props = defineProps<{
  items: T[];
  selected?: T;
  labelKey: keyof T;
  valueKey: keyof T;
  options?: SelectOption<U>[];
  multiple?: boolean;
}>();

// Emits with generic types
const emit = defineEmits<{
  select: [item: T];
  change: [items: T[]];
  optionSelect: [value: U];
}>();

// Expose generic return type
defineExpose<{
  getSelected: () => T | undefined;
  getItems: () => T[];
}>();

// Generic utility functions
function getLabel(item: T): string {
  return String(item[props.labelKey]);
}

function getValue(item: T): unknown {
  return item[props.valueKey];
}

function isSelected(item: T): boolean {
  return props.selected === item;
}

function selectItem(item: T) {
  emit("select", item);
}

function handleChange(items: T[]) {
  emit("change", items);
}

function selectOption(value: U) {
  emit("optionSelect", value);
}

// Generic computed (computed imported at top)
const itemCount = computed(() => props.items.length);
const selectedLabel = computed(() =>
  props.selected ? getLabel(props.selected) : ""
);
</script>

<template>
  <div class="generic-component">
    <div>Selected: {{ selectedLabel }}</div>
    <div>Total items: {{ itemCount }}</div>
    <ul>
      <li
        v-for="(item, index) in items"
        :key="index"
        :class="{ selected: isSelected(item) }"
        @click="selectItem(item)"
      >
        {{ getLabel(item) }} - {{ getValue(item) }}
      </li>
    </ul>
    <div v-if="options?.length">
      <select @change="selectOption(($event.target as HTMLSelectElement).value as U)">
        <option
          v-for="opt in options"
          :key="String(opt.value)"
          :value="opt.value"
          :disabled="opt.disabled"
        >
          {{ opt.label }}
        </option>
      </select>
    </div>
  </div>
</template>
