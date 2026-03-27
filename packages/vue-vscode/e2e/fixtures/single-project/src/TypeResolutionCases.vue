<script setup lang="ts">
import { ref, computed, reactive } from "vue";

// Union types
const mixed = ref<string | number>(0);

// Nested object types
interface DeepNested {
  deep: { value: string; count: number };
}
const nested = reactive<DeepNested>({ deep: { value: "hello", count: 1 } });

// Enum-like const objects
const Status = { Active: "active", Inactive: "inactive" } as const;
type StatusType = (typeof Status)[keyof typeof Status];
const currentStatus = ref<StatusType>("active");

// Intersection types
interface HasName {
  name: string;
}
interface HasAge {
  age: number;
}
type Person = HasName & HasAge;
const person = ref<Person>({ name: "Alice", age: 30 });

// Computed with complex return
const summary = computed(() => `${person.value.name}: ${person.value.age}`);
</script>
<template>
  <div>
    <p>{{ mixed }}</p>
    <p>{{ nested.deep.va }}</p>
    <p>{{ nested.deep }}</p>
    <p>{{ currentStatus }}</p>
    <p>{{ person }}</p>
    <p>{{ summary }}</p>
  </div>
</template>
