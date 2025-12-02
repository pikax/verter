<script>
import { defineComponent } from "vue";

export default defineComponent({
  name: "Generics",
  props: {
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
  },
  emits: ["select", "change", "optionSelect"],
  computed: {
    itemCount() {
      return this.items.length;
    },
    selectedLabel() {
      return this.selected ? this.getLabel(this.selected) : "";
    },
  },
  methods: {
    getLabel(item) {
      return String(item[this.labelKey]);
    },
    getValue(item) {
      return item[this.valueKey];
    },
    isSelected(item) {
      return this.selected === item;
    },
    selectItem(item) {
      this.$emit("select", item);
    },
    handleChange(items) {
      this.$emit("change", items);
    },
    selectOption(value) {
      this.$emit("optionSelect", value);
    },
    getSelected() {
      return this.selected;
    },
    getItems() {
      return this.items;
    },
  },
  expose: ["getSelected", "getItems"],
});
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
