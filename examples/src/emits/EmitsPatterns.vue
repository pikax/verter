<script setup lang="ts">
// defineEmits patterns for parser testing

// 1. Type-only emits (recommended)
const emit1 = defineEmits<{
  change: [value: string];
  update: [id: number, data: { name: string }];
  submit: [];  // no payload
  "kebab-case": [value: boolean];
}>();

// 2. Call signature syntax (alternative)
const emit2 = defineEmits<{
  (e: "change", value: string): void;
  (e: "update", id: number, data: { name: string }): void;
  (e: "submit"): void;
}>();

// 3. Runtime emits (with validation)
const emit3 = defineEmits({
  change: (value: string) => {
    // Validation - return false to warn
    return typeof value === "string";
  },
  submit: null,  // No validation
  update: (id: number, data: object) => {
    return typeof id === "number" && data !== null;
  },
});

// 4. Array syntax (no validation, no typing)
const emit4 = defineEmits(["change", "update", "submit"]);

// 5. v-model related emits
const modelValue = defineModel<string>();
const title = defineModel<string>("title");

// These are auto-generated, but can be explicit:
const emit5 = defineEmits<{
  "update:modelValue": [value: string];
  "update:title": [value: string];
}>();

// Usage examples
function handleClick() {
  emit1("change", "new value");
  emit1("update", 1, { name: "test" });
  emit1("submit");
  emit1("kebab-case", true);
}

function handleChange(e: Event) {
  const value = (e.target as HTMLInputElement).value;
  emit2("change", value);
}
</script>

<template>
  <div>
    <button @click="handleClick">Emit Events</button>
    <input @input="handleChange" />
  </div>
</template>
