<script setup lang="ts">
// Basic model with type-based declaration
const modelValue = defineModel<string>({ required: true });

// Named model with default value
const count = defineModel<number>("count", { default: 0 });

// Model with modifiers
const [text, textModifiers] = defineModel<string>("text", {
  set(value) {
    if (textModifiers.capitalize) {
      return value.charAt(0).toUpperCase() + value.slice(1);
    }
    if (textModifiers.lowercase) {
      return value.toLowerCase();
    }
    return value;
  },
});

// Model with get/set transformers
const [numeric, numericModifiers] = defineModel<number>("numeric", {
  get(value) {
    return value ?? 0;
  },
  set(value) {
    if (numericModifiers.double) {
      return value * 2;
    }
    return value;
  },
});

// Optional model
const optional = defineModel<string | undefined>("optional");

// Model with union type
const status = defineModel<"pending" | "active" | "completed">("status", {
  default: "pending",
});

// Model for array
const items = defineModel<string[]>("items", { default: () => [] });

// Model for object
interface Config {
  enabled: boolean;
  threshold: number;
}
const config = defineModel<Config>("config", {
  default: () => ({ enabled: false, threshold: 10 }),
});

function updateValue(newValue: string) {
  modelValue.value = newValue;
}

function incrementCount() {
  count.value++;
}

function addItem(item: string) {
  items.value = [...items.value, item];
}

function toggleEnabled() {
  config.value = { ...config.value, enabled: !config.value.enabled };
}
</script>

<template>
  <div class="model-demo">
    <div>
      <label>Main Model:</label>
      <input :value="modelValue" @input="updateValue(($event.target as HTMLInputElement).value)" />
    </div>
    <div>
      <label>Count: {{ count }}</label>
      <button @click="incrementCount">Increment</button>
    </div>
    <div>
      <label>Text with modifiers:</label>
      <input v-model="text" />
      <span>Modifiers: {{ textModifiers }}</span>
    </div>
    <div>
      <label>Numeric ({{ numericModifiers }}):</label>
      <input type="number" v-model.number="numeric" />
    </div>
    <div>
      <label>Optional:</label>
      <input v-model="optional" />
    </div>
    <div>
      <label>Status:</label>
      <select v-model="status">
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
