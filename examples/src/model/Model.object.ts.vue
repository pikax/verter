<script lang="ts">
import { defineComponent, type PropType } from "vue";

interface Config {
  enabled: boolean;
  threshold: number;
}

export default defineComponent({
  name: "Model",
  props: {
    modelValue: {
      type: String,
      required: true,
    },
    count: {
      type: Number,
      default: 0,
    },
    text: {
      type: String,
      default: "",
    },
    textModifiers: {
      type: Object as PropType<{ capitalize?: boolean; lowercase?: boolean }>,
      default: () => ({}),
    },
    status: {
      type: String as PropType<"pending" | "active" | "completed">,
      default: "pending",
    },
    items: {
      type: Array as PropType<string[]>,
      default: () => [],
    },
    config: {
      type: Object as PropType<Config>,
      default: () => ({ enabled: false, threshold: 10 }),
    },
  },
  emits: {
    "update:modelValue": (value: string) => true,
    "update:count": (value: number) => true,
    "update:text": (value: string) => true,
    "update:status": (value: "pending" | "active" | "completed") => true,
    "update:items": (value: string[]) => true,
    "update:config": (value: Config) => true,
  },
  computed: {
    localValue: {
      get(): string {
        return this.modelValue;
      },
      set(value: string): void {
        this.$emit("update:modelValue", value);
      },
    },
    localCount: {
      get(): number {
        return this.count;
      },
      set(value: number): void {
        this.$emit("update:count", value);
      },
    },
    localText: {
      get(): string {
        return this.text;
      },
      set(value: string): void {
        let transformed = value;
        if (this.textModifiers.capitalize) {
          transformed = value.charAt(0).toUpperCase() + value.slice(1);
        }
        if (this.textModifiers.lowercase) {
          transformed = value.toLowerCase();
        }
        this.$emit("update:text", transformed);
      },
    },
    localStatus: {
      get(): "pending" | "active" | "completed" {
        return this.status;
      },
      set(value: "pending" | "active" | "completed"): void {
        this.$emit("update:status", value);
      },
    },
  },
  methods: {
    incrementCount(): void {
      this.$emit("update:count", this.count + 1);
    },
    addItem(item: string): void {
      this.$emit("update:items", [...this.items, item]);
    },
    toggleEnabled(): void {
      this.$emit("update:config", {
        ...this.config,
        enabled: !this.config.enabled,
      });
    },
  },
});
</script>

<template>
  <div class="model-demo">
    <div>
      <label>Main Model:</label>
      <input v-model="localValue" />
    </div>
    <div>
      <label>Count: {{ localCount }}</label>
      <button @click="incrementCount">Increment</button>
    </div>
    <div>
      <label>Text with modifiers:</label>
      <input v-model="localText" />
      <span>Modifiers: {{ textModifiers }}</span>
    </div>
    <div>
      <label>Status:</label>
      <select v-model="localStatus">
        <option value="pending">Pending</option>
        <option value="active">Active</option>
        <option value="completed">Completed</option>
      </select>
    </div>
    <div>
      <label>Items:</label>
      <ul>
        <li v-for="item in items" :key="item">{{ item }}</li>
      </ul>
      <button @click="addItem('new')">Add Item</button>
    </div>
    <div>
      <label>Config enabled: {{ config.enabled }}</label>
      <button @click="toggleEnabled">Toggle</button>
    </div>
  </div>
</template>
