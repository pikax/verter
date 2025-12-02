<script setup>
// defineProps - runtime syntax
const props = defineProps({
  name: { type: String, required: true },
  count: { type: Number, required: true },
  active: { type: Boolean, default: false },
});

// defineEmits - array syntax
const emit = defineEmits(["change", "update"]);

function handleClick() {
  emit("change", props.name);
  emit("update", props.count);
}
</script>

<template>
  <div>
    <p>Name: {{ props.name }}</p>
    <p>Count: {{ props.count }}</p>
    <p>Active: {{ props.active }}</p>
    <button @click="handleClick">Emit</button>
  </div>
</template>
