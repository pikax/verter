<script>
import { defineComponent } from "vue";

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
      type: Object,
      default: () => ({}),
    },
    status: {
      type: String,
      default: "pending",
      validator: (value) => ["pending", "active", "completed"].includes(value),
    },
    items: {
      type: Array,
      default: () => [],
    },
    config: {
      type: Object,
      default: () => ({ enabled: false, threshold: 10 }),
    },
  },
  emits: [
    "update:modelValue",
    "update:count",
    "update:text",
    "update:status",
    "update:items",
    "update:config",
  ],
  computed: {
    localValue: {
      get() {
        return this.modelValue;
      },
      set(value) {
        this.$emit("update:modelValue", value);
      },
    },
    localCount: {
      get() {
        return this.count;
      },
      set(value) {
        this.$emit("update:count", value);
      },
    },
    localText: {
      get() {
        return this.text;
      },
      set(value) {
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
      get() {
        return this.status;
      },
      set(value) {
        this.$emit("update:status", value);
      },
    },
  },
  methods: {
    incrementCount() {
      this.$emit("update:count", this.count + 1);
    },
    addItem(item) {
      this.$emit("update:items", [...this.items, item]);
    },
    toggleEnabled() {
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
