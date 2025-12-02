<script setup lang="ts">
import { ref, computed } from "vue";

const firstName = ref("John");
const lastName = ref("Doe");

// Read-only computed
const fullName = computed(() => `${firstName.value} ${lastName.value}`);

// Writable computed with getter/setter
const fullNameWritable = computed({
  get: () => `${firstName.value} ${lastName.value}`,
  set: (val: string) => {
    const parts = val.split(" ");
    firstName.value = parts[0] || "";
    lastName.value = parts.slice(1).join(" ") || "";
  },
});

// Computed with explicit type annotation
const greeting = computed<string>(() => {
  return `Hello, ${fullName.value}!`;
});

// Computed with complex return type
interface Stats {
  charCount: number;
  wordCount: number;
  isEmpty: boolean;
}

const nameStats = computed<Stats>(() => ({
  charCount: fullName.value.length,
  wordCount: fullName.value.split(" ").filter(Boolean).length,
  isEmpty: fullName.value.trim() === "",
}));

// Computed with debugger options
const debuggedComputed = computed(
  () => firstName.value.toUpperCase(),
  {
    onTrack(e) {
      console.log("Tracked:", e.target, e.type);
    },
    onTrigger(e) {
      console.log("Triggered:", e.target, e.type, e.key);
    },
  }
);

// Chained computed
const upperFullName = computed(() => fullName.value.toUpperCase());
const formattedName = computed(() => `[${upperFullName.value}]`);

// Computed based on props
const props = defineProps<{
  prefix?: string;
}>();

const prefixedName = computed(() => {
  return props.prefix ? `${props.prefix} ${fullName.value}` : fullName.value;
});
</script>

<template>
  <div>
    <input v-model="firstName" placeholder="First name" />
    <input v-model="lastName" placeholder="Last name" />
    <input v-model="fullNameWritable" placeholder="Full name" />

    <p>Full name: {{ fullName }}</p>
    <p>Greeting: {{ greeting }}</p>
    <p>Stats: {{ nameStats }}</p>
    <p>Formatted: {{ formattedName }}</p>
    <p>Prefixed: {{ prefixedName }}</p>
  </div>
</template>
