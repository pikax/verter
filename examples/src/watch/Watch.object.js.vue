<script>
export default {
  name: "Watch",
  data() {
    return {
      count: 0,
      name: "John",
    };
  },
  watch: {
    count(newVal, oldVal) {
      console.log(`Count: ${oldVal} -> ${newVal}`);
    },
    name: {
      handler(newVal) {
        console.log(`Name changed to: ${newVal}`);
      },
      immediate: true,
      deep: true,
    },
  },
  methods: {
    increment() {
      this.count++;
    },
  },
};
</script>

<template>
  <div>
    <p>Count: {{ count }}</p>
    <p>Name: {{ name }}</p>
    <button @click="increment">Increment</button>
    <input v-model="name" />
  </div>
</template>
