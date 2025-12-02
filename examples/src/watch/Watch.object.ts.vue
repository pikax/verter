<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  name: "Watch",
  data() {
    return {
      count: 0,
      name: "John",
    };
  },
  watch: {
    count(newVal: number, oldVal: number) {
      console.log(`Count: ${oldVal} -> ${newVal}`);
    },
    name: {
      handler(newVal: string) {
        console.log(`Name changed to: ${newVal}`);
      },
      immediate: true,
      deep: true,
    },
  },
  methods: {
    increment(): void {
      this.count++;
    },
  },
});
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <p>Name: {{ name }}</p>
    <button @click="increment">Increment</button>
    <input v-model="name" />
  </div>
</template>
