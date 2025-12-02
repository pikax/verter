<script setup>
// JavaScript doesn't have generics - using runtime props instead
// This demonstrates the equivalent dynamic approach

const props = defineProps({
  items: {
    type: Array,
    required: true,
    default: () => [],
  },
  selected: {
    type: [Object, String, Number],
    default: undefined,
  },
  labelKey: {
    type: String,
    required: true,
  },
  valueKey: {
    type: String,
    required: true,
  },
  options: {
    type: Array,
    default: () => [],
  },
  multiple: {
    type: Boolean,
    default: false,
  },
});

const emit = defineEmits([
  "select",
  "change",
  "optionSelect",
]);

defineExpose({
  getSelected: () => props.selected,
  getItems: () => props.items,
});

function getLabel(item) {
  return String(item[props.labelKey]);
}

function getValue(item) {
  return item[props.valueKey];
}

function isSelected(item) {
  return props.selected === item;
}

function selectItem(item) {
  emit("select", item);
}

function handleChange(items) {
  emit("change", items);
}

function selectOption(value) {
  emit("optionSelect", value);
}

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
      <select @change="selectOption($event.target.value)">
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
