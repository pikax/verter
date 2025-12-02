<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  name: "Reactivity",
  data() {
    return {
      count: 0,
      name: "John",
    };
  },
  computed: {
    doubleCount(): number {
      return this.count * 2;
    },
    greeting(): string {
      return `Hello, ${this.name}!`;
    },
  },
  methods: {
    increment(): void {
      this.count++;
    },
    updateName(newName: string): void {
      this.name = newName;
    },
  },
});
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <p>Double: {{ doubleCount }}</p>
    <p>{{ greeting }}</p>
    <button @click="increment">Increment</button>
    <input :value="name" @input="(e) => updateName((e.target as HTMLInputElement).value)" />
  </div>
</template>
