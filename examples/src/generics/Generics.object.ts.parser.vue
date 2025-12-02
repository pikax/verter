<script lang="ts">
import { defineComponent, type PropType, computed } from "vue";

// Generic types defined externally for Options API
interface SelectOption<V> {
  label: string;
  value: V;
  disabled?: boolean;
}

// Since Options API doesn't support generic="T", we use a factory function
export function createGenericComponent<T, U extends string | number = string>() {
  return defineComponent({
    name: "Generics",
    props: {
      items: {
        type: Array as PropType<T[]>,
        required: true,
        default: () => [],
      },
      selected: {
        type: [Object, String, Number] as PropType<T | undefined>,
        default: undefined,
      },
      labelKey: {
        type: String as unknown as PropType<keyof T>,
        required: true,
      },
      valueKey: {
        type: String as unknown as PropType<keyof T>,
        required: true,
      },
      options: {
        type: Array as PropType<SelectOption<U>[]>,
        default: () => [],
      },
      multiple: {
        type: Boolean,
        default: false,
      },
    },
    emits: {
      select: (item: T) => true,
      change: (items: T[]) => true,
      optionSelect: (value: U) => true,
    },
    computed: {
      itemCount(): number {
        return this.items.length;
      },
      selectedLabel(): string {
        return this.selected ? this.getLabel(this.selected) : "";
      },
    },
    methods: {
      getLabel(item: T): string {
        return String(item[this.labelKey]);
      },
      getValue(item: T): unknown {
        return item[this.valueKey];
      },
      isSelected(item: T): boolean {
        return this.selected === item;
      },
      selectItem(item: T): void {
        this.$emit("select", item);
      },
      handleChange(items: T[]): void {
        this.$emit("change", items);
      },
      selectOption(value: U): void {
        this.$emit("optionSelect", value);
      },
      getSelected(): T | undefined {
        return this.selected;
      },
      getItems(): T[] {
        return this.items;
      },
    },
    expose: ["getSelected", "getItems"],
  });
}

// Default export with concrete types for direct usage
export default defineComponent({
  name: "Generics",
  props: {
    items: {
      type: Array as PropType<Record<string, unknown>[]>,
      required: true,
      default: () => [],
    },
    selected: {
      type: Object as PropType<Record<string, unknown> | undefined>,
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
      type: Array as PropType<SelectOption<string>[]>,
      default: () => [],
    },
    multiple: {
      type: Boolean,
      default: false,
    },
  },
  emits: {
    select: (item: Record<string, unknown>) => true,
    change: (items: Record<string, unknown>[]) => true,
    optionSelect: (value: string) => true,
  },
  computed: {
    itemCount(): number {
      return this.items.length;
    },
    selectedLabel(): string {
      return this.selected ? this.getLabel(this.selected) : "";
    },
  },
  methods: {
    getLabel(item: Record<string, unknown>): string {
      return String(item[this.labelKey]);
    },
    getValue(item: Record<string, unknown>): unknown {
      return item[this.valueKey];
    },
    isSelected(item: Record<string, unknown>): boolean {
      return this.selected === item;
    },
    selectItem(item: Record<string, unknown>): void {
      this.$emit("select", item);
    },
    handleChange(items: Record<string, unknown>[]): void {
      this.$emit("change", items);
    },
    selectOption(value: string): void {
      this.$emit("optionSelect", value);
    },
    getSelected(): Record<string, unknown> | undefined {
      return this.selected;
    },
    getItems(): Record<string, unknown>[] {
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
      <select @change="selectOption(($event.target as HTMLSelectElement).value)">
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
