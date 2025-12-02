<script setup>
// Basic model with runtime declaration
const modelValue = defineModel({ type: String, required: true });

// Named model with default value
const count = defineModel("count", { type: Number, default: 0 });

// Model with modifiers
const [text, textModifiers] = defineModel("text", {
  type: String,
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
const [numeric, numericModifiers] = defineModel("numeric", {
  type: Number,
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
const optional = defineModel("optional", { type: String });

// Model with validator
const validated = defineModel("validated", {
  type: String,
  default: "pending",
  validator: (value) => ["pending", "active", "completed"].includes(value),
});

// Model for array
const items = defineModel("items", { type: Array, default: () => [] });

// Model for object
const config = defineModel("config", {
  type: Object,
  default: () => ({ enabled: false, threshold: 10 }),
});

function updateValue(newValue) {
  modelValue.value = newValue;
}

function incrementCount() {
  count.value++;
}

function addItem(item) {
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
      <input :value="modelValue" @input="updateValue($event.target.value)" />
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
      <label>Validated:</label>
      <select v-model="validated">
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
